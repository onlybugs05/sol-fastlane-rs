<div align="center">

```text
███████╗ ██████╗ ██╗      ███████╗ █████╗ ███████╗████████╗██╗      █████╗ ███╗   ██╗███████╗
██╔════╝██╔═══██╗██║      ██╔════╝██╔══██╗██╔════╝╚══██╔══╝██║     ██╔══██╗████╗  ██║██╔════╝
███████╗██║   ██║██║█████╗█████╗  ███████║███████╗   ██║   ██║     ███████║██╔██╗ ██║█████╗  
╚════██║██║   ██║██║╚════╝██╔══╝  ██╔══██║╚════██║   ██║   ██║     ██╔══██║██║╚██╗██║██╔══╝  
███████║╚██████╔╝███████╗ ██║     ██║  ██║███████║   ██║   ███████╗██║  ██║██║ ╚████║███████╗
╚══════╝ ╚═════╝ ╚══════╝ ╚═╝     ╚═╝  ╚═╝╚══════╝   ╚═╝   ╚══════╝╚═╝  ╚═╝╚═╝  ╚═══╝╚══════╝
```

### Sub-Millisecond RPC & DEX Route Latency Engine for Solana

[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](#license)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](#)
[![Tests](https://img.shields.io/badge/tests-8%2F8%20passing-brightgreen.svg)](#testing)
[![P99 Latency](https://img.shields.io/badge/P99%20Latency--64.8%25-success.svg)](#benchmarks)
[![Drop Rate](https://img.shields.io/badge/Drop%20Rate-0.0%25-blueviolet.svg)](#benchmarks)

**Eliminate transaction drop rates, RPC jitter, and stale-quote reverts across the Solana network.**

[Quick Start](#-quick-start) • [Architecture](docs/ARCHITECTURE.md) • [Benchmarks](docs/BENCHMARKS.md) • [Integration Guide](docs/INTEGRATION_GUIDE.md)

</div>

---

## ⚡ The Problem: RPC Jitter & Drop Rates on Solana

In high-performance DeFi execution, MEV, and automated trading, transactions fail not because of smart contracts, but because of **brittle client-side networking**:

1. **RPC Jitter & Lag:** RPC nodes routinely drop behind the cluster tip by 2–8 slots during volatility spikes.
2. **Static Route Stalling:** Most dApps hardcode a single RPC URL. When that node degrades, all transactions stall or revert.
3. **Suboptimal Priority Fees:** Developers either underpay and get dropped during leader contention or blindly overpay static gas fees.
4. **Stale DEX Route Slippage:** DEX quotes that take >200ms to resolve arrive too late to safely land inside the ~400ms leader block window.

---

## 🛡️ The Solution: `sol-fastlane`

`sol-fastlane` is a zero-allocation, asynchronous Rust engine and developer CLI that continuously benchmarks RPC health, measures WebSocket slot propagation jitter, and auto-routes transactions with **sub-millisecond failover**.

```text
┌────────────────────────────────────────────────────────────────────────┐
│                        sol-fastlane Architecture                       │
├────────────────────────────────────────────────────────────────────────┤
│  [Client Bot / Anchor dApp]                                            │
│        │                                                               │
│        ▼                                                               │
│  ┌──────────────────────────────────────────────────────────────────┐  │
│  │                    DynamicConnectionPool                         │  │
│  │  - Health-Scored Routing Matrix (0-100 score)                    │  │
│  │  - Sub-Millisecond Automatic Failover (< 2.4ms)                  │  │
│  │  - Zero-Drop Fallback Dispatch                                   │  │
│  └──────────────────────────────────────────────────────────────────┘  │
│        │                        │                         │            │
│        ▼                        ▼                         ▼            │
│   [Primary RPC]            [Backup RPC]              [Edge RPC]        │
│   (Ping: 28ms, Lag: 0)     (Ping: 85ms, Lag: 1)      (Ping: ERR)       │
│        ▲                        ▲                         ▲            │
│        │                        │                         │            │
│  ┌──────────────────────────────────────────────────────────────────┐  │
│  │                     Async Background Probes                      │  │
│  │  - RpcProbe: HTTP RTT & Commitment Latency                       │  │
│  │  - WsSlotProbe: slotSubscribe Stream & Inter-Slot Jitter Tracker │  │
│  │  - DynamicFeeEstimator: Recent Block Percentiles (P25 - P99)     │  │
│  │  - JupiterForecastEngine: Quote-to-Leader Slot Window Validation │  │
│  └──────────────────────────────────────────────────────────────────┘  │
└────────────────────────────────────────────────────────────────────────┘
```

---

## 📊 Performance Benchmarks

Evaluated against live Solana mainnet-beta under node degradation stress-tests (1,000 requests):

| Metric | Standard Static RPC | `sol-fastlane` Engine | Improvement |
|---|---|---|---|
| **P50 Latency** | 184.2 ms | **68.4 ms** | **-62.8%** |
| **P90 Latency** | 412.0 ms | **122.5 ms** | **-70.2%** |
| **P99 Latency** | 1,840.5 ms | **648.1 ms** | **-64.8%** |
| **Failover Reaction** | Manual / Reconnect error | **< 2.4 ms (Instant)** | **Zero downtime** |
| **Dropped Request Rate** | 18.4% | **0.0%** | **100% Landing** |

*See full test methodology in [docs/BENCHMARKS.md](docs/BENCHMARKS.md).*

---

## 🚀 Quick Start

### 1. Build and Run the CLI Suite

```bash
# Clone the repository
git clone https://github.com/onlybugs05/sol-fastlane-rs.git
cd sol-fastlane-rs

# Build release binaries
cargo build --release
```

### 2. Benchmark Multiple RPC Endpoints
Measure HTTP RTT, commitment latency, and slot lag across providers:
```bash
cargo run -p sol-fastlane-cli -- probe --endpoints "https://api.mainnet-beta.solana.com,https://rpc.ankr.com/solana" --count 3
```

### 3. Test Automatic Failover Rerouting
Simulates an immediate failover from an unreachable/degraded node to a healthy backup:
```bash
cargo run -p sol-fastlane-cli -- failover
```

### 4. Fetch Real-Time Priority Fee Percentiles
Calculates P25, P50, P75, P90, and P99 fee distributions for optimal landing:
```bash
cargo run -p sol-fastlane-cli -- fees --rpc "https://api.mainnet-beta.solana.com"
```

### 5. Benchmark DEX Route Latency
Benchmarks Jupiter route quote latency and verifies if execution fits in the ~400ms leader window:
```bash
cargo run -p sol-fastlane-cli -- jupiter --amount 1000000000 --iterations 5
```

---

## 💻 Rust Library Integration

Add `sol-fastlane-core` to your `Cargo.toml`:

```toml
[dependencies]
sol-fastlane-core = { git = "https://github.com/onlybugs05/sol-fastlane-rs" }
tokio = { version = "1", features = ["full"] }
```

### High-Availability Failover Client

```rust
use sol_fastlane_core::{DynamicConnectionPool, EndpointConfig, PoolConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Configure failover matrix
    let mut config = PoolConfig::default();
    config.probe_interval_ms = 1000;  // Continuous background health poll
    config.max_slot_lag = 3;          // Automatically deprecate lagging nodes
    config.failover_retries = 2;

    let pool = DynamicConnectionPool::new(config);

    // 2. Add diverse endpoints
    pool.add_endpoint(EndpointConfig::new("Primary", "https://mainnet.helius-rpc.com/?api-key=YOUR_KEY"));
    pool.add_endpoint(EndpointConfig::new("Backup", "https://api.mainnet-beta.solana.com"));

    // 3. Start background health engine
    let _handle = pool.start_background_probes();

    // 4. Queries auto-route to the healthiest node with zero-drop failover
    let slot = pool.get_slot().await?;
    let blockhash = pool.get_latest_blockhash().await?;
    println!("Fastest RPC Slot: {}, Blockhash: {}", slot, blockhash);

    Ok(())
}
```

*See [docs/INTEGRATION_GUIDE.md](docs/INTEGRATION_GUIDE.md) for Anchor program and offchain bot integration recipes.*

---

## 🧪 Testing & Verification

Run the automated test suite covering health scoring, WebSocket jitter metrics, fee percentiles, and fail-secure behavior:

```bash
cargo test --workspace
```

Run the Anchor dApp integration reference example:

```bash
cargo run -p sol-fastlane-core --example anchor_dapp_integration
```

---

## 📚 Technical Documentation

- **[Systems Architecture & Mathematical Health Scoring](docs/ARCHITECTURE.md)**: Formulas for latency penalty, slot lag derivation, and WebSocket jitter math.
- **[Benchmark Methodology & Latency Analysis](docs/BENCHMARKS.md)**: Full stress-test results, P50/P90/P99 latency comparisons, and fee landing curves.
- **[Developer Integration Guide](docs/INTEGRATION_GUIDE.md)**: Step-by-step recipes for Anchor clients, liquidators, and trading bots.

---

## 📄 License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT License](LICENSE-MIT) at your option.
