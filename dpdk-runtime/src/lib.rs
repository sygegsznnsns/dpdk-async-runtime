//! DPDK async runtime library.
//!
//! Provides:
//! - Safe DPDK wrappers (EAL, mempool, mbuf, ethdev)
//! - Protocol helpers (Ethernet, IPv4, ICMP, ARP)
//! - Single-threaded async runtime (executor, waker, timer, reactor)
//! - Measurement infrastructure (TSC, latency stats)

pub mod dpdk;
pub mod net;
pub mod rt;
pub mod stats;
