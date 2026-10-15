//! Latency statistics collection and reporting.

use crate::stats::tsc::{cycles_to_ns, rdtsc};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

/// A single latency metric with all its samples (in nanoseconds).
#[derive(Default)]
pub struct Metric {
    samples: Vec<u64>,
}

impl Metric {
    pub fn new() -> Self {
        Self { samples: Vec::new() }
    }

    #[inline]
    pub fn record_ns(&mut self, ns: u64) {
        self.samples.push(ns);
    }

    #[inline]
    pub fn record_cycles(&mut self, cycles: u64) {
        self.samples.push(cycles_to_ns(cycles));
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    pub fn percentile(&self, p: f64) -> u64 {
        if self.samples.is_empty() {
            return 0;
        }
        let mut sorted = self.samples.clone();
        sorted.sort_unstable();
        let idx = ((p / 100.0) * (sorted.len() - 1) as f64).round() as usize;
        sorted[idx.min(sorted.len() - 1)]
    }

    pub fn max(&self) -> u64 {
        self.samples.iter().copied().max().unwrap_or(0)
    }

    pub fn min(&self) -> u64 {
        self.samples.iter().copied().min().unwrap_or(0)
    }

    pub fn mean(&self) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }
        let sum: u128 = self.samples.iter().map(|&x| x as u128).sum();
        sum as f64 / self.samples.len() as f64
    }

    pub fn summary(&self, name: &str) -> String {
        if self.is_empty() {
            return format!("{:<20} (no samples)", name);
        }
        format!(
            "{:<20} n={:<10} p50={:<10} p90={:<10} p99={:<10} p99.9={:<10} p99.99={:<10} max={:<10}",
            name,
            self.len(),
            self.percentile(50.0),
            self.percentile(90.0),
            self.percentile(99.0),
            self.percentile(99.9),
            self.percentile(99.99),
            self.max(),
        )
    }
}

/// All latency metrics collected during a run.
#[derive(Default)]
pub struct LatencyStats {
    /// In-process time = segment 1 (send) + segment 2 (recv).
    pub in_proc: Metric,
    /// Segment 1: T0 -> T1 (build + send).
    pub seg1: Metric,
    /// Segment 2: T2 -> T3 (recv + dispatch).
    pub seg2: Metric,
    /// End-to-end latency = T3 - T0.
    pub e2e: Metric,
    /// Segment 3: sleep expiry -> next T0.
    pub seg3: Metric,
    /// Sleep error: actual wake - expected deadline.
    pub sleep_error: Metric,
    pub timeouts: u64,
    pub sent: u64,
    pub received: u64,
}

impl LatencyStats {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_send_recv(&mut self, t0: u64, t1: u64, t2: u64, t3: u64) {
        let seg1 = t1.wrapping_sub(t0);
        let seg2 = t3.wrapping_sub(t2);
        self.seg1.record_cycles(seg1);
        self.seg2.record_cycles(seg2);
        self.in_proc.record_cycles(seg1.wrapping_add(seg2));
        self.e2e.record_cycles(t3.wrapping_sub(t0));
    }

    pub fn record_seg3(&mut self, cycles: u64) {
        self.seg3.record_cycles(cycles);
    }

    pub fn record_sleep_error(&mut self, cycles: u64) {
        self.sleep_error.record_cycles(cycles);
    }

    pub fn print_summary(&self, duration_sec: u64) {
        println!("=== Latency Summary (duration={}s) ===", duration_sec);
        println!("sent={} received={} timeouts={}", self.sent, self.received, self.timeouts);
        if self.sent > 0 {
            let loss = (self.sent - self.received) as f64 / self.sent as f64 * 100.0;
            println!("loss={:.4}%", loss);
        }
        println!();
        println!("{}", self.in_proc.summary("in-process (ns)"));
        println!("{}", self.seg1.summary("seg1 send (ns)"));
        println!("{}", self.seg2.summary("seg2 recv (ns)"));
        println!("{}", self.e2e.summary("e2e (ns)"));
        println!("{}", self.seg3.summary("seg3 (ns)"));
        println!("{}", self.sleep_error.summary("sleep_error (ns)"));
        println!("=== End Summary ===");
    }

    pub fn write_csv<P: AsRef<Path>>(&self, path: P) -> std::io::Result<()> {
        let f = File::create(path)?;
        let mut w = BufWriter::new(f);
        writeln!(w, "in_proc_ns,seg1_ns,seg2_ns,e2e_ns,seg3_ns,sleep_error_ns")?;
        let n = self.in_proc.len().max(self.e2e.len());
        for i in 0..n {
            let ip = self.in_proc.samples.get(i).copied().unwrap_or(0);
            let s1 = self.seg1.samples.get(i).copied().unwrap_or(0);
            let s2 = self.seg2.samples.get(i).copied().unwrap_or(0);
            let e2e = self.e2e.samples.get(i).copied().unwrap_or(0);
            let s3 = self.seg3.samples.get(i).copied().unwrap_or(0);
            let se = self.sleep_error.samples.get(i).copied().unwrap_or(0);
            writeln!(w, "{},{},{},{},{},{}", ip, s1, s2, e2e, s3, se)?;
        }
        Ok(())
    }
}

/// Read TSC at call site — convenience for callers.
#[inline(always)]
pub fn now() -> u64 {
    rdtsc()
}
