//! Node lifecycle: start/stop/status/logs for bootstrap + validators.
//!
//! Networks:
//! - testnet: all 3 roles (bootstrap aggregates both chains). Wallets come
//!   from the genesis accounts file.
//! - mainnet: the protocol runs a SINGLE bootstrap (used to boot the network
//!   initially), so kdc only deploys dpos|pbft VALIDATORS there. Operator
//!   provides --node-id and --bootstrap-nodes; data dirs are ./data-mainnet-*.
//!
//! A JSON registry (~/.local/share/kdc/nodes.json) tracks managed nodes
//! (network, mode, data dir, port) so stop/status/logs find custom-dir
//! nodes. Pidfiles live next to data-dirs (stale pids impossible: a reboot
//! wipes neither, but process_alive gates everything).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Bootstrap,
    Dpos,
    Pbft,
}

impl Mode {
    pub fn all() -> [Mode; 3] {
        [Mode::Bootstrap, Mode::Dpos, Mode::Pbft]
    }
    pub fn parse(s: &str) -> Result<Mode> {
        match s {
            "bootstrap" => Ok(Mode::Bootstrap),
            "dpos" | "dpos01" => Ok(Mode::Dpos),
            "pbft" | "pbft01" => Ok(Mode::Pbft),
            _ => anyhow::bail!("mode must be all|bootstrap|dpos|pbft"),
        }
    }
    pub fn name(&self) -> &'static str {
        match self {
            Mode::Bootstrap => "bootstrap",
            Mode::Dpos => "dpos01",
            Mode::Pbft => "pbft01",
        }
    }
    pub fn http_port(&self) -> u16 {
        match self {
            Mode::Bootstrap => 8084,
            Mode::Dpos => 8082,
            Mode::Pbft => 8081,
        }
    }
    pub fn p2p_port(&self) -> u16 {
        match self {
            Mode::Bootstrap => 30403,
            Mode::Dpos => 30404,
            Mode::Pbft => 30505,
        }
    }
    pub fn log_file(&self) -> &'static str {
        match self {
            Mode::Bootstrap => "logs/bootstrap.log",
            Mode::Dpos => "logs/dpos01.log",
            Mode::Pbft => "logs/pbft01.log",
        }
    }
}

/// Start parameters: flags > network conventions.
pub struct StartParams {
    pub network: String,
    pub node_id: Option<String>,
    pub validator_address: Option<String>,
    pub bootstrap_nodes: Option<String>,
    pub port: Option<u16>,
    pub p2p_port: Option<u16>,
    pub udp_port: Option<u16>,
    pub data_dir: Option<PathBuf>,
    pub genesis_dpos: Option<String>,
    pub genesis_pbft: Option<String>,
}

/// Pure offline validation: mainnet deploys validators only (the protocol
/// runs a single bootstrap there, used to boot the network initially).
pub fn validate_start_mode(network: &str, mode: &str) -> Result<()> {
    match mode {
        "all" | "bootstrap" | "dpos" | "pbft" => {}
        _ => anyhow::bail!("mode must be all|bootstrap|dpos|pbft"),
    }
    if network == "mainnet" && (mode == "bootstrap" || mode == "all") {
        anyhow::bail!(
            "mainnet runs a SINGLE protocol bootstrap: kdc only deploys dpos|pbft \
             validators there (pass --mode dpos|pbft with --node-id and --bootstrap-nodes)"
        );
    }
    Ok(())
}

pub struct NodeSet {
    pub engine: PathBuf,
    pub network: String,
    /// Testnet genesis wallets (None on mainnet — operator provides ids).
    pub faucet_key: Option<String>,
    pub boot_id: Option<String>,
    pub dpos_id: Option<String>,
    pub pbft_id: Option<String>,
    pub lan_ip: String,
    pub beacon_interval: String,
    pub bidir_sync: String,
}

impl NodeSet {
    pub fn load(engine: PathBuf, network: &str) -> Result<Self> {
        let (faucet_key, boot_id, dpos_id, pbft_id) = if network == "mainnet" {
            (None, None, None, None)
        } else {
            let txt = std::fs::read_to_string(engine.join("archives/genesis_testnet_accounts.txt"))
                .context("cannot read archives/genesis_testnet_accounts.txt (regenesis first?)")?;
            let wallets = crate::genesis::parse_accounts(&txt)?;
            let get = |n: &str| crate::genesis::find_wallet(&wallets, n).cloned();
            let faucet = get("Testnet_Faucet")?;
            (
                Some(faucet.private_key.clone()),
                Some(faucet.address.clone()),
                Some(get("Testnet_Validator_01")?.address.clone()),
                Some(get("Testnet_PBFT_Validator_01")?.address.clone()),
            )
        };
        Ok(Self {
            engine,
            network: network.to_string(),
            faucet_key,
            boot_id,
            dpos_id,
            pbft_id,
            lan_ip: local_ip(),
            beacon_interval: std::env::var("KODECHAIN_BEACON_INTERVAL").unwrap_or_else(|_| "5".into()),
            bidir_sync: std::env::var("KODECHAIN_BIDIR_SYNC").unwrap_or_else(|_| "30s".into()),
        })
    }

    fn default_data_dir(&self, mode: Mode) -> PathBuf {
        let leaf = match (self.network.as_str(), mode) {
            (_, Mode::Bootstrap) => "archive-testnet/data-testnet-bootstrap",
            ("mainnet", Mode::Dpos) => "data-mainnet-dpos01",
            ("mainnet", Mode::Pbft) => "data-mainnet-pbft01",
            (_, Mode::Dpos) => "archive-testnet/data-testnet-dpos01",
            (_, Mode::Pbft) => "archive-testnet/data-testnet-pbft01",
        };
        self.engine.join(leaf)
    }

    fn pid_path(data_dir: &PathBuf) -> PathBuf {
        data_dir.join("kdc.pid")
    }

    /// Resolve the node identity: flag > testnet genesis > (mainnet: required).
    fn resolve_id(&self, mode: Mode, p: &StartParams) -> Result<String> {
        if let Some(id) = &p.node_id {
            return Ok(id.clone());
        }
        let genesis = match mode {
            Mode::Bootstrap => &self.boot_id,
            Mode::Dpos => &self.dpos_id,
            Mode::Pbft => &self.pbft_id,
        };
        genesis.clone().ok_or_else(|| {
            anyhow::anyhow!("--node-id is required on mainnet (your validator wallet address)")
        })
    }

    /// Start one node; waits until /api/node/health is 200.
    pub async fn start(&self, mode: Mode, p: &StartParams) -> Result<u32> {
        let data_dir = p.data_dir.clone().unwrap_or_else(|| self.default_data_dir(mode));
        let pidf = Self::pid_path(&data_dir);
        if let Ok(pid) = std::fs::read_to_string(&pidf).unwrap_or_default().trim().parse::<u32>() {
            if process_alive(pid) {
                anyhow::bail!("{} already running (pid {pid})", mode.name());
            }
        }
        let port = p.port.unwrap_or_else(|| mode.http_port());
        // Pre-flight: refuse to start over a port that already answers —
        // otherwise we'd report success for a STALE node we don't own while
        // our child dies on bind conflict (seen live).
        if port_answers(port).await {
            anyhow::bail!(
                "{} port :{port} already serves a node NOT managed by kdc (no pidfile). \
                 Stop it first, or free the port and retry",
                mode.name()
            );
        }
        let node_id = self.resolve_id(mode, p)?;
        let validator_addr = p.validator_address.clone().unwrap_or_else(|| node_id.clone());
        let bin = self.engine.join("kodechain-node-validator");
        if !bin.exists() {
            anyhow::bail!("missing {} (build the engine first)", bin.display());
        }
        let mut args: Vec<String> = vec![
            "--network".into(),
            self.network.clone(),
            "-node_id".into(),
            node_id,
            "-port".into(),
            port.to_string(),
            "-data_dir".into(),
            data_dir.display().to_string(),
        ];
        if let Some(g) = &p.genesis_dpos {
            args.push("-genesis-dpos".into());
            args.push(g.clone());
        }
        if let Some(g) = &p.genesis_pbft {
            args.push("-genesis-pbft".into());
            args.push(g.clone());
        }
        let mut envs: Vec<(String, String)> = vec![
            ("BEACON_BLOCK_INTERVAL".into(), self.beacon_interval.clone()),
            ("KODECHAIN_BIDIR_SYNC".into(), self.bidir_sync.clone()),
        ];
        match mode {
            Mode::Bootstrap => {
                args.push("-bootstrap".into());
                let fk = self.faucet_key.clone().ok_or_else(|| {
                    anyhow::anyhow!("bootstrap needs the faucet key (testnet genesis only)")
                })?;
                envs.push(("FAUCET_PRIVATE_KEY".into(), fk));
            }
            Mode::Dpos | Mode::Pbft => {
                let (ct, dflt_p2p) = match mode {
                    Mode::Dpos => ("DPOS", 30404),
                    _ => ("PBFT", 30505),
                };
                let p2p = p.p2p_port.unwrap_or(dflt_p2p);
                let udp = p.udp_port.unwrap_or(p2p);
                let boot_nodes = match &p.bootstrap_nodes {
                    Some(b) => b.clone(),
                    None => match &self.boot_id {
                        Some(id) => format!("enode://{id}@{}:30403?http_port=8084", self.lan_ip),
                        None => std::env::var("KODECHAIN_MAINNET_BOOTSTRAP").map_err(|_| {
                            anyhow::anyhow!(
                                "mainnet needs --bootstrap-nodes (or KODECHAIN_MAINNET_BOOTSTRAP): \
                                 enode://<bootstrap-id>@<ip>:<p2p-port>?http_port=<http-port>"
                            )
                        })?,
                    },
                };
                args.extend([
                    "-consensus_type".into(),
                    ct.into(),
                    "-bootstrap-nodes".into(),
                    boot_nodes,
                    "-p2p_port".into(),
                    p2p.to_string(),
                    "-udp_port".into(),
                    udp.to_string(),
                ]);
                envs.push(("VALIDATOR_ADDRESS".into(), validator_addr));
            }
        }

        let log_path = self.engine.join(mode.log_file());
        if let Some(parent) = log_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::create_dir_all(&data_dir)?;
        let log = std::fs::OpenOptions::new().create(true).append(true).open(&log_path)?;
        let log2 = log.try_clone()?;

        let child = Command::new(&bin)
            .args(&args)
            .envs(envs)
            .current_dir(&self.engine)
            .stdin(Stdio::null())
            .stdout(log)
            .stderr(log2)
            .spawn()
            .context("spawning node")?;
        let pid = child.id().ok_or_else(|| anyhow::anyhow!("no pid"))?;
        std::fs::write(&pidf, pid.to_string())?;
        // Detach: forget the handle, the pidfile owns the lifecycle now.
        std::mem::forget(child);

        // Wait for health (up to 60s), verifying OUR child owns it.
        let client = reqwest::Client::new();
        let url = format!("http://localhost:{port}/api/node/health");
        for _ in 0..60 {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            if !process_alive(pid) {
                anyhow::bail!("{} died during startup (see {})", mode.name(), mode.log_file());
            }
            if let Ok(r) = client.get(&url).send().await {
                if r.status().is_success() {
                    // Our pid is alive AND the port answers: confirm the
                    // pidfile still points at our child (no takeover).
                    let cur: Option<u32> = std::fs::read_to_string(&pidf).unwrap_or_default().trim().parse().ok();
                    if cur == Some(pid) {
                        register_node(&mode, &self.network, &data_dir, port)?;
                        return Ok(pid);
                    }
                    anyhow::bail!("pidfile changed during startup — aborting");
                }
            }
        }
        anyhow::bail!("{} unhealthy after 60s (see {})", mode.name(), mode.log_file())
    }

    /// Stop one node via its pidfile (registry-aware for custom dirs).
    pub fn stop(&self, mode: Mode, data_dir: Option<PathBuf>) -> Result<bool> {
        let pidf = match data_dir {
            Some(d) => Self::pid_path(&d),
            None => registry_pidfile(&self.network, mode)?.unwrap_or_else(|| Self::pid_path(&self.default_data_dir(mode))),
        };
        unregister_node(&self.network, mode, &pidf);
        match std::fs::read_to_string(&pidf).unwrap_or_default().trim().parse::<u32>() {
            Ok(pid) if process_alive(pid) => {
                libc_kill(pid);
                std::fs::remove_file(&pidf).ok();
                Ok(true)
            }
            _ => {
                std::fs::remove_file(&pidf).ok();
                Ok(false)
            }
        }
    }

    /// Pidfile for status checks (registry-aware).
    pub fn pid(&self, mode: Mode) -> Option<u32> {
        let pidf = registry_pidfile(&self.network, mode)
            .unwrap_or_default()
            .unwrap_or_else(|| Self::pid_path(&self.default_data_dir(mode)));
        std::fs::read_to_string(pidf).unwrap_or_default().trim().parse().ok()
    }

    pub fn log_tail(&self, mode: Mode, lines: usize, filter: Option<&str>) -> Result<Vec<String>> {
        let raw = std::fs::read(self.engine.join(mode.log_file())).unwrap_or_default();
        let text = String::from_utf8_lossy(&raw).replace('\0', "");
        let mut v: Vec<String> = text
            .lines()
            .filter(|l| match filter {
                Some(f) => l.to_lowercase().contains(&f.to_lowercase()),
                None => true,
            })
            .map(|s| s.to_string())
            .collect();
        if v.len() > lines {
            v = v[v.len() - lines..].to_vec();
        }
        Ok(v)
    }
}

pub fn process_alive_pub(pid: u32) -> bool {
    process_alive(pid)
}

fn process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        return std::path::Path::new(&format!("/proc/{pid}")).exists();
    }
    #[cfg(windows)]
    {
        return tasklist_has(pid);
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        return false;
    }
}

/// Windows: exact PID lookup via `tasklist /FO CSV` (no new deps).
#[cfg(windows)]
fn tasklist_has(pid: u32) -> bool {
    let out = std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .output();
    match out {
        Ok(o) => {
            let text = String::from_utf8_lossy(&o.stdout);
            text.lines().any(|l| l.split(',').nth(1).map(|c| c.trim_matches('"') == pid.to_string()).unwrap_or(false))
        }
        Err(_) => false,
    }
}

#[cfg(unix)]
fn libc_kill(pid: u32) {
    unsafe {
        libc::kill(pid as i32, libc::SIGTERM);
    }
}

/// Windows: force-kill via taskkill (no new deps).
#[cfg(windows)]
fn libc_kill(pid: u32) {
    let _ = std::process::Command::new("taskkill")
        .args(["/F", "/PID", &pid.to_string()])
        .output();
}

#[cfg(not(any(unix, windows)))]
fn libc_kill(_pid: u32) {}

/// True if something already answers health on the port (stale node guard).
async fn port_answers(port: u16) -> bool {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build();
    let Ok(client) = client else { return false };
    matches!(
        client
            .get(format!("http://localhost:{port}/api/node/health"))
            .send()
            .await
            .map(|r| r.status().is_success()),
        Ok(true)
    )
}

/// LAN IP via routing lookup (no packets sent): UDP "connect" only resolves
/// the source address the kernel would use toward the internet.
pub fn local_ip() -> String {
    if let Ok(ip) = std::env::var("KDC_LAN_IP") {
        return ip;
    }
    let sock = std::net::UdpSocket::bind("0.0.0.0:0").expect("udp bind");
    if sock.connect("8.8.8.8:80").is_ok() {
        if let Ok(addr) = sock.local_addr() {
            return addr.ip().to_string();
        }
    }
    "127.0.0.1".to_string()
}

// ---- managed-node registry (~/.local/share/kdc/nodes.json) ----

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RegistryEntry {
    network: String,
    mode: String,
    data_dir: String,
    http_port: u16,
}

fn registry_path() -> Result<PathBuf> {
    let home = crate::config::home_dir()?;
    std::fs::create_dir_all(&home)?;
    Ok(home.join("nodes.json"))
}

fn load_registry() -> Vec<RegistryEntry> {
    let Ok(p) = registry_path() else { return Vec::new() };
    std::fs::read_to_string(p)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn register_node(mode: &Mode, network: &str, data_dir: &PathBuf, port: u16) -> Result<()> {
    let mut reg = load_registry();
    reg.retain(|e| !(e.network == network && e.mode == mode.name()));
    reg.push(RegistryEntry {
        network: network.to_string(),
        mode: mode.name().to_string(),
        data_dir: data_dir.display().to_string(),
        http_port: port,
    });
    std::fs::write(registry_path()?, serde_json::to_string_pretty(&reg)?)?;
    Ok(())
}

fn unregister_node(network: &str, mode: Mode, _pidf: &PathBuf) {
    if let Ok(reg) = (|| -> Result<Vec<RegistryEntry>> {
        let mut reg = load_registry();
        reg.retain(|e| !(e.network == network && e.mode == mode.name()));
        std::fs::write(registry_path()?, serde_json::to_string_pretty(&reg)?)?;
        Ok(reg)
    })() {
        let _ = reg;
    }
}

fn registry_pidfile(network: &str, mode: Mode) -> Result<Option<PathBuf>> {
    for e in load_registry() {
        if e.network == network && e.mode == mode.name() {
            return Ok(Some(PathBuf::from(e.data_dir).join("kdc.pid")));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_ports() {
        assert_eq!(Mode::Bootstrap.http_port(), 8084);
        assert_eq!(Mode::Dpos.http_port(), 8082);
        assert_eq!(Mode::Pbft.http_port(), 8081);
        assert_eq!(Mode::Bootstrap.p2p_port(), 30403);
        assert_eq!(Mode::Dpos.p2p_port(), 30404);
        assert_eq!(Mode::Pbft.p2p_port(), 30505);
    }

    #[test]
    fn mainnet_rejects_bootstrap() {
        assert!(validate_start_mode("mainnet", "bootstrap").is_err());
        assert!(validate_start_mode("mainnet", "all").is_err());
        assert!(validate_start_mode("mainnet", "dpos").is_ok());
        assert!(validate_start_mode("mainnet", "pbft").is_ok());
        assert!(validate_start_mode("testnet", "all").is_ok());
        assert!(validate_start_mode("testnet", "bootstrap").is_ok());
        assert!(validate_start_mode("x", "dpos").is_ok());
    }

    #[test]
    fn local_ip_is_valid() {
        let ip: std::net::IpAddr = local_ip().parse().expect("valid ip");
        assert!(!ip.is_unspecified());
    }

    #[tokio::test]
    async fn port_answers_detects_occupant() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        assert!(!port_answers(port).await);
    }
}
