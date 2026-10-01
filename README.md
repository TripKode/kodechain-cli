# KodeChain CLI

Rust CLI for KodeChain: run nodes, manage ML-DSA-65 wallets, and talk to
both consensus chains. Post-quantum throughout (QSH sponge + ML-DSA-65
signatures, same schemes as the engine).

## Install

Prebuilt (recommended) — download the assets from the repo's latest
GitHub Release (`Releases → latest`):

```bash
# Linux x86_64
curl -sSL -o kdc <release-url>/kdc-linux-amd64
chmod +x kdc && ./kdc --version
# Windows x86_64: download kdc-windows-x86_64-*.zip, unzip, run kdc.exe
```

From source:

```bash
cargo install --path .
# kdc 0.1.2 in ~/.cargo/bin (rustup must be installed)
./install.sh              # Linux: toolchain → release build → install → offline tests
```

## Windows

No install needed: download `kdc-windows-x86_64-*.zip` from the latest
GitHub Release, unzip, and use `kdc.exe` from PowerShell or CMD.

```powershell
.\kdc.exe --version
.\kdc.exe manual overview
.\kdc.exe node status
.\kdc.exe wallet new --name alice
```

- **Nodes**: `kdc node start` needs the engine binary
  (`kodechain-node-validator.exe`) built for Windows. Process
  management uses `taskkill`/`tasklist` internally (no dependencies).
- **Keystore**: `%APPDATA%\kdc\wallets.json` (inherits the user
  profile's private ACL).
- **Env** (PowerShell):
  `$env:KODECHAIN_ENGINE_DIR = "C:\kodechain\kodechain-engine"`;
  persistent via `setx`, or per-command: `kdc --engine-dir C:\... node status`.
- **Ports**: bootstrap 8084, DPOS 8082, PBFT 8081 — allow the firewall
  prompt (P2P 30403/30404/30505).
- **TLS**: rustls (pure Rust, no OpenSSL). All commands work exactly as
  on Linux.

## Networks: testnet + mainnet

```bash
kdc --network testnet node start --mode all   # default: full local testnet
kdc --network mainnet node start --mode dpos \
  --node-id 0x<your-validator-wallet> \
  --bootstrap-nodes "enode://<id>@<ip>:<p2p>?http_port=<port>"
```

Protocol rule: **mainnet runs a SINGLE bootstrap** (it boots the network
at genesis), so `kdc` only deploys `dpos`|`pbft` validators there —
`--mode bootstrap|all` is refused with an explanation. Testnet deploys
all three roles. Mainnet data dirs are `./data-mainnet-*` (testnet:
`archive-testnet/...`); ports/p2p/data-dir are overridable per flag, so
both networks can run on one machine. No faucet on mainnet.

Env: `KODECHAIN_NETWORK`, `KODECHAIN_MAINNET_BOOTSTRAP` (instead of
`--bootstrap-nodes`). `kdc manual mainnet` has the full guide.

## Run paths (prebuilt, docker, or source)

```bash
kdc setup                        # prebuilt engine binary (releases) → fallback hint to --build
kdc setup --build                # force source build (needs Go toolchain)
kdc setup --from-url <url>       # explicit binary URL (or KODECHAIN_ENGINE_RELEASE_URL)
kdc doctor                       # binary version, free ports, genesis, data dirs, node health
kdc compose --dpos 2 --pbft 1    # render docker-compose.yml (wallets generated)
docker compose up -d --build     # run the rendered topology
```

`setup` verifies the binary answers `-version` before reporting ready.
Prebuilt binaries live in GitHub Releases
(`.../releases/download/<tag>/kodechain-node-validator-<os>-<arch>`,
Linux amd64/arm64 + Windows amd64); nothing is published yet, so today
`setup` downloads only with an explicit `--from-url`, otherwise use
`--build`.

## Quick start (engine checkout next to this repo, or set `--engine-dir`)

```bash
kdc node start --mode all   # bootstrap :8084 + dpos01 :8082 + pbft01 :8081
kdc node status             # process + health + heights + stake locks
kdc wallet new --name alice # fresh ML-DSA-65 wallet, saved in keystore
kdc faucet <address>        # 1000 test KDC (~15s to mine, 1h cooldown)
kdc transfer --from alice --to bob --kdc 5
kdc chain blocks --chain dpos --limit 20
kdc manual                  # built-in manual, no network needed
```

Keystore names resolve anywhere an address is accepted
(`faucet`, `transfer --from/--to`, `balance`, `history`).

## Commands

| Command | What it does |
|---|---|
| `node start --mode all\|bootstrap\|dpos\|pbft` | Start nodes (health-waited, pid-tracked). Refuses over an occupied port; per-flag `--port/--p2p-port/--udp-port/--data-dir/--node-id/--bootstrap-nodes/--genesis-dpos/--genesis-pbft` |
| `node stop --mode …` | SIGTERM (taskkill on Windows) via pidfile |
| `node status` | process + HTTP health + DPOS/PBFT heights + KDC_STAKED locks |
| `node logs --node … --lines N --filter x` | Tail node logs with grep filter (`bootstrap\|dpos01\|pbft01`, `dpos`/`pbft` aliases) |
| `wallet new/import/list/balance/address/sign/history` | Keystore (0600). Import accepts 64-hex seeds AND 8064-hex engine FIPS secrets |
| `faucet <addr>` | 1000 KDC via DPOS (testnet only; refused on mainnet) |
| `transfer --from --to --kdc/--proton` | Exact decimal math (no float error). Mined by DPOS |
| `critical --from --record [--detail]` | PBFT-anchored record (instant finality) |
| `chain height/blocks/block --chain` | Both chains, real pagination (`--limit/--offset`); block by height or `0x` hash |
| `tx <hash> [--watch]` | Mined block or mempool status; `--watch` polls up to 5 min |
| `mempool`, `validators`, `network` | Pending pools, stake table (reg vs lock match), node mesh + peers |
| `manual [topic]` | overview, node, mainnet, wallet, transactions, chains, troubleshoot |
| `config` | Resolved paths and endpoints |
| `setup [--version] [--from-url] [--build] [--dest]` | Install engine binary, verify `-version` |
| `doctor` | Binary, ports (serving/free), genesis, data dirs, node health |
| `compose --dpos N --pbft M` | Render docker-compose.yml for N+M validators (wallets generated). Flags: `--dpos-ids/--pbft-ids/--bootstrap-id/--bootstrap-nodes/--genesis-dpos/--genesis-pbft/--output/--context`; secrets via `.env` (`FAUCET_PRIVATE_KEY`, `GENESIS_MESSAGE`) |

Global flags: `--json` (machine output), `--network testnet|mainnet`,
`--engine-dir`, `--boot-url`.

## Chain routing + money units

- Transfers route to **DPOS**, critical records to **PBFT** (engine
  policy; overriding it is rejected).
- Amounts on the wire are proton (1 KDC = 10¹⁸ proton, exact strings).
  `--kdc 1.234567` converts exactly; legacy `--proton` is raw proton.
- DPOS is slot-based (~1 block/5s even empty — heartbeats keep schedule
  alive); PBFT is event-driven (mines only with something to confirm).
  Thousands of empty DPOS blocks vs dozens of PBFT blocks is normal.
- Faucet applies on the validator first; the bootstrap syncs every
  ~30s (`KODECHAIN_BIDIR_SYNC`), so wait ~30–60s after funding a fresh
  address before spending from it.

## Nodes: dirs, logs, ports

- Canonical ports — HTTP 8084/8082/8081, P2P 30403/30404/30505
  (bootstrap/dpos/pbft); beacon = HTTP+1000. `kdc compose` keeps the
  first node of each role on these and shifts extras by +10/+10.
- Data dirs (engine checkout): testnet
  `archive-testnet/data-testnet-{bootstrap,dpos01,pbft01}`, mainnet
  `./data-mainnet-{dpos01,pbft01}`. State survives reboot (LevelDB):
  just `start` again.
- Pidfiles live inside each data dir (`kdc.pid`) + a registry at
  `<keystore>/nodes.json`, so custom `--data-dir` nodes are found by
  `stop`/`status`/`logs`. Logs: `<engine>/logs/{bootstrap,dpos01,pbft01}.log`.
- Validators need their genesis wallets registered once per fresh
  data dir (faucet the PBFT wallet first — it is not in the DPOS
  genesis the bootstrap loads). After regenesis, plain `start --mode
  all` picks up the new wallets automatically.

## Compose topologies

```bash
kdc compose --dpos 2 --pbft 1 --output docker-compose.yml --context /path/to/kodechain-engine
kdc compose --dpos-ids 0xaaa,0xbbb --pbft-ids 0xccc --network mainnet \
  --bootstrap-nodes "enode://0xboot@<ip>:30403?http_port=8084"
```

- Services are `bootstrap`, `dpos-1..N`, `pbft-1..M` — never fixed
  `dpos01`/`pbft01` names. Each gets its own wallet, ports, volume.
- Missing ids are generated (`validator-dpos-N` / `validator-pbft-N`
  in the keystore) — fund + register them before expecting blocks.
- Testnet: bootstrap enode auto-built from the genesis faucet wallet
  (Docker DNS `bootstrap:30403`); mainnet renders validators only and
  requires the protocol bootstrap enode. Secrets stay in env
  (`.env`, fail-fast `VAR:?msg`), never baked into the file.

## Configuration (flags > env > defaults)

| Env | Default |
|---|---|
| `KODECHAIN_ENGINE_DIR` | auto-detect (CWD, `./kodechain-engine`, `../kodechain-engine` — needs the engine binary) |
| `KODECHAIN_NETWORK` / `--network` | `testnet` |
| `KODECHAIN_BOOT_URL` / `_DPOS_URL` / `_PBFT_URL` / `--boot-url` | localhost:8084/8082/8081 |
| `KODECHAIN_MAINNET_BOOTSTRAP` / `--bootstrap-nodes` | (required on mainnet) `enode://<id>@<ip>:<p2p>?http_port=<http>` |
| `KODECHAIN_ENGINE_RELEASE_URL` / `--from-url` | `github.com/TripKode/kodechain-engine/releases/download/<version>/kodechain-node-validator-<os>-<arch>` |
| `KODECHAIN_BEACON_INTERVAL` | `5` |
| `KODECHAIN_BIDIR_SYNC` | `30s` |
| `KDC_LAN_IP` | auto-detected LAN IP (used for validator → bootstrap enodes) |
| `KDC_HOME` | OS data dir (`~/.local/share/kdc`, `%APPDATA%\kdc` on Windows) |
| `FAUCET_PRIVATE_KEY` | read from genesis accounts file (bootstrap only, never CLI-managed) |

## Cryptography

- **QSH sponge**: exact Rust port of the engine's hardened sponge
  (multi-rate padding, 24 rounds, domain separation). Unit tests pin
  engine vectors — any drift fails the build.
- **Addresses**: canonical `0x` + last-40-hex of `QSH_KDC-ADDR(pubkey)`,
  identical to engine wallets (verified byte-for-byte against all
  genesis pubkeys).
- **Wallets**: ML-DSA-65 via the `ml-dsa` crate. `new` emits 32-byte
  seeds; `import` also accepts engine FIPS secrets, reconstructing the
  public key through a from-scratch FIPS decode + NTT implementation
  verified against a NIST KAT.
- Engine-key signing is unsupported in v0.1 (expanded key unavailable);
  `sign` works with seed-format keys.

## Testing

```bash
cargo test            # 30 unit + 13 CLI + 5 live-integration (48 total)
```

- **Unit** (`src/`): QSH engine vectors, KSEL-8 frozen selectors,
  avalanche sanity, KDC↔proton exactness, genesis parser, keystore,
  FIPS/NTT math (roundtrips, convolution theorem, NIST KAT sk→pk),
  compose rendering (canonical ports, no collisions for 10 nodes/role,
  no fixed names, mainnet without bootstrap), node/mode guards,
  release-URL pattern.
- **CLI** (`tests/cli.rs`, assert_cmd): version/help/manual/config,
  offline wallet ops, argument validation, compose file generation. No
  node needed.
- **Integration** (`tests/integration.rs`): live testnet, skips
  gracefully offline — heights advance, faucet→transfer moves value,
  mempool/validators live, pagination works, engine-key addresses match.

## Releases (maintainers)

Binaries are never committed — every push to `master` builds and
publishes them automatically:

```bash
git push origin master
# → .github/workflows/release.yml: offline tests → Linux + Windows
#   builds → GitHub Release vX.Y.Z (version from Cargo.toml) with
#   kdc-linux-amd64 + kdc-windows-x86_64-vX.Y.Z.zip
#   (kdc.exe + README.md + SHA256SUMS.txt)
```

New version = bump `version` in `Cargo.toml` and push. Pushing again
on the same version just refreshes that release's assets (the `vX.Y.Z`
tag is moved to the latest commit). Manual re-run without pushing:
Actions → release → Run workflow. To reproduce the Windows zip
locally: `./dist-windows.sh` (needs `cargo install cargo-xwin`; MSVC
toolchain is downloaded automatically, no sudo or Visual Studio).
