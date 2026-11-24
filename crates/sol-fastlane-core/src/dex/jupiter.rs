use crate::error::{FastlaneError, Result};
use crate::types::DexRouteLatencySample;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::time::Instant;

pub const SOL_MINT: &str = "So11111111111111111111111111111111111111112";
pub const USDC_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JupiterQuoteParams {
    pub input_mint: String,
    pub output_mint: String,
    pub amount: u64,
    pub slippage_bps: u16,
}

impl Default for JupiterQuoteParams {
    fn default() -> Self {
        Self {
            input_mint: SOL_MINT.to_string(),
            output_mint: USDC_MINT.to_string(),
            amount: 1_000_000_000, // 1 SOL
            slippage_bps: 50,      // 0.5%
        }
    }
}

/// Measures and forecasts DEX quote-to-route execution latency via Jupiter API.
#[derive(Debug, Clone)]
pub struct JupiterForecastEngine {
    client: reqwest::Client,
    base_url: String,
}

impl JupiterForecastEngine {
    pub fn new(base_url: Option<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(5000))
            .tcp_nodelay(true)
            .build()
            .unwrap_or_default();

        Self {
            client,
            base_url: base_url.unwrap_or_else(|| "https://quote-api.jup.ag/v6".to_string()),
        }
    }

    /// Fetches a live quote for the token pair and benchmarks round-trip latency.
    pub async fn benchmark_quote(&self, params: &JupiterQuoteParams) -> Result<DexRouteLatencySample> {
        let endpoint_url = format!("{}/quote", self.base_url.trim_end_matches('/'));

        let start = Instant::now();
        let query_params = [
            ("inputMint", params.input_mint.as_str()),
            ("outputMint", params.output_mint.as_str()),
            ("amount", &params.amount.to_string()),
            ("slippageBps", &params.slippage_bps.to_string()),
        ];

        let resp_result = self.client.get(&endpoint_url).query(&query_params).send().await;
        let quote_rtt_ms = start.elapsed().as_secs_f64() * 1000.0;
        let timestamp = Utc::now();

        match resp_result {
            Ok(resp) => {
                let status = resp.status();
                if !status.is_success() {
                    return Ok(DexRouteLatencySample {
                        dex_name: "Jupiter".to_string(),
                        input_mint: params.input_mint.clone(),
                        output_mint: params.output_mint.clone(),
                        in_amount: params.amount,
                        out_amount: 0,
                        quote_rtt_ms,
                        price_impact_pct: 0.0,
                        timestamp,
                        is_ok: false,
                        error_msg: Some(format!("HTTP {}", status)),
                    });
                }

                let body: serde_json::Value = resp.json().await.map_err(FastlaneError::Http)?;
                let out_amount: u64 = body
                    .get("outAmount")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);

                let price_impact_pct: f64 = body
                    .get("priceImpactPct")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0.0);

                Ok(DexRouteLatencySample {
                    dex_name: "Jupiter".to_string(),
                    input_mint: params.input_mint.clone(),
                    output_mint: params.output_mint.clone(),
                    in_amount: params.amount,
                    out_amount,
                    quote_rtt_ms,
                    price_impact_pct,
                    timestamp,
                    is_ok: true,
                    error_msg: None,
                })
            }
            Err(e) => Ok(DexRouteLatencySample {
                dex_name: "Jupiter".to_string(),
                input_mint: params.input_mint.clone(),
                output_mint: params.output_mint.clone(),
                in_amount: params.amount,
                out_amount: 0,
                quote_rtt_ms,
                price_impact_pct: 0.0,
                timestamp,
                is_ok: false,
                error_msg: Some(e.to_string()),
            }),
        }
    }

    /// Determines if a quote-to-route execution fits safely inside the remaining slot window.
    /// Average Solana slot duration: ~400ms.
    pub fn is_executable_in_window(&self, quote_rtt_ms: f64, estimated_tx_landing_ms: f64, remaining_slot_ms: f64) -> bool {
        let total_required = quote_rtt_ms + estimated_tx_landing_ms;
        total_required <= remaining_slot_ms
    }
}
