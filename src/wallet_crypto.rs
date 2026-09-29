//! ML-DSA-65 wallet cryptography (post-quantum, matches engine's circl usage).
//!
//! Key formats:
//! - seed: 32 bytes (64 hex) — what `kdc wallet new` generates and stores.
//! - FIPS secret: 4032 bytes (8064 hex) — what the engine's genesis tooling
//!   emits. Its FIRST 32 bytes are the seed (FIPS 204 KeyGen), so engine keys
//!   import losslessly via [`seed_from_any_key`].
//! - public: 1952 bytes (3904 hex), FIPS `pk.encode()`.
//! Address = QSH(pubkey)[KDC-ADDR] last-40 hex (see `qsh` module).

use anyhow::{bail, Result};
use ml_dsa::signature::Keypair;
use ml_dsa::signature::{Signer, Verifier};
use ml_dsa::{MlDsa65, SigningKey, VerifyingKey};
use rand::RngExt as _;

pub const SEED_LEN: usize = 32;
pub const FIPS_SK_LEN: usize = 4032;
pub const PUB_LEN: usize = 1952;

/// Generate a fresh random 32-byte seed.
pub fn random_seed() -> [u8; SEED_LEN] {
    let mut rng = rand::rng();
    let mut s = [0u8; SEED_LEN];
    rng.fill(&mut s);
    s
}

/// Key material: seeds sign; FIPS secrets (engine format) derive
/// address + verify via [`crate::fips`] reconstruction.
#[derive(Debug, Clone)]
pub enum KeyMaterial {
    Seed([u8; SEED_LEN]),
    Fips(Box<[u8; FIPS_SK_LEN]>),
}

/// Accept a 64-hex seed OR an 8064-hex FIPS secret (engine format).
pub fn key_from_any_hex(hex_key: &str) -> Result<KeyMaterial> {
    let h: String = hex_key.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    let bytes = hex::decode(&h)?;
    match bytes.len() {
        SEED_LEN => {
            let mut s = [0u8; SEED_LEN];
            s.copy_from_slice(&bytes);
            Ok(KeyMaterial::Seed(s))
        }
        FIPS_SK_LEN => {
            let mut f = Box::new([0u8; FIPS_SK_LEN]);
            f.copy_from_slice(&bytes);
            Ok(KeyMaterial::Fips(f))
        }
        n => bail!("key must be 32 bytes (seed) or 4032 bytes (FIPS secret), got {n}"),
    }
}

/// Legacy helper: seeds only (FIPS secrets have no recoverable seed —
/// FIPS 204 stores rho, not the seed — use [`key_from_any_hex`]).
pub fn seed_from_any_key(hex_key: &str) -> Result<[u8; SEED_LEN]> {
    match key_from_any_hex(hex_key)? {
        KeyMaterial::Seed(s) => Ok(s),
        KeyMaterial::Fips(_) => bail!("FIPS secret: seed not recoverable (use key_from_any_hex)"),
    }
}

fn signing_key(seed: &[u8; SEED_LEN]) -> SigningKey<MlDsa65> {
    let arr = ml_dsa::Seed::try_from(&seed[..]).expect("32 bytes");
    SigningKey::<MlDsa65>::from_seed(&arr)
}

/// FIPS-encoded 1952-byte public key from a seed.
pub fn public_bytes(seed: &[u8; SEED_LEN]) -> [u8; PUB_LEN] {
    let sk = signing_key(seed);
    let vk: VerifyingKey<MlDsa65> = Keypair::verifying_key(&sk);
    let enc = vk.encode();
    let mut out = [0u8; PUB_LEN];
    out.copy_from_slice(enc.as_slice());
    out
}

/// Public key for any key material (FIPS via [`crate::fips`] reconstruction).
pub fn public_bytes_for_key(key: &KeyMaterial) -> Result<[u8; PUB_LEN]> {
    match key {
        KeyMaterial::Seed(s) => Ok(public_bytes(s)),
        KeyMaterial::Fips(f) => crate::fips::pubkey_from_fips_sk(&f[..]),
    }
}

/// Canonical wallet address for any key material.
pub fn address_for_key(key: &KeyMaterial) -> Result<String> {
    Ok(crate::qsh::address_hex(&public_bytes_for_key(key)?))
}

/// Canonical wallet address for a seed.
pub fn address_for_seed(seed: &[u8; SEED_LEN]) -> String {
    crate::qsh::address_hex(&public_bytes(seed))
}

/// Deterministic ML-DSA-65 signature (empty context, like the engine SDK).
/// Seeds only: FIPS secrets cannot sign in v0.1 (expanded key unavailable).
pub fn sign(seed: &[u8; SEED_LEN], msg: &[u8]) -> Result<Vec<u8>> {
    let sk = signing_key(seed);
    let sig = sk.try_sign(msg)?;
    Ok(sig.encode().as_slice().to_vec())
}

/// Sign with any key material (FIPS → explicit error).
pub fn sign_with(key: &KeyMaterial, msg: &[u8]) -> Result<Vec<u8>> {
    match key {
        KeyMaterial::Seed(s) => sign(s, msg),
        KeyMaterial::Fips(_) => bail!(
            "signing with engine FIPS keys unsupported in v0.1 (expanded key unavailable); \
             use a seed-format key, or the engine's sign_message tool"
        ),
    }
}

/// Verify an ML-DSA-65 signature against raw public key bytes.
pub fn verify(pubkey: &[u8], msg: &[u8], sig: &[u8]) -> Result<()> {
    use ml_dsa::EncodedVerifyingKey;
    if pubkey.len() != PUB_LEN {
        bail!("public key must be {PUB_LEN} bytes, got {}", pubkey.len());
    }
    let enc = EncodedVerifyingKey::<MlDsa65>::try_from(pubkey)?;
    let vk = VerifyingKey::<MlDsa65>::decode(&enc);
    let sig = ml_dsa::Signature::<MlDsa65>::try_from(sig)?;
    vk.verify(msg, &sig)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_sign_verify() {
        let seed = random_seed();
        let msg = b"KODECHAIN-CLI-TEST";
        let sig = sign(&seed, msg).unwrap();
        assert_eq!(sig.len(), 3309); // ML-DSA-65 signature size
        verify(&public_bytes(&seed), msg, &sig).unwrap();
        assert!(verify(&public_bytes(&seed), b"tampered", &sig).is_err());
    }

    #[test]
    fn key_formats() {
        // 64-hex seed accepted with full capability
        assert!(matches!(
            key_from_any_hex(&"ab".repeat(32)).unwrap(),
            KeyMaterial::Seed(_)
        ));
        // 8064-hex FIPS secret accepted as Fips (seed NOT recoverable)
        let fips = "ab".repeat(32) + &"00".repeat(4000);
        assert!(matches!(
            key_from_any_hex(&fips).unwrap(),
            KeyMaterial::Fips(_)
        ));
        assert!(seed_from_any_key(&fips).is_err());
        // garbage rejected
        assert!(key_from_any_hex("zz").is_err());
        assert!(key_from_any_hex(&"ab".repeat(10)).is_err());
    }

    #[test]
    fn address_is_canonical() {
        let seed = [7u8; 32];
        let addr = address_for_seed(&seed);
        assert!(addr.starts_with("0x") && addr.len() == 42, "{addr}");
        // determinism
        assert_eq!(address_for_seed(&seed), addr);
    }
}
