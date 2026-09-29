//! CLI-level tests (no node needed): help, manual, version, config,
//! offline wallet ops, and argument validation.

use assert_cmd::Command;
use predicates::prelude::*;

fn kdc() -> Command {
    Command::cargo_bin("kdc").unwrap()
}

#[test]
fn version_flag() {
    kdc()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("kdc"));
}

#[test]
fn top_help_lists_commands() {
    for cmd in [
        "node",
        "wallet",
        "faucet",
        "transfer",
        "critical",
        "chain",
        "tx",
        "mempool",
        "validators",
        "network",
        "manual",
        "config",
    ] {
        kdc()
            .arg("--help")
            .assert()
            .success()
            .stdout(predicate::str::contains(cmd));
    }
}

#[test]
fn manual_topics() {
    for topic in [
        "overview",
        "node",
        "mainnet",
        "wallet",
        "transactions",
        "chains",
        "troubleshoot",
    ] {
        kdc().args(["manual", topic]).assert().success();
    }
    // unknown topic lists available ones
    kdc()
        .args(["manual", "nope"])
        .assert()
        .success()
        .stdout(predicate::str::contains("overview"));
}

#[test]
fn wallet_new_offline() {
    kdc()
        .args(["wallet", "new", "--name", "cli-test"])
        .env("KDC_HOME", "/tmp/kdc-cli-test")
        .assert()
        .success()
        .stdout(predicate::str::contains("0x"));
    std::fs::remove_dir_all("/tmp/kdc-cli-test").ok();
}

#[test]
fn wallet_address_derives_canonical() {
    // 32-byte seed → 0x + 40 hex, deterministic
    let out = kdc()
        .args(["wallet", "address", "--key", &"07".repeat(32)])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let s = String::from_utf8(out).unwrap();
    assert!(s.contains("0x") && s.len() >= 42, "{s}");
    // same key twice → same address
    let out2 = kdc()
        .args(["wallet", "address", "--key", &"07".repeat(32)])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(s, String::from_utf8(out2).unwrap());
}

#[test]
fn transfer_requires_amount() {
    kdc()
        .args(["transfer", "--from", "0xabc", "--to", "0xdef"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--kdc").or(predicate::str::contains("proton")));
}

#[test]
fn config_shows_resolution() {
    kdc()
        .arg("config")
        .assert()
        .success()
        .stdout(predicate::str::contains("engine_dir").or(predicate::str::contains("bootstrap")));
}

#[test]
fn mainnet_refuses_bootstrap() {
    for mode in ["bootstrap", "all"] {
        kdc()
            .args(["--network", "mainnet", "node", "start", "--mode", mode])
            .assert()
            .failure()
            .stderr(predicate::str::contains("SINGLE"));
    }
}

#[test]
fn mainnet_faucet_refused() {
    kdc()
        .args([
            "--network",
            "mainnet",
            "faucet",
            "0x0000000000000000000000000000000000000001",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("mainnet"));
}

#[test]
fn setup_detects_ready_binary() {
    // engine checkout has a built binary: setup must short-circuit offline
    kdc()
        .args([
            "setup",
            "--engine-dir",
            "../kodechain-engine",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("ready").or(predicate::str::contains("version")));
}

#[test]
fn setup_refuses_non_http() {
    kdc()
        .args(["setup", "--from-url", "ftp://example.com/x", "--engine-dir", "."])
        .assert()
        .failure()
        .stderr(predicate::str::contains("non-HTTP").or(predicate::str::contains("refusing")));
}

#[test]
fn doctor_runs_offline() {
    kdc()
        .args(["doctor", "--engine-dir", "."])
        .assert()
        .success()
        .stdout(predicate::str::contains("engine binary"));
}

#[test]
fn compose_renders_n_validators() {
    let dir = "/tmp/kdc-compose-cli-test";
    std::fs::remove_dir_all(dir).ok();
    std::fs::create_dir_all(dir).unwrap();
    let out = format!("{dir}/docker-compose.yml");
    kdc()
        .env("KDC_HOME", dir)
        .args([
            "compose", "--dpos", "2", "--pbft", "1",
            "--dpos-ids", "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa,0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "--pbft-ids", "0xcccccccccccccccccccccccccccccccccccccccc",
            "--bootstrap-nodes", "enode://0xboot@127.0.0.1:30403?http_port=8084",
            "--network", "mainnet",
            "--output", &out,
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("2 dpos + 1 pbft"));
    let yml = std::fs::read_to_string(&out).unwrap();
    for svc in ["dpos-1:", "dpos-2:", "pbft-1:"] {
        assert!(yml.contains(svc), "missing {svc}");
    }
    assert!(!yml.contains("dpos01") && !yml.contains("pbft01"), "no fixed names");
    assert!(!yml.contains("bootstrap:"), "mainnet has no bootstrap service");
    std::fs::remove_dir_all(dir).ok();
}
