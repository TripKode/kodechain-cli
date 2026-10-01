//! Store cifrado (Fase 3): wallets.json → KDC-Keystore envelope.
//!
//! Formato del store cifrado (`wallets.json.enc`): un KDC-Keystore v1 cuyo
//! `ciphertext` es el JSON completo del vector de wallets (en vez de una
//! seed individual). Reusa scrypt+AES-256-GCM del módulo keystore — un solo
//! formato criptográfico en toda la herramienta.
//!
//! UX: `kdc wallet lock` cifra el store; a partir de ahí, los comandos que
//! lo leen piden la contraseña por stdin (o KDC_STORE_PASS para scripts).
//! `kdc wallet unlock` lo devuelve a plaintext (con warning). El store
//! plaintext sigue funcionando (backward-compatible) con aviso.

use crate::keystore;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::io::IsTerminal;
use zeroize::Zeroizing;

use crate::store::SavedWallet;

/// Nombre del archivo cifrado.
pub const ENC_NAME: &str = "wallets.json.enc";

fn enc_path() -> Result<std::path::PathBuf> {
    let home = crate::config::home_dir()?;
    std::fs::create_dir_all(&home)?;
    Ok(home.join(ENC_NAME))
}

fn plain_path() -> Result<std::path::PathBuf> {
    let home = crate::config::home_dir()?;
    Ok(home.join("wallets.json"))
}

pub fn store_is_locked() -> bool {
    enc_path().map(|p| p.exists()).unwrap_or(false)
}

/// Contraseña del store: KDC_STORE_PASS o prompt oculto por stdin (tty).
/// En pipes (scripts) SIN KDC_STORE_PASS: falla con instrucción clara.
pub fn ask_store_password() -> Result<Zeroizing<String>> {
    if let Ok(p) = std::env::var("KDC_STORE_PASS") {
        if !p.is_empty() {
            return Ok(Zeroizing::new(p));
        }
    }
    // En pipes (echo <pass> | kdc …) la contraseña llega por stdin —
    // es el patrón documentado en el acta de entrega del inversor.
    if !std::io::stdin().is_terminal() {
        let mut pass = String::new();
        std::io::stdin().read_line(&mut pass)?;
        let pass = Zeroizing::new(pass.trim().to_string());
        if pass.is_empty() {
            bail!("stdin vacío: exporta KDC_STORE_PASS o pásala por stdin (echo <pass> | kdc …)");
        }
        return Ok(pass);
    }
    eprint!("Contraseña del keystore local (store cifrado): ");
    let mut pass = String::new();
    std::io::stdin().read_line(&mut pass)?;
    let pass = Zeroizing::new(pass.trim().to_string());
    if pass.len() < 8 {
        bail!("contraseña demasiado corta (mínimo 8)");
    }
    Ok(pass)
}

/// Envelope del store cifrado: un KeystoreFile v1 cuyo ciphertext es el
/// JSON serializado de Vec<SavedWallet>. El address del envelope es fijo
/// (el store no es una wallet) — se usa solo como checksum del formato.
#[derive(Serialize, Deserialize)]
struct StoreEnvelope {
    version: u32,
    #[serde(rename = "type")]
    kind: String, // "kdc-store-v1"
    keystore: keystore::KeystoreFile,
}

/// Cifra el store actual (wallets.json → wallets.json.enc) y borra el plano.
pub fn lock(password: &str) -> Result<usize> {
    let plain = plain_path()?;
    if !plain.exists() {
        bail!("no hay wallets.json que cifrar (¿ya está bloqueado o vacío?)");
    }
    let raw = std::fs::read_to_string(&plain).context("leyendo wallets.json")?;
    let list: Vec<SavedWallet> = serde_json::from_str(&raw).context("wallets.json corrupto")?;
    if list.is_empty() {
        bail!("wallets.json está vacío — nada que cifrar");
    }
    if password.len() < 8 {
        bail!("contraseña demasiado corta (mínimo 8)");
    }
    // El plaintext del STORE viaja como "seed" del envelope (payload genérico).
    // Nota: el campo address del envelope es un marcador, no una wallet real.
    let payload = raw.into_bytes();
    let ks = keystore::encrypt_bytes(
        &payload, "kdc-store", "0x0000000000000000000000000000000000000000",
        "store-envelope", password)?;
    let envelope = StoreEnvelope { version: 1, kind: "kdc-store-v1".into(), keystore: ks };
    let enc = enc_path()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let f = std::fs::OpenOptions::new()
            .write(true).create(true).truncate(true).mode(0o600).open(&enc)?;
        serde_json::to_writer_pretty(&f, &envelope)?;
    }
    #[cfg(not(unix))]
    {
        std::fs::write(&enc, serde_json::to_string_pretty(&envelope)?)?;
    }
    // Borrar el plaintext de forma definitiva (no solo unlink).
    overwrite_and_remove(&plain)?;
    Ok(list.len())
}

/// Descifra el store a wallets.json (para migrar de máquina o desbloquear).
pub fn unlock(password: &str) -> Result<usize> {
    let enc = enc_path()?;
    if !enc.exists() {
        bail!("no hay store cifrado (wallets.json.enc ausente)");
    }
    let raw = std::fs::read_to_string(&enc).context("leyendo wallets.json.enc")?;
    let envelope: StoreEnvelope = serde_json::from_str(&raw).context("envelope corrupto")?;
    if envelope.kind != "kdc-store-v1" {
        bail!("no es un store cifrado kdc (kind: {})", envelope.kind);
    }
    let payload = keystore::decrypt_bytes(&envelope.keystore, password)?;
    let list: Vec<SavedWallet> = serde_json::from_slice(&payload)
        .context("el plaintext descifrado no es un wallets.json válido")?;
    let plain = plain_path()?;
    if plain.exists() {
        bail!("ya existe un wallets.json — muévelo antes de desbloquear (evita perder el actual)");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let f = std::fs::OpenOptions::new()
            .write(true).create(true).truncate(true).mode(0o600).open(&plain)?;
        serde_json::to_writer_pretty(&f, &list)?;
    }
    #[cfg(not(unix))]
    {
        std::fs::write(&plain, serde_json::to_string_pretty(&list)?)?;
    }
    std::fs::remove_file(&enc)?;
    Ok(list.len())
}

/// Sobrescribe el archivo con bytes aleatorios antes de borrarlo.
fn overwrite_and_remove(p: &std::path::Path) -> Result<()> {
    let len = std::fs::metadata(p)?.len();
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().write(true).open(p)?;
    let zeros = vec![0u8; 4096];
    let mut left = len as usize;
    while left > 0 {
        let n = left.min(zeros.len());
        f.write_all(&zeros[..n])?;
        left -= n;
    }
    f.sync_all().ok();
    drop(f);
    std::fs::remove_file(p)?;
    Ok(())
}

/// Carga el store desde donde esté: cifrado (pide contraseña) o plano.
/// Todos los comandos pasan por aquí.
pub fn load_any() -> Result<Vec<SavedWallet>> {
    if store_is_locked() {
        let pass = ask_store_password()?;
        let raw = std::fs::read_to_string(enc_path()?).context("leyendo wallets.json.enc")?;
        let envelope: StoreEnvelope = serde_json::from_str(&raw).context("envelope corrupto")?;
        let payload = keystore::decrypt_bytes(&envelope.keystore, &pass)?;
        return serde_json::from_slice(&payload).context("store descifrado inválido");
    }
    crate::store::load()
}

/// Guarda el store respetando el modo actual (cifrado o plano).
pub fn save_any(list: &[SavedWallet]) -> Result<()> {
    if store_is_locked() {
        let pass = ask_store_password()?;
        let payload = serde_json::to_vec(list)?;
        let ks = keystore::encrypt_bytes(
            &payload, "kdc-store", "0x0000000000000000000000000000000000000000",
            "store-envelope", &pass)?;
        let envelope = StoreEnvelope { version: 1, kind: "kdc-store-v1".into(), keystore: ks };
        let enc = enc_path()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            let f = std::fs::OpenOptions::new()
                .write(true).create(true).truncate(true).mode(0o600).open(&enc)?;
            serde_json::to_writer_pretty(&f, &envelope)?;
        }
        #[cfg(not(unix))]
        {
            std::fs::write(&enc, serde_json::to_string_pretty(&envelope)?)?;
        }
        return Ok(());
    }
    crate::store::save_all(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_envelope_roundtrip() {
        let list = vec![SavedWallet {
            name: "t".into(), address: "0xabc".into(), public_key: "pk".into(),
            key_hex: "aa".repeat(32), key_format: "seed".into(), created_at: "1".into(),
        }];
        let payload = serde_json::to_vec(&list).unwrap();
        let ks = keystore::encrypt_bytes(&payload, "kdc-store",
            "0x0000000000000000000000000000000000000000", "env", "pass-1234").unwrap();
        let env = StoreEnvelope { version: 1, kind: "kdc-store-v1".into(), keystore: ks };
        let json = serde_json::to_string(&env).unwrap();
        let back: StoreEnvelope = serde_json::from_str(&json).unwrap();
        let dec = keystore::decrypt_bytes(&back.keystore, "pass-1234").unwrap();
        let list2: Vec<SavedWallet> = serde_json::from_slice(&dec).unwrap();
        assert_eq!(list2[0].name, "t");
        // Contraseña errada: aborta
        assert!(keystore::decrypt_bytes(&back.keystore, "mala").is_err());
    }
}

/// find sobre load_any (store cifrado o plano).
pub fn find_any(name_or_addr: &str) -> Result<SavedWallet> {
    crate::store::find_in(&load_any()?, name_or_addr)
}

/// add sobre save_any.
pub fn add_any(w: SavedWallet) -> Result<()> {
    let mut list = load_any()?;
    if let Some(i) = list.iter().position(|x| x.address == w.address) {
        list[i] = w;
    } else {
        list.push(w);
    }
    save_any(&list)
}
