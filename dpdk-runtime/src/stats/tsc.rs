//! TSC (Time Stamp Counter) helpers.
//!
//! Uses `rdtsc` for cycle-accurate timing. The TSC frequency is calibrated
//! at startup via CPUID leaf 0x15/0x16, falling back to kernel cpufreq info.

use std::sync::OnceLock;

static TSC_HZ: OnceLock<u64> = OnceLock::new();

/// Read the current TSC value.
#[inline(always)]
pub fn rdtsc() -> u64 {
    unsafe { core::arch::x86_64::_rdtsc() }
}

/// A serializing `rdtsc` (lfence + rdtsc), useful for measuring before/after.
#[inline(always)]
pub fn rdtscp() -> (u64, u32) {
    unsafe {
        let mut aux: u32 = 0;
        let tsc = core::arch::x86_64::__rdtscp(&mut aux as *mut u32);
        (tsc, aux)
    }
}

/// Calibrate and return the TSC frequency in Hz.
///
/// Strategy:
/// 1. Try CPUID leaf 0x15 (TSC/Crystal ratio) — most accurate on modern Intel/AMD.
/// 2. Try CPUID leaf 0x16 (Processor frequency).
/// 3. Fall back to `/sys/devices/system/cpu/cpu0/cpufreq/scaling_max_freq`.
/// 4. Last resort: measure against `clock_gettime(CLOCK_MONOTONIC_RAW)`.
pub fn tsc_hz() -> u64 {
    *TSC_HZ.get_or_init(tsc_hz_init)
}

fn tsc_hz_init() -> u64 {
    if let Some(hz) = tsc_hz_cpuid_0x15() {
        return hz;
    }
    if let Some(hz) = tsc_hz_cpuid_0x16() {
        return hz;
    }
    if let Some(hz) = tsc_hz_cpufreq() {
        return hz;
    }
    tsc_hz_measure()
}

fn tsc_hz_cpuid_0x15() -> Option<u64> {
    let r = core::arch::x86_64::__cpuid_count(0x15, 0);
    if r.eax == 0 || r.ebx == 0 || r.ecx == 0 {
        return None;
    }
    let crystal_hz = r.ecx as u64;
    Some(crystal_hz * r.ebx as u64 / r.eax as u64)
}

fn tsc_hz_cpuid_0x16() -> Option<u64> {
    let r = core::arch::x86_64::__cpuid_count(0x16, 0);
    if r.eax == 0 {
        return None;
    }
    Some(r.eax as u64 * 1_000_000)
}

fn tsc_hz_cpufreq() -> Option<u64> {
    let path = "/sys/devices/system/cpu/cpu0/cpufreq/scaling_max_freq";
    let content = std::fs::read_to_string(path).ok()?;
    let khz: u64 = content.trim().parse().ok()?;
    Some(khz * 1000)
}

fn tsc_hz_measure() -> u64 {
    use std::time::{Duration, Instant};
    let start = Instant::now();
    let tsc0 = rdtsc();
    std::thread::sleep(Duration::from_millis(100));
    let tsc1 = rdtsc();
    let elapsed = start.elapsed();
    let cycles = tsc1 - tsc0;
    let secs = elapsed.as_secs_f64();
    (cycles as f64 / secs) as u64
}

/// Convert TSC cycles to nanoseconds.
#[inline]
pub fn cycles_to_ns(cycles: u64) -> u64 {
    let hz = tsc_hz();
    (cycles as u128 * 1_000_000_000u128 / hz as u128) as u64
}

/// Convert nanoseconds to TSC cycles.
#[inline]
pub fn ns_to_cycles(ns: u64) -> u64 {
    let hz = tsc_hz();
    (ns as u128 * hz as u128 / 1_000_000_000u128) as u64
}

/// Type alias for TSC frequency, used by other modules.
pub type TscHz = u64;

/// TSC context: cached frequency + cycle reading.
pub struct Tsc {
    hz: u64,
}

impl Tsc {
    pub fn new() -> Self {
        Tsc { hz: tsc_hz() }
    }

    #[inline(always)]
    pub fn now_cycles(&self) -> u64 {
        rdtsc()
    }

    #[inline]
    pub fn cycles_per_sec(&self) -> u64 {
        self.hz
    }

    #[inline]
    pub fn cycles_to_ns(&self, cycles: u64) -> u64 {
        (cycles as u128 * 1_000_000_000u128 / self.hz as u128) as u64
    }

    #[inline]
    pub fn ns_to_cycles(&self, ns: u64) -> u64 {
        (ns as u128 * self.hz as u128 / 1_000_000_000u128) as u64
    }
}

impl Default for Tsc {
    fn default() -> Self {
        Self::new()
    }
}
