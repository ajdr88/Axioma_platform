# Axioma — Implementation Kickoff Instructions (for Claude Code)

**Purpose:** This document tells Claude Code, working directly in the Axioma repository, how to start turning the current spec set into code — starting with the platform gaps identified in the ADSG/turbofan work and a seeded full-engine system model instance. It assumes Claude Code is pointed at the real `axioma` repo (the monorepo `CLAUDE.md` describes: `apps/{web,api,docs}`, `packages/{sysml-core,cem-core,cem-geometry,cem-connectors,llm-gateway,scheduler,fuml-runtime,alf-lite,diagram-engine,shared-types,ui-components}`), **not** this `docs/` folder — see the Scope Caveat at the end.

**Read-first order.** Before writing any code, read these in this order — each builds on the last, and several later documents amend or extend earlier ones:

1. `CLAUDE.md` — architectural non-negotiables, data model, design system, repo layout. Read this first, every session.
2. `Axioma_requirements_v4.md` — the current functional/non-functional requirements and architecture reference (§5). This is the source of truth for *what*.
3. `Axioma_implementation_v4.md` — the current API spec, service topology, data model, ADR log, roadmap. Source of truth for *how*.
4. `claude/Axioma_cameo_tutorial_gap_analysis.md` then `claude/Axioma_gap_closure_amendment.md` — a gap analysis against a SysML v1 (Cameo) tutorial and the amendment that closes it (Parametrics, Data/Information architecture, Interactions, Export/Reporting, Dynamic Collections, Swimlane allocation, requirements-dependency taxonomy). Not yet merged into the two core docs above.
5. `claude/Axioma_document_import_pipeline_amendment.md` — specifies the "documents → draft model" pipeline (FR-CORE-07) in full: 5-stage async job, citation provenance, review-gate integration. Not yet merged.
6. `claude/Axioma_literature_extraction.md` — literature extraction from three reference sources (Bussemaker 2025 thesis on Architecture Design Space Graphs / ADSG, NASA SP-36 1965 axial-compressor design handbook, Tan et al. 2024 MBSE hydrogen-turbofan paper). Background/citation source for #7 and #8.
7. `claude/Axioma_turbofan_system_model_amendment.md` — Part 1: Fan & LP Compression / Core (HP) Compressor requirements (FR-COMP-01…06). Part 2: a full reconciled turbofan engine system model built on Bussemaker's ADSG methodology, cross-checked against Tan et al.'s decomposition and Axioma's resolved 5-subsystem structure. Part 3: the resulting platform gap analysis (new FR-ARCH group, data-model additions, `cem-core` algorithmic gaps, new endpoints, diagram-engine/UI gaps).
8. `claude/Axioma_sysml_tool_landscape_evaluation.md` — evaluation of the free/open-source SysML v2 tool landscape (sysml.org tools, ADORE, Sinelabore) against Axioma; the resulting recommendation to adopt `adsg-core` and `SBArchOpt` (MIT-licensed Python libraries) behind a gRPC sidecar as the concrete implementation path for Mode B's architecture-optimization core.

None of #4–8 are folded into the two core docs (#2, #3) yet. Treat them as *accepted direction, not yet merged text* — this kickoff plan proceeds as if they will be merged (Phase 0 below is precisely that merge), but if you find something in them you disagree with, flag it before writing code against it rather than treating it as immutable.

---

## Phase 0 — Resolve doc collisions and merge amendments (no code yet)

Before any implementation work, reconcile the spec itself. Two problems need fixing:

**0.1 — ADR-011 numbering collision.** Two independently-drafted amendments each propose their own, unrelated "ADR-011":
- `claude/Axioma_document_import_pipeline_amendment.md` §3.4 proposes ADR-011 = "Confirm `llm-gateway` as a Product-1-tier shared dependency."
- `claude/Axioma_turbofan_system_model_amendment.md` Part 3 (updated by `claude/Axioma_sysml_tool_landscape_evaluation.md`) proposes ADR-011 = "Adopt `adsg-core`/`SBArchOpt` behind a Python gRPC sidecar for Mode B's architecture design space representation."

Action: renumber one. Recommendation — keep the ADSG/SBArchOpt decision as **ADR-011** (it's the more architecturally significant decision, and the tool-landscape evaluation already refers to it by that number in its synthesis section), and renumber the `llm-gateway`-as-shared-dependency decision to **ADR-012**. Update the "New ADR Candidate" table in `claude/Axioma_document_import_pipeline_amendment.md` §3.4 accordingly before merging.

**0.2 — Merge the five amendment docs into the two core docs.** Once ADR numbering is fixed, fold `claude/Axioma_gap_closure_amendment.md`, `claude/Axioma_document_import_pipeline_amendment.md`, and `claude/Axioma_turbofan_system_model_amendment.md` (Parts 1–3) into `Axioma_requirements_v4.md` and `Axioma_implementation_v4.md` directly, producing v5 of each. Concretely:
- New FR groups (`FR-PARAM`, `FR-INFO`, `FR-INTX`, `FR-EXPORT`, `FR-COMP`, `FR-ARCH`) and new/amended IDs (`FR-CORE-10`…`18`) get appended to the requirements tables in reqs §matching sections, with their `§5.x` design references appended to reqs §5.
- New REST endpoints, data-model additions (node labels, edge types, properties), and the two resolved ADRs get appended to the implementation doc's API spec, data model, and ADR log respectively.
- New test IDs get appended to `Axioma_test_specification_v3.md` (candidate for a v4 rename once this merge lands, given the volume of additions).
- The roadmap placements each amendment already recommends (P1.1–P1.4 for the gap-closure items; P2.1 re-scoping per the turbofan amendment Part 3 §3.7) get reflected in impl §4.1's phase table.

This phase produces no application code — it's a documentation-consolidation pass. Do it first anyway: every phase below cites FR/ADR IDs, and those IDs should live in one authoritative place before implementation starts, not be split across six files.

---

## Phase 1 — Data-model and schema additions

Land the schema-level additions from the gap-closure and turbofan amendments before anything depends on them:

- New node labels: `:Constraint`, `:Parameter`, `:InformationElement`, `:Interaction`, `:InteractionFragment`, `:Collection`, `:CandidateStructureSuggestion` (proposal-scoped only).
- New edge types: `Bound`, `Derive`, `Copy`, `member`.
- New properties: `citation` and `confidence` on `:Requirement`; `document-import` as a third value of the existing `proposalOrigin` enum (alongside `human-authored`, `cem-generated`).
- FR-ARCH group's data-model additions from the turbofan amendment Part 3 (architecture-choice/design-space representation — confirm exact shape against `adsg-core`'s own graph model once Phase 2's spike is running, since the two should align rather than diverge).

Update `sysml-core`'s semantic-validation layer (FR-CORE-05) to cover the new element/edge types' well-formedness rules (e.g., FR-CORE-13's orphan-Action and ambiguous-guard rejections) as part of this phase, not as an afterthought — every new write path in later phases depends on validation already knowing about these types.

## Phase 2 — Mode B foundation: `adsg-core`/`SBArchOpt` sidecar (ADR-011)

This is the headline finding from the turbofan/tool-landscape work: FR-CEM-02 never specified *how* Mode B represents an architecture design space, and `adsg-core` (MIT-licensed, github.com/jbussemaker/adsg-core) plus `SBArchOpt` (MIT-licensed, built on `pymoo`) are ready-made implementations of exactly that representation and its hierarchical-Bayesian-optimization search.

- Stand up a Python gRPC sidecar service wrapping `adsg-core`/`SBArchOpt`, mirroring the existing `fuml-runtime` pattern (ADR-005) — a JVM sidecar wrapping an external reference implementation, reached over gRPC per ADR-008's external-tool-boundary convention. This is a spike: get selection choices, connection choices, and incompatibility/choice constraints round-tripping between Axioma's graph representation and an in-memory ADSG instance.
- Confirm the mapping between Axioma's `:Constraint`/`:Parameter`/new FR-ARCH node types and `adsg-core`'s own DSG node types (FUN/COMP/MULTI/NOF/DE/CON) — this determines whether Axioma stores the ADSG natively in its own graph store or treats the sidecar's in-memory ADSG as the working representation with a sync/export step. Resolve this as part of the ADR-011 write-up, not silently in code.
- Do not block this phase on Phase 4 (system-model seeding) — the sidecar can be validated against a small synthetic design space before the full turbofan instance is seeded, and the full instance from Phase 4 becomes the sidecar's first real-world test case.

## Phase 3 — Compressor requirements as a concrete case (FR-COMP)

Land FR-COMP-01…06 (Fan & LP Compression and Core/HP Compressor requirements, from the turbofan amendment Part 1) as actual `:Requirement` elements plus the extended Interface Contract examples that accompany them. This phase is mostly content-authoring against the schema from Phase 1, and gives Phase 4 concrete, already-modeled Requirements to satisfy/verify against rather than inventing them ad hoc during instance seeding.

## Phase 4 — Seed the full turbofan system-model instance

Using the turbofan amendment Part 2 as the source of truth, seed an actual instance of the reconciled 5-subsystem engine model (Fan & LP Compression, Core (HP) Compressor, Combustor, Turbine (HP & LP), Control (FADEC/EEC)) into a real Axioma project:

- Boundary functions and per-subsystem breakdowns as specified.
- Cross-cutting connection choices and constraints, and the station-numbering scheme (0–8), as Ports/Interfaces per FR-CORE's existing structural model.
- The two flagged reconciliation decisions carried forward as-is unless this phase's implementation surfaces a reason to revisit them: Nozzle folded into Turbine's exit port; Inlet excluded from the engine System-of-Interest entirely.
- Satisfy/Verify edges from Phase 3's FR-COMP requirements (and existing higher-level requirements) into the seeded structure, exercising the traceability machinery end-to-end.

This is the first real integration test of Phases 1–3 together, and should surface schema gaps early rather than late.

## Phase 5 — `diagram-engine` and API surface for the new capability

Once Phase 4's instance exists and is inspectable only via API/database, build the UI/API surface to display and edit it:

- New REST endpoints from the gap-closure and document-import amendments (`/parametrics/*`, `/information/*`, `/interactions/*`, `/export/*`, `/collections/*`, `/import/documents/*` per each amendment's §3.1).
- `diagram-engine` additions: the Interaction/Lifeline-Message view (per ADR-009's recommendation — a view-layer concern, decoupled from the underlying SysML v2 storage representation), Swimlane/partition allocation mode for Activity-equivalent diagrams (FR-CORE-12), and whatever canvas support the turbofan amendment Part 3 identifies as missing for displaying an ADSG-backed architecture design space (selection/connection choices, incompatibility constraints) rather than a plain structural BDD.
- Wire the new `document-import` proposal origin into the existing review-gate UI (FR-CORE-16) as a third tab/filter alongside `human-authored` and `cem-generated`, reusing the existing accept/reject components rather than building new ones.

## Phase 6 — Test coverage

Land the new test IDs from each amendment's §4 as they become exercisable: `T-PARAM-*`, `T-INFO-*`, `T-INTX-*`, `T-EXPORT-*`, `T-CORE-10/12-*`, `T-CORE-03-EXT`, `T-DOCIMPORT-01…07`. Prioritize `T-DOCIMPORT-06` (pipeline works with zero CEM services running) and the Phase 2 sidecar's own round-trip tests early — both are architectural-boundary tests, and catching a boundary violation early is much cheaper than after Phase 4/5 code depends on the wrong boundary.

---

## Scope caveat — read before starting

This `docs/` folder (the connected device folder this kickoff plan and the spec docs were written into) contains **only documentation** — the spec, requirements, and amendment docs listed above. It is not the Axioma source repository. Claude Code will need to be pointed separately at the actual `axioma` monorepo (the one `CLAUDE.md` describes, with `apps/` and `packages/`) to execute any of the phases above. This kickoff plan is spec-grounded and phase-ordered, but every phase's "land X here" instruction assumes a working checkout of that repo, its existing `sysml-core`/`cem-core`/`diagram-engine`/etc. packages, and its CI/test tooling already in place — none of which live in this docs folder. If no such repo checkout is available yet, Phase 0 (the documentation merge) is the only phase that can proceed from this folder alone.
