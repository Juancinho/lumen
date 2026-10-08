//! Process CPU time (T014): the throughput runs report CPU share next to chunks/s, so the
//! indexing budget (docs/PERFORMANCE.md §9: "≥8 chunks/s @ ≤50% CPU") can be checked
//! per configuration. Measures this process, or another one (`--cpu-pid`, e.g. a
//! `llama-server` doing the work).

use std::time::Duration;

/// CPU time (user + kernel) consumed so far by `pid` (this process when `None`).
pub(crate) fn cpu_time(pid: Option<u32>) -> Option<Duration> {
    imp(pid)
}

#[cfg(windows)]
fn imp(pid: Option<u32>) -> Option<Duration> {
    lumen_windows::process::cpu_time(pid)
}

#[cfg(target_os = "linux")]
fn imp(pid: Option<u32>) -> Option<Duration> {
    let path = pid.map_or_else(
        || "/proc/self/stat".to_owned(),
        |p| format!("/proc/{p}/stat"),
    );
    parse_stat(&std::fs::read_to_string(path).ok()?)
}

#[cfg(not(any(windows, target_os = "linux")))]
fn imp(_pid: Option<u32>) -> Option<Duration> {
    None
}

/// `utime + stime` from `/proc/<pid>/stat` (fields 14 and 15, in clock ticks; USER_HZ is
/// 100 on every mainstream Linux). The command name (field 2) may contain spaces, so
/// fields are counted after its closing parenthesis.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_stat(stat: &str) -> Option<Duration> {
    let rest = &stat[stat.rfind(')')? + 1..];
    let mut fields = rest.split_whitespace().skip(11);
    let utime: u64 = fields.next()?.parse().ok()?;
    let stime: u64 = fields.next()?.parse().ok()?;
    Some(Duration::from_millis((utime + stime) * 10))
}

/// CPU measured over a wall-clock interval.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub(crate) struct CpuUse {
    pub(crate) cpu_s: f64,
    /// Average busy cores (`cpu_s / wall_s`).
    pub(crate) cores: f64,
    /// Share of the whole machine (all logical CPUs), 0–100: Task Manager's CPU column.
    pub(crate) machine_percent: f64,
}

impl CpuUse {
    pub(crate) fn between(
        start: Option<Duration>,
        end: Option<Duration>,
        wall_s: f64,
    ) -> Option<Self> {
        let cpu_s = end?.checked_sub(start?)?.as_secs_f64();
        if wall_s <= 0.0 {
            return None;
        }
        let cores = cpu_s / wall_s;
        #[allow(clippy::cast_precision_loss)]
        let logical = std::thread::available_parallelism().map_or(1, usize::from) as f64;
        Some(Self {
            cpu_s,
            cores,
            machine_percent: 100.0 * cores / logical,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_proc_stat_with_spaces_in_the_name() {
        let stat = "4242 (my (odd) proc) R 1 2 3 4 5 6 7 8 9 10 150 25 0 0 20 0 8 0";
        assert_eq!(parse_stat(stat), Some(Duration::from_millis(1750)));
        assert_eq!(parse_stat("garbage"), None);
    }

    #[test]
    fn own_cpu_time_grows_and_converts_to_a_share() {
        let start = cpu_time(None);
        let mut x = 0u64;
        let t = std::time::Instant::now();
        while t.elapsed() < Duration::from_millis(60) {
            x = std::hint::black_box(x.wrapping_mul(31).wrapping_add(7));
        }
        let end = cpu_time(None);
        if cfg!(any(windows, target_os = "linux")) {
            assert!(end >= start && start.is_some());
        }
        let u = CpuUse::between(Some(Duration::ZERO), Some(Duration::from_secs(2)), 4.0).unwrap();
        assert!((u.cores - 0.5).abs() < 1e-9 && u.machine_percent > 0.0);
        assert!(CpuUse::between(None, Some(Duration::ZERO), 1.0).is_none());
    }
}
