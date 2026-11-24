use crate::error::{FastlaneError, Result};
use crate::types::PriorityFeePercentiles;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FeeUrgency {
    Low,     // 25th percentile
    Medium,  // 50th percentile
    High,    // 75th percentile
    Turbo,   // 90th percentile
    MaxLanding, // 99th percentile
}

/// Dynamic priority fee estimator using recent block prioritization fee percentiles.
#[derive(Debug, Clone)]
pub struct DynamicFeeEstimator {
    client: reqwest::Client,
}

impl DynamicFeeEstimator {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(4000))
            .tcp_nodelay(true)
            .build()
            .unwrap_or_default();

        Self { client }
    }

    /// Fetches recent prioritization fees from the given RPC endpoint and calculates percentiles.
    pub async fn fetch_percentiles(
        &self,
        rpc_url: &str,
        account_keys: Option<&[&str]>,
    ) -> Result<PriorityFeePercentiles> {
        let params = match account_keys {
            Some(keys) if !keys.is_empty() => json!([keys]),
            _ => json!([]),
        };

        let payload = json!({
            "jsonrpc": "2.0",
            "id": 10,
            "method": "getRecentPrioritizationFees",
            "params": params
        });

        let resp = self
            .client
            .post(rpc_url)
            .json(&payload)
            .send()
            .await
            .map_err(FastlaneError::Http)?;

        let body: serde_json::Value = resp.json().await.map_err(FastlaneError::Http)?;

        if let Some(err) = body.get("error") {
            return Err(FastlaneError::Rpc {
                endpoint: rpc_url.to_string(),
                message: err.to_string(),
            });
        }

        let fees_array = body
            .get("result")
            .and_then(|r| r.as_array())
            .ok_or_else(|| FastlaneError::ParseError("Missing result array in getRecentPrioritizationFees".to_string()))?;

        if fees_array.is_empty() {
            return Ok(PriorityFeePercentiles {
                timestamp: Utc::now(),
                ..Default::default()
            });
        }

        let mut fees: Vec<u64> = fees_array
            .iter()
            .filter_map(|item| item.get("prioritizationFee").and_then(|f| f.as_u64()))
            .collect();

        if fees.is_empty() {
            return Ok(PriorityFeePercentiles {
                timestamp: Utc::now(),
                ..Default::default()
            });
        }

        fees.sort_unstable();

        let count = fees.len();
        let last_slot = fees_array
            .last()
            .and_then(|i| i.get("slot").and_then(|s| s.as_u64()))
            .unwrap_or(0);

        let p25 = Self::percentile(&fees, 0.25);
        let p50 = Self::percentile(&fees, 0.50);
        let p75 = Self::percentile(&fees, 0.75);
        let p90 = Self::percentile(&fees, 0.90);
        let p99 = Self::percentile(&fees, 0.99);
        let min = *fees.first().unwrap_or(&0);
        let max = *fees.last().unwrap_or(&0);

        Ok(PriorityFeePercentiles {
            slot: last_slot,
            p25,
            p50,
            p75,
            p90,
            p99,
            min,
            max,
            sample_count: count,
            timestamp: Utc::now(),
        })
    }

    /// Returns recommended priority fee (micro-lamports per compute unit) based on urgency.
    pub fn recommend_fee(&self, percentiles: &PriorityFeePercentiles, urgency: FeeUrgency) -> u64 {
        match urgency {
            FeeUrgency::Low => percentiles.p25,
            FeeUrgency::Medium => percentiles.p50,
            FeeUrgency::High => percentiles.p75,
            FeeUrgency::Turbo => percentiles.p90,
            FeeUrgency::MaxLanding => percentiles.p99,
        }
    }

    fn percentile(sorted: &[u64], pct: f64) -> u64 {
        if sorted.is_empty() {
            return 0;
        }
        let clamped_pct = pct.clamp(0.0, 1.0);
        let index = ((sorted.len() as f64 * clamped_pct).ceil() as usize).saturating_sub(1);
        sorted[index.min(sorted.len() - 1)]
    }
}

impl Default for DynamicFeeEstimator {
    fn default() -> Self {
        Self::new()
    }
}
