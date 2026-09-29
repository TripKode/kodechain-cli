//! Parser for `archives/genesis_testnet_accounts.txt`.
//! Block format per wallet:
//!   NAME: <name>
//!   ROLE: <role>
//!   ADDRESS: 0x...
//!   PUBLIC KEY: <hex, possibly multiline>
//!   PRIVATE KEY: <hex, possibly multiline + trailing garbage trimmed to size>
//! ML-DSA-65 sizes: public 1952 bytes (3904 hex), secret 4032 bytes (8064 hex).

use anyhow::{bail, Result};

#[derive(Debug, Clone)]
pub struct GenesisWallet {
    pub name: String,
    pub role: String,
    pub address: String,
    pub public_key: String,
    pub private_key: String,
}

fn clean_hex(s: &str, want_len: usize) -> String {
    let h: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    h[..h.len().min(want_len)].to_string()
}

pub fn parse_accounts(text: &str) -> Result<Vec<GenesisWallet>> {
    let mut out = Vec::new();
    for block in text.split("NAME: ").skip(1) {
        let name = block.lines().next().unwrap_or("").trim().to_string();
        let addr = block
            .lines()
            .find_map(|l| l.trim().strip_prefix("ADDRESS: "))
            .unwrap_or("")
            .trim()
            .to_string();
        if addr.is_empty() {
            continue;
        }
        let role = block
            .lines()
            .find_map(|l| l.trim().strip_prefix("ROLE: "))
            .unwrap_or("")
            .trim()
            .to_string();
        // PUBLIC KEY: everything hex until PRIVATE KEY:
        let pub_raw = block
            .split("PUBLIC KEY:")
            .nth(1)
            .unwrap_or("")
            .split("PRIVATE KEY:")
            .next()
            .unwrap_or("");
        let priv_raw = block.split("PRIVATE KEY:").nth(1).unwrap_or("");
        out.push(GenesisWallet {
            name,
            role,
            address: addr,
            public_key: clean_hex(pub_raw, 3904),
            private_key: clean_hex(priv_raw, 8064),
        });
    }
    if out.is_empty() {
        bail!("no wallets parsed (is this genesis_testnet_accounts.txt?)");
    }
    Ok(out)
}

pub fn find_wallet<'a>(wallets: &'a [GenesisWallet], name: &str) -> Result<&'a GenesisWallet> {
    wallets
        .iter()
        .find(|w| w.name == name)
        .ok_or_else(|| anyhow::anyhow!("wallet {name} not in genesis file"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = "KODECHAIN TESTNET ACCOUNTS\n\nNAME: Testnet_Faucet\nROLE: faucet\nADDRESS: 0xaaaabbbbccccddddeeeeffff0000111122223333\nPUBLIC KEY: 001122aabbcc\nPRIVATE KEY: deadbeef0123\n\nNAME: Testnet_Validator_01\nROLE: validator\nADDRESS: 0x1111222233334444555566667777888899990000\nPUBLIC KEY: 998877\nPRIVATE KEY: cafe01\n";

    #[test]
    fn parses_blocks() {
        let w = parse_accounts(FIXTURE).unwrap();
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].name, "Testnet_Faucet");
        assert_eq!(w[0].role, "faucet");
        assert_eq!(w[0].address, "0xaaaabbbbccccddddeeeeffff0000111122223333");
        assert_eq!(
            find_wallet(&w, "Testnet_Validator_01").unwrap().address,
            "0x1111222233334444555566667777888899990000"
        );
        assert!(find_wallet(&w, "Nope").is_err());
        assert!(parse_accounts("garbage").is_err());
    }
}
