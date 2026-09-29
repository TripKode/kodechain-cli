//! Built-in manual (`kdc manual [topic]`). No network needed.

pub const TOPICS: &[(&str, &str)] = &[
    ("overview", OVERVIEW),
    ("node", NODE),
    ("mainnet", MAINNET),
    ("wallet", WALLET),
    ("transactions", TRANSACTIONS),
    ("chains", CHAINS),
    ("troubleshoot", TROUBLESHOOT),
];

pub fn show(topic: Option<&str>) {
    match topic {
        None => {
            println!("kdc manual — topics:\n");
            for (name, _) in TOPICS {
                println!("  {name}");
            }
            println!("\nUsage: kdc manual <topic>   |   kdc <cmd> --help");
        }
        Some(t) => match TOPICS.iter().find(|(n, _)| *n == t) {
            Some((_, text)) => println!("{text}"),
            None => {
                println!("unknown topic '{t}'. Available:");
                for (name, _) in TOPICS {
                    println!("  {name}");
                }
            }
        },
    }
}

const OVERVIEW: &str = r#"kdc — KodeChain testnet CLI (post-quantum: QSH sponge + ML-DSA-65)

WHAT IT DOES
  Runs testnet nodes (bootstrap + DPOS/PBFT validators), manages
  ML-DSA-65 wallets, and talks to both consensus chains: transfers,
  faucet, critical records, blocks, mempool, validators, network.

QUICK START (fresh machine)
  kdc setup                      # install engine binary (download → build fallback)
  kdc doctor                     # verify binary, ports, genesis, data dirs
  kdc node start --mode all      # boots bootstrap + dpos01 + pbft01
  # ...or N validators via containers:
  kdc compose --dpos 2 --pbft 1  # renders docker-compose.yml (wallets generated)
  docker compose up -d --build
  kdc node status                # health + heights + stake locks
  kdc wallet new --name alice    # ML-DSA-65 wallet, saved locally
  kdc faucet <address>           # 1000 KDC (1h cooldown per address)
  kdc transfer --from <a> --to <b> --kdc 5
  kdc chain blocks --chain dpos --limit 20

MODES
  Node roles are fixed by the testnet genesis: bootstrap aggregates both
  chains, dpos01 mines DPOS, pbft01 mines PBFT. Use --mode all for the
  standard 3-node testnet, or start roles individually.

MONEY UNITS
  Amounts on the wire are proton (1 KDC = 10^18 proton, exact strings).
  kdc accepts decimal KDC (--kdc 1.234567) and converts exactly.

LEARN MORE
  kdc manual node|mainnet|wallet|transactions|chains|troubleshoot
"#;

const NODE: &str = r#"kdc node — lifecycle of testnet nodes

START
  kdc node start --mode all                 # bootstrap + dpos01 + pbft01
  kdc node start --mode bootstrap           # one role only
  kdc node start --mode dpos --port 8082    # flags override defaults
  Ports: bootstrap :8084, dpos :8082, pbft :8081 (+ beacon :9084/82/81).
  Env: KODECHAIN_BEACON_INTERVAL (default 5), KODECHAIN_BIDIR_SYNC (30s),
       KDC_LAN_IP (auto-detected otherwise), FAUCET key read from the
       genesis accounts file automatically.

STOP / STATUS / LOGS
  kdc node stop [--mode all|bootstrap|dpos|pbft]
  kdc node status            # process + health + DPOS/PBFT heights + locks
  kdc node logs --node dpos01 --lines 100 --filter gas

AFTER A REBOOT
  State survives (LevelDB): just `kdc node start --mode all` again.

MAINNET
  The protocol runs a SINGLE bootstrap there (it boots the network at
  genesis). kdc only deploys validators: see `kdc manual mainnet`.

NOTES
  - Pidfiles live next to engine data-dirs (archive-testnet/*.pid),
    so stale pids are impossible after reboot.
  - Validators need their genesis wallets registered once per fresh
    data-dir (the ritual): see `kdc manual troubleshoot`.
"#;

const WALLET: &str = r#"kdc wallet — ML-DSA-65 wallets (post-quantum signatures)

CREATE / IMPORT
  kdc wallet new --name alice [--save]   # random seed, prints keys
  kdc wallet import --key <hex> [--name x]
      Accepts 64-hex seeds (what `new` emits) AND 8064-hex FIPS secrets
      (what the engine genesis tooling emits — first 32 bytes are the seed).
  kdc wallet list                        # keystore (~/.local/share/kdc)
  kdc wallet balance <address|name>      # KDC + KDC_STAKED buckets
  kdc wallet address --key <hex>         # derive address, verify ownership

ADDRESS SCHEME (canonical, matches engine wallets)
  address = "0x" + last-40-hex(QSH_KDC-ADDR(public_key)).
  `wallet new` output is directly usable as validator/faucet address.

CUSTODY
  Keys live in wallets.json (0600). This is TESTNET tooling — never reuse
  these keys anywhere with value.
"#;

const TRANSACTIONS: &str = r#"kdc transfer / faucet / critical — moving value

FUNDING
  kdc faucet <address>                   # 1000 KDC via DPOS (~15s to mine,
                                         # 1h cooldown per address)

TRANSFERS (DPOS chain, mined by the validator)
  kdc transfer --from <addr> --to <addr> --kdc 5
  kdc transfer --from alice --to bob --kdc 0.5   # keystore names allowed
  Amounts are exact: --kdc 1.234567 → 1234567000000000000 proton.
  NOTE: `amount` (legacy) means RAW PROTON — always prefer --kdc.

CRITICAL RECORDS (PBFT chain, instant finality)
  kdc critical --from <addr> --record "anchor-001" [--detail ...]

TRACKING
  kdc tx <hash> [--watch]                # mined block or mempool status
  kdc mempool                            # pending of both chains
"#;

const CHAINS: &str = r#"kdc chain — reading both consensus chains

HEIGHTS
  kdc chain height [--chain dpos|pbft|all]   # lag <= 1 is healthy

BLOCKS (paginated, newest last in each page)
  kdc chain blocks --chain dpos --limit 20 --offset 0
  kdc chain blocks --chain pbft --limit 100
  kdc chain block 12345 [--chain dpos]       # full block detail
  kdc chain block 0xhash...                  # by hash

WHY DPOS HAS THOUSANDS OF BLOCKS AND PBFT HAS DOZENS
  DPOS is slot-based: one block ~every 5s even when empty (heartbeat
  keeps schedule/rotation alive). PBFT is event-driven: it only mines
  when there is something to confirm. Empty DPOS blocks are normal.

HISTORY
  kdc wallet history <address|name>          # scan both chains for
                                             # movements (from/to match)
"#;

const TROUBLESHOOT: &str = r#"kdc troubleshooting

NODE WON'T START / UNHEALTHY
  kdc node status                          # which role is down?
  kdc node logs --node <role> --lines 60   # crash reason is at the tail
  Port in use → another instance holds it: `kdc node stop --mode all`
  first, then start again.

TRANSFER SAYS "insufficient balance" RIGHT AFTER FAUCET
  Faucet applies on the VALIDATOR first; the bootstrap syncs every
  ~30s (KODECHAIN_BIDIR_SYNC). Wait 30-60s and retry.

TX ACCEPTED BUT NOT MINED
  kdc mempool                              # is it pending? on which chain?
  TRANSFERS always route to DPOS; critical records to PBFT (policy).
  kdc tx <hash> --watch                    # wait for inclusion.

RE-STARTING AFTER REGENESIS
  New wallets = new node_ids: update your start invocation (kdc reads
  them from archives/genesis_testnet_accounts.txt automatically, so a
  plain `kdc node start --mode all` picks them up). Validators must be
  re-registered per fresh data-dir (faucet the PBFT wallet first —
  it is not in the DPOS genesis the bootstrap loads).

CONFIG
  kdc config                               # show resolved paths/endpoints
  Flags > env (KODECHAIN_ENGINE_DIR, KODECHAIN_*_URL, KDC_HOME) > defaults.
"#;

const MAINNET: &str = r#"kdc on mainnet — validators only

PROTOCOL RULE
  Mainnet runs a SINGLE bootstrap node (it boots the network at genesis).
  kdc refuses `--mode bootstrap` and `--mode all` on mainnet: deploy only
  `dpos` or `pbft` validator nodes pointing at that bootstrap.

DEPLOY A MAINNET VALIDATOR
  kdc --network mainnet node start --mode dpos \
    --node-id 0x<your-validator-wallet> \
    --bootstrap-nodes "enode://<bootstrap-id>@<ip>:<p2p-port>?http_port=<port>"
  Data dir defaults to ./data-mainnet-dpos01 (./data-mainnet-pbft01 for
  pbft); override with --data-dir. Ports/p2p/udp overridable per flag
  (handy to run both networks on one machine).

  Env equivalents: KODECHAIN_NETWORK=mainnet,
  KODECHAIN_MAINNET_BOOTSTRAP="enode://..." (instead of --bootstrap-nodes),
  VALIDATOR_ADDRESS is set automatically (= --node-id unless given).

INTERACTING WITH MAINNET
  Point the URLs at mainnet nodes:
    kdc --network mainnet --boot-url https://<mainnet-node>:8084 chain height
  There is NO faucet on mainnet (`kdc faucet` refuses immediately).
  Transfers/criticals work the same (you pay real KDC gas).
"#;
