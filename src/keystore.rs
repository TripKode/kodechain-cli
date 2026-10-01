//! KDC-Keystore v1 — cifrado de seeds estilo Web3 Secret Storage (adaptado).
//!
//! Formato JSON autónomo por wallet:
//!   { "version": 1, "name", "address", "public_key",
//!     "crypto": { "kdf": "scrypt", "kdfparams": {n,r,p,salt_hex},
//!                 "cipher": "aes-256-gcm", "cipherparams": {nonce_hex},
//!                 "ciphertext": hex, "tag": hex } }
//!
//! - KDF: scrypt N=32768 r=8 p=1 (~100ms, estándar Ethereum-compatible).
//! - Cifrado: AES-256-GCM (autenticado: tag detecta adulteración).
//! - El salt y el nonce son aleatorios por archivo: nunca reutilizados.
//! - Al importar se RE-DERIVA address desde la seed (QSH_KDC-ADDR, misma
//!   cripto del engine) y se exige coincidencia con el campo address:
//!   un keystore adulterado o corrupto ABORTA antes de guardar nada.

use anyhow::{bail, Context, Result};
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use rand::RngExt as _;
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

pub const KDF_N: u32 = 32768;
pub const KDF_R: u32 = 8;
pub const KDF_P: u32 = 1;
const KEY_LEN: usize = 32; // AES-256
const NONCE_LEN: usize = 12; // GCM estándar

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct KdfParams {
    pub n: u32,
    pub r: u32,
    pub p: u32,
    pub salt: String, // hex
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CipherParams {
    pub nonce: String, // hex
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CryptoBlock {
    pub kdf: String,
    pub kdfparams: KdfParams,
    pub cipher: String,
    pub cipherparams: CipherParams,
    pub ciphertext: String, // hex (seed cifrada)
    pub tag: String,        // hex (GCM auth tag)
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct KeystoreFile {
    pub version: u32,
    pub name: String,
    pub address: String,      // 0x + 40 hex — debe == derivar(seed)
    pub public_key: String,   // hex — metadato para verificación externa
    pub crypto: CryptoBlock,
}

/// Deriva la llave AES desde la contraseña con scrypt.
fn derive_key(password: &str, params: &KdfParams, salt: &[u8]) -> Result<Zeroizing<[u8; KEY_LEN]>> {
    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    scrypt::scrypt(
        password.as_bytes(),
        salt,
        &scrypt::Params::new(params.n.trailing_zeros() as u8, params.r, params.p, KEY_LEN)
            .context("parámetros scrypt inválidos")?,
        key.as_mut(),
    )
    .context("scrypt falló (¿memoria?)")?;
    Ok(key)
}

/// Cifra un payload arbitrario (seed O store completo) → KeystoreFile.
/// encrypt_seed valida la longitud de seed; encrypt_bytes es genérico.
pub fn encrypt_bytes(
    payload: &[u8],
    name: &str,
    address: &str,
    public_key: &str,
    password: &str,
) -> Result<KeystoreFile> {
    let mut salt = [0u8; 16];
    let mut nonce = [0u8; NONCE_LEN];
    rand::rng().fill(&mut salt);
    rand::rng().fill(&mut nonce);

    let params = KdfParams { n: KDF_N, r: KDF_R, p: KDF_P, salt: hex::encode(salt) };
    let key = derive_key(password, &params, &salt)?;
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key[..]));
    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce), payload)
        .map_err(|_| anyhow::anyhow!("cifrado AES-GCM falló"))?;
    let (ciphertext, tag) = ct.split_at(ct.len() - 16); // GCM tag = 16 bytes finales

    Ok(KeystoreFile {
        version: 1,
        name: name.to_string(),
        address: address.to_string(),
        public_key: public_key.to_string(),
        crypto: CryptoBlock {
            kdf: "scrypt".into(),
            kdfparams: params,
            cipher: "aes-256-gcm".into(),
            cipherparams: CipherParams { nonce: hex::encode(nonce) },
            ciphertext: hex::encode(ciphertext),
            tag: hex::encode(tag),
        },
    })
}

/// Descifra un KeystoreFile → seed. Falla limpio con contraseña errada
/// (GCM tag inválido) o con formato corrupto.
pub fn decrypt_seed(ks: &KeystoreFile, password: &str) -> Result<Zeroizing<[u8; 32]>> {
    if ks.version != 1 {
        bail!("versión de keystore no soportada: {}", ks.version);
    }
    if ks.crypto.kdf != "scrypt" || ks.crypto.cipher != "aes-256-gcm" {
        bail!("KDF/cifrado no soportados: {}/{}", ks.crypto.kdf, ks.crypto.cipher);
    }
    let salt = hex::decode(&ks.crypto.kdfparams.salt).context("salt hex inválida")?;
    let nonce = hex::decode(&ks.crypto.cipherparams.nonce).context("nonce hex inválido")?;
    let mut ct = hex::decode(&ks.crypto.ciphertext).context("ciphertext hex inválido")?;
    let tag = hex::decode(&ks.crypto.tag).context("tag hex inválido")?;
    if nonce.len() != NONCE_LEN || tag.len() != 16 {
        bail!("nonce/tag con longitud inesperada (formato corrupto)");
    }
    ct.extend_from_slice(&tag);

    let key = derive_key(password, &ks.crypto.kdfparams, &salt)?;
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key[..]));
    let seed_vec = cipher
        .decrypt(Nonce::from_slice(&nonce), ct.as_ref())
        .map_err(|_| anyhow::anyhow!("contraseña incorrecta o archivo adulterado (GCM tag inválido)"))?;
    if seed_vec.len() != 32 {
        bail!("seed descifrada con longitud inesperada: {}", seed_vec.len());
    }
    let mut seed = Zeroizing::new([0u8; 32]);
    seed[..].copy_from_slice(&seed_vec);
    ct.zeroize();
    Ok(seed)
}

/// Cifra una SEED de 32 bytes (wallet individual).
pub fn encrypt_seed(
    seed: &[u8],
    name: &str,
    address: &str,
    public_key: &str,
    password: &str,
) -> Result<KeystoreFile> {
    if seed.len() != 32 {
        bail!("seed debe ser de 32 bytes, recibí {}", seed.len());
    }
    encrypt_bytes(seed, name, address, public_key, password)
}

/// Descifra un payload arbitrario (store). Igual que decrypt_seed sin
/// la restricción de 32 bytes.
pub fn decrypt_bytes(ks: &KeystoreFile, password: &str) -> Result<Zeroizing<Vec<u8>>> {
    if ks.version != 1 {
        bail!("versión de keystore no soportada: {}", ks.version);
    }
    if ks.crypto.kdf != "scrypt" || ks.crypto.cipher != "aes-256-gcm" {
        bail!("KDF/cifrado no soportados: {}/{}", ks.crypto.kdf, ks.crypto.cipher);
    }
    let salt = hex::decode(&ks.crypto.kdfparams.salt).context("salt hex inválida")?;
    let nonce = hex::decode(&ks.crypto.cipherparams.nonce).context("nonce hex inválido")?;
    let mut ct = hex::decode(&ks.crypto.ciphertext).context("ciphertext hex inválido")?;
    let tag = hex::decode(&ks.crypto.tag).context("tag hex inválido")?;
    if nonce.len() != NONCE_LEN || tag.len() != 16 {
        bail!("nonce/tag con longitud inesperada (formato corrupto)");
    }
    ct.extend_from_slice(&tag);
    let key = derive_key(password, &ks.crypto.kdfparams, &salt)?;
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key[..]));
    let out = cipher
        .decrypt(Nonce::from_slice(&nonce), ct.as_ref())
        .map_err(|_| anyhow::anyhow!("contraseña incorrecta o archivo adulterado (GCM tag inválido)"))?;
    ct.zeroize();
    Ok(Zeroizing::new(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wallet_crypto;

    const PASS: &str = "correct horse battery staple";
    const SEED: [u8; 32] = [0xABu8; 32];

    #[test]
    fn roundtrip_encrypt_decrypt() {
        let addr = wallet_crypto::address_for_seed(&SEED);
        let pubk = hex::encode(wallet_crypto::public_bytes(&SEED));
        let ks = encrypt_seed(&SEED, "test", &addr, &pubk, PASS).unwrap();
        let seed = decrypt_seed(&ks, PASS).unwrap();
        assert_eq!(&seed[..], &SEED);
    }

    #[test]
    fn wrong_password_fails_clean() {
        let addr = wallet_crypto::address_for_seed(&SEED);
        let ks = encrypt_seed(&SEED, "test", &addr, "pk:123", PASS).unwrap();
        assert!(decrypt_seed(&ks, "otra").is_err());
    }

    #[test]
    fn tampered_ciphertext_fails() {
        let addr = wallet_crypto::address_for_seed(&SEED);
        let ks = encrypt_seed(&SEED, "t", &addr, "k", PASS).unwrap();
        let mut bad = ks.clone();
        let mut ct = hex::decode(&bad.crypto.ciphertext).unwrap();
        ct[0] ^= 0xFF; // un solo bit adulterado
        bad.crypto.ciphertext = hex::encode(ct);
        assert!(decrypt_seed(&bad, PASS).is_err(), "GCM debe detectar la adulteración");
    }

    #[test]
    fn salt_and_nonce_unique_per_file() {
        let addr = wallet_crypto::address_for_seed(&SEED);
        let a = encrypt_seed(&SEED, "a", &addr, "k", PASS).unwrap();
        let b = encrypt_seed(&SEED, "a", &addr, "k", PASS).unwrap();
        assert_ne!(a.crypto.kdfparams.salt, b.crypto.kdfparams.salt, "salt debe ser único");
        assert_ne!(a.crypto.cipherparams.nonce, b.crypto.cipherparams.nonce, "nonce debe ser único");
    }

    #[test]
    fn json_roundtrip_stable_schema() {
        let addr = wallet_crypto::address_for_seed(&SEED);
        let ks = encrypt_seed(&SEED, "mn-test", &addr, "pubk", PASS).unwrap();
        let json_out = serde_json::to_string_pretty(&ks).unwrap();
        let ks2: KeystoreFile = serde_json::from_str(&json_out).unwrap();
        assert_eq!(ks2.address, addr);
        let seed = decrypt_seed(&ks2, PASS).unwrap();
        assert_eq!(&seed[..], &SEED);
        // El address del archivo DEBE derivar de la seed (invariante del import)
        assert_eq!(wallet_crypto::address_for_seed(&seed), ks2.address);
    }
}
