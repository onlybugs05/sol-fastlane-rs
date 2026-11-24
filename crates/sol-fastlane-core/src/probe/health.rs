use crate::types::{NodeHealthScore, RpcLatencySample, WsSlotSample};
use chrono::Utc;
use parking_lot::RwLock;
use std::collections::VecDeque;

const DEFAULT_WINDOW_SIZE: usize = 30;

/// Tracks sliding window metrics and calculates real-time health scores for a single node.
#[derive(Debug)]
pub struct NodeHealthTracker {
    endpoint: String,
    name: String,
    window_size: usize,
    samples: RwLock<VecDeque<RpcLatencySample>>,
    ws_samples: RwLock<VecDeque<WsSlotSample>>,
    consecutive_errors: parking_lot::Mutex<u32>,
    highest_seen_slot: parking_lot::Mutex<u64>,
}

impl NodeHealthTracker {
    pub fn new(name: impl Into<String>, endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            name: name.into(),
            window_size: DEFAULT_WINDOW_SIZE,
            samples: RwLock::new(VecDeque::with_capacity(DEFAULT_WINDOW_SIZE)),
            ws_samples: RwLock::new(VecDeque::with_capacity(DEFAULT_WINDOW_SIZE)),
            consecutive_errors: parking_lot::Mutex::new(0),
            highest_seen_slot: parking_lot::Mutex::new(0),
        }
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn record_sample(&self, sample: RpcLatencySample) {
        if sample.is_ok {
            *self.consecutive_errors.lock() = 0;
            let mut highest = self.highest_seen_slot.lock();
            if sample.slot > *highest {
                *highest = sample.slot;
            }
        } else {
            *self.consecutive_errors.lock() += 1;
        }

        let mut samples = self.samples.write();
        if samples.len() >= self.window_size {
            samples.pop_front();
        }
        samples.push_back(sample);
    }

    pub fn record_ws_sample(&self, sample: WsSlotSample) {
        let mut highest = self.highest_seen_slot.lock();
        if sample.slot > *highest {
            *highest = sample.slot;
        }

        let mut ws_samples = self.ws_samples.write();
        if ws_samples.len() >= self.window_size {
            ws_samples.pop_front();
        }
        ws_samples.push_back(sample);
    }

    pub fn last_slot(&self) -> u64 {
        *self.highest_seen_slot.lock()
    }

    /// Evaluates real-time health score given the cluster tip slot.
    pub fn compute_health(&self, cluster_tip_slot: u64) -> NodeHealthScore {
        let samples = self.samples.read();
        let consecutive_err = *self.consecutive_errors.lock();
        let last_slot = *self.highest_seen_slot.lock();

        let slot_lag = if cluster_tip_slot > last_slot && last_slot > 0 {
            (cluster_tip_slot.saturating_sub(last_slot)).min(i64::MAX as u64) as i64
        } else {
            0
        };

        if samples.is_empty() {
            let mut score = NodeHealthScore::new(self.endpoint.clone(), self.name.clone());
            score.slot_lag = slot_lag;
            score.last_slot = last_slot;
            return score;
        }

        let total = samples.len();
        let failed_count = samples.iter().filter(|s| !s.is_ok).count();
        let error_rate = failed_count as f64 / total as f64;

        let mut latencies: Vec<f64> = samples
            .iter()
            .filter(|s| s.is_ok && s.rtt_ms.is_finite() && s.rtt_ms >= 0.0)
            .map(|s| s.rtt_ms)
            .collect();
        latencies.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let (avg_rtt, min_rtt, max_rtt, p95_rtt) = if latencies.is_empty() {
            (9999.0, 9999.0, 9999.0, 9999.0)
        } else {
            let sum: f64 = latencies.iter().sum();
            let avg = sum / latencies.len() as f64;
            let min = *latencies.first().unwrap();
            let max = *latencies.last().unwrap();
            let p95_idx = ((latencies.len() as f64 * 0.95).ceil() as usize).saturating_sub(1);
            let p95 = latencies[p95_idx];
            (avg, min, max, p95)
        };

        // Score Calculation (0.0 to 100.0)
        // 1. Latency component (up to 40 pts):
        //    < 50ms: 40 pts, 150ms: 25 pts, 300ms: 10 pts, > 500ms: 0 pts
        let latency_score = if avg_rtt <= 50.0 {
            40.0
        } else if avg_rtt <= 500.0 {
            40.0 * (1.0 - (avg_rtt - 50.0) / 450.0)
        } else {
            0.0
        };

        // 2. Slot lag component (up to 35 pts):
        //    0 lag: 35 pts, 1 slot lag: 28 pts, 2 lag: 20 pts, >= 5 lag: 0 pts
        let lag_score = match slot_lag {
            0 => 35.0,
            1 => 28.0,
            2 => 20.0,
            3 => 12.0,
            4 => 5.0,
            _ => 0.0,
        };

        // 3. Reliability / Error rate component (up to 25 pts):
        let reliability_score = (1.0 - error_rate).max(0.0) * 25.0;

        let mut total_score = latency_score + lag_score + reliability_score;

        // Hard penalty for multiple consecutive errors
        if consecutive_err >= 3 {
            total_score = total_score.min(10.0);
        } else if consecutive_err >= 1 {
            total_score *= 0.7;
        }

        let is_healthy = consecutive_err < 3 && slot_lag <= 5 && error_rate < 0.3;

        NodeHealthScore {
            endpoint: self.endpoint.clone(),
            name: self.name.clone(),
            score: total_score.clamp(0.0, 100.0),
            avg_rtt_ms: avg_rtt,
            p95_rtt_ms: p95_rtt,
            min_rtt_ms: min_rtt,
            max_rtt_ms: max_rtt,
            last_slot,
            slot_lag,
            error_rate,
            consecutive_errors: consecutive_err,
            is_healthy,
            last_updated: Utc::now(),
        }
    }
}
