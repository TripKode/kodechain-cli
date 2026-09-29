//! Async HTTP client for the KodeChain engine API.
//! All chain reads/writes go through here; pages/commands never touch URLs.

use anyhow::{Context, Result};
use serde_json::Value;

#[derive(Clone)]
pub struct Engine {
    base: String,
    http: reqwest::Client,
}

impl Engine {
    pub fn new(base: &str) -> Self {
        Self {
            base: base.trim_end_matches('/').to_string(),
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("http client"),
        }
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    async fn get(&self, path: &str, chain: Option<&str>) -> Result<Value> {
        let mut req = self.http.get(format!("{}{}", self.base, path));
        if let Some(c) = chain {
            req = req.header("X-Consensus-Type", c);
        }
        let resp = req.send().await.context("node unreachable")?;
        let status = resp.status();
        let body: Value = resp.json().await.context("invalid JSON from node")?;
        if !status.is_success() {
            anyhow::bail!("node HTTP {status}: {}", body);
        }
        Ok(body)
    }

    async fn post(&self, path: &str, body: &Value) -> Result<Value> {
        let resp = self
            .http
            .post(format!("{}{}", self.base, path))
            .json(body)
            .send()
            .await
            .context("node unreachable")?;
        let status = resp.status();
        let out: Value = resp.json().await.context("invalid JSON from node")?;
        if !status.is_success() {
            anyhow::bail!("node HTTP {status}: {}", out);
        }
        Ok(out)
    }

    pub async fn health(&self) -> Result<bool> {
        let v = self.get("/api/node/health", None).await?;
        Ok(v.get("status").and_then(|s| s.as_str()).unwrap_or("") != "unhealthy")
    }

    pub async fn height(&self, chain: &str) -> Result<u64> {
        let v = self.get("/api/sync/height", Some(chain)).await?;
        v.get("height")
            .and_then(|h| h.as_u64())
            .ok_or_else(|| anyhow::anyhow!("no height in response: {v}"))
    }

    pub async fn blocks(&self, chain: &str, limit: u32, offset: u64) -> Result<(Vec<Value>, u64)> {
        let v = self
            .get(
                &format!("/api/block/all?limit={limit}&offset={offset}"),
                Some(chain),
            )
            .await?;
        let total = v.get("total").and_then(|t| t.as_u64()).unwrap_or(0);
        let blocks = v
            .get("blocks")
            .and_then(|b| b.as_array())
            .cloned()
            .unwrap_or_default();
        Ok((blocks, total))
    }

    pub async fn account(&self, address: &str) -> Result<Value> {
        self.get(&format!("/api/smart-accounts/{address}"), None)
            .await
    }

    pub async fn create_account(&self, address: &str) -> Result<Value> {
        self.post(
            "/api/smart-accounts",
            &serde_json::json!({ "address": address }),
        )
        .await
    }

    pub async fn faucet(&self, address: &str) -> Result<Value> {
        self.post(
            "/api/testnet/faucet",
            &serde_json::json!({ "recipientAddress": address }),
        )
        .await
    }

    pub async fn submit_tx(&self, body: &Value) -> Result<Value> {
        let out = self.post("/api/transaction/create", body).await?;
        if out.get("success").and_then(|s| s.as_bool()) != Some(true) {
            anyhow::bail!("rejected: {}", out);
        }
        Ok(out)
    }

    pub async fn mempool_pending(&self) -> Result<Value> {
        self.get("/api/mempool/pending", None).await
    }

    pub async fn mempool_stats(&self) -> Result<Value> {
        self.get("/api/transaction/mempool/stats", None).await
    }

    pub async fn validators(&self) -> Result<Vec<Value>> {
        let v = self.get("/api/validator/list", None).await?;
        Ok(v.get("validators")
            .and_then(|l| l.as_array())
            .cloned()
            .unwrap_or_default())
    }

    pub async fn peers(&self) -> Result<Value> {
        self.get("/api/network/peers", None).await
    }

    pub async fn contacts(&self) -> Result<Value> {
        self.get("/api/p2p/contacts", None).await
    }
}

/// Extract a u256 word from 0x-hex return_data at word index.
pub fn return_word(return_data: &str, index: usize) -> Option<u128> {
    let h = return_data.strip_prefix("0x")?;
    let word = h.get(index * 64..(index + 1) * 64)?;
    u128::from_str_radix(&word[word.len().saturating_sub(32)..], 16).ok()
}
