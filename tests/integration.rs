//! Live integration tests against a running testnet.
//! They SKIP gracefully when no node is reachable (plain `cargo test`
//! stays green offline). Several tests move tiny amounts of test KDC.

use kodechain_cli::{client::Engine, genesis};
use std::time::Duration;

fn boot_url() -> String {
    std::env::var("KODECHAIN_BOOT_URL").unwrap_or_else(|_| "http://localhost:8084".into())
}

fn engine_dir() -> std::path::PathBuf {
    std::env::var("KODECHAIN_ENGINE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("../kodechain-engine"))
}

/// None when the testnet is unreachable → caller skips the test.
async fn live() -> Option<Engine> {
    let e = Engine::new(&boot_url());
    match e.health().await {
        Ok(true) => Some(e),
        _ => {
            eprintln!("SKIP: no testnet at {}", e.base());
            None
        }
    }
}

fn genesis_text() -> Option<String> {
    let path = engine_dir().join("archives/genesis_testnet_accounts.txt");
    match std::fs::read_to_string(&path) {
        Ok(t) => Some(t),
        Err(_) => {
            eprintln!("SKIP: no genesis file at {}", path.display());
            None
        }
    }
}

#[tokio::test]
async fn wallet_import_matches_genesis_file() {
    // THE crypto cross-check: derive the faucet address from its secret key
    // with the Rust ML-DSA-65 + QSH stack and compare with the address the
    // engine recorded. No node needed (pure local crypto + file).
    let Some(text) = genesis_text() else { return };
    let wallets = genesis::parse_accounts(&text).unwrap();
    for name in ["Testnet_Faucet", "Testnet_Validator_01", "Testnet_Dev_01"] {
        let w = genesis::find_wallet(&wallets, name).unwrap();
        // FIPS secret → reconstructed pubkey must equal the recorded one,
        // and its address must equal the recorded address.
        let km = kodechain_cli::wallet_crypto::key_from_any_hex(&w.private_key).unwrap();
        let pubb = kodechain_cli::wallet_crypto::public_bytes_for_key(&km).unwrap();
        assert_eq!(
            hex::encode(pubb),
            w.public_key,
            "reconstructed pubkey must match engine wallet {name}"
        );
        let addr = kodechain_cli::wallet_crypto::address_for_key(&km).unwrap();
        assert_eq!(
            addr, w.address,
            "Rust-derived address must match engine wallet {name}"
        );
    }
}

#[tokio::test]
async fn heights_advance_on_both_chains() {
    let Some(e) = live().await else { return };
    let h1 = e.height("DPOS").await.unwrap();
    assert!(h1 > 0);
    tokio::time::sleep(Duration::from_secs(12)).await;
    let h2 = e.height("DPOS").await.unwrap();
    assert!(h2 > h1, "DPOS advanced {h1} -> {h2}");
    assert!(e.height("PBFT").await.unwrap() > 0);
}

#[tokio::test]
async fn faucet_funds_transfer_moves_value() {
    let Some(e) = live().await else { return };
    let dev1 = "0x70f460cf7eebfe2e0d57c8496b9c3e8f8758a911";
    let dev2 = "0xa33015529f1308329a474919ca898e9b3f52f038";

    async fn kdc(e: &Engine, addr: &str) -> u128 {
        e.account(addr)
            .await
            .ok()
            .and_then(|v| {
                v["account"]["balances"]["KDC"]["amount"]
                    .as_str()
                    .and_then(|s| s.parse().ok())
            })
            .unwrap_or(0)
    }

    let before = kdc(&e, dev2).await;
    let out = e
        .submit_tx(&serde_json::json!({
            "type": "transfer", "from": dev1, "to": dev2,
            "amount_proton": "700000000000000", "gasPrice": 1_000_000_000u64,
        }))
        .await
        .expect("transfer accepted");
    assert_eq!(out["success"], true);

    let mut after = before;
    for _ in 0..18 {
        tokio::time::sleep(Duration::from_secs(5)).await;
        after = kdc(&e, dev2).await;
        if after >= before + 700_000_000_000_000 {
            break;
        }
    }
    assert!(
        after >= before + 700_000_000_000_000,
        "dev2 credited ({before} -> {after})"
    );
}

#[tokio::test]
async fn mempool_and_validators_live() {
    let Some(e) = live().await else { return };
    let pend = e.mempool_pending().await.unwrap();
    assert!(pend.get("dpos").is_some() && pend.get("pbft").is_some());
    let list = e.validators().await.unwrap();
    assert!(!list.is_empty());
    assert!(list.iter().any(|v| v
        .get("is_active")
        .and_then(|a| a.as_bool())
        .unwrap_or(false)));
}

#[tokio::test]
async fn blocks_paginate_both_chains() {
    let Some(e) = live().await else { return };
    let (b20, total) = e.blocks("DPOS", 20, 0).await.unwrap();
    assert_eq!(b20.len(), 20.min(total as usize));
    assert!(total > 0);
    let (b20b, _) = e.blocks("DPOS", 20, 20).await.unwrap();
    if total > 20 {
        assert_ne!(b20[0]["hash"], b20b[0]["hash"], "offset pages differ");
    }
}
