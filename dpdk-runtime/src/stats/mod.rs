//! Measurement infrastructure: TSC and latency stats.

pub mod stats;
pub mod tsc;

pub use stats::LatencyStats;
pub use tsc::{cycles_to_ns, ns_to_cycles, rdtsc, rdtscp, tsc_hz, Tsc, TscHz};
