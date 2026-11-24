# sol-fastlane ⚡

An asynchronous, low-overhead Rust engine and developer CLI designed to eliminate transaction delivery bottlenecks, RPC node jitter, and dropped transactions across Solana.

---

## Key Features

- **Asynchronous Latency Probing:** Continuously checks RPC round-trip times (RTT), block commitment times, and slot propagation.
- **WebSocket Slot & Jitter Tracking:** Subscribes to real-time `slotSubscribe` feeds to calculate slot lag against cluster roots and track arrival jitter.
- **Dynamic Connection Pooling:** Maintains a weighted, health-scored connection pool that automatically routes queries to the fastest, healthiest RPC node with sub-millisecond failover.
- **Jupiter DEX Route Forecaster:** Measures live quote latency and verifies whether quote-to-route execution fits safely inside the Solana ~400ms leader slot.
- **Real-Time Priority Fee Percentiles:** Analyzes recent block fee distributions (P25, P50, P75, P90, P99) to determine optimal landing fees without overpaying.

---

## Project Structure

```text
sol-fastlane-rs/
├── crates/
│   ├── sol-fastlane-core/     # Core library: probes, health tracker, fee estimator, connection pool
│   └── sol-fastlane-cli/      # Terminal CLI tool for monitoring and benchmarking
├── examples/
│   ├── pool_failover.rs           # Automated failover connection pool example
│   └── anchor_dapp_integration.rs # Offchain Anchor & dApp client integration
└── Cargo.toml
```

---

## Installation & CLI Usage

### Build from Source

```bash
cargo build --release
```

The compiled binary will be located at `target/release/sol-fastlane`.

### 1. Benchmark RPC Nodes

```bash
cargo run -p sol-fastlane-cli -- probe --endpoints "https://api.mainnet-beta.solana.com,https://rpc.ankr.com/solana" --count 3
```

### 2. Live Dynamic RPC Failover Test

```bash
cargo run -p sol-fastlane-cli -- failover
```

### 3. Estimate Priority Fees

```bash
cargo run -p sol-fastlane-cli -- fees --rpc "https://api.mainnet-beta.solana.com"
```

### 4. Benchmark DEX Route Latency

```bash
cargo run -p sol-fastlane-cli -- jupiter --amount 1000000000 --iterations 5
```

### 5. Continuous Cluster Monitor

```bash
cargo run -p sol-fastlane-cli -- monitor --endpoints "https://api.mainnet-beta.solana.com,https://rpc.ankr.com/solana" --interval-ms 1500
```

---

## Library Usage

Add `sol-fastlane-core` to your `Cargo.toml`:

```toml
[dependencies]
sol-fastlane-core = { git = "https://github.com/onlybugs05/sol-fastlane-rs" }
```

### Quickstart Example

```rust
use sol_fastlane_core::{DynamicConnectionPool, EndpointConfig, PoolConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pool = DynamicConnectionPool::new(PoolConfig::default());

    pool.add_endpoint(EndpointConfig::new("Primary", "https://api.mainnet-beta.solana.com"));
    pool.add_endpoint(EndpointConfig::new("Backup", "https://rpc.ankr.com/solana"));

    // Start background health probes
    let _handle = pool.start_background_probes();

    // Query with automatic failover
    let slot = pool.get_slot().await?;
    let blockhash = pool.get_latest_blockhash().await?;
    println!("Healthy Slot: {}, Blockhash: {}", slot, blockhash);

    Ok(())
}
```

---

## Automated Tests

Run the test suite across the workspace:

```bash
cargo test --workspace
```

---

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.
