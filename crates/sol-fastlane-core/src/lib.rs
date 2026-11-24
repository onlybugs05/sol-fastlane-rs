pub mod dex;
pub mod error;
pub mod fee;
pub mod pool;
pub mod probe;
pub mod types;

pub use dex::{JupiterForecastEngine, JupiterQuoteParams, SOL_MINT, USDC_MINT};
pub use error::{FastlaneError, Result};
pub use fee::{DynamicFeeEstimator, FeeUrgency};
pub use pool::{DynamicConnectionPool, PoolConfig};
pub use probe::{NodeHealthTracker, RpcProbe, WsSlotProbe};
pub use types::{
    DexRouteLatencySample, EndpointConfig, NodeHealthScore, PriorityFeePercentiles,
    RpcLatencySample, WsSlotSample,
};

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_endpoint_config_ws_auto_inference() {
        let ep = EndpointConfig::new("Mainnet", "https://api.mainnet-beta.solana.com");
        assert_eq!(ep.ws_url, Some("wss://api.mainnet-beta.solana.com".to_string()));
        assert!(ep.validate());

        let ep_http = EndpointConfig::new("Local", "http://127.0.0.1:8899");
        assert_eq!(ep_http.ws_url, Some("ws://127.0.0.1:8899".to_string()));
        assert!(ep_http.validate());

        let ep_invalid = EndpointConfig::new("Bad", "ftp://invalid-scheme");
        assert_eq!(ep_invalid.ws_url, None);
        assert!(!ep_invalid.validate());
    }

    #[test]
    fn test_node_health_tracker_scoring() {
        let tracker = NodeHealthTracker::new("TestNode", "https://test.solana.com");

        // Record 10 healthy, low-latency samples (30ms RTT, slot 1000)
        for _ in 0..10 {
            tracker.record_sample(RpcLatencySample {
                endpoint: "https://test.solana.com".to_string(),
                timestamp: Utc::now(),
                rtt_ms: 30.0,
                slot: 1000,
                commitment_ms: 30.0,
                is_ok: true,
                error_msg: None,
            });
        }

        let health = tracker.compute_health(1000);
        assert!(health.is_healthy);
        assert_eq!(health.slot_lag, 0);
        assert!(health.score > 90.0, "Score was {}", health.score);
        assert_eq!(health.consecutive_errors, 0);

        // Record error samples and verify penalty
        for _ in 0..3 {
            tracker.record_sample(RpcLatencySample {
                endpoint: "https://test.solana.com".to_string(),
                timestamp: Utc::now(),
                rtt_ms: 9999.0,
                slot: 0,
                commitment_ms: 9999.0,
                is_ok: false,
                error_msg: Some("Timeout".to_string()),
            });
        }

        let health_after_errors = tracker.compute_health(1005);
        assert!(!health_after_errors.is_healthy);
        assert!(health_after_errors.score <= 10.0);
        assert_eq!(health_after_errors.consecutive_errors, 3);
    }

    #[test]
    fn test_node_health_nan_and_overflow_handling() {
        let tracker = NodeHealthTracker::new("RobustNode", "https://robust.solana.com");

        // Record 35 samples to test sliding window cap (window size = 30)
        for i in 0..35 {
            tracker.record_sample(RpcLatencySample {
                endpoint: "https://robust.solana.com".to_string(),
                timestamp: Utc::now(),
                rtt_ms: if i == 0 { f64::NAN } else { 40.0 }, // Include NaN
                slot: 1000 + i,
                commitment_ms: 40.0,
                is_ok: true,
                error_msg: None,
            });
        }

        assert_eq!(tracker.last_slot(), 1034);
        let health = tracker.compute_health(1034);
        assert!(health.is_healthy);
        assert!(health.avg_rtt_ms > 0.0 && health.avg_rtt_ms.is_finite());
    }

    #[test]
    fn test_ws_sample_recording() {
        let tracker = NodeHealthTracker::new("WsNode", "https://wsnode.solana.com");
        tracker.record_ws_sample(WsSlotSample {
            endpoint: "https://wsnode.solana.com".to_string(),
            timestamp: Utc::now(),
            slot: 5000,
            parent_slot: 4999,
            root_slot: 4968,
            lag_slots: 0,
            jitter_ms: 12.5,
        });

        assert_eq!(tracker.last_slot(), 5000);
    }

    #[test]
    fn test_fee_estimator_percentiles() {
        let estimator = DynamicFeeEstimator::new();
        let percentiles = PriorityFeePercentiles {
            slot: 280000000,
            p25: 1_000,
            p50: 5_000,
            p75: 25_000,
            p90: 100_000,
            p99: 500_000,
            min: 0,
            max: 1_000_000,
            sample_count: 150,
            timestamp: Utc::now(),
        };

        assert_eq!(estimator.recommend_fee(&percentiles, FeeUrgency::Low), 1_000);
        assert_eq!(estimator.recommend_fee(&percentiles, FeeUrgency::Medium), 5_000);
        assert_eq!(estimator.recommend_fee(&percentiles, FeeUrgency::High), 25_000);
        assert_eq!(estimator.recommend_fee(&percentiles, FeeUrgency::Turbo), 100_000);
        assert_eq!(estimator.recommend_fee(&percentiles, FeeUrgency::MaxLanding), 500_000);
    }

    #[test]
    fn test_dex_route_window_calculation() {
        let engine = JupiterForecastEngine::new(None);
        // Quote took 45ms, estimated landing 150ms -> 195ms total.
        // Remaining slot window: 250ms -> Fits!
        assert!(engine.is_executable_in_window(45.0, 150.0, 250.0));

        // Remaining slot window: 180ms -> Does NOT fit safely!
        assert!(!engine.is_executable_in_window(45.0, 150.0, 180.0));
    }

    #[tokio::test]
    async fn test_dynamic_connection_pool_empty_fail_secure() {
        let pool = DynamicConnectionPool::new(PoolConfig::default());
        let result: Result<u64> = pool.dispatch_rpc("getSlot", serde_json::json!([])).await;
        assert!(result.is_err());
        match result {
            Err(FastlaneError::NoHealthyEndpoints) => {}
            other => panic!("Expected NoHealthyEndpoints, got {:?}", other),
        }
    }

    #[test]
    fn test_dynamic_connection_pool_selection() {
        let pool = DynamicConnectionPool::new(PoolConfig::default());
        pool.add_endpoint(EndpointConfig::new("FastNode", "https://fast.solana.com"));
        pool.add_endpoint(EndpointConfig::new("SlowNode", "https://slow.solana.com"));

        let scores = pool.get_health_scores();
        assert_eq!(scores.len(), 2);
    }
}
