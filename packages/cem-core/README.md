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

**Explicitly not built here**: the L0–L4 autonomy policy engine and proposal/branch review
workflow (P2.2's own deliverable — `mode_b::accept` is a direct, unconditional write, not a
reviewable proposal); any UI; `cem-geometry`/`cem-connectors`/Mode C; the `scheduler`/Campaigns.
