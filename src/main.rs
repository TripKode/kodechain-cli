//! kdc — KodeChain testnet CLI.
//! Run `kdc manual` for the built-in manual, `kdc <cmd> --help` per command.

use kodechain_cli::amount;
use kodechain_cli::client;
use kodechain_cli::compose;
use kodechain_cli::config;
use kodechain_cli::genesis;
use kodechain_cli::manual;
use kodechain_cli::node;
use kodechain_cli::output;
use kodechain_cli::setup;
use kodechain_cli::store;
use kodechain_cli::wallet_crypto;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use client::Engine;

#[derive(Parser)]
#[command(
    name = "kdc",
    version,
    about = "KodeChain testnet CLI — nodes, wallets, transactions (post-quantum)"
)]
struct Cli {
    /// Output raw JSON instead of tables
    #[arg(long, global = true)]
    json: bool,
    /// Engine checkout dir (default: auto-detect)
    #[arg(long, global = true)]
    engine_dir: Option<String>,
    /// Bootstrap node URL override
    #[arg(long, global = true)]
    boot_url: Option<String>,
    /// Network: testnet (3 roles) or mainnet (validators only — the
    /// protocol runs a single bootstrap there)
    #[arg(long, global = true, default_value = "testnet", env = "KODECHAIN_NETWORK")]
    network: String,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Start/stop/inspect testnet nodes
    Node {
        #[command(subcommand)]
        op: NodeOp,
    },
    /// ML-DSA-65 wallets (create, import, balances, history)
    Wallet {
        #[command(subcommand)]
        op: WalletOp,
    },
    /// Request 1000 test KDC for an address
    Faucet {
        /// Destination address (or keystore name)
        address: String,
    },
    /// Transfer KDC (mined by DPOS consensus)
    Transfer {
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
        /// Decimal KDC (exact, e.g. 1.234567)
        #[arg(long, conflicts_with = "proton")]
        kdc: Option<String>,
        /// Raw proton (legacy)
        #[arg(long)]
        proton: Option<String>,
    },
    /// Anchor a critical record (mined by PBFT, instant finality)
    Critical {
        #[arg(long)]
        from: String,
        #[arg(long)]
        record: String,
        #[arg(long, default_value = "")]
        detail: String,
    },
    /// Read both consensus chains
    Chain {
        #[command(subcommand)]
        op: ChainOp,
    },
    /// Transaction status (mined block or mempool)
    Tx {
        hash: String,
        /// Keep polling until mined
        #[arg(long)]
        watch: bool,
    },
    /// Pending transactions of both mempools
    Mempool,
    /// Validators with on-chain stake
    Validators,
    /// Dynamic network view (nodes + health + P2P peers)
    Network,
    /// Built-in manual
    Manual { topic: Option<String> },
    /// Render a docker-compose.yml for N validators (no fixed topology)
    Compose {
        /// DPOS validators to render
        #[arg(long, default_value_t = 1)]
        dpos: usize,
        /// PBFT validators to render
        #[arg(long, default_value_t = 1)]
        pbft: usize,
        /// DPOS wallet addresses (comma-separated; generated + saved if fewer)
        #[arg(long, default_value = "")]
        dpos_ids: String,
        /// PBFT wallet addresses (comma-separated; generated + saved if fewer)
        #[arg(long, default_value = "")]
        pbft_ids: String,
        /// Bootstrap enode (mainnet required; testnet auto-built)
        #[arg(long)]
        bootstrap_nodes: Option<String>,
        /// Output file
        #[arg(long, default_value = "docker-compose.yml")]
        output: String,
        /// Engine checkout used as compose build context
        #[arg(long, default_value = ".")]
        context: String,
    },
    /// Show resolved configuration
    Config,
    /// Install the engine binary (download prebuilt → fallback source build)
    Setup {
        /// Engine release tag (default: latest known)
        #[arg(long, default_value = "latest")]
        version: String,
        /// Explicit release URL (or KODECHAIN_ENGINE_RELEASE_URL)
        #[arg(long)]
        from_url: Option<String>,
        /// Force build from source instead of downloading
        #[arg(long)]
        build: bool,
        /// Destination dir for the binary (default: engine dir)
        #[arg(long)]
        dest: Option<String>,
    },
    /// Environment check: binary, ports, genesis, data dirs, nodes
    Doctor,
}

#[derive(Subcommand)]
enum NodeOp {
    /// Start nodes (mode: all|bootstrap|dpos|pbft;
    /// on mainnet only dpos|pbft — the protocol runs a single bootstrap)
    Start {
        #[arg(long, default_value = "all")]
        mode: String,
        /// Validator wallet address (node identity). Testnet: from genesis
        /// file. Mainnet: REQUIRED (your own wallet).
        #[arg(long)]
        node_id: Option<String>,
        /// Staking wallet (defaults to --node-id)
        #[arg(long)]
        validator_address: Option<String>,
        /// Bootstrap enode(s), comma-separated. Mainnet: REQUIRED
        /// (env KODECHAIN_MAINNET_BOOTSTRAP). Testnet: auto-built.
        #[arg(long)]
        bootstrap_nodes: Option<String>,
        /// HTTP port override (default per role: 8084/8082/8081)
        #[arg(long)]
        port: Option<u16>,
        /// P2P port override (validators default 30404/30505)
        #[arg(long)]
        p2p_port: Option<u16>,
        /// UDP port override (defaults to P2P port)
        #[arg(long)]
        udp_port: Option<u16>,
        /// Data dir override (defaults: archive-testnet/... or data-mainnet-...)
        #[arg(long)]
        data_dir: Option<String>,
    },
    /// Stop nodes
    Stop {
        #[arg(long, default_value = "all")]
        mode: String,
    },
    /// Health + heights + stake locks
    Status,
    /// Tail node logs
    Logs {
        #[arg(long, default_value = "bootstrap")]
        node: String,
        #[arg(long, default_value_t = 100)]
        lines: usize,
        #[arg(long)]
        filter: Option<String>,
    },
}

#[derive(Subcommand)]
enum WalletOp {
    /// Generate a fresh ML-DSA-65 wallet
    New {
        #[arg(long, default_value = "")]
        name: String,
        /// Save into the local keystore
        #[arg(long, default_value_t = true)]
        save: bool,
    },
    /// Import from 64-hex seed or 8064-hex FIPS secret
    Import {
        #[arg(long)]
        key: String,
        #[arg(long, default_value = "")]
        name: String,
    },
    /// List keystore wallets
    List,
    /// KDC + KDC_STAKED balances
    Balance { address: String },
    /// Derive address from a key (verify ownership)
    Address {
        #[arg(long)]
        key: String,
    },
    /// Sign a message with a wallet key
    Sign {
        #[arg(long)]
        key: String,
        #[arg(long)]
        message: String,
    },
    /// Movements of an address on both chains
    History { address: String },
}

#[derive(Subcommand)]
enum ChainOp {
    /// Block heights (lag <= 1 is healthy)
    Height {
        #[arg(long, default_value = "all")]
        chain: String,
    },
    /// List blocks (paginated)
    Blocks {
        #[arg(long, default_value = "dpos")]
        chain: String,
        #[arg(long, default_value_t = 20)]
        limit: u32,
        #[arg(long, default_value_t = 0)]
        offset: u64,
    },
    /// Full block detail (by height or 0x hash)
    Block {
        id: String,
        #[arg(long, default_value = "dpos")]
        chain: String,
    },
}

fn engine(cli: &Cli) -> Result<Engine> {
    Ok(Engine::new(&config::node_url(
        "boot",
        cli.boot_url.as_deref(),
    )))
}

fn resolve_addr(s: &str) -> String {
    match store::find(s) {
        Ok(w) => w.address,
        Err(_) => s.to_string(),
    }
}

fn short(s: &str, n: usize) -> String {
    if s.len() > n {
        s[..n].to_string()
    } else {
        s.to_string()
    }
}

fn kdc_fmt(proton: &str) -> String {
    amount::proton_to_kdc(proton)
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.network.as_str() {
        "testnet" | "mainnet" => {}
        other => anyhow::bail!("--network must be testnet|mainnet, got '{other}'"),
    }
    let out_json = cli.json;
    let j = |v: &serde_json::Value| {
        if out_json {
            output::json(v);
            true
        } else {
            false
        }
    };

    match &cli.cmd {
        Cmd::Manual { topic } => manual::show(topic.as_deref()),
        Cmd::Compose { dpos, pbft, dpos_ids, pbft_ids, bootstrap_nodes, output, context } => {
            async fn resolve_ids(prefix: &str, want: usize, given: &str) -> Result<Vec<String>> {
                let mut ids: Vec<String> = given
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                ids = ids
                    .into_iter()
                    .map(|x| match store::find(&x) {
                        Ok(w) => w.address,
                        Err(_) => x,
                    })
                    .collect();
                let mut n = 0;
                while ids.len() < want {
                    n += 1;
                    let seed = wallet_crypto::random_seed();
                    let addr = wallet_crypto::address_for_seed(&seed);
                    let name = format!("{prefix}-{n}");
                    if store::find(&name).is_ok() {
                        continue;
                    }
                    store::add(store::SavedWallet {
                        name: name.clone(),
                        address: addr.clone(),
                        public_key: hex::encode(wallet_crypto::public_bytes(&seed)),
                        key_hex: hex::encode(seed),
                        key_format: "seed".into(),
                        created_at: chrono_now(),
                    })?;
                    output::info(&format!("generated wallet {name} ({addr}) — fund + register it"));
                    ids.push(addr);
                }
                Ok(ids)
            }
            let dpos_ids = resolve_ids("validator-dpos", *dpos, dpos_ids).await?;
            let pbft_ids = resolve_ids("validator-pbft", *pbft, pbft_ids).await?;
            let spec = compose::ComposeSpec {
                network: cli.network.clone(),
                dpos_ids,
                pbft_ids,
                bootstrap_nodes: bootstrap_nodes.clone().or_else(|| std::env::var("KODECHAIN_MAINNET_BOOTSTRAP").ok()),
                context: context.clone(),
                beacon_interval: std::env::var("KODECHAIN_BEACON_INTERVAL").unwrap_or_else(|_| "5".into()),
                bidir_sync: std::env::var("KODECHAIN_BIDIR_SYNC").unwrap_or_else(|_| "30s".into()),
            };
            let boot_id = if cli.network == "mainnet" {
                None
            } else {
                let engine_dir = config::resolve_engine_dir(cli.engine_dir.as_deref())?;
                let txt = std::fs::read_to_string(engine_dir.join("archives/genesis_testnet_accounts.txt"))?;
                let wallets = genesis::parse_accounts(&txt)?;
                Some(genesis::find_wallet(&wallets, "Testnet_Faucet")?.address.clone())
            };
            let yml = compose::render(&spec, boot_id.as_deref())?;
            std::fs::write(output, &yml)?;
            if !j(&serde_json::json!({"compose": output})) {
                output::ok(&format!("wrote {output} ({dpos} dpos + {pbft} pbft)"));
                output::info("next: docker compose up -d --build  |  then register validators (ritual)");
            }
        }
        Cmd::Setup { version, from_url, build, dest } => {
            let engine_dir = config::resolve_engine_dir(cli.engine_dir.as_deref()).or_else(|_| {
                // setup must work even without an engine checkout (download path)
                Ok::<_, anyhow::Error>(std::env::current_dir()?)
            })?;
            let dest = dest.clone().map(std::path::PathBuf::from).unwrap_or_else(|| engine_dir.join(setup::binary_name()));
            if dest.exists() && !build {
                // already present: verify it answers -version
                match setup::binary_version(&dest).await {
                    Ok(v) => {
                        if !j(&serde_json::json!({"binary": dest.display().to_string(), "version": v, "ready": true})) {
                            output::ok(&format!("engine ready: {} ({})", dest.display(), v));
                        }
                        return Ok(());
                    }
                    Err(e) => output::info(&format!("existing binary broken ({e}), reinstalling…")),
                }
            }
            if *build {
                let src_dir = config::resolve_engine_dir(cli.engine_dir.as_deref())?;
                output::info(&format!("building from {} …", src_dir.display()));
                setup::build_from_source(&src_dir, &dest).await?;
            } else {
                let url = from_url.clone().unwrap_or_else(|| setup::default_release_url(version));
                output::info(&format!("downloading {url} …"));
                let n = setup::download(&url, &dest).await?;
                output::info(&format!("downloaded {} bytes", n));
            }
            let v = setup::binary_version(&dest).await?;
            if !j(&serde_json::json!({"binary": dest.display().to_string(), "version": v})) {
                output::ok(&format!("engine ready: {} ({})", dest.display(), v));
            }
        }
        Cmd::Doctor => {
            // binary
            let mut rows: Vec<(String, String)> = Vec::new();
            let bin_candidates = [
                config::resolve_engine_dir(cli.engine_dir.as_deref())
                    .map(|d| d.join(setup::binary_name()))
                    .unwrap_or_else(|_| std::path::PathBuf::from(setup::binary_name())),
            ];
            for bin in bin_candidates {
                match setup::binary_version(&bin).await {
                    Ok(v) => rows.push(("engine binary".into(), format!("{} ({})", bin.display(), v))),
                    Err(_) => rows.push(("engine binary".into(), format!("MISSING at {} (kdc setup)", bin.display()))),
                }
            }
            // ports
            for (label, port) in [("bootstrap :8084", 8084u16), ("dpos :8082", 8082), ("pbft :8081", 8081)] {
                let url = format!("http://localhost:{port}/api/node/health");
                let ok = reqwest::get(&url).await.map(|r| r.status().is_success()).unwrap_or(false);
                rows.push((format!("port {label}"), if ok { "serving".into() } else { "free".into() }));
            }
            // genesis + data dirs
            if let Ok(dir) = config::resolve_engine_dir(cli.engine_dir.as_deref()) {
                let g = dir.join("archives/genesis_testnet_accounts.txt");
                rows.push(("genesis wallets".into(), if g.exists() { "present".into() } else { "MISSING (make gen-testnet)".into() }));
                for d in ["archive-testnet/data-testnet-bootstrap", "archive-testnet/data-testnet-dpos01", "archive-testnet/data-testnet-pbft01"] {
                    let p = dir.join(d);
                    rows.push((format!("data {d}"), if p.exists() { "present".into() } else { "absent (fresh)".into() }));
                }
            }
            // node reachability
            for (label, url) in [("bootstrap", config::node_url("boot", cli.boot_url.as_deref())), ("dpos01", config::node_url("dpos", None)), ("pbft01", config::node_url("pbft", None))] {
                let eng = Engine::new(&url);
                let h = eng.health().await.unwrap_or(false);
                rows.push((format!("node {label}"), if h { "healthy".into() } else { "down".into() }));
            }
            if !j(&serde_json::json!({"ok": true})) {
                output::kv(rows);
            }
        }
        Cmd::Config => {
            let engine = config::resolve_engine_dir(cli.engine_dir.as_deref());
            let rows = vec![
                (
                    "engine_dir".into(),
                    engine
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|e| format!("ERR: {e}")),
                ),
                (
                    "bootstrap".into(),
                    config::node_url("boot", cli.boot_url.as_deref()),
                ),
                ("dpos01".into(), config::node_url("dpos", None)),
                ("pbft01".into(), config::node_url("pbft", None)),
                (
                    "keystore".into(),
                    config::home_dir()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|e| format!("ERR: {e}")),
                ),
            ];
            if !j(&serde_json::json!({"ok": true})) {
                output::kv(rows);
            }
        }
        Cmd::Node { op } => node_cmd(&cli, op, out_json).await?,
        Cmd::Wallet { op } => wallet_cmd(&cli, op, out_json).await?,
        Cmd::Faucet { address } => {
            if cli.network == "mainnet" {
                anyhow::bail!("no faucet on mainnet (testnet-only subsidy)");
            }
            let e = engine(&cli)?;
            let addr = resolve_addr(address);
            let out = e.faucet(&addr).await?;
            if !j(&out) {
                output::ok(&format!(
                    "faucet accepted for {addr} (mined by DPOS in ~15s)"
                ));
                if let Some(h) = out.get("transactionHash").and_then(|h| h.as_str()) {
                    output::info(&format!("tx: {h}"));
                }
            }
        }
        Cmd::Transfer {
            from,
            to,
            kdc,
            proton,
        } => {
            let e = engine(&cli)?;
            let proton = match (kdc, proton) {
                (Some(k), _) => amount::kdc_to_proton(k)?,
                (None, Some(p)) => p.clone(),
                (None, None) => anyhow::bail!("pass --kdc or --proton"),
            };
            let body = serde_json::json!({
                "type": "transfer",
                "from": resolve_addr(from),
                "to": resolve_addr(to),
                "amount_proton": proton,
                "gasPrice": 1_000_000_000u64,
            });
            let out = e.submit_tx(&body).await?;
            let hash = out
                .get("transaction")
                .and_then(|t| t.get("hash"))
                .and_then(|h| h.as_str())
                .unwrap_or("?");
            if !j(&out) {
                output::ok(&format!("transfer accepted: {hash} (mined by DPOS)"));
            }
        }
        Cmd::Critical {
            from,
            record,
            detail,
        } => {
            let e = engine(&cli)?;
            let body = serde_json::json!({
                "type": "critical",
                "from": resolve_addr(from),
                "to": resolve_addr(from),
                "amount_proton": "1",
                "data": { "record": record, "detail": detail },
                "gasPrice": 1_000_000_000u64,
            });
            let out = e.submit_tx(&body).await?;
            let hash = out
                .get("transaction")
                .and_then(|t| t.get("hash"))
                .and_then(|h| h.as_str())
                .unwrap_or("?");
            if !j(&out) {
                output::ok(&format!("critical record accepted: {hash} (PBFT finality)"));
            }
        }
        Cmd::Chain { op } => chain_cmd(&cli, op, out_json).await?,
        Cmd::Tx { hash, watch } => tx_cmd(&cli, hash, *watch, out_json).await?,
        Cmd::Mempool => {
            let e = engine(&cli)?;
            let (pend, stats) = tokio::join!(e.mempool_pending(), e.mempool_stats());
            let pend = pend?;
            if !j(&pend) {
                for chain in ["dpos", "pbft"] {
                    let list = pend
                        .get(chain)
                        .and_then(|c| c.get("pending"))
                        .and_then(|p| p.as_array());
                    let n = list.map(|l| l.len()).unwrap_or(0);
                    output::info(&format!("{chain}: {n} pending"));
                    for t in list.unwrap_or(&vec![]).iter().take(10) {
                        output::info(&format!(
                            "  {} {} {}→{}",
                            t.get("hash")
                                .and_then(|h| h.as_str())
                                .unwrap_or("?")
                                .get(..16)
                                .unwrap_or("?"),
                            t.get("type").and_then(|t| t.as_str()).unwrap_or("?"),
                            t.get("from")
                                .and_then(|f| f.as_str())
                                .unwrap_or("?")
                                .get(..10)
                                .unwrap_or("?"),
                            t.get("to")
                                .and_then(|f| f.as_str())
                                .unwrap_or("?")
                                .get(..10)
                                .unwrap_or("?"),
                        ));
                    }
                }
                let _ = stats;
            }
        }
        Cmd::Validators => {
            let e = engine(&cli)?;
            let list = e.validators().await?;
            if !j(&serde_json::json!({ "validators": list })) {
                use tabled::Tabled;
                #[derive(Tabled)]
                struct V {
                    name: String,
                    chain: String,
                    active: String,
                    reg_stake: String,
                    kdc: String,
                    staked: String,
                    match_: String,
                }
                let mut rows = Vec::new();
                for v in &list {
                    let addr = v.get("address").and_then(|a| a.as_str()).unwrap_or("?");
                    let (kdc, staked) = match e.account(addr).await {
                        Ok(a) => {
                            let b = a.get("account").and_then(|a| a.get("balances"));
                            (
                                b.and_then(|b| b.get("KDC"))
                                    .and_then(|k| k.get("amount"))
                                    .and_then(|a| a.as_str())
                                    .unwrap_or("0")
                                    .to_string(),
                                b.and_then(|b| b.get("KDC_STAKED"))
                                    .and_then(|k| k.get("amount"))
                                    .and_then(|a| a.as_str())
                                    .unwrap_or("0")
                                    .to_string(),
                            )
                        }
                        Err(_) => ("?".into(), "?".into()),
                    };
                    let reg = v
                        .get("stake_amount")
                        .and_then(|s| s.as_str())
                        .unwrap_or("0");
                    rows.push(V {
                        name: short(v.get("name").and_then(|n| n.as_str()).unwrap_or("?"), 18),
                        chain: v
                            .get("consensus_type")
                            .and_then(|c| c.as_str())
                            .unwrap_or("?")
                            .into(),
                        active: if v
                            .get("is_active")
                            .and_then(|a| a.as_bool())
                            .unwrap_or(false)
                        {
                            "yes".into()
                        } else {
                            "no".into()
                        },
                        reg_stake: kdc_fmt(reg),
                        kdc: kdc_fmt(&kdc),
                        staked: kdc_fmt(&staked),
                        match_: if staked == *reg {
                            "✓".into()
                        } else {
                            "≠".into()
                        },
                    });
                }
                println!("{}", tabled::Table::new(rows));
            }
        }
        Cmd::Network => {
            let urls = [
                (
                    "bootstrap",
                    config::node_url("boot", cli.boot_url.as_deref()),
                ),
                ("dpos01", config::node_url("dpos", None)),
                ("pbft01", config::node_url("pbft", None)),
            ];
            let mut rows = Vec::new();
            for (name, url) in urls {
                let eng = Engine::new(&url);
                let (health, hd, hp) =
                    tokio::join!(eng.health(), eng.height("DPOS"), eng.height("PBFT"));
                let status = if health.unwrap_or(false) {
                    "online"
                } else {
                    "offline"
                };
                rows.push((
                    name.to_string(),
                    url,
                    status.to_string(),
                    hd.map(|h| h.to_string())
                        .unwrap_or_else(|e| format!("ERR {e}")),
                    hp.map(|h| h.to_string())
                        .unwrap_or_else(|e| format!("ERR {e}")),
                ));
            }
            if !j(&serde_json::json!({"nodes": rows.iter().map(|r| &r.0).collect::<Vec<_>>() })) {
                use tabled::Tabled;
                #[derive(Tabled)]
                struct N {
                    node: String,
                    url: String,
                    status: String,
                    dpos_h: String,
                    pbft_h: String,
                }
                println!(
                    "{}",
                    tabled::Table::new(
                        rows.into_iter()
                            .map(|(node, url, status, dpos_h, pbft_h)| N {
                                node,
                                url,
                                status,
                                dpos_h,
                                pbft_h
                            })
                            .collect::<Vec<_>>()
                    )
                );
            }
            let e = engine(&cli)?;
            match e.peers().await {
                Ok(p) => {
                    if !out_json {
                        output::info(&format!(
                            "peers: {}",
                            serde_json::to_string(&p)
                                .unwrap_or_default()
                                .get(..200.min(serde_json::to_string(&p).unwrap_or_default().len()))
                                .unwrap_or("")
                        ));
                    }
                }
                Err(err) => {
                    if !out_json {
                        output::info(&format!("peers unavailable: {err}"));
                    }
                }
            }
        }
    }
    Ok(())
}

async fn node_cmd(cli: &Cli, op: &NodeOp, out_json: bool) -> Result<()> {
    if let NodeOp::Start { mode, .. } = op {
        // protocol guard first: offline-friendly error, no engine needed
        node::validate_start_mode(&cli.network, mode)?;
    }
    let engine_dir = config::resolve_engine_dir(cli.engine_dir.as_deref())?;
    let ns = node::NodeSet::load(engine_dir, &cli.network)?;
    let modes = |m: &str| -> Result<Vec<node::Mode>> {
        Ok(match m {
            "all" => node::Mode::all().to_vec(),
            "bootstrap" => vec![node::Mode::Bootstrap],
            "dpos" => vec![node::Mode::Dpos],
            "pbft" => vec![node::Mode::Pbft],
            _ => anyhow::bail!("mode must be all|bootstrap|dpos|pbft"),
        })
    };
    let j = |v: &serde_json::Value| {
        if out_json {
            output::json(v);
            true
        } else {
            false
        }
    };
    match op {
        NodeOp::Start {
            mode,
            node_id,
            validator_address,
            bootstrap_nodes,
            port,
            p2p_port,
            udp_port,
            data_dir,
        } => {
            let params = node::StartParams {
                network: cli.network.clone(),
                node_id: node_id.clone(),
                validator_address: validator_address.clone(),
                bootstrap_nodes: bootstrap_nodes.clone(),
                port: *port,
                p2p_port: *p2p_port,
                udp_port: *udp_port,
                data_dir: data_dir.clone().map(std::path::PathBuf::from),
            };
            for m in modes(mode)? {
                match ns.start(m, &params).await {
                    Ok(pid) => {
                        if !j(&serde_json::json!({"node": m.name(), "pid": pid})) {
                            output::ok(&format!(
                                "{} started (pid {pid}, :{}) [{}]",
                                m.name(),
                                params.port.unwrap_or_else(|| m.http_port()),
                                params.network
                            ));
                        }
                    }
                    Err(e) => output::info(&format!("{}: {e}", m.name())),
                }
            }
        }
        NodeOp::Stop { mode } => {
            for m in modes(mode)? {
                if ns.stop(m, None)? {
                    if !j(&serde_json::json!({"node": m.name(), "stopped": true})) {
                        output::ok(&format!("{} stopped", m.name()));
                    }
                } else if !out_json {
                    output::info(&format!("{} was not running", m.name()));
                }
            }
        }
        NodeOp::Status => {
            use tabled::Tabled;
            #[derive(Tabled)]
            struct S {
                node: String,
                process: String,
                health: String,
                dpos_h: String,
                pbft_h: String,
            }
            let mut rows = Vec::new();
            for m in node::Mode::all() {
                let alive = ns.pid(m).map(node::process_alive_pub);
                let url = format!("http://localhost:{}", m.http_port());
                let eng = Engine::new(&url);
                let (health, hd, hp) =
                    tokio::join!(eng.health(), eng.height("DPOS"), eng.height("PBFT"));
                rows.push(S {
                    node: m.name().into(),
                    process: match alive {
                        Some(true) => "running".into(),
                        _ => "stopped".into(),
                    },
                    health: if health.unwrap_or(false) {
                        "200".into()
                    } else {
                        "down".into()
                    },
                    dpos_h: hd.map(|h| h.to_string()).unwrap_or_else(|_| "-".into()),
                    pbft_h: hp.map(|h| h.to_string()).unwrap_or_else(|_| "-".into()),
                });
            }
            if !j(&serde_json::json!({"ok": true})) {
                println!("{}", tabled::Table::new(rows));
            }
            // stake locks of genesis validators (testnet bootstrap view;
            // mainnet has no genesis file — section skipped)
            let eng = engine(cli)?;
            let mut locks: Vec<(&str, String)> = Vec::new();
            if let Some(a) = ns.dpos_id.clone() {
                locks.push(("dpos-val", a));
            }
            if let Some(a) = ns.pbft_id.clone() {
                locks.push(("pbft-val", a));
            }
            for (label, addr) in locks {
                if let Ok(a) = eng.account(&addr).await {
                    let b = a.get("account").and_then(|a| a.get("balances"));
                    let staked = b
                        .and_then(|b| b.get("KDC_STAKED"))
                        .and_then(|k| k.get("amount"))
                        .and_then(|x| x.as_str())
                        .unwrap_or("0");
                    if !out_json {
                        output::info(&format!("{label} KDC_STAKED: {} KDC", kdc_fmt(staked)));
                    }
                }
            }
        }
        NodeOp::Logs {
            node,
            lines,
            filter,
        } => {
            let m = match node.as_str() {
                "bootstrap" => node::Mode::Bootstrap,
                "dpos" | "dpos01" => node::Mode::Dpos,
                "pbft" | "pbft01" => node::Mode::Pbft,
                _ => anyhow::bail!("node must be bootstrap|dpos01|pbft01"),
            };
            for l in ns.log_tail(m, *lines, filter.as_deref())? {
                println!("{l}");
            }
        }
    }
    Ok(())
}

async fn wallet_cmd(cli: &Cli, op: &WalletOp, out_json: bool) -> Result<()> {
    let j = |v: &serde_json::Value| {
        if out_json {
            output::json(v);
            true
        } else {
            false
        }
    };
    match op {
        WalletOp::New { name, save } => {
            let seed = wallet_crypto::random_seed();
            let addr = wallet_crypto::address_for_seed(&seed);
            let pubb = wallet_crypto::public_bytes(&seed);
            let rec = store::SavedWallet {
                name: if name.is_empty() {
                    addr.clone()
                } else {
                    name.clone()
                },
                address: addr.clone(),
                public_key: hex::encode(pubb),
                key_hex: hex::encode(seed),
                key_format: "seed".into(),
                created_at: chrono_now(),
            };
            if *save {
                store::add(rec.clone())?;
            }
            if !j(
                &serde_json::json!({"address": rec.address, "public_key": rec.public_key, "seed": rec.key_hex}),
            ) {
                output::kv(vec![
                    ("address".into(), rec.address.clone()),
                    ("public_key".into(), format!("{}…", &rec.public_key[..64])),
                    ("seed (private, 64 hex)".into(), rec.key_hex.clone()),
                ]);
                output::info("Saved to keystore. Guard the seed: it signs for this address.");
            }
        }
        WalletOp::Import { key, name } => {
            let km = wallet_crypto::key_from_any_hex(key)?;
            let addr = wallet_crypto::address_for_key(&km)?;
            let pubb = wallet_crypto::public_bytes_for_key(&km)?;
            let (fmt, hexmat) = match &km {
                wallet_crypto::KeyMaterial::Seed(s) => ("seed", hex::encode(s)),
                wallet_crypto::KeyMaterial::Fips(f) => ("fips", hex::encode(&f[..])),
            };
            let rec = store::SavedWallet {
                name: if name.is_empty() {
                    addr.clone()
                } else {
                    name.clone()
                },
                address: addr.clone(),
                public_key: hex::encode(pubb),
                key_hex: hexmat,
                key_format: fmt.into(),
                created_at: chrono_now(),
            };
            store::add(rec.clone())?;
            if !j(&serde_json::json!({"address": rec.address, "format": fmt})) {
                output::ok(&format!("imported {addr} [{fmt}] (saved)"));
            }
        }
        WalletOp::List => {
            let list = store::load()?;
            if !j(&serde_json::json!({ "wallets": list })) {
                use tabled::Tabled;
                #[derive(Tabled)]
                struct W {
                    name: String,
                    address: String,
                }
                println!(
                    "{}",
                    tabled::Table::new(
                        list.into_iter()
                            .map(|w| W {
                                name: w.name,
                                address: w.address
                            })
                            .collect::<Vec<_>>()
                    )
                );
            }
        }
        WalletOp::Balance { address } => {
            let e = engine(cli)?;
            let addr = resolve_addr(address);
            let a = e.account(&addr).await?;
            let b = a.get("account").and_then(|a| a.get("balances"));
            let kdc = b
                .and_then(|b| b.get("KDC"))
                .and_then(|k| k.get("amount"))
                .and_then(|x| x.as_str())
                .unwrap_or("0");
            let staked = b
                .and_then(|b| b.get("KDC_STAKED"))
                .and_then(|k| k.get("amount"))
                .and_then(|x| x.as_str())
                .unwrap_or("0");
            if !j(&serde_json::json!({"address": addr, "kdc": kdc, "staked": staked})) {
                output::kv(vec![
                    ("address".into(), addr),
                    ("KDC".into(), format!("{} ({kdc} proton)", kdc_fmt(kdc))),
                    (
                        "KDC_STAKED".into(),
                        format!("{} ({staked} proton)", kdc_fmt(staked)),
                    ),
                ]);
            }
        }
        WalletOp::Address { key } => {
            let km = wallet_crypto::key_from_any_hex(key)?;
            let addr = wallet_crypto::address_for_key(&km)?;
            if !j(&serde_json::json!({"address": addr})) {
                output::ok(&format!("address: {addr}"));
            }
        }
        WalletOp::Sign { key, message } => {
            let km = wallet_crypto::key_from_any_hex(key)?;
            let sig = wallet_crypto::sign_with(&km, message.as_bytes())?;
            if !j(&serde_json::json!({"signature": hex::encode(&sig)})) {
                output::kv(vec![(
                    "signature (ML-DSA-65, 3309 B)".into(),
                    hex::encode(&sig),
                )]);
            }
        }
        WalletOp::History { address } => {
            let e = engine(cli)?;
            let addr = resolve_addr(address);
            let needle = addr.to_lowercase();
            let mut movs = Vec::new();
            for chain in ["DPOS", "PBFT"] {
                let url = match chain {
                    "PBFT" => config::node_url("pbft", None),
                    _ => config::node_url("boot", cli.boot_url.as_deref()),
                };
                let eng = Engine::new(&url);
                let (blocks, _) = eng.blocks(chain, 300, 0).await?;
                for b in blocks {
                    let idx = b.get("index").and_then(|i| i.as_u64()).unwrap_or(0);
                    for t in b
                        .get("transactions")
                        .and_then(|t| t.as_array())
                        .cloned()
                        .unwrap_or_default()
                    {
                        let f = t
                            .get("from")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_lowercase();
                        let to = t
                            .get("to")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_lowercase();
                        if f == needle || to == needle {
                            movs.push((idx, chain, t));
                        }
                    }
                }
            }
            movs.sort_by(|a, b| b.0.cmp(&a.0));
            let _ = e;
            if !j(
                &serde_json::json!({"movements": movs.iter().map(|(_,_,t)| t).collect::<Vec<_>>() }),
            ) {
                use tabled::Tabled;
                #[derive(Tabled)]
                struct M {
                    block: String,
                    chain: String,
                    dir: String,
                    type_: String,
                    amount: String,
                    hash: String,
                }
                println!(
                    "{}",
                    tabled::Table::new(
                        movs.into_iter()
                            .map(|(idx, chain, t)| {
                                let f = t.get("from").and_then(|x| x.as_str()).unwrap_or("?");
                                M {
                                    block: idx.to_string(),
                                    chain: chain.to_string(),
                                    dir: if f.to_lowercase() == needle {
                                        "sent".into()
                                    } else {
                                        "received".into()
                                    },
                                    type_: t
                                        .get("type")
                                        .and_then(|x| x.as_str())
                                        .unwrap_or("?")
                                        .into(),
                                    amount: kdc_fmt(
                                        t.get("value").and_then(|x| x.as_str()).unwrap_or("0"),
                                    ),
                                    hash: t
                                        .get("hash")
                                        .and_then(|x| x.as_str())
                                        .unwrap_or("?")
                                        .get(..16)
                                        .unwrap_or("?")
                                        .into(),
                                }
                            })
                            .collect::<Vec<_>>()
                    )
                );
            }
        }
    }
    Ok(())
}

async fn chain_cmd(cli: &Cli, op: &ChainOp, out_json: bool) -> Result<()> {
    let j = |v: &serde_json::Value| {
        if out_json {
            output::json(v);
            true
        } else {
            false
        }
    };
    let chains = |c: &str| -> Result<Vec<String>> {
        Ok(match c {
            "all" => vec!["DPOS".into(), "PBFT".into()],
            "dpos" => vec!["DPOS".into()],
            "pbft" => vec!["PBFT".into()],
            _ => anyhow::bail!("chain must be dpos|pbft|all"),
        })
    };
    let eng_for = |chain: &str| {
        Engine::new(&match chain {
            "PBFT" => config::node_url("pbft", None),
            _ => config::node_url("boot", cli.boot_url.as_deref()),
        })
    };
    match op {
        ChainOp::Height { chain } => {
            let mut rows = Vec::new();
            for c in chains(chain)? {
                let h = eng_for(&c).height(&c).await?;
                rows.push((format!("{c} height"), h.to_string()));
                if j(&serde_json::json!({"chain": c, "height": h})) {
                    return Ok(());
                }
            }
            output::kv(rows);
        }
        ChainOp::Blocks {
            chain,
            limit,
            offset,
        } => {
            let c = match chain.as_str() {
                "dpos" => "DPOS",
                "pbft" => "PBFT",
                _ => anyhow::bail!("chain must be dpos|pbft"),
            };
            let (blocks, total) = eng_for(c).blocks(c, *limit, *offset).await?;
            if !j(&serde_json::json!({"chain": c, "total": total, "blocks": blocks})) {
                use tabled::Tabled;
                #[derive(Tabled)]
                struct B {
                    index: String,
                    hash: String,
                    txs: String,
                    validator: String,
                }
                println!(
                    "{}",
                    tabled::Table::new(
                        blocks
                            .into_iter()
                            .map(|b| B {
                                index: b
                                    .get("index")
                                    .and_then(|i| i.as_u64())
                                    .unwrap_or(0)
                                    .to_string(),
                                hash: b
                                    .get("hash")
                                    .and_then(|h| h.as_str())
                                    .unwrap_or("?")
                                    .get(..16)
                                    .unwrap_or("?")
                                    .into(),
                                txs: b
                                    .get("transactions")
                                    .and_then(|t| t.as_array())
                                    .map(|a| a.len().to_string())
                                    .unwrap_or("0".into()),
                                validator: b
                                    .get("validator")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("?")
                                    .get(..10)
                                    .unwrap_or("?")
                                    .into(),
                            })
                            .collect::<Vec<_>>()
                    )
                );
                output::info(&format!("total: {total} (offset {offset}, limit {limit})"));
            }
        }
        ChainOp::Block { id, chain } => {
            let c = match chain.as_str() {
                "dpos" => "DPOS",
                "pbft" => "PBFT",
                _ => anyhow::bail!("chain must be dpos|pbft"),
            };
            let eng = eng_for(c);
            let found = if id.starts_with("0x") && id.len() > 42 {
                let (blocks, _) = eng.blocks(c, 1000, 0).await?;
                blocks
                    .into_iter()
                    .find(|b| b.get("hash").and_then(|h| h.as_str()) == Some(id.as_str()))
            } else {
                let idx: u64 = id.parse().context("id must be height or 0x hash")?;
                let (_, total) = eng.blocks(c, 1, 0).await?;
                if idx >= total {
                    None
                } else {
                    let (blocks, _) = eng.blocks(c, 1, total - 1 - idx).await?;
                    blocks.into_iter().next()
                }
            };
            match found {
                Some(b) => {
                    if !j(&b) {
                        output::json(&b);
                    }
                }
                None => anyhow::bail!("block not found (hash search covers recent 1000)"),
            }
        }
    }
    Ok(())
}

async fn tx_cmd(cli: &Cli, hash: &str, watch: bool, out_json: bool) -> Result<()> {
    async fn find(
        eng: &Engine,
        chain: &str,
        hash: &str,
    ) -> Result<Option<(String, u64, serde_json::Value)>> {
        let (blocks, _) = eng.blocks(chain, 300, 0).await?;
        for b in blocks {
            for t in b
                .get("transactions")
                .and_then(|t| t.as_array())
                .cloned()
                .unwrap_or_default()
            {
                if t.get("hash").and_then(|h| h.as_str()) == Some(hash) {
                    let idx = b.get("index").and_then(|i| i.as_u64()).unwrap_or(0);
                    return Ok(Some((chain.to_string(), idx, t)));
                }
            }
        }
        Ok(None)
    }
    let check = || async {
        for (chain, url) in [
            ("DPOS", config::node_url("boot", cli.boot_url.as_deref())),
            ("PBFT", config::node_url("pbft", None)),
        ] {
            let eng = Engine::new(&url);
            if let Some(hit) = find(&eng, chain, hash).await? {
                return Ok::<_, anyhow::Error>(Some(hit));
            }
            // mempool?
            if let Ok(p) = eng.mempool_pending().await {
                for mchain in ["dpos", "pbft"] {
                    for t in p
                        .get(mchain)
                        .and_then(|c| c.get("pending"))
                        .and_then(|x| x.as_array())
                        .cloned()
                        .unwrap_or_default()
                    {
                        if t.get("hash").and_then(|h| h.as_str()) == Some(hash) {
                            return Ok(Some(("mempool".to_string(), 0, t)));
                        }
                    }
                }
            }
        }
        Ok(None)
    };
    if watch {
        for _ in 0..60 {
            if let Some((chain, idx, _)) = check().await? {
                if !out_json {
                    output::ok(&format!("mined: {chain} block #{idx}"));
                } else {
                    output::json(
                        &serde_json::json!({"status": "mined", "chain": chain, "block": idx}),
                    );
                }
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
        anyhow::bail!("not mined after 5 min");
    }
    match check().await? {
        Some((chain, idx, t)) => {
            if !out_json {
                if chain == "mempool" {
                    output::info(&format!(
                        "pending in mempool: {}",
                        t.get("type").and_then(|x| x.as_str()).unwrap_or("?")
                    ));
                } else {
                    output::ok(&format!("mined: {chain} block #{idx}"));
                    output::json(&t);
                }
            } else {
                output::json(
                    &serde_json::json!({"status": "mined", "chain": chain, "block": idx, "tx": t}),
                );
            }
        }
        None => {
            if !out_json {
                output::info("not found in recent blocks nor mempool");
            } else {
                output::json(&serde_json::json!({"status": "unknown"}));
            }
        }
    }
    Ok(())
}

fn chrono_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default()
}
