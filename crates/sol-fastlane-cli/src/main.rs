use clap::{Parser, Subcommand};
use comfy_table::presets::UTF8_FULL;
use comfy_table::{Cell, Color, ContentArrangement, Table};
use sol_fastlane_core::{
    DynamicConnectionPool, DynamicFeeEstimator, EndpointConfig, FeeUrgency, JupiterForecastEngine,
    JupiterQuoteParams, PoolConfig, RpcProbe, SOL_MINT, USDC_MINT,
};
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(name = "sol-fastlane")]
#[command(author = "Hrushikesh Jetti <onlybugs05@gmail.com>")]
#[command(version = "0.1.0")]
#[command(about = "Sub-Millisecond RPC & DEX Route Latency Engine for Solana", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Benchmark latency and health across RPC endpoints
    Probe {
        /// RPC endpoints to benchmark (comma-separated or multiple flags)
        #[arg(
            short,
            long,
            value_delimiter = ',',
            default_value = "https://api.mainnet-beta.solana.com,https://rpc.ankr.com/solana"
        )]
        endpoints: Vec<String>,
        /// Number of probe iterations per endpoint
        #[arg(short, long, default_value_t = 3)]
        count: usize,
    },

    /// Benchmark live DEX route quote latency via Jupiter API
    Jupiter {
        /// Input token mint
        #[arg(long, default_value = SOL_MINT)]
        input_mint: String,
        /// Output token mint
        #[arg(long, default_value = USDC_MINT)]
        output_mint: String,
        /// Trade amount (in base units, e.g. lamports)
        #[arg(long, default_value_t = 1_000_000_000)]
        amount: u64,
        /// Number of quote probes
        #[arg(short, long, default_value_t = 5)]
        iterations: usize,
    },

    /// Inspect real-time priority fee percentiles on Solana
    Fees {
        /// Target RPC URL
        #[arg(short, long, default_value = "https://api.mainnet-beta.solana.com")]
        rpc: String,
    },

    /// Run continuous live monitoring and connection pool router
    Monitor {
        /// Comma-separated list of RPC endpoints
        #[arg(
            short,
            long,
            value_delimiter = ',',
            default_value = "https://api.mainnet-beta.solana.com,https://rpc.ankr.com/solana"
        )]
        endpoints: Vec<String>,
        /// Probe interval in milliseconds
        #[arg(short, long, default_value_t = 1500)]
        interval_ms: u64,
    },

    /// Test sub-millisecond dynamic RPC failover in real-time
    Failover {
        /// Primary RPC endpoint (e.g. simulated degraded node or public endpoint)
        #[arg(long, default_value = "https://unreachable-mock-node.solana.com")]
        primary: String,
        /// Failover backup RPC endpoint
        #[arg(long, default_value = "https://api.mainnet-beta.solana.com")]
        backup: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    match cli.command {
        Commands::Probe { endpoints, count } => {
            println!("🚀 sol-fastlane: Benchmarking {} RPC endpoints ({} iterations)...\n", endpoints.len(), count);

            let mut table = Table::new();
            table
                .load_preset(UTF8_FULL)
                .set_content_arrangement(ContentArrangement::Dynamic)
                .set_header(vec![
                    "Endpoint",
                    "Slot",
                    "Avg RTT (ms)",
                    "Min RTT (ms)",
                    "Max RTT (ms)",
                    "Status",
                ]);

            for url in endpoints {
                let probe = RpcProbe::new(url.clone(), url.clone());
                let mut latencies = Vec::new();
                let mut last_slot = 0;
                let mut all_ok = true;

                for _ in 0..count {
                    match probe.ping_slot().await {
                        Ok(sample) => {
                            if sample.is_ok {
                                latencies.push(sample.rtt_ms);
                                if sample.slot > last_slot {
                                    last_slot = sample.slot;
                                }
                            } else {
                                all_ok = false;
                            }
                        }
                        Err(_) => {
                            all_ok = false;
                        }
                    }
                    tokio::time::sleep(Duration::from_millis(150)).await;
                }

                if latencies.is_empty() {
                    table.add_row(vec![
                        Cell::new(&url),
                        Cell::new("-"),
                        Cell::new("ERR"),
                        Cell::new("ERR"),
                        Cell::new("ERR"),
                        Cell::new("UNREACHABLE").fg(Color::Red),
                    ]);
                } else {
                    let sum: f64 = latencies.iter().sum();
                    let avg = sum / latencies.len() as f64;
                    let min = latencies.iter().cloned().fold(f64::INFINITY, f64::min);
                    let max = latencies.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

                    let status_cell = if all_ok {
                        Cell::new("HEALTHY").fg(Color::Green)
                    } else {
                        Cell::new("DEGRADED").fg(Color::Yellow)
                    };

                    table.add_row(vec![
                        Cell::new(&url),
                        Cell::new(last_slot.to_string()),
                        Cell::new(format!("{:.1}", avg)),
                        Cell::new(format!("{:.1}", min)),
                        Cell::new(format!("{:.1}", max)),
                        status_cell,
                    ]);
                }
            }

            println!("{table}");
        }

        Commands::Jupiter {
            input_mint,
            output_mint,
            amount,
            iterations,
        } => {
            println!("⚡ sol-fastlane: Benchmarking Jupiter Route Quote Latency...\n");
            let engine = JupiterForecastEngine::new(None);
            let params = JupiterQuoteParams {
                input_mint,
                output_mint,
                amount,
                slippage_bps: 50,
            };

            let mut table = Table::new();
            table
                .load_preset(UTF8_FULL)
                .set_content_arrangement(ContentArrangement::Dynamic)
                .set_header(vec![
                    "Iter",
                    "Quote RTT (ms)",
                    "Out Amount",
                    "Price Impact %",
                    "Slot Safe (400ms)",
                    "Status",
                ]);

            let mut rtts = Vec::new();
            for i in 1..=iterations {
                let sample = engine.benchmark_quote(&params).await?;
                if sample.is_ok {
                    rtts.push(sample.quote_rtt_ms);
                    let safe = engine.is_executable_in_window(sample.quote_rtt_ms, 150.0, 400.0);
                    let safe_cell = if safe {
                        Cell::new("YES (Fits)").fg(Color::Green)
                    } else {
                        Cell::new("NO (Tight)").fg(Color::Yellow)
                    };

                    table.add_row(vec![
                        Cell::new(i.to_string()),
                        Cell::new(format!("{:.1}", sample.quote_rtt_ms)),
                        Cell::new(sample.out_amount.to_string()),
                        Cell::new(format!("{:.4}%", sample.price_impact_pct)),
                        safe_cell,
                        Cell::new("OK").fg(Color::Green),
                    ]);
                } else {
                    table.add_row(vec![
                        Cell::new(i.to_string()),
                        Cell::new(format!("{:.1}", sample.quote_rtt_ms)),
                        Cell::new("0"),
                        Cell::new("0"),
                        Cell::new("NO").fg(Color::Red),
                        Cell::new(sample.error_msg.unwrap_or_else(|| "ERR".to_string())).fg(Color::Red),
                    ]);
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }

            println!("{table}");

            if !rtts.is_empty() {
                let avg = rtts.iter().sum::<f64>() / rtts.len() as f64;
                let min = rtts.iter().cloned().fold(f64::INFINITY, f64::min);
                let max = rtts.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                println!("📊 Summary: Avg: {:.1}ms | Min: {:.1}ms | Max: {:.1}ms", avg, min, max);
            }
        }

        Commands::Fees { rpc } => {
            println!("💎 sol-fastlane: Querying real-time priority fees on {}...\n", rpc);
            let estimator = DynamicFeeEstimator::new();
            let p = estimator.fetch_percentiles(&rpc, None).await?;

            let mut table = Table::new();
            table
                .load_preset(UTF8_FULL)
                .set_content_arrangement(ContentArrangement::Dynamic)
                .set_header(vec!["Percentile / Level", "Fee (micro-lamports / CU)", "Urgency Strategy"]);

            table.add_row(vec!["Min", &p.min.to_string(), "Base"]);
            table.add_row(vec!["P25", &p.p25.to_string(), "Low urgency"]);
            table.add_row(vec!["P50", &p.p50.to_string(), "Standard / Medium"]);
            table.add_row(vec!["P75", &p.p75.to_string(), "High Priority"]);
            table.add_row(vec!["P90", &p.p90.to_string(), "Turbo (Fast Landing)"]);
            table.add_row(vec!["P99", &p.p99.to_string(), "Max Landing (Contested)"]);
            table.add_row(vec!["Max", &p.max.to_string(), "Ceiling"]);

            println!("{table}");
            println!("Target Slot: {} | Recent Block Sample Count: {}", p.slot, p.sample_count);
            println!(
                "Recommended Turbo Fee: {} micro-lamports/CU",
                estimator.recommend_fee(&p, FeeUrgency::Turbo)
            );
        }

        Commands::Monitor { endpoints, interval_ms } => {
            println!("📡 sol-fastlane: Starting Dynamic Connection Pool live monitor...\n");
            let mut pool_cfg = PoolConfig::default();
            pool_cfg.probe_interval_ms = interval_ms;

            let pool = DynamicConnectionPool::new(pool_cfg);
            for ep in endpoints {
                pool.add_endpoint(EndpointConfig::new(ep.clone(), ep));
            }

            let handle = pool.start_background_probes();

            // Run 5 monitor update cycles for demo/verification
            for cycle in 1..=5 {
                tokio::time::sleep(Duration::from_millis(interval_ms + 100)).await;
                let scores = pool.get_health_scores();

                println!("--- [Cycle #{}] Live Pool State ---", cycle);
                let mut table = Table::new();
                table
                    .load_preset(UTF8_FULL)
                    .set_content_arrangement(ContentArrangement::Dynamic)
                    .set_header(vec![
                        "Node",
                        "Health Score",
                        "Avg RTT (ms)",
                        "P95 RTT (ms)",
                        "Slot",
                        "Slot Lag",
                        "State",
                    ]);

                for s in &scores {
                    let state_cell = if s.is_healthy {
                        Cell::new("ACTIVE").fg(Color::Green)
                    } else {
                        Cell::new("DEGRADED").fg(Color::Red)
                    };

                    table.add_row(vec![
                        Cell::new(&s.name),
                        Cell::new(format!("{:.1}/100", s.score)),
                        Cell::new(format!("{:.1}", s.avg_rtt_ms)),
                        Cell::new(format!("{:.1}", s.p95_rtt_ms)),
                        Cell::new(s.last_slot.to_string()),
                        Cell::new(s.slot_lag.to_string()),
                        state_cell,
                    ]);
                }
                println!("{table}\n");

                if let Some(best) = pool.get_best_endpoint() {
                    println!("🎯 Optimal Fastlane Route: {} ({})\n", best.name, best.http_url);
                }
            }

            pool.stop();
            let _ = handle.await;
        }

        Commands::Failover { primary, backup } => {
            println!("🔄 sol-fastlane: Simulating Dynamic Connection Pool Failover...\n");
            let mut pool_cfg = PoolConfig::default();
            pool_cfg.failover_retries = 2;
            pool_cfg.request_timeout_ms = 1500;

            let pool = DynamicConnectionPool::new(pool_cfg);
            pool.add_endpoint(EndpointConfig::new("Primary (Degraded/Down)", &primary));
            pool.add_endpoint(EndpointConfig::new("Backup (Healthy Node)", &backup));

            println!("1. Probing initial node cluster health...");
            pool.probe_all_nodes().await;

            let scores = pool.get_health_scores();
            let mut table = Table::new();
            table
                .load_preset(UTF8_FULL)
                .set_content_arrangement(ContentArrangement::Dynamic)
                .set_header(vec!["Endpoint", "Status", "Score", "RTT (ms)"]);

            for s in &scores {
                let status_cell = if s.is_healthy {
                    Cell::new("HEALTHY").fg(Color::Green)
                } else {
                    Cell::new("DEGRADED/DEAD").fg(Color::Red)
                };
                table.add_row(vec![
                    Cell::new(&s.name),
                    status_cell,
                    Cell::new(format!("{:.1}/100", s.score)),
                    Cell::new(format!("{:.1}", s.avg_rtt_ms)),
                ]);
            }
            println!("{table}\n");

            println!("2. Dispatching live RPC query: `get_latest_blockhash()`...");
            let start = std::time::Instant::now();
            match pool.get_latest_blockhash().await {
                Ok(blockhash) => {
                    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
                    println!("   ✅ Success via Automatic Failover!");
                    println!("   - Blockhash: {}", blockhash);
                    println!("   - Failover Latency: {:.1}ms", elapsed_ms);
                }
                Err(e) => {
                    println!("   ❌ Failover failed: {}", e);
                }
            }

            println!("\n3. Dispatching live RPC query: `get_slot()`...");
            let start = std::time::Instant::now();
            match pool.get_slot().await {
                Ok(slot) => {
                    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
                    println!("   ✅ Success via Automatic Failover!");
                    println!("   - Current Cluster Slot: {}", slot);
                    println!("   - Failover Latency: {:.1}ms", elapsed_ms);
                }
                Err(e) => {
                    println!("   ❌ Failover failed: {}", e);
                }
            }
        }
    }

    Ok(())
}
