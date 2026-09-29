//! `kdc setup` / `kdc doctor`: leave the engine binary ready, like eth's
//! prebuilt-binary flow (download → fallback source build → verify).
//!
//! Hosting model (same as geth/nethermind): prebuilt binaries live in
//! GitHub Releases (`.../releases/download/<tag>/kodechain-node-validator-<os>-<arch>`);
//! this command resolves them, or builds from source when Go is available.

use anyhow::{Context, Result};

/// Default release URL pattern (override via --from-url or
/// KODECHAIN_ENGINE_RELEASE_URL).
pub fn default_release_url(version: &str) -> String {
    std::env::var("KODECHAIN_ENGINE_RELEASE_URL").unwrap_or_else(|_| {
        let (os, arch) = if cfg!(target_os = "windows") {
            ("windows", "amd64")
        } else if cfg!(target_arch = "aarch64") {
            ("linux", "arm64")
        } else {
            ("linux", "amd64")
        };
        format!(
            "https://github.com/TripKode/kodechain-engine/releases/download/{version}/kodechain-node-validator-{os}-{arch}"
        )
    })
}

pub fn binary_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "kodechain-node-validator.exe"
    } else {
        "kodechain-node-validator"
    }
}

/// Query `<bin> -version` (exit code must be 0).
pub async fn binary_version(bin: &std::path::Path) -> Result<String> {
    let out = tokio::process::Command::new(bin)
        .arg("-version")
        .output()
        .await
        .context("executing -version")?;
    if !out.status.success() {
        anyhow::bail!("-version failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Download a prebuilt binary to `dest` (HTTP/HTTPS only).
pub async fn download(url: &str, dest: &std::path::Path) -> Result<u64> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        anyhow::bail!("refusing non-HTTP(S) URL: {url}");
    }
    let resp = reqwest::get(url).await.context("downloading release")?;
    if !resp.status().is_success() {
        anyhow::bail!("release server HTTP {} for {url} (not published yet? use --build)", resp.status());
    }
    let bytes = resp.bytes().await?;
    tokio::fs::write(dest, &bytes).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut p = tokio::fs::metadata(dest).await?.permissions();
        p.set_mode(0o755);
        tokio::fs::set_permissions(dest, p).await?;
    }
    Ok(bytes.len() as u64)
}

/// Build from source dir (needs Go toolchain).
pub async fn build_from_source(src_dir: &std::path::Path, dest: &std::path::Path) -> Result<()> {
    if !src_dir.join("go.mod").exists() {
        anyhow::bail!("{} is not an engine checkout (no go.mod)", src_dir.display());
    }
    let st = tokio::process::Command::new("go")
        .args(["build", "-o"])
        .arg(dest)
        .arg(".")
        .current_dir(src_dir)
        .status()
        .await
        .context("go toolchain missing? install from https://go.dev/dl")?;
    if !st.success() {
        anyhow::bail!("go build failed");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_url_pattern() {
        std::env::remove_var("KODECHAIN_ENGINE_RELEASE_URL");
        let u = default_release_url("v2.0.0");
        assert!(u.contains("v2.0.0") && u.contains("kodechain-node-validator-"));
        assert!(u.starts_with("https://"));
    }

    #[test]
    fn refuses_non_http() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let r = rt.block_on(download("ftp://x/y", std::path::Path::new("/tmp/kdc-nope")));
        assert!(r.is_err());
    }
}
