use sol_fastlane_core::{
    DynamicConnectionPool, DynamicFeeEstimator, EndpointConfig, FeeUrgency, PoolConfig,
};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== sol-fastlane Connection Pool & Fee Estimator Example ===");

    let mut config = PoolConfig::default();
    config.probe_interval_ms = 1000;
    config.failover_retries = 2;

    let pool = DynamicConnectionPool::new(config);

    // Register endpoints
    pool.add_endpoint(EndpointConfig::new(
        "Solana Public Mainnet",
        "https://api.mainnet-beta.solana.com",
    ));
    pool.add_endpoint(EndpointConfig::new(
        "Ankr Solana",
        "https://rpc.ankr.com/solana",
    ));

    // Start background health probes
    let handle = pool.start_background_probes();

    // Allow probes to collect initial metrics
    tokio::time::sleep(Duration::from_millis(1500)).await;

    // Display health scores
    let scores = pool.get_health_scores();
    for s in &scores {
        println!(
            "Node: {} | Health: {:.1}/100 | Avg RTT: {:.1}ms | Slot: {} (lag: {})",
            s.name, s.score, s.avg_rtt_ms, s.last_slot, s.slot_lag
        );
    }

    if let Some(best) = pool.get_best_endpoint() {
        println!("\n✅ Best Endpoint: {} ({})", best.name, best.http_url);

        // Fetch priority fees from best node
        let fee_estimator = DynamicFeeEstimator::new();
        match fee_estimator.fetch_percentiles(&best.http_url, None).await {
            Ok(fees) => {
                println!("\n📊 Live Priority Fees (Slot {}):", fees.slot);
                println!("   - P25 (Low): {} micro-lamports/CU", fees.p25);
                println!("   - P50 (Medium): {} micro-lamports/CU", fees.p50);
                println!("   - P75 (High): {} micro-lamports/CU", fees.p75);
                println!("   - P90 (Turbo): {} micro-lamports/CU", fees.p90);
                println!(
                    "   🚀 Recommended Turbo Fee: {}",
                    fee_estimator.recommend_fee(&fees, FeeUrgency::Turbo)
                );
            }
            Err(e) => println!("Failed to fetch priority fees: {}", e),
        }
    }

    pool.stop();
    let _ = handle.await;
    Ok(())
}
