use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use url::Url;

/// Configuration for an RPC endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointConfig {
    pub name: String,
    pub http_url: String,
    pub ws_url: Option<String>,
    pub weight: u32,
}

impl EndpointConfig {
    pub fn new(name: impl Into<String>, http_url: impl Into<String>) -> Self {
        let http = http_url.into();
        let ws = if let Ok(parsed) = Url::parse(&http) {
            match parsed.scheme() {
                "https" => Some(http.replacen("https://", "wss://", 1)),
                "http" => Some(http.replacen("http://", "ws://", 1)),
                _ => None,
            }
        } else {
            None
        };

        Self {
            name: name.into(),
            http_url: http,
            ws_url: ws,
            weight: 100,
        }
    }

    pub fn with_ws(mut self, ws_url: impl Into<String>) -> Self {
        self.ws_url = Some(ws_url.into());
        self
    }

    /// Validates that the endpoint URL is a valid HTTP/HTTPS address.
    pub fn validate(&self) -> bool {
        if let Ok(url) = Url::parse(&self.http_url) {
            url.scheme() == "http" || url.scheme() == "https"
        } else {
            false
        }
    }
}

/// A measured latency sample from an RPC HTTP check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcLatencySample {
    pub endpoint: String,
    pub timestamp: DateTime<Utc>,
    pub rtt_ms: f64,
    pub slot: u64,
    pub commitment_ms: f64,
    pub is_ok: bool,
    pub error_msg: Option<String>,
}

/// A measured slot update sample from an RPC WebSocket feed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsSlotSample {
    pub endpoint: String,
    pub timestamp: DateTime<Utc>,
    pub slot: u64,
    pub parent_slot: u64,
    pub root_slot: u64,
    pub lag_slots: i64,
    pub jitter_ms: f64,
}

/// Aggregated real-time node health score and performance metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeHealthScore {
    pub endpoint: String,
    pub name: String,
    /// Overall health score from 0.0 (dead/useless) to 100.0 (optimal).
    pub score: f64,
    pub avg_rtt_ms: f64,
    pub p95_rtt_ms: f64,
    pub min_rtt_ms: f64,
    pub max_rtt_ms: f64,
    pub last_slot: u64,
    pub slot_lag: i64,
    pub error_rate: f64,
    pub consecutive_errors: u32,
    pub is_healthy: bool,
    pub last_updated: DateTime<Utc>,
}

impl NodeHealthScore {
    pub fn new(endpoint: String, name: String) -> Self {
        Self {
            endpoint,
            name,
            score: 100.0,
            avg_rtt_ms: 0.0,
            p95_rtt_ms: 0.0,
            min_rtt_ms: 0.0,
            max_rtt_ms: 0.0,
            last_slot: 0,
            slot_lag: 0,
            error_rate: 0.0,
            consecutive_errors: 0,
            is_healthy: true,
            last_updated: Utc::now(),
        }
    }
}

/// Priority fee percentiles calculated from recent blocks.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PriorityFeePercentiles {
    pub slot: u64,
    pub p25: u64,
    pub p50: u64,
    pub p75: u64,
    pub p90: u64,
    pub p99: u64,
    pub min: u64,
    pub max: u64,
    pub sample_count: usize,
    pub timestamp: DateTime<Utc>,
}

/// Benchmark sample for DEX route quote latency (e.g., Jupiter API).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DexRouteLatencySample {
    pub dex_name: String,
    pub input_mint: String,
    pub output_mint: String,
    pub in_amount: u64,
    pub out_amount: u64,
    pub quote_rtt_ms: f64,
    pub price_impact_pct: f64,
    pub timestamp: DateTime<Utc>,
    pub is_ok: bool,
    pub error_msg: Option<String>,
}
