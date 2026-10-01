//! Local keystore: wallets saved as {name, address, pubkey, seed} in a
//! 0600 JSON file under the kdc home dir. Testnet tooling — same trust
//! level as the dashboard's dashboard_wallets.json.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedWallet {
    pub name: String,
    pub address: String,
    pub public_key: String,
    /// seed (64 hex) or full FIPS secret (8064 hex)
    pub key_hex: String,
    pub key_format: String,
    pub created_at: String,
}

fn path() -> Result<PathBuf> {
    let home = crate::config::home_dir()?;
    std::fs::create_dir_all(&home)?;
    Ok(home.join("wallets.json"))
}

pub fn load() -> Result<Vec<SavedWallet>> {
    let p = path()?;
    if !p.exists() {
        return Ok(Vec::new());
    }
    let raw = std::fs::read_to_string(&p)?;
    serde_json::from_str(&raw).context("corrupt wallets.json")
}

pub fn save_all(list: &[SavedWallet]) -> Result<()> {
    let p = path()?;
    write_private_json(&p, list)
}

/// Unix: 0600 file mode for key material.
#[cfg(unix)]
fn write_private_json(p: &std::path::Path, list: &[SavedWallet]) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(p)?;
    serde_json::to_writer_pretty(&f, list)?;
    Ok(())
}

/// Non-Unix (Windows): ACLs inherit from %APPDATA% (user-private).
#[cfg(not(unix))]
fn write_private_json(p: &std::path::Path, list: &[SavedWallet]) -> Result<()> {
    let raw = serde_json::to_string_pretty(list)?;
    std::fs::write(p, raw)?;
    Ok(())
}

pub fn add(w: SavedWallet) -> Result<()> {
    let mut list = load()?;
    if let Some(i) = list.iter().position(|x| x.address == w.address) {
        list[i] = w;
    } else {
        list.push(w);
    }
    save_all(&list)
}

pub fn find_in(list: &[SavedWallet], name_or_addr: &str) -> Result<SavedWallet> {
    let needle = name_or_addr.to_lowercase();
    for w in list {
        if w.name == name_or_addr || w.address.to_lowercase() == needle {
            return Ok(w.clone());
        }
    }
    anyhow::bail!("wallet '{}' no encontrada en el keystore", name_or_addr)
}

pub fn find(name_or_addr: &str) -> Result<SavedWallet> {
    find_in(&load()?, name_or_addr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keystore_roundtrip() {
        std::env::set_var("KDC_HOME", "/tmp/kdc-test-store");
        let _ = std::fs::remove_dir_all("/tmp/kdc-test-store");
        let w = SavedWallet {
            name: "t".into(),
            address: "0xabc".into(),
            public_key: "00".into(),
            key_hex: "ff".into(),
            key_format: "seed".into(),
            created_at: "now".into(),
        };
        add(w).unwrap();
        assert_eq!(find("t").unwrap().address, "0xabc");
        assert_eq!(find("0xABC").unwrap().name, "t");
        assert!(find("nope").is_err());
        let _ = std::fs::remove_dir_all("/tmp/kdc-test-store");
        std::env::remove_var("KDC_HOME");
    }
}
