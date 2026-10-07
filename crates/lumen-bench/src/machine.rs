//! Machine/build metadata and process memory, recorded with every report
//! (docs/PERFORMANCE.md §12: baselines need machine/config context).

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct MachineInfo {
    pub(crate) os: &'static str,
    pub(crate) arch: &'static str,
    pub(crate) logical_cpus: usize,
    pub(crate) cpu: Option<String>,
    /// `release` is the only acceptable profile for evidence.
    pub(crate) build_profile: &'static str,
    pub(crate) lumen_version: &'static str,
    pub(crate) unix_time_s: u64,
}

impl MachineInfo {
    pub(crate) fn collect() -> Self {
        Self {
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            logical_cpus: std::thread::available_parallelism().map_or(0, usize::from),
            cpu: cpu_name(),
            build_profile: if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            },
            lumen_version: env!("CARGO_PKG_VERSION"),
            unix_time_s: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
        }
    }
}

fn cpu_name() -> Option<String> {
    if cfg!(target_os = "linux") {
        let info = std::fs::read_to_string("/proc/cpuinfo").ok()?;
        return info
            .lines()
            .find(|l| l.starts_with("model name"))
            .and_then(|l| l.split_once(':'))
            .map(|(_, v)| v.trim().to_owned());
    }
    // Windows: family/model/stepping string; exact marketing name needs the registry.
    std::env::var("PROCESSOR_IDENTIFIER").ok()
}

/// Process memory snapshot in MiB. On Windows `resident` is the working set (not the
/// private working set PERFORMANCE.md budgets refer to; use Task Manager/WPA for that).
#[derive(Debug, Clone, Copy, Serialize)]
pub(crate) struct MemorySnapshot {
    pub(crate) resident_mib: f64,
    pub(crate) virtual_mib: f64,
}

pub(crate) fn memory() -> Option<MemorySnapshot> {
    #[allow(clippy::cast_precision_loss)]
    let mib = |bytes: usize| bytes as f64 / (1024.0 * 1024.0);
    memory_stats::memory_stats().map(|m| MemorySnapshot {
        resident_mib: mib(m.physical_mem),
        virtual_mib: mib(m.virtual_mem),
    })
}
