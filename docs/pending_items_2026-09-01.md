# Pending Items — 2026-09-01

A full sweep of every genuinely still-open/pending/not-built item across the Axioma monorepo's
docs, as of `HEAD abb198e` (main), prioritized by impact on full platform functionality. Produced
right after FR-ARCH-01…08 closed out (see `Axioma_implementation_v5.md` §22, `CLAUDE.md`'s status
header). Two corrections made during this audit that a naive doc-read would have missed:

- **P2.2's core mechanics (proposal/branch workflow + L0–L4 autonomy) are already fully built and
  tested**, despite no "phase closed" narrative existing for it anywhere — confirmed directly via
  `apps/api/src/autonomy.rs` and the `mode_b_propose_at_l0/l1/l3/l4_*` integration tests. Not
  listed below as open.
- **ADR-005's status line in `Axioma_implementation_v5.md` §2.5 still says "Recommended — spike to
  ratify subset"**, even though the spike ran, passed, and the full `alf-lite`/State-Machine build
  (T-P1.4-01…05) is done and tested. That's doc staleness, not a real functional gap — listed under
  Tier 3, not Tier 0.

## Tier 0 — Correctness / safety-integrity gaps

Fix first — these undermine guarantees the rest of the platform already relies on.

1. ~~**T-P1.4-06 not wired into CI.**~~ **Closed 2026-09-01 (impl v5 §23.1).** A new `perf-gate`
   CI job seeds the real fixture and runs a new automated assertion (`p95 < 2s` for NFR-PERF-04
   traceability), failing the build on regression. The browser-side "load < 5s; client memory <
   2GB" half is still not covered — flagged, not silently dropped; a separate, heavier follow-up.
2. ~~**FR-ARCH-02: no cyclic `ArchDerives` derivation graph.**~~ **Closed 2026-09-01 (impl v5
   §23.2).** `sysml_core::compute_derived_existence` evaluates through real cycles; a new
   `derived-existence` endpoint exposes it; `Turbofan-Ref`'s seed now carries the requirement's own
   literal Compressor/Combustor/Turbine example as real content, confirmed end to end by a new
   integration test.
3. ~~**FR-ARCH-03: no `ConnectionChoice` cardinality enforcement.**~~ **Closed 2026-09-01 (impl v5
   §23.3).** `sysml_core::check_connection_cardinality` enforces a new structured cardinality shape
   (this pass's own invented encoding — no doc specifies one); `resolve_choice` now accepts a real
   `connections` array and validates its count, confirmed end to end against real seeded content.
4. ~~**FR-CORE-13: orphan-Action rejection missing.**~~ **Closed 2026-09-01 (impl v5 §23.4).** Real
   `Action`/`Flow` kinds now exist; the "rejection" is a last-Flow-edge deletion guard plus an
   orphan-Actions report (a hard reject-on-create was structurally impossible — a fresh Action
   necessarily starts with zero Flow edges). The Decision-node guard-conflict half of FR-CORE-13
   remains explicitly not built (needs a Decision node + guard expressions + a satisfiability
   check). Building this also found and fixed a real, previously-latent canvas-crashing bug
   (`RangeError: Invalid array length`) exposed by FR-ARCH-02's own new cyclic seed content.

## Tier 1 — Product 2 / Mode B maturity

High value for CEM specifically; Product 1 is unaffected either way.

5. **`SolverResultState` not retrofitted into `trade_study.rs`/`fuml_client.rs`.** Those still use
   ad hoc result shapes — the platform-wide "solver results are typed, never blindly trusted" rule
   (CLAUDE.md non-negotiable #3) isn't actually uniform outside `archspace`. Flagged in impl v5 §22
   as a real, deliberately-not-attempted follow-up.
6. **`cem-archspace` design-space state is in-memory, process-lifetime only.** A sidecar restart
   silently loses in-progress trade studies; nothing persists it or syncs it to Neo4j. Impl v5
   §10.3 ("still open... it's still open after it").
7. **Only a single-objective/simple viability search is wired.** SBArchOpt's real multi-objective,
   hierarchical-BO (`ArchSBO`) capability sits unused; Mode B's actual optimization power is well
   below what the sidecar library can do. Impl v5 §10.3.
8. **FR-ARCH-06 exposes 1 of 4 real metrics.** Only Imputation Ratio is computed/surfaced via
   `DesignSpaceStats`; Correction Ratio, Correction Fraction, and Max Rate Diversity were never
   mapped through, even after the later §20 FR-ARCH-06 build-out (which only wires IR through).
   Impl v5 §10.3.
9. **`ChoiceConstraint`'s "uses parameter" relationship is a JSON array, not a graph edge.**
   Flagged in impl v5 §11.1 as a real schema gap "for whenever Parametrics evaluation actually gets
   built" — Parametrics was later built (§13) via linear interpolation over tabulated points, never
   through this edge, so the gap was never actually closed.
10. **FR-COMP-02 performance-map data is illustrative, not real.** The seeded
    `sampledPointsAtDesignSpeed` carries `sourceNote: "illustrative shape only -- real constitutive
    equations not yet sourced"` — reqs v5 §5.15 itself says the real off-design equations aren't
    sourced yet. Impl v5 §11.1. A domain-correctness gap for any real trade study run against it.

## Tier 2 — Product 1 feature completeness

Self-contained gaps; none block other features.

11. FR-INFO-02: no dedicated Data Type/Enumeration authoring endpoint — a Data Type is just a
    plain `:InformationElement` today. Impl v5 §13.4/§17.4/§18.1; test spec T-INFO-01's Data Type
    half.
12. FR-INTX-02 timing constraints are captured on Interaction messages but nothing
    analyzes/evaluates them. Impl v5 §16.5.
13. No UI flow to create a new Interaction with participants — the toolbar's "+ Add Node" is
    hardcoded to a different creation path. Impl v5 §16.1/§16.5/§17.
14. Swimlane View layout is a manual React Flow grid, not ELK-driven (cosmetic — click-to-allocate
    and drag-to-allocate both work fine today). Impl v5 §16.5.
15. Parametrics evaluator only does linear interpolation over tabulated points — no general
    algebraic expression/constraint evaluator, per the reqs' own non-concrete spec. Impl v5
    §13.2/§18.1/§18.3.
16. `human-authored` proposal origin has no real producer — the other two origins (`cem-generated`,
    `document-import`) both work; nothing creates a `human-authored` proposal. Impl v5 §13.1.
17. No LIST endpoint for saved-but-not-frozen Dynamic Collections — they live in `Home`-level React
    state only, lost on page reload. Impl v5 §17.2 ("a real, accepted gap, not silently hidden").
18. Attachment upload capped at axum's default 2MB (`DefaultBodyLimit` never raised). Impl v5
    §17.2/§17.4.
19. No column-selection on table export — CSV/XLSX export uses fixed baseline columns only. Impl
    v5 §17.4; test spec T-EXPORT-02's literal "4 of 8 columns" scenario isn't matched.
20. Only one report template (`"risk-register"`) exists; no PDF output, no MIL-STD-882/ISO 26262
    template variants. Impl v5 §14.3.

## Tier 3 — Documentation hygiene only

No functional gap; cheap correctness fixes so the docs stay trustworthy.

21. ADR-005's status line still says "Recommended — spike to ratify" despite the spike having run,
    passed, and the full build being done. Impl v5 §2.5, §9.3.
22. `Axioma_requirements_v5.md`'s FR-COMP/FR-ARCH status column universally still says "not yet
    specified — Phase 6," even for items closed long since (FR-COMP-01…06, FR-ARCH-01…08).
23. ~~`.github/workflows/ci.yml`'s own comment claims the 1M-element fixture "doesn't exist
    yet."~~ **Closed 2026-09-01**, as a side effect of Tier 0 item 1.

## Tier 4 — Not started by design

Explicitly allowed to lag per the project's own roadmap sequencing — not really "gaps" so much as
unstarted future phases.

24. `cem-geometry` (Mode C) — research-risk, explicitly deferred to P2.3+. `CLAUDE.md` "What this
    is"; `packages/cem-geometry/README.md`.
25. `cem-connectors` (external FEA/CFD solver adapters) — not started, planned for P2.3. `CLAUDE.md`
    roadmap; `packages/cem-connectors/README.md`.
26. `scheduler` (governed Campaign job queue — concurrency/quota/cost/retry) — not started. Backs
    the non-negotiable "Campaigns must declare a budget" rule (CLAUDE.md rule #5), but no Campaign
    feature exists yet either, so nothing currently violates it. Worth building before Campaigns
    themselves land, not before. `CLAUDE.md` roadmap; `packages/scheduler/README.md`.

---

*Starting point chosen 2026-09-01: Tier 0, in order — item 1 (wire T-P1.4-06 into CI) first as the
cheapest, highest-leverage fix, then FR-ARCH-02/03 and FR-CORE-13.*

*Update 2026-09-01: all four Tier 0 items closed (impl v5 §23.1–§23.4). Tier 1 (Product 2 / Mode B
maturity — items 5–10 above) is the natural next tier, not yet started.*
