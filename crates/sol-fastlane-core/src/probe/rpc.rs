use crate::error::{FastlaneError, Result};
use crate::types::RpcLatencySample;
use chrono::Utc;
use serde_json::json;
use std::time::Instant;

/// Probes an RPC HTTP endpoint for latency, slot, and commitment metrics.
#[derive(Debug, Clone)]
pub struct RpcProbe {
    client: reqwest::Client,
    endpoint: String,
    name: String,
}

impl RpcProbe {
    pub fn new(name: impl Into<String>, endpoint: impl Into<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(5000))
            .tcp_nodelay(true)
            .build()
            .unwrap_or_default();

        Self {
            client,
            endpoint: endpoint.into(),
            name: name.into(),
        }
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Single round-trip ping checking `getSlot` with commitment "processed".
    pub async fn ping_slot(&self) -> Result<RpcLatencySample> {
        let payload = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getSlot",
            "params": [{"commitment": "processed"}]
        });

        let start = Instant::now();
        let resp_result = self.client.post(&self.endpoint).json(&payload).send().await;

        let rtt_ms = start.elapsed().as_secs_f64() * 1000.0;
        let timestamp = Utc::now();

        match resp_result {
            Ok(resp) => {
                let status = resp.status();
                if !status.is_success() {
                    return Ok(RpcLatencySample {
                        endpoint: self.endpoint.clone(),
                        timestamp,
                        rtt_ms,
                        slot: 0,
                        commitment_ms: rtt_ms,
                        is_ok: false,
                        error_msg: Some(format!("HTTP status: {}", status)),
                    });
                }

                let body: serde_json::Value = resp.json().await.map_err(FastlaneError::Http)?;
                if let Some(err) = body.get("error") {
                    return Ok(RpcLatencySample {
                        endpoint: self.endpoint.clone(),
                        timestamp,
                        rtt_ms,
                        slot: 0,
                        commitment_ms: rtt_ms,
                        is_ok: false,
                        error_msg: Some(err.to_string()),
                    });
                }

                let slot = body
                    .get("result")
                    .and_then(|r| r.as_u64())
                    .unwrap_or(0);

                Ok(RpcLatencySample {
                    endpoint: self.endpoint.clone(),
                    timestamp,
                    rtt_ms,
                    slot,
                    commitment_ms: rtt_ms,
                    is_ok: true,
                    error_msg: None,
                })
            }
            Err(e) => Ok(RpcLatencySample {
                endpoint: self.endpoint.clone(),
                timestamp,
                rtt_ms,
                slot: 0,
                commitment_ms: rtt_ms,
                is_ok: false,
                error_msg: Some(e.to_string()),
            }),
        }
    }

    /// Measures commitment response latency by checking latest blockhash confirmation.
    pub async fn ping_commitment_latency(&self) -> Result<RpcLatencySample> {
        let payload = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "getLatestBlockhash",
            "params": [{"commitment": "confirmed"}]
        });

        let start = Instant::now();
        let resp_result = self.client.post(&self.endpoint).json(&payload).send().await;
        let rtt_ms = start.elapsed().as_secs_f64() * 1000.0;
        let timestamp = Utc::now();

        match resp_result {
            Ok(resp) => {
                let body: serde_json::Value = resp.json().await.map_err(FastlaneError::Http)?;
                let slot = body
                    .get("result")
                    .and_then(|r| r.get("context"))
                    .and_then(|c| c.get("slot"))
                    .and_then(|s| s.as_u64())
                    .unwrap_or(0);

                let is_ok = body.get("error").is_none() && slot > 0;
                let error_msg = body.get("error").map(|e| e.to_string());

                Ok(RpcLatencySample {
                    endpoint: self.endpoint.clone(),
                    timestamp,
                    rtt_ms,
                    slot,
                    commitment_ms: rtt_ms,
                    is_ok,
                    error_msg,
                })
            }
            Err(e) => Ok(RpcLatencySample {
                endpoint: self.endpoint.clone(),
                timestamp,
                rtt_ms,
                slot: 0,
                commitment_ms: rtt_ms,
                is_ok: false,
                error_msg: Some(e.to_string()),
            }),
        }
    }
}
