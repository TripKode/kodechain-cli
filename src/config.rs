//! Configuration resolution: flags > env > defaults.
//! Env vars: KODECHAIN_ENGINE_DIR, KODECHAIN_BOOT_URL, KODECHAIN_DPOS_URL,
//! KODECHAIN_PBFT_URL, KODECHAIN_BEACON_INTERVAL, KODECHAIN_BIDIR_SYNC.

use anyhow::{bail, Result};
use std::path::PathBuf;

pub const DEFAULT_BOOT_URL: &str = "http://localhost:8084";
pub const DEFAULT_DPOS_URL: &str = "http://localhost:8082";
pub const DEFAULT_PBFT_URL: &str = "http://localhost:8081";

/// Resolve the engine checkout dir: explicit flag, env, CWD detection,
/// `./kodechain-engine` fallback — with a clear error otherwise.
pub fn resolve_engine_dir(flag: Option<&str>) -> Result<PathBuf> {
    if let Some(f) = flag {
        let p = PathBuf::from(f);
        if p.join("kodechain-node-validator").exists() {
            return Ok(p);
        }
        bail!("--engine-dir {f} has no kodechain-node-validator binary");
    }
    if let Ok(e) = std::env::var("KODECHAIN_ENGINE_DIR") {
        let p = PathBuf::from(&e);
        if p.join("kodechain-node-validator").exists() {
            return Ok(p);
        }
        bail!("KODECHAIN_ENGINE_DIR={e} has no kodechain-node-validator binary");
    }
    let cwd = std::env::current_dir()?;
    let mut candidates = vec![cwd.clone(), cwd.join("kodechain-engine")];
    if let Some(parent) = cwd.parent() {
        candidates.push(parent.join("kodechain-engine"));
    }
    for cand in &candidates {
        if cand.join("kodechain-node-validator").exists() {
            return Ok(cand.clone());
        }
    }
    bail!(
        "engine not found (looked in {}). Pass --engine-dir or set KODECHAIN_ENGINE_DIR",
        candidates.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")
    );
}

pub fn node_url(kind: &str, flag: Option<&str>) -> String {
    if let Some(f) = flag {
        return f.to_string();
    }
    let env = match kind {
        "boot" => std::env::var("KODECHAIN_BOOT_URL").ok(),
        "dpos" => std::env::var("KODECHAIN_DPOS_URL").ok(),
        "pbft" => std::env::var("KODECHAIN_PBFT_URL").ok(),
        _ => None,
    };
    env.unwrap_or_else(|| {
        match kind {
            "dpos" => DEFAULT_DPOS_URL,
            "pbft" => DEFAULT_PBFT_URL,
            _ => DEFAULT_BOOT_URL,
        }
        .to_string()
    })
}

/// Keystore dir: ~/.local/share/kdc (via dirs) or $KDC_HOME.
pub fn home_dir() -> Result<PathBuf> {
    if let Ok(h) = std::env::var("KDC_HOME") {
        return Ok(PathBuf::from(h));
    }
    match dirs::data_dir() {
        Some(d) => Ok(d.join("kdc")),
        None => bail!("cannot determine data dir (set KDC_HOME)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_overrides_defaults() {
        std::env::set_var("KODECHAIN_BOOT_URL", "http://example:9999");
        assert_eq!(node_url("boot", None), "http://example:9999");
        std::env::remove_var("KODECHAIN_BOOT_URL");
        assert_eq!(node_url("boot", None), DEFAULT_BOOT_URL);
        assert_eq!(node_url("dpos", Some("http://x:1")), "http://x:1");
    }
}
