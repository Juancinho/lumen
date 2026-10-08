# ADR-019 — Embedding device policy: CPU by default, accelerators only on measured proof


**Status:** Accepted (T013). Evidence: `docs/benchmarks/t013/2026-10-08-joao-pc/` (Ryzen 5 5600H,
GTX 1650 4 GB, driver 32.0.15.9227, ORT CPU 1.30 / DirectML 1.24.4, q4).
Refines ADR-015 §3–4. Code: `lumen_embedding::policy` (pure rules), `lumen_embedding::probe`
(measurement through the production `Embedder`), `lumen-bench probe` / `device-policy`.

**Decision**

- **CPU is always available and always the fallback**; a missing, failed or rejected
  accelerator never blocks search or indexing.
- **Same space only:** a device is considered only if its probe ran the index generation's
  exact `EmbeddingSpace` (weights, prompts, dimension). Device choice never changes weights.
- **Eligibility (all required):** successful probe; stable output (two runs ≥ 0.99999 cosine,
  no NaN/zero); vectors interchangeable with CPU ones (min cosine ≥ 0.999 on the same
  inputs); ≥ 90 % of graph nodes offloaded (placement known); device memory known and
  ≤ min(1.5 GiB, 50 % of the device) — in **Turbo** (user decision 2026-10-08) ≤ 60 % of the
  device, total memory required; not an integrated GPU (opt-in only — T006 device hang);
  not quarantined; a successful CPU probe exists to compare with.
- **Query lane:** stays on CPU while CPU p95 ≤ 120 ms; otherwise the fastest eligible
  accelerator if ≥ 1.5× faster. Never paused.
- **Indexing lane:** the fastest eligible accelerator if ≥ 1.5× CPU throughput, only on AC,
  Balanced and idle (or Turbo); otherwise CPU with Eco 1 thread / Balanced n/2 (n/4 while the
  user is active, 1 on battery) / Turbo n−1. Paused below 20 % battery (not Turbo), on battery
  in Eco, and under 768 MiB available memory (any profile).
- **Quarantine:** a runtime `Backend`/`NonFinite`/`ZeroVector`/`OutputShape` error on an
  accelerator quarantines it for its `runtime_key` (runtime + driver versions) and the batch is
  re-run on CPU; a driver/runtime update lifts it. Cancellation/input errors never do.
- Probes run once per device per `runtime_key`, each in its own process; device memory comes
  from the platform layer (Windows: GPU Process Memory perf counters).

**Evidence (joao-pc)**

| probe | query p50/p95 ms | indexing chunks/s | cos vs CPU | offloaded | GPU memory |
|---|---:|---:|---:|---:|---:|
| cpu | 29.9 / 34.2 | 3.10 | 1 | — | — |
| dml:high (GTX 1650) | 417.8 / 513.8 | 7.28 (2.35×) | 0.9999995 | 94 % | 2,296 of 4,096 MiB |

The GPU passes stability, fidelity and placement and is fast enough for indexing, but needs
2.3 GB of a 4 GB card (limit min(1.5 GiB, 50 %)) → rejected; all 7 scenarios run on CPU
(Balanced AC idle 6 threads, active 3, battery 1, battery 15 % paused, Eco 1, Turbo 11, low
memory paused). Vectors from DirectML are interchangeable with CPU ones (cos ≥ 0.9999995), so a
future accelerator can join an existing index generation.
With the Turbo cap (60 % = 2,458 MiB) the same probes give `turbo → indexing on dml:high`
(2.35× faster), every other scenario unchanged (`policy-v2.json`).

**Consequences**

- The probe compares against CPU at the runtime's default threads (all cores): conservative.
- `lumen-windows` (M1+) must supply power source, battery %, available memory, user activity
  and per-process GPU memory; the shell persists probes and the quarantine in settings.
