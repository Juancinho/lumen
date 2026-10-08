# ADR-013 — Wire DTOs are shell-owned projections; core types carry no serialization


**Status:** Accepted (T011)

Domain types in `crates/` (`ResultItem`, `ActionDescriptor`, ids, …) do not derive serde or any
wire format. The shell maps them into explicit camelCase DTOs (`apps/desktop/src-tauri/src/dto*`),
mirrored by hand-written TypeScript types in `apps/desktop/src/ipc/types.ts`, each guarded by a
Rust JSON-shape test.

Reasons:

- the wire contract is a *projection*, not the domain model: `Payload` (paths, provider keys) and
  provider confidence must never reach the UI; the UI refers to results only by `ResultId` and so
  cannot ask Lumen to act on arbitrary paths;
- keeps `lumen-core` dependency-free and shell-agnostic (ADR-002); another shell may need another
  encoding;
- explicit DTOs make breaking wire changes visible in review.

Revisit (generated TS bindings such as ts-rs/specta) when the DTO set grows beyond roughly ten
types or hand-mirroring causes a real defect. Generation must still run on shell DTOs, not core
types.
