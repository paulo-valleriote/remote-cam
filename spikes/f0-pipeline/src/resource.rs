//! CPU and memory sampler for the pipeline run (T080).
//!
//! **NON-PRODUCTION** — F0 risk-reduction spike.
//!
//! The gated statistics are **machine-normalised CPU p95** and **maximum
//! private working set**. The sampler reads process and system times, derives
//! `cpu_pct_single_core` and divides by the logical core count, and records the
//! values Task Manager shows as "Memory".
//!
//! On Windows the counters come from `GetProcessTimes`, `GetSystemTimes` and
//! `GetProcessMemoryInfo`. The CSV/summary layer is platform neutral so the CI
//! can test the arithmetic without a GPU or a Windows runner.

use std::fmt::Write as _;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub const RESOURCE_CSV_HEADER: &str = "sample_id,run_id,elapsed_ms,cpu_pct_machine,cpu_pct_single_core,private_working_set_bytes,total_working_set_bytes,peak_working_set_bytes,thread_count,handle_count";

/// One resource sample.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResourceSample {
    pub sample_id: u64,
    pub run_id: String,
    pub elapsed_ms: f64,
    pub cpu_pct_machine: f64,
    pub cpu_pct_single_core: f64,
    pub private_working_set_bytes: u64,
    pub total_working_set_bytes: u64,
    pub peak_working_set_bytes: u64,
    pub thread_count: u32,
    pub handle_count: u32,
}

impl ResourceSample {
    /// Contract cross-check: machine CPU equals single-core CPU / logical cores.
    pub fn is_consistent(&self, logical_cores: u32) -> bool {
        if logical_cores == 0 {
            return false;
        }
        (self.cpu_pct_machine - self.cpu_pct_single_core / logical_cores as f64).abs() < 0.5
    }
}

/// Linear-interpolated percentile, matching the Python tooling.
pub fn percentile(values: &[f64], q: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|left, right| left.total_cmp(right));
    let rank = (q / 100.0).clamp(0.0, 1.0) * (sorted.len() - 1) as f64;
    let low = rank.floor() as usize;
    let high = rank.ceil() as usize;
    if low == high {
        sorted[low]
    } else {
        let fraction = rank - low as f64;
        sorted[low] + (sorted[high] - sorted[low]) * fraction
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PercentileSet {
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
    pub max: f64,
}

impl PercentileSet {
    pub fn from_values(values: &[f64]) -> Self {
        Self {
            p50: percentile(values, 50.0),
            p95: percentile(values, 95.0),
            p99: percentile(values, 99.0),
            max: values.iter().copied().fold(0.0, f64::max),
        }
    }
}

/// Resource summary contract.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResourceSummary {
    pub run_id: String,
    pub duration_s: f64,
    pub sample_count: usize,
    pub cpu_pct_machine: PercentileSet,
    pub private_working_set_mb: PercentileSet,
    pub criterion: serde_json::Value,
    pub verdict: String,
    pub sampler: String,
    pub logical_cores: u32,
}

impl ResourceSummary {
    pub fn from_samples(
        run_id: &str,
        duration_s: f64,
        logical_cores: u32,
        samples: &[ResourceSample],
    ) -> Self {
        let cpu: Vec<f64> = samples
            .iter()
            .map(|sample| sample.cpu_pct_machine)
            .collect();
        let memory: Vec<f64> = samples
            .iter()
            .map(|sample| sample.private_working_set_bytes as f64 / (1024.0 * 1024.0))
            .collect();
        let cpu_set = PercentileSet::from_values(&cpu);
        let memory_set = PercentileSet::from_values(&memory);
        let passed = cpu_set.p95 <= 15.0 && memory_set.max <= 400.0;
        Self {
            run_id: run_id.to_owned(),
            duration_s,
            sample_count: samples.len(),
            cpu_pct_machine: cpu_set,
            private_working_set_mb: memory_set,
            criterion: serde_json::json!({
                "cpu": { "metric": "cpu_pct_machine.p95", "threshold": 15, "comparison": "<=" },
                "memory": { "metric": "private_working_set_mb.max", "threshold": 400, "comparison": "<=" },
            }),
            verdict: if passed { "pass" } else { "fail" }.to_owned(),
            sampler: "GetProcessTimes + GetSystemTimes + GetProcessMemoryInfo".to_owned(),
            logical_cores,
        }
    }

    pub fn document(&self, cross_check: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "run_id": self.run_id,
            "duration_s": self.duration_s,
            "sample_count": self.sample_count,
            "cpu_pct_machine": self.cpu_pct_machine,
            "private_working_set_mb": self.private_working_set_mb,
            "criterion": self.criterion,
            "verdict": self.verdict,
            "sampler": self.sampler,
            "logical_cores": self.logical_cores,
            "cross_check": cross_check,
        })
    }
}

pub fn write_resource_csv(path: &Path, samples: &[ResourceSample]) -> Result<(), String> {
    let mut output = String::new();
    let _ = writeln!(output, "{RESOURCE_CSV_HEADER}");
    for sample in samples {
        let _ = writeln!(
            output,
            "{},{},{:.3},{:.4},{:.4},{},{},{},{},{}",
            sample.sample_id,
            sample.run_id,
            sample.elapsed_ms,
            sample.cpu_pct_machine,
            sample.cpu_pct_single_core,
            sample.private_working_set_bytes,
            sample.total_working_set_bytes,
            sample.peak_working_set_bytes,
            sample.thread_count,
            sample.handle_count,
        );
    }
    std::fs::write(path, output).map_err(|error| error.to_string())
}

#[cfg(windows)]
pub mod windows_sampler {
    use std::time::Instant;

    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows::Win32::System::Threading::{
        GetCurrentProcess, GetProcessHandleCount, GetProcessTimes, GetSystemTimes,
    };

    use super::ResourceSample;

    fn filetime_ticks(time: FILETIME) -> u64 {
        ((time.dwHighDateTime as u64) << 32) | time.dwLowDateTime as u64
    }

    /// Sample the current process. Returns `None` when a counter fails.
    pub fn sample(
        sample_id: u64,
        run_id: &str,
        started: Instant,
        previous: &mut Option<(u64, u64, u64)>,
        logical_cores: u32,
    ) -> Option<ResourceSample> {
        unsafe {
            let process = GetCurrentProcess();
            let mut creation = FILETIME::default();
            let mut exit = FILETIME::default();
            let mut kernel = FILETIME::default();
            let mut user = FILETIME::default();
            GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user).ok()?;
            let process_ticks = filetime_ticks(kernel) + filetime_ticks(user);

            let mut idle = FILETIME::default();
            let mut system_kernel = FILETIME::default();
            let mut system_user = FILETIME::default();
            GetSystemTimes(Some(&mut idle), Some(&mut system_kernel), Some(&mut system_user)).ok()?;
            let system_ticks = filetime_ticks(system_kernel) + filetime_ticks(system_user);

            let (previous_process, previous_system, previous_wall) = previous.unwrap_or((0, 0, 0));
            let wall_ticks = started.elapsed().as_nanos() as u64 / 100;
            let delta_process = process_ticks.saturating_sub(previous_process);
            let delta_system = system_ticks.saturating_sub(previous_system);
            let delta_wall = wall_ticks.saturating_sub(previous_wall).max(1);
            *previous = Some((process_ticks, system_ticks, wall_ticks));

            let cpu_pct_machine = if delta_system == 0 {
                0.0
            } else {
                100.0 * delta_process as f64 / delta_system as f64
            };
            let cpu_pct_single_core = 100.0 * delta_process as f64 / delta_wall as f64;

            let mut counters = PROCESS_MEMORY_COUNTERS::default();
            counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
            GetProcessMemoryInfo(process, &mut counters, counters.cb).ok()?;

            let mut handles = 0_u32;
            GetProcessHandleCount(process, &mut handles).ok()?;

            Some(ResourceSample {
                sample_id,
                run_id: run_id.to_owned(),
                elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
                cpu_pct_machine,
                cpu_pct_single_core,
                private_working_set_bytes: counters.PagefileUsage as u64,
                total_working_set_bytes: counters.WorkingSetSize as u64,
                peak_working_set_bytes: counters.PeakWorkingSetSize as u64,
                thread_count: 0,
                handle_count: handles,
            })
        }
    }

    /// `typeperf` cross-check for the sampler, in percent of the whole machine.
    pub fn typeperf_cpu(process_name: &str) -> Result<f64, String> {
        use std::process::Command;

        let counter = format!("\\Process({process_name})\\% Processor Time");
        let output = Command::new("typeperf")
            .args([counter.as_str(), "-sc", "1"])
            .output()
            .map_err(|error| format!("cannot run typeperf: {error}"))?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let value = text
            .lines()
            .filter(|line| line.starts_with('"'))
            .nth(1)
            .and_then(|line| line.split(',').next_back())
            .map(|value| value.trim_matches('"').replace(',', "."))
            .ok_or_else(|| "typeperf returned no sample".to_owned())?;
        // `typeperf` reports percent of one core. The caller normalises by the
        // recorded logical core count before comparing with the sampler, so the
        // core count stays visible in the evidence.
        value
            .parse::<f64>()
            .map_err(|_| format!("cannot parse typeperf value {value:?}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(index: u64, cpu: f64, memory_mb: f64) -> ResourceSample {
        ResourceSample {
            sample_id: index,
            run_id: "res-01".to_owned(),
            elapsed_ms: index as f64 * 1000.0,
            cpu_pct_machine: cpu,
            cpu_pct_single_core: cpu * 16.0,
            private_working_set_bytes: (memory_mb * 1024.0 * 1024.0) as u64,
            total_working_set_bytes: 0,
            peak_working_set_bytes: 0,
            thread_count: 10,
            handle_count: 20,
        }
    }

    #[test]
    fn percentiles_match_the_interpolated_definition() {
        let values = [1.0, 2.0, 3.0, 4.0];
        assert_eq!(percentile(&values, 50.0), 2.5);
        assert_eq!(percentile(&values, 0.0), 1.0);
        assert_eq!(percentile(&values, 100.0), 4.0);
    }

    #[test]
    fn summary_gates_on_p95_and_max_not_the_mean() {
        let samples: Vec<_> = (0..100)
            .map(|index| sample(index, if index == 99 { 20.0 } else { 5.0 }, 200.0))
            .collect();
        let summary = ResourceSummary::from_samples("res-01", 100.0, 16, &samples);
        // 99 samples at 5 % and one at 20 %: the p95 is 5 %, the maximum 20 %.
        assert!((summary.cpu_pct_machine.p95 - 5.0).abs() < 0.001);
        assert_eq!(summary.cpu_pct_machine.max, 20.0);
        assert_eq!(summary.private_working_set_mb.max, 200.0);
        assert_eq!(summary.verdict, "pass");
    }

    #[test]
    fn the_cross_check_rejects_mismatched_normalisation() {
        let good = sample(1, 6.25, 100.0);
        assert!(good.is_consistent(16));
        let mut bad = good.clone();
        bad.cpu_pct_machine = 8.0;
        assert!(!bad.is_consistent(16));
    }
}
