# sol-fastlane: Developer Integration Guide & Production Recipes

This guide demonstrates how to integrate `sol-fastlane` into Anchor programs, trading bots, and Solana off-chain indexers.

---

## 1. Quick Installation

Add `sol-fastlane-core` to your project's `Cargo.toml`:

```toml
[dependencies]
sol-fastlane-core = { git = "https://github.com/onlybugs05/sol-fastlane-rs" }
tokio = { version = "1", features = ["full"] }
```

---

## 2. Recipe 1: High-Availability Failover RPC Client

Replace brittle single-endpoint RPC setups with an enterprise dynamic failover pool:

```rust
use sol_fastlane_core::{DynamicConnectionPool, EndpointConfig, PoolConfig};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Configure the failover engine
    let mut config = PoolConfig::default();
    config.probe_interval_ms = 1000;  // Continuous health poll every 1s
    config.max_slot_lag = 3;          // Deprecate nodes > 3 slots behind cluster tip
    config.failover_retries = 2;      // Retry up to 2 backup nodes before error

    let pool = DynamicConnectionPool::new(config);

    // 2. Register diverse providers (Helius, Triton, QuickNode, Public)
    pool.add_endpoint(EndpointConfig::new("Primary-Helius", "https://mainnet.helius-rpc.com/?api-key=YOUR_KEY"));
    pool.add_endpoint(EndpointConfig::new("Backup-QuickNode", "https://solana-mainnet.quiknode.pro/YOUR_KEY/"));
    pool.add_endpoint(EndpointConfig::new("Fallback-Public", "https://api.mainnet-beta.solana.com"));

    // 3. Start background health probes in an async task
    let _probe_handle = pool.start_background_probes();

    // 4. Dispatch queries with automatic sub-millisecond failover
    let current_slot = pool.get_slot().await?;
    let blockhash = pool.get_latest_blockhash().await?;
    println!("Safely fetched slot {} and blockhash {}", current_slot, blockhash);

    Ok(())
}
```

---

## 3. Recipe 2: Dynamic Priority Fee Estimation

Calculate the optimal priority fee before signing transactions:

```rust
use sol_fastlane_core::{DynamicFeeEstimator, FeeUrgency};

async fn build_optimized_transaction(rpc_url: &str) -> Result<u64, Box<dyn std::error::Error>> {
    let fee_estimator = DynamicFeeEstimator::new();
    
    // Fetch real-time percentiles from recent blocks
    let percentiles = fee_estimator.fetch_percentiles(rpc_url, None).await?;

    // Select urgency level: Low, Medium, High, Turbo, or MaxLanding
    let priority_fee_micro_lamports = fee_estimator.recommend_fee(&percentiles, FeeUrgency::Turbo);

    println!("Applying Turbo priority fee: {} micro-lamports/CU", priority_fee_micro_lamports);
    Ok(priority_fee_micro_lamports)
}
```

---

## 4. Recipe 3: DEX Arbitrage & Quote Execution Window Check

Verify that a quote arrives with enough time to land in the current leader slot:

```rust
use sol_fastlane_core::{JupiterForecastEngine, JupiterQuoteParams, SOL_MINT, USDC_MINT};

async fn check_dex_window() -> Result<(), Box<dyn std::error::Error>> {
    let jupiter = JupiterForecastEngine::new(None);
    let params = JupiterQuoteParams {
        input_mint: SOL_MINT.to_string(),
        output_mint: USDC_MINT.to_string(),
        amount: 1_000_000_000, // 1 SOL
        slippage_bps: 50,
    };

    let sample = jupiter.benchmark_quote(&params).await?;
    println!("Quote latency: {:.1}ms | Out amount: {}", sample.quote_rtt_ms, sample.out_amount);

    // Assuming ~150ms expected transaction landing and 400ms slot duration
    let is_safe = jupiter.is_executable_in_window(sample.quote_rtt_ms, 150.0, 400.0);
    if is_safe {
        println!("✅ Within slot budget. Broadcast immediately!");
    } else {
        println!("⚠️ Stale route warning: Quote latency exceeded slot window.");
    }

    Ok(())
}
```
