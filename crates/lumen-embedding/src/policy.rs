//! Embedding device selection and fallback policy (T013, ADR-019).
//!
//! Pure decision logic: the caller supplies measured [`DeviceProbe`]s, the current
//! [`SystemState`] (power, profile, memory — read by the shell/platform layer) and the
//! [`Quarantine`]; [`plan`] returns which device serves interactive queries and how
//! background indexing runs. Nothing here touches a runtime or the OS, so every rule is
//! unit-tested.
//!
//! Rules, in order of precedence:
//! 1. **CPU is always available and always the fallback.** A failed, missing or rejected
//!    accelerator never blocks search or indexing.
//! 2. **Same space only.** A device is considered only if its probe ran the index
//!    generation's exact [`crate::EmbeddingSpace`] (same weights, prompts, dimension):
//!    device choice never changes weights inside a generation (ADR-015 §4).
//! 3. **Prove it first.** An accelerator is eligible only with a successful probe that shows
//!    stable output, vectors interchangeable with CPU ones (cosine ≥ 0.999 on the same
//!    inputs), the graph actually offloaded, and device memory within budget
//!    (min(1.5 GiB, 50 %); Turbo: 60 % of the device). Integrated
//!    GPUs are excluded by default (T006: device hang on a Radeon iGPU).
//! 4. **Faster by a margin, where it matters.** Queries stay on CPU while CPU meets the
//!    latency budget; indexing moves to an accelerator only when its throughput beats CPU by
//!    [`PolicyConfig::min_index_speedup`].
//! 5. **Resources.** Battery, low battery, memory pressure, an active user and the
//!    Eco/Balanced/Turbo profile bound indexing; interactive queries are never paused.
//! 6. **Quarantine on failure.** A device that fails at runtime is quarantined for its
//!    `runtime_key` (runtime + driver versions); a driver/runtime update lifts it.

use std::collections::BTreeMap;
use std::fmt;

use crate::backend::EmbeddingError;
use crate::model::ExecutionTarget;

/// Backend-specific device id of the CPU (`lumen-embedding-ort` uses `cpu`, `dml:N`, …).
pub const CPU_DEVICE: &str = "cpu";

/// One measured candidate device (see [`crate::probe`]).
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceProbe {
    /// Backend-specific id, e.g. `cpu`, `dml:0`.
    pub device: String,
    pub target: ExecutionTarget,
    /// Integrated GPU / shared system memory.
    pub integrated: bool,
    /// [`crate::EmbeddingSpace::key`] the probe ran.
    pub space_key: String,
    /// Runtime + driver versions. A change invalidates the probe and lifts a quarantine.
    pub runtime_key: String,
    pub outcome: ProbeOutcome,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProbeOutcome {
    Measured(ProbeMetrics),
    /// Session creation or inference failed; message for logs only.
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProbeMetrics {
    pub query_p50_ms: f64,
    pub query_p95_ms: f64,
    /// Document chunks per second at the probe's batch size.
    pub index_chunks_per_s: f64,
    /// Lowest cosine between this device's vectors and the CPU's for the same inputs
    /// (1.0 for the CPU itself). `None` = not compared.
    pub min_cosine_vs_cpu: Option<f64>,
    /// Repeated runs agreed and no vector was NaN or zero.
    pub stable: bool,
    /// Fraction of graph nodes on the accelerator (1.0 = fully offloaded); `None` = unknown.
    pub offloaded_fraction: Option<f64>,
    /// Device memory used by the session, and the device's total.
    pub device_memory_mib: Option<f64>,
    pub device_memory_total_mib: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerSource {
    Ac,
    Battery { percent: Option<u8> },
    Unknown,
}

/// User-facing resource profile (docs/SEARCH_AND_INDEXING.md §21).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResourceProfile {
    /// Low concurrency, nothing heavy on battery.
    Eco,
    #[default]
    Balanced,
    /// The user asked for faster indexing, also on battery.
    Turbo,
}

/// What the platform layer reports right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemState {
    pub power: PowerSource,
    pub profile: ResourceProfile,
    pub logical_cpus: usize,
    /// Available physical memory; `None` = unknown.
    pub available_memory_mib: Option<u64>,
    /// The user is interacting with the PC (input in the last minutes, foreground load).
    pub user_active: bool,
}

/// Thresholds. Defaults are justified in ADR-019.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PolicyConfig {
    /// Warm query p95 budget (docs/PERFORMANCE.md §2).
    pub query_budget_p95_ms: f64,
    /// An accelerator serves queries only if CPU misses the budget and it is this much faster.
    pub min_query_speedup: f64,
    /// An accelerator indexes only if this much faster than CPU.
    pub min_index_speedup: f64,
    /// Vectors from another device must match CPU vectors at least this well.
    pub min_cosine_vs_cpu: f64,
    /// Accelerators must run at least this fraction of the graph (CPU fallback = copies).
    pub min_offloaded_fraction: f64,
    /// Absolute device memory cap for one session.
    pub max_device_memory_mib: f64,
    /// Cap as a fraction of the device's total memory (leave room for games/other apps).
    pub max_device_memory_fraction: f64,
    /// Turbo (the user asked for speed): the only cap is this fraction of the device's total
    /// memory, which must be known.
    pub turbo_max_device_memory_fraction: f64,
    pub allow_integrated_gpu: bool,
    /// Below this battery percentage indexing pauses (except Turbo).
    pub low_battery_percent: u8,
    /// Below this available memory indexing pauses.
    pub min_available_memory_mib: u64,
}

impl Default for PolicyConfig {
    fn default() -> Self {
        Self {
            query_budget_p95_ms: 120.0,
            min_query_speedup: 1.5,
            min_index_speedup: 1.5,
            min_cosine_vs_cpu: 0.999,
            min_offloaded_fraction: 0.9,
            max_device_memory_mib: 1536.0,
            max_device_memory_fraction: 0.5,
            turbo_max_device_memory_fraction: 0.6,
            allow_integrated_gpu: false,
            low_battery_percent: 20,
            min_available_memory_mib: 768,
        }
    }
}

/// Why a device was not used (shown in diagnostics/settings).
#[derive(Debug, Clone, PartialEq)]
pub enum Rejection {
    ProbeFailed(String),
    /// Probe ran other weights/prompts/dimension than the index generation.
    DifferentSpace,
    Quarantined(String),
    Unstable,
    /// Not compared with CPU, or below [`PolicyConfig::min_cosine_vs_cpu`].
    Fidelity(Option<f64>),
    /// Placement unknown, or below [`PolicyConfig::min_offloaded_fraction`].
    Offload(Option<f64>),
    IntegratedGpu,
    DeviceMemory {
        used_mib: Option<f64>,
        limit_mib: f64,
    },
    /// No successful CPU probe to compare with.
    NoCpuBaseline,
    /// Eligible but not used for this lane.
    NotFasterEnough {
        lane: Lane,
        speedup: f64,
    },
    CpuMeetsQueryBudget,
    /// Eligible but resources/profile keep it off (battery, Eco, user active).
    Resources {
        lane: Lane,
        why: PauseReason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    Query,
    Indexing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PauseReason {
    OnBattery,
    LowBattery,
    MemoryPressure,
    EcoProfile,
    UserActive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexingPlan {
    Run {
        device: String,
        /// Intra-op threads for CPU; 1 host thread for accelerators.
        threads: usize,
    },
    Paused(PauseReason),
}

#[derive(Debug, Clone, PartialEq)]
pub struct DevicePlan {
    pub query_device: String,
    pub indexing: IndexingPlan,
    /// Every non-chosen accelerator with the reason (one entry per device and lane).
    pub rejected: Vec<(String, Rejection)>,
}

/// Devices that failed at runtime, keyed by device id; cleared when `runtime_key` changes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Quarantine {
    entries: BTreeMap<String, (String, String)>,
}

impl Quarantine {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a runtime failure. Returns `true` if `error` quarantines the device (lost
    /// device, broken output) — the caller then re-runs the batch on CPU.
    /// Cancellation and input errors never quarantine.
    pub fn record_failure(
        &mut self,
        device: &str,
        runtime_key: &str,
        error: &EmbeddingError,
    ) -> bool {
        if device == CPU_DEVICE || !is_device_failure(error) {
            return false;
        }
        self.entries.insert(
            device.to_owned(),
            (runtime_key.to_owned(), error.to_string()),
        );
        true
    }

    /// Reason, if `device` is quarantined for this `runtime_key`.
    #[must_use]
    pub fn reason(&self, device: &str, runtime_key: &str) -> Option<&str> {
        self.entries
            .get(device)
            .filter(|(key, _)| key == runtime_key)
            .map(|(_, why)| why.as_str())
    }

    /// For persistence in settings: `(device, runtime_key, reason)`.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str, &str)> {
        self.entries
            .iter()
            .map(|(d, (k, r))| (d.as_str(), k.as_str(), r.as_str()))
    }

    pub fn restore(&mut self, device: &str, runtime_key: &str, reason: &str) {
        self.entries.insert(
            device.to_owned(),
            (runtime_key.to_owned(), reason.to_owned()),
        );
    }
}

/// Errors that mean "this device cannot be trusted": move the work to CPU.
#[must_use]
pub fn is_device_failure(error: &EmbeddingError) -> bool {
    matches!(
        error,
        EmbeddingError::Backend(_)
            | EmbeddingError::NonFinite { .. }
            | EmbeddingError::ZeroVector { .. }
            | EmbeddingError::OutputShape { .. }
    )
}

/// Decides the query device and the indexing plan for the index generation `space_key`.
#[must_use]
pub fn plan(
    space_key: &str,
    probes: &[DeviceProbe],
    quarantine: &Quarantine,
    state: &SystemState,
    cfg: &PolicyConfig,
) -> DevicePlan {
    let cpu = probes
        .iter()
        .find(|p| p.device == CPU_DEVICE && p.space_key == space_key)
        .and_then(|p| match &p.outcome {
            ProbeOutcome::Measured(m) => Some(*m),
            ProbeOutcome::Failed(_) => None,
        });

    let mut rejected = Vec::new();
    let mut eligible: Vec<(&DeviceProbe, ProbeMetrics)> = Vec::new();
    for p in probes.iter().filter(|p| p.device != CPU_DEVICE) {
        match eligibility(p, space_key, quarantine, cpu.as_ref(), state, cfg) {
            Ok(m) => eligible.push((p, m)),
            Err(why) => rejected.push((p.device.clone(), why)),
        }
    }

    // ---- query lane: never paused, CPU unless it misses the budget -------------------------
    let mut query_device = CPU_DEVICE.to_owned();
    if let Some(cpu_m) = cpu {
        let cpu_meets = cpu_m.query_p95_ms <= cfg.query_budget_p95_ms;
        let best = eligible
            .iter()
            .min_by(|a, b| a.1.query_p95_ms.total_cmp(&b.1.query_p95_ms));
        for (p, m) in &eligible {
            let speedup = cpu_m.query_p95_ms / m.query_p95_ms.max(f64::MIN_POSITIVE);
            let is_best = best.is_some_and(|(b, _)| b.device == p.device);
            let why = if cpu_meets {
                Some(Rejection::CpuMeetsQueryBudget)
            } else if speedup < cfg.min_query_speedup || !is_best {
                Some(Rejection::NotFasterEnough {
                    lane: Lane::Query,
                    speedup,
                })
            } else {
                accelerator_blocked(state).map(|r| Rejection::Resources {
                    lane: Lane::Query,
                    why: r,
                })
            };
            match why {
                Some(r) => rejected.push((p.device.clone(), r)),
                None => query_device.clone_from(&p.device),
            }
        }
    }

    // ---- indexing lane --------------------------------------------------------------------
    let indexing = match indexing_pause(state, cfg) {
        Some(reason) => {
            for (p, _) in &eligible {
                rejected.push((
                    p.device.clone(),
                    Rejection::Resources {
                        lane: Lane::Indexing,
                        why: reason,
                    },
                ));
            }
            IndexingPlan::Paused(reason)
        }
        None => {
            let mut chosen: Option<String> = None;
            if let Some(cpu_m) = cpu {
                let best = eligible
                    .iter()
                    .max_by(|a, b| a.1.index_chunks_per_s.total_cmp(&b.1.index_chunks_per_s));
                for (p, m) in &eligible {
                    let speedup = m.index_chunks_per_s / cpu_m.index_chunks_per_s.max(1e-9);
                    let is_best = best.is_some_and(|(b, _)| b.device == p.device);
                    let why = if speedup < cfg.min_index_speedup || !is_best {
                        Some(Rejection::NotFasterEnough {
                            lane: Lane::Indexing,
                            speedup,
                        })
                    } else {
                        accelerator_blocked(state).map(|r| Rejection::Resources {
                            lane: Lane::Indexing,
                            why: r,
                        })
                    };
                    match why {
                        Some(r) => rejected.push((p.device.clone(), r)),
                        None => chosen = Some(p.device.clone()),
                    }
                }
            }
            match chosen {
                Some(device) => IndexingPlan::Run { device, threads: 1 },
                None => IndexingPlan::Run {
                    device: CPU_DEVICE.to_owned(),
                    threads: cpu_index_threads(state),
                },
            }
        }
    };

    DevicePlan {
        query_device,
        indexing,
        rejected,
    }
}

fn eligibility(
    p: &DeviceProbe,
    space_key: &str,
    quarantine: &Quarantine,
    cpu: Option<&ProbeMetrics>,
    state: &SystemState,
    cfg: &PolicyConfig,
) -> Result<ProbeMetrics, Rejection> {
    if p.space_key != space_key {
        return Err(Rejection::DifferentSpace);
    }
    if let Some(why) = quarantine.reason(&p.device, &p.runtime_key) {
        return Err(Rejection::Quarantined(why.to_owned()));
    }
    let m = match &p.outcome {
        ProbeOutcome::Measured(m) => *m,
        ProbeOutcome::Failed(why) => return Err(Rejection::ProbeFailed(why.clone())),
    };
    if cpu.is_none() {
        return Err(Rejection::NoCpuBaseline);
    }
    if !m.stable {
        return Err(Rejection::Unstable);
    }
    if m.min_cosine_vs_cpu
        .is_none_or(|c| c < cfg.min_cosine_vs_cpu)
    {
        return Err(Rejection::Fidelity(m.min_cosine_vs_cpu));
    }
    if m.offloaded_fraction
        .is_none_or(|f| f < cfg.min_offloaded_fraction)
    {
        return Err(Rejection::Offload(m.offloaded_fraction));
    }
    if p.integrated && !cfg.allow_integrated_gpu {
        return Err(Rejection::IntegratedGpu);
    }
    let limit = match (state.profile, m.device_memory_total_mib) {
        (ResourceProfile::Turbo, Some(total)) => total * cfg.turbo_max_device_memory_fraction,
        (_, Some(total)) => cfg
            .max_device_memory_mib
            .min(total * cfg.max_device_memory_fraction),
        (_, None) => cfg.max_device_memory_mib,
    };
    if m.device_memory_mib.is_none_or(|used| used > limit) {
        return Err(Rejection::DeviceMemory {
            used_mib: m.device_memory_mib,
            limit_mib: limit,
        });
    }
    Ok(m)
}

/// Accelerators (extra power draw, VRAM shared with games) stay off on battery, in Eco and
/// while the user is active, unless the user chose Turbo.
fn accelerator_blocked(state: &SystemState) -> Option<PauseReason> {
    match state.profile {
        ResourceProfile::Turbo => None,
        ResourceProfile::Eco => Some(PauseReason::EcoProfile),
        ResourceProfile::Balanced => {
            if matches!(state.power, PowerSource::Battery { .. }) {
                Some(PauseReason::OnBattery)
            } else if state.user_active {
                Some(PauseReason::UserActive)
            } else {
                None
            }
        }
    }
}

fn indexing_pause(state: &SystemState, cfg: &PolicyConfig) -> Option<PauseReason> {
    if state
        .available_memory_mib
        .is_some_and(|m| m < cfg.min_available_memory_mib)
    {
        return Some(PauseReason::MemoryPressure);
    }
    if state.profile == ResourceProfile::Turbo {
        return None;
    }
    match state.power {
        PowerSource::Battery { percent }
            if percent.is_some_and(|p| p < cfg.low_battery_percent) =>
        {
            Some(PauseReason::LowBattery)
        }
        PowerSource::Battery { .. } if state.profile == ResourceProfile::Eco => {
            Some(PauseReason::OnBattery)
        }
        _ => None,
    }
}

/// CPU indexing threads: never all cores outside Turbo (interactive queries preempt, the user
/// keeps a responsive PC).
fn cpu_index_threads(state: &SystemState) -> usize {
    let n = state.logical_cpus.max(1);
    let threads = match state.profile {
        ResourceProfile::Eco => 1,
        ResourceProfile::Turbo => n.saturating_sub(1),
        ResourceProfile::Balanced => {
            if matches!(state.power, PowerSource::Battery { .. }) {
                1
            } else if state.user_active {
                n / 4
            } else {
                n / 2
            }
        }
    };
    threads.max(1)
}

impl fmt::Display for Rejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProbeFailed(why) => write!(f, "probe failed: {why}"),
            Self::DifferentSpace => f.write_str("probe ran a different embedding space"),
            Self::Quarantined(why) => write!(f, "quarantined after a runtime failure: {why}"),
            Self::Unstable => f.write_str("output not stable across runs (NaN/zero/mismatch)"),
            Self::Fidelity(c) => match c {
                Some(c) => write!(f, "vectors differ from CPU (min cosine {c:.5})"),
                None => f.write_str("not compared with CPU vectors"),
            },
            Self::Offload(o) => match o {
                Some(o) => write!(f, "only {:.0}% of the graph offloaded", o * 100.0),
                None => f.write_str("graph placement unknown"),
            },
            Self::IntegratedGpu => f.write_str("integrated GPU (disabled by default)"),
            Self::DeviceMemory {
                used_mib,
                limit_mib,
            } => match used_mib {
                Some(u) => write!(f, "uses {u:.0} MiB device memory (limit {limit_mib:.0})"),
                None => f.write_str("device memory use unknown"),
            },
            Self::NoCpuBaseline => f.write_str("no CPU measurement to compare with"),
            Self::NotFasterEnough { lane, speedup } => {
                write!(f, "{lane:?}: only {speedup:.2}x CPU")
            }
            Self::CpuMeetsQueryBudget => f.write_str("Query: CPU already meets the budget"),
            Self::Resources { lane, why } => write!(f, "{lane:?}: held back ({why:?})"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPACE: &str = "embeddinggemma-2@q4/pre1/r@1/d256/l2";

    fn metrics(query_p95: f64, chunks_per_s: f64) -> ProbeMetrics {
        ProbeMetrics {
            query_p50_ms: query_p95 * 0.8,
            query_p95_ms: query_p95,
            index_chunks_per_s: chunks_per_s,
            min_cosine_vs_cpu: Some(0.9999),
            stable: true,
            offloaded_fraction: Some(0.97),
            device_memory_mib: Some(900.0),
            device_memory_total_mib: Some(4096.0),
        }
    }

    fn probe(device: &str, m: ProbeMetrics) -> DeviceProbe {
        DeviceProbe {
            device: device.to_owned(),
            target: if device == CPU_DEVICE {
                ExecutionTarget::Cpu
            } else {
                ExecutionTarget::Gpu
            },
            integrated: false,
            space_key: SPACE.to_owned(),
            runtime_key: "ort-1.24/driver-1".to_owned(),
            outcome: ProbeOutcome::Measured(m),
        }
    }

    fn cpu() -> DeviceProbe {
        let mut m = metrics(37.0, 3.5);
        m.offloaded_fraction = None;
        m.device_memory_mib = None;
        probe(CPU_DEVICE, m)
    }

    fn ac_idle() -> SystemState {
        SystemState {
            power: PowerSource::Ac,
            profile: ResourceProfile::Balanced,
            logical_cpus: 12,
            available_memory_mib: Some(8000),
            user_active: false,
        }
    }

    fn run(probes: &[DeviceProbe], state: &SystemState) -> DevicePlan {
        plan(
            SPACE,
            probes,
            &Quarantine::new(),
            state,
            &PolicyConfig::default(),
        )
    }

    fn reasons(p: &DevicePlan, device: &str) -> Vec<Rejection> {
        p.rejected
            .iter()
            .filter(|(d, _)| d == device)
            .map(|(_, r)| r.clone())
            .collect()
    }

    #[test]
    fn cpu_only_machine_uses_cpu_with_half_the_threads() {
        let p = run(&[cpu()], &ac_idle());
        assert_eq!(p.query_device, CPU_DEVICE);
        assert_eq!(
            p.indexing,
            IndexingPlan::Run {
                device: CPU_DEVICE.into(),
                threads: 6
            }
        );
        assert!(p.rejected.is_empty());
    }

    #[test]
    fn no_probes_at_all_still_runs_on_cpu() {
        let p = run(&[], &ac_idle());
        assert_eq!(p.query_device, CPU_DEVICE);
        assert!(matches!(p.indexing, IndexingPlan::Run { ref device, .. } if device == CPU_DEVICE));
    }

    #[test]
    fn t006_gtx1650_profile_indexes_on_gpu_but_queries_on_cpu() {
        // T006: DML q4 queries ~300 ms vs CPU 37 ms p95; indexing ~8.4 vs 3.5 chunks/s.
        let gpu = probe("dml:0", metrics(320.0, 8.4));
        let p = run(&[cpu(), gpu], &ac_idle());
        assert_eq!(p.query_device, CPU_DEVICE);
        assert_eq!(
            p.indexing,
            IndexingPlan::Run {
                device: "dml:0".into(),
                threads: 1
            }
        );
        assert_eq!(reasons(&p, "dml:0"), [Rejection::CpuMeetsQueryBudget]);
    }

    #[test]
    fn accelerator_must_beat_cpu_by_the_margin() {
        let gpu = probe("dml:0", metrics(320.0, 4.5)); // 1.29x
        let p = run(&[cpu(), gpu], &ac_idle());
        assert!(matches!(p.indexing, IndexingPlan::Run { ref device, .. } if device == CPU_DEVICE));
        assert!(reasons(&p, "dml:0").iter().any(|r| matches!(
            r,
            Rejection::NotFasterEnough {
                lane: Lane::Indexing,
                ..
            }
        )));
    }

    #[test]
    fn slow_cpu_moves_queries_to_a_much_faster_accelerator() {
        let mut slow = cpu();
        if let ProbeOutcome::Measured(m) = &mut slow.outcome {
            m.query_p95_ms = 250.0;
        }
        let npu = probe("npu:0", metrics(20.0, 30.0));
        let p = run(&[slow, npu], &ac_idle());
        assert_eq!(p.query_device, "npu:0");
    }

    #[test]
    fn every_eligibility_gate_rejects_with_its_reason() {
        let cfg = PolicyConfig::default();
        type Case = (DeviceProbe, fn(&Rejection) -> bool);
        let cases: Vec<Case> = vec![
            (
                {
                    let mut p = probe("a", metrics(10.0, 50.0));
                    p.space_key = "other-weights".into();
                    p
                },
                |r| *r == Rejection::DifferentSpace,
            ),
            (
                DeviceProbe {
                    outcome: ProbeOutcome::Failed("no adapter".into()),
                    ..probe("b", metrics(10.0, 50.0))
                },
                |r| matches!(r, Rejection::ProbeFailed(_)),
            ),
            (
                probe(
                    "c",
                    ProbeMetrics {
                        stable: false,
                        ..metrics(10.0, 50.0)
                    },
                ),
                |r| *r == Rejection::Unstable,
            ),
            (
                probe(
                    "d",
                    ProbeMetrics {
                        min_cosine_vs_cpu: Some(0.98),
                        ..metrics(10.0, 50.0)
                    },
                ),
                |r| matches!(r, Rejection::Fidelity(Some(_))),
            ),
            (
                probe(
                    "e",
                    ProbeMetrics {
                        min_cosine_vs_cpu: None,
                        ..metrics(10.0, 50.0)
                    },
                ),
                |r| *r == Rejection::Fidelity(None),
            ),
            (
                probe(
                    "f",
                    ProbeMetrics {
                        offloaded_fraction: Some(0.6),
                        ..metrics(10.0, 50.0)
                    },
                ),
                |r| matches!(r, Rejection::Offload(Some(_))),
            ),
            (
                DeviceProbe {
                    integrated: true,
                    ..probe("g", metrics(10.0, 50.0))
                },
                |r| *r == Rejection::IntegratedGpu,
            ),
            (
                probe(
                    "h",
                    ProbeMetrics {
                        device_memory_mib: Some(2900.0),
                        ..metrics(10.0, 50.0)
                    },
                ),
                |r| matches!(r, Rejection::DeviceMemory { .. }),
            ),
            (
                // 1.2 GB used of a 2 GB card: over 50% of its total.
                probe(
                    "i",
                    ProbeMetrics {
                        device_memory_mib: Some(1200.0),
                        device_memory_total_mib: Some(2048.0),
                        ..metrics(10.0, 50.0)
                    },
                ),
                |r| matches!(r, Rejection::DeviceMemory { limit_mib, .. } if (*limit_mib - 1024.0).abs() < 1e-9),
            ),
        ];
        for (p, expect) in cases {
            let name = p.device.clone();
            let plan = plan(SPACE, &[cpu(), p], &Quarantine::new(), &ac_idle(), &cfg);
            assert_eq!(plan.query_device, CPU_DEVICE, "{name}");
            assert!(
                matches!(plan.indexing, IndexingPlan::Run { ref device, .. } if device == CPU_DEVICE),
                "{name}"
            );
            let rs = reasons(&plan, &name);
            assert!(rs.len() == 1 && expect(&rs[0]), "{name}: {rs:?}");
        }
    }

    #[test]
    fn accelerators_need_a_cpu_baseline() {
        let failed_cpu = DeviceProbe {
            outcome: ProbeOutcome::Failed("x".into()),
            ..cpu()
        };
        let p = run(
            &[failed_cpu, probe("dml:0", metrics(10.0, 50.0))],
            &ac_idle(),
        );
        assert_eq!(reasons(&p, "dml:0"), [Rejection::NoCpuBaseline]);
        assert!(matches!(p.indexing, IndexingPlan::Run { ref device, .. } if device == CPU_DEVICE));
    }

    #[test]
    fn battery_eco_and_activity_keep_accelerators_off() {
        let gpu = || probe("dml:0", metrics(320.0, 8.4));
        let mut s = ac_idle();
        s.power = PowerSource::Battery { percent: Some(80) };
        let p = run(&[cpu(), gpu()], &s);
        assert_eq!(
            p.indexing,
            IndexingPlan::Run {
                device: CPU_DEVICE.into(),
                threads: 1
            }
        );

        let mut s = ac_idle();
        s.user_active = true;
        let p = run(&[cpu(), gpu()], &s);
        assert_eq!(
            p.indexing,
            IndexingPlan::Run {
                device: CPU_DEVICE.into(),
                threads: 3
            }
        );
        assert!(reasons(&p, "dml:0").contains(&Rejection::Resources {
            lane: Lane::Indexing,
            why: PauseReason::UserActive
        }));

        let mut s = ac_idle();
        s.profile = ResourceProfile::Eco;
        let p = run(&[cpu(), gpu()], &s);
        assert_eq!(
            p.indexing,
            IndexingPlan::Run {
                device: CPU_DEVICE.into(),
                threads: 1
            }
        );
    }

    #[test]
    fn indexing_pauses_but_queries_never_do() {
        let mut s = ac_idle();
        s.power = PowerSource::Battery { percent: Some(10) };
        let p = run(&[cpu()], &s);
        assert_eq!(p.indexing, IndexingPlan::Paused(PauseReason::LowBattery));
        assert_eq!(p.query_device, CPU_DEVICE);

        s.power = PowerSource::Battery { percent: Some(60) };
        s.profile = ResourceProfile::Eco;
        assert_eq!(
            run(&[cpu()], &s).indexing,
            IndexingPlan::Paused(PauseReason::OnBattery)
        );

        let mut s = ac_idle();
        s.available_memory_mib = Some(300);
        s.profile = ResourceProfile::Turbo;
        assert_eq!(
            run(&[cpu()], &s).indexing,
            IndexingPlan::Paused(PauseReason::MemoryPressure)
        );
    }

    #[test]
    fn turbo_uses_all_but_one_core_and_accelerators_on_battery() {
        let mut s = ac_idle();
        s.profile = ResourceProfile::Turbo;
        s.power = PowerSource::Battery { percent: Some(10) };
        assert_eq!(
            run(&[cpu()], &s).indexing,
            IndexingPlan::Run {
                device: CPU_DEVICE.into(),
                threads: 11
            }
        );
        let p = run(&[cpu(), probe("dml:0", metrics(320.0, 8.4))], &s);
        assert!(matches!(p.indexing, IndexingPlan::Run { ref device, .. } if device == "dml:0"));
    }

    #[test]
    fn turbo_relaxes_the_device_memory_cap_to_60_percent() {
        // joao-pc (T013): GTX 1650 q4 used 2296 of 4096 MiB, 7.28 vs 3.10 chunks/s.
        let gtx = || {
            probe(
                "dml:high",
                ProbeMetrics {
                    device_memory_mib: Some(2296.0),
                    device_memory_total_mib: Some(4096.0),
                    ..metrics(513.8, 7.28)
                },
            )
        };
        let mut cpu_probe = cpu();
        if let ProbeOutcome::Measured(m) = &mut cpu_probe.outcome {
            m.index_chunks_per_s = 3.10;
        }
        let balanced = run(&[cpu_probe.clone(), gtx()], &ac_idle());
        assert!(
            matches!(balanced.indexing, IndexingPlan::Run { ref device, .. } if device == CPU_DEVICE)
        );

        let mut turbo = ac_idle();
        turbo.profile = ResourceProfile::Turbo;
        let p = run(&[cpu_probe.clone(), gtx()], &turbo);
        assert!(matches!(p.indexing, IndexingPlan::Run { ref device, .. } if device == "dml:high"));
        assert_eq!(p.query_device, CPU_DEVICE, "queries stay on the faster CPU");

        // Above 60 % even Turbo refuses; unknown totals keep the absolute cap.
        let greedy = probe(
            "dml:high",
            ProbeMetrics {
                device_memory_mib: Some(2600.0),
                device_memory_total_mib: Some(4096.0),
                ..metrics(513.8, 7.28)
            },
        );
        let p = run(&[cpu_probe.clone(), greedy], &turbo);
        assert!(matches!(p.indexing, IndexingPlan::Run { ref device, .. } if device == CPU_DEVICE));
        let unknown_total = probe(
            "dml:high",
            ProbeMetrics {
                device_memory_mib: Some(2296.0),
                device_memory_total_mib: None,
                ..metrics(513.8, 7.28)
            },
        );
        let p = run(&[cpu_probe, unknown_total], &turbo);
        assert!(matches!(p.indexing, IndexingPlan::Run { ref device, .. } if device == CPU_DEVICE));
    }

    #[test]
    fn best_of_several_accelerators_wins() {
        let a = probe("dml:0", metrics(320.0, 8.0));
        let b = probe("dml:1", metrics(320.0, 12.0));
        let p = run(&[cpu(), a, b], &ac_idle());
        assert!(matches!(p.indexing, IndexingPlan::Run { ref device, .. } if device == "dml:1"));
        assert!(reasons(&p, "dml:0").iter().any(|r| matches!(
            r,
            Rejection::NotFasterEnough {
                lane: Lane::Indexing,
                ..
            }
        )));
    }

    #[test]
    fn quarantine_is_per_runtime_key_and_ignores_benign_errors() {
        let mut q = Quarantine::new();
        assert!(!q.record_failure("dml:0", "k1", &EmbeddingError::Cancelled));
        assert!(!q.record_failure("dml:0", "k1", &EmbeddingError::EmptyInput { index: 0 }));
        assert!(!q.record_failure(CPU_DEVICE, "k1", &EmbeddingError::Backend("x".into())));
        assert!(q.record_failure("dml:0", "k1", &EmbeddingError::ZeroVector { index: 3 }));
        assert!(q.reason("dml:0", "k1").is_some());
        assert!(q.reason("dml:0", "k2").is_none(), "driver update lifts it");

        let gpu = probe("dml:0", metrics(320.0, 8.4)); // runtime_key ort-1.24/driver-1
        q.restore("dml:0", "ort-1.24/driver-1", "device removed");
        let p = plan(
            SPACE,
            &[cpu(), gpu],
            &q,
            &ac_idle(),
            &PolicyConfig::default(),
        );
        assert!(matches!(p.indexing, IndexingPlan::Run { ref device, .. } if device == CPU_DEVICE));
        assert!(matches!(reasons(&p, "dml:0")[0], Rejection::Quarantined(_)));
        assert_eq!(q.iter().count(), 1);
    }

    #[test]
    fn single_core_machines_get_one_thread() {
        let mut s = ac_idle();
        s.logical_cpus = 1;
        for profile in [
            ResourceProfile::Eco,
            ResourceProfile::Balanced,
            ResourceProfile::Turbo,
        ] {
            s.profile = profile;
            assert_eq!(
                run(&[cpu()], &s).indexing,
                IndexingPlan::Run {
                    device: CPU_DEVICE.into(),
                    threads: 1
                }
            );
        }
    }

    #[test]
    fn rejections_render_for_diagnostics() {
        let text = Rejection::DeviceMemory {
            used_mib: Some(2900.0),
            limit_mib: 1536.0,
        }
        .to_string();
        assert!(text.contains("2900") && text.contains("1536"));
    }
}
