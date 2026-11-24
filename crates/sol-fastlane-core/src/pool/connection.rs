use crate::error::{FastlaneError, Result};
use crate::probe::health::NodeHealthTracker;
use crate::probe::rpc::RpcProbe;
use crate::types::{EndpointConfig, NodeHealthScore};
use parking_lot::RwLock;
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tracing::{info, warn};

#[derive(Debug, Clone)]
pub struct PoolConfig {
    pub probe_interval_ms: u64,
    pub max_slot_lag: i64,
    pub failover_retries: usize,
    pub request_timeout_ms: u64,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            probe_interval_ms: 1000,
            max_slot_lag: 3,
            failover_retries: 2,
            request_timeout_ms: 3000,
        }
    }
}

struct NodeEntry {
    config: EndpointConfig,
    probe: RpcProbe,
    tracker: Arc<NodeHealthTracker>,
}

/// Dynamic Connection Pool that continuously benchmarks nodes and auto-routes requests to the fastest, healthiest RPC.
pub struct DynamicConnectionPool {
    config: PoolConfig,
    nodes: RwLock<Vec<NodeEntry>>,
    cluster_tip_slot: AtomicU64,
    client: reqwest::Client,
    shutdown_tx: broadcast::Sender<()>,
}

impl DynamicConnectionPool {
    pub fn new(config: PoolConfig) -> Arc<Self> {
        let (shutdown_tx, _) = broadcast::channel(1);
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(config.request_timeout_ms))
            .tcp_nodelay(true)
            .build()
            .unwrap_or_default();

        Arc::new(Self {
            config,
            nodes: RwLock::new(Vec::new()),
            cluster_tip_slot: AtomicU64::new(0),
            client,
            shutdown_tx,
        })
    }

    /// Adds an RPC endpoint to the connection pool.
    pub fn add_endpoint(&self, endpoint: EndpointConfig) {
        let probe = RpcProbe::new(endpoint.name.clone(), endpoint.http_url.clone());
        let tracker = Arc::new(NodeHealthTracker::new(endpoint.name.clone(), endpoint.http_url.clone()));

        let entry = NodeEntry {
            config: endpoint,
            probe,
            tracker,
        };

        self.nodes.write().push(entry);
    }

    /// Starts asynchronous background probing across all configured endpoints.
    pub fn start_background_probes(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let pool = Arc::clone(self);
        let mut shutdown_rx = pool.shutdown_tx.subscribe();
        let interval_ms = pool.config.probe_interval_ms;

        tokio::spawn(async move {
            info!("DynamicConnectionPool background probe started (interval: {}ms)", interval_ms);
            let mut ticker = tokio::time::interval(Duration::from_millis(interval_ms));

            loop {
                tokio::select! {
                    _ = shutdown_rx.recv() => {
                        info!("DynamicConnectionPool background probe shutting down");
                        break;
                    }
                    _ = ticker.tick() => {
                        pool.probe_all_nodes().await;
                    }
                }
            }
        })
    }

    /// Executes a single probe cycle across all endpoints concurrently.
    pub async fn probe_all_nodes(&self) {
        let entries: Vec<(RpcProbe, Arc<NodeHealthTracker>)> = {
            let nodes = self.nodes.read();
            nodes.iter().map(|n| (n.probe.clone(), Arc::clone(&n.tracker))).collect()
        };

        if entries.is_empty() {
            return;
        }

        let mut tasks = Vec::with_capacity(entries.len());
        for (probe, tracker) in entries {
            tasks.push(tokio::spawn(async move {
                let sample = probe.ping_slot().await.unwrap_or_else(|e| crate::types::RpcLatencySample {
                    endpoint: probe.endpoint().to_string(),
                    timestamp: chrono::Utc::now(),
                    rtt_ms: 9999.0,
                    slot: 0,
                    commitment_ms: 9999.0,
                    is_ok: false,
                    error_msg: Some(e.to_string()),
                });

                tracker.record_sample(sample);
            }));
        }

        for task in tasks {
            let _ = task.await;
        }

        // Update cluster tip slot to the maximum slot observed across all trackers
        let max_slot = {
            let nodes = self.nodes.read();
            nodes.iter().map(|n| n.tracker.last_slot()).max().unwrap_or(0)
        };

        if max_slot > self.cluster_tip_slot.load(Ordering::Relaxed) {
            self.cluster_tip_slot.store(max_slot, Ordering::Relaxed);
        }
    }

    /// Returns ranked list of health scores for all nodes.
    pub fn get_health_scores(&self) -> Vec<NodeHealthScore> {
        let cluster_tip = self.cluster_tip_slot.load(Ordering::Relaxed);
        let nodes = self.nodes.read();
        let mut scores: Vec<NodeHealthScore> = nodes
            .iter()
            .map(|n| n.tracker.compute_health(cluster_tip))
            .collect();

        // Sort descending by health score, then ascending by avg_rtt
        scores.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.avg_rtt_ms.partial_cmp(&b.avg_rtt_ms).unwrap_or(std::cmp::Ordering::Equal))
        });

        scores
    }

    /// Returns the currently fastest, healthiest RPC endpoint.
    pub fn get_best_endpoint(&self) -> Option<EndpointConfig> {
        let cluster_tip = self.cluster_tip_slot.load(Ordering::Relaxed);
        let nodes = self.nodes.read();

        nodes
            .iter()
            .filter_map(|n| {
                let health = n.tracker.compute_health(cluster_tip);
                if health.is_healthy && health.slot_lag <= self.config.max_slot_lag {
                    Some((n.config.clone(), health))
                } else {
                    None
                }
            })
            .max_by(|a, b| {
                a.1.score
                    .partial_cmp(&b.1.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(config, _)| config)
            .or_else(|| {
                // Fallback to highest score even if marked degraded
                nodes.first().map(|n| n.config.clone())
            })
    }

    /// Dispatches a JSON-RPC request with automatic sub-millisecond failover.
    pub async fn dispatch_rpc<T: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<T> {
        let scores = self.get_health_scores();
        let healthy_endpoints: Vec<String> = scores
            .iter()
            .filter(|s| s.is_healthy)
            .map(|s| s.endpoint.clone())
            .collect();

        let fallback_endpoints: Vec<String> = if healthy_endpoints.is_empty() {
            scores.iter().map(|s| s.endpoint.clone()).collect()
        } else {
            healthy_endpoints
        };

        if fallback_endpoints.is_empty() {
            return Err(FastlaneError::NoHealthyEndpoints);
        }

        let retries = self.config.failover_retries.min(fallback_endpoints.len());
        let mut last_error = None;

        for endpoint in fallback_endpoints.iter().take(retries + 1) {
            let payload = json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": method,
                "params": params.clone()
            });

            match self.client.post(endpoint).json(&payload).send().await {
                Ok(resp) => {
                    if !resp.status().is_success() {
                        last_error = Some(FastlaneError::Rpc {
                            endpoint: endpoint.clone(),
                            message: format!("HTTP error {}", resp.status()),
                        });
                        continue;
                    }

                    let body: serde_json::Value = match resp.json().await {
                        Ok(b) => b,
                        Err(e) => {
                            last_error = Some(FastlaneError::Http(e));
                            continue;
                        }
                    };

                    if let Some(err) = body.get("error") {
                        last_error = Some(FastlaneError::Rpc {
                            endpoint: endpoint.clone(),
                            message: err.to_string(),
                        });
                        continue;
                    }

                    if let Some(result_val) = body.get("result") {
                        let parsed: T = serde_json::from_value(result_val.clone())?;
                        return Ok(parsed);
                    } else {
                        last_error = Some(FastlaneError::ParseError("Missing result key in RPC response".to_string()));
                    }
                }
                Err(e) => {
                    warn!("Request to {} failed: {}. Retrying on failover node...", endpoint, e);
                    last_error = Some(FastlaneError::Http(e));
                }
            }
        }

        Err(last_error.unwrap_or(FastlaneError::NoHealthyEndpoints))
    }

    /// Gracefully stops background tasks.
    pub fn stop(&self) {
        let _ = self.shutdown_tx.send(());
    }

    /// Queries the current slot with automatic failover across healthy nodes.
    pub async fn get_slot(&self) -> Result<u64> {
        self.dispatch_rpc("getSlot", json!([{"commitment": "processed"}])).await
    }

    /// Fetches the latest blockhash with automatic failover.
    pub async fn get_latest_blockhash(&self) -> Result<String> {
        let res: serde_json::Value = self
            .dispatch_rpc("getLatestBlockhash", json!([{"commitment": "confirmed"}]))
            .await?;
        res.get("value")
            .and_then(|v| v.get("blockhash"))
            .and_then(|b| b.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| FastlaneError::ParseError("Missing blockhash in response".to_string()))
    }

    /// Queries account balance (in lamports) for a public key with automatic failover.
    pub async fn get_balance(&self, pubkey: &str) -> Result<u64> {
        let res: serde_json::Value = self
            .dispatch_rpc("getBalance", json!([pubkey, {"commitment": "confirmed"}]))
            .await?;
        res.get("value")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| FastlaneError::ParseError("Missing value in getBalance response".to_string()))
    }

    /// Broadcasts a signed base64 transaction with automatic failover.
    pub async fn send_transaction(&self, base64_tx: &str) -> Result<String> {
        self.dispatch_rpc(
            "sendTransaction",
            json!([
                base64_tx,
                {
                    "encoding": "base64",
                    "skipPreflight": false,
                    "preflightCommitment": "processed"
                }
            ]),
        )
        .await
    }
}
