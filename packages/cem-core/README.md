# cem-core

**P2.1 (Mode B) — built.** Deterministic 0D/1D performance and mass-budget optimizer for
system-level architecture synthesis (FR-CEM-02). **Never calls an LLM to decide** (ADR-004) — this
crate has exactly one dependency, `serde`, and does no I/O at all; that's what makes T-P2.1-01's
"no LLM in the decision path" requirement structurally true rather than something to remember.

Pure computation, like `sysml-core`/`sysml-textual` — no store access, no HTTP. `apps/api/src/mode_b.rs`
is the thin HTTP layer around it (`POST .../cem/mode-b/optimize`, `POST .../cem/mode-b/accept`,
`GET .../cem/mode-b/interface-contract/:subsystemId`).

- `optimize(targets, constraints) -> Vec<Candidate>` — enumerates a fixed parameter grid across
  the four thermodynamically-relevant reference subsystems (bypass ratio, pressure ratio, turbine
  inlet temp, turbine stage count — `ControlFadecEec` is software/logic, deliberately excluded),
  filters to feasible candidates, ranks by ascending mass. **Determinism by construction** — no
  randomness anywhere in this crate, not determinism-by-seeding.
- `build_interface_contract(subsystem_id, candidate)` — FR-CEM-08's six named fields. Only three
  (performance targets, boundary conditions, mass target) are legitimately derivable from a 0D
  model; the other three (geometric envelope, interface/port definitions, material/process
  constraints) are Mode C's domain (`cem-geometry`, P2.3, not built) and say so honestly.

**The 0D formulas are this crate's own invented default, not sourced from the docs** — nothing in
the doc set gives a concrete equation, only "deterministic 0D/1D... mass-budget models." Same
"invent a reasonable, documented default" precedent as `HazardRiskPanel`'s Risk Index or Trade
Study's thrust formula elsewhere in this codebase. See `src/lib.rs`'s doc comments for the exact
formulas and reference constants.

**P2.2 (L0–L4 autonomy, hazard override, proposal/branch review) is also built** —
`apps/api/src/autonomy.rs`/`mode_b.rs`'s `propose`/`accept_proposal`/`reject_proposal`. This crate
itself is untouched by that work: autonomy is a review-gate concern layered on top of `optimize`'s
output, not something `cem-core` needs to know about.

**`docs/IMPLEMENTATION_KICKOFF.md` Phase 2 (ADR-011) is also built and verified** —
`packages/cem-archspace/`, a Python gRPC sidecar wrapping `adsg-core`/`SBArchOpt` for Mode B's
architecture *design-space* representation (reqs v5 §5.17, FR-ARCH: what a future, fuller Mode B
would search over — selection choices, connection choices, incompatibility/choice constraints —
as opposed to this crate's current fixed parameter-grid enumeration). **This crate is untouched by
that spike, deliberately**: `cem-core` stays "pure computation, no I/O"; a real FR-ARCH build-out
would add a `cem-core`-owned client calling into `cem-archspace` (mirroring
`apps/api/src/archspace_client.rs`'s existing spike client) rather than pulling gRPC/networking
into this crate directly. See `packages/cem-archspace/README.md` and
`Axioma_implementation_v5.md` §10 for what the spike proved.

**Explicitly not built here**: any UI; `cem-geometry`/`cem-connectors`/Mode C; the
`scheduler`/Campaigns; wiring `cem-archspace` into this crate's own `optimize` (P2.1's FR-ARCH
re-scoping, still open — see `Axioma_implementation_v5.md` §4.1's P2.1 note).
