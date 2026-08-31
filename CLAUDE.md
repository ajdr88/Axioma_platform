# Axioma — Project Context for Claude Code

**Source of truth:** `docs/Axioma_requirements_v5.md`, `docs/Axioma_implementation_v5.md`,
`docs/Axioma_test_specification_v4.md`, `docs/Axioma_design_philosophy.md`. These four supersede
any earlier v1–v4 docs or prior drafts of this file — if something here conflicts with those docs,
the docs win; flag it and ask. (v3/v4 requirements/implementation docs are kept in `docs/` for
history, not deleted — do not treat their presence as ambiguity about which is current.)

**v5/v4 status (2026-08-31):** these are the result of `docs/IMPLEMENTATION_KICKOFF.md`'s Phase 0
doc-consolidation pass — a large body of new capability (Parametrics, Information/Data Architecture,
Interaction/Timing modeling, Export/Reporting, a documents→draft-model pipeline, compressor
subsystem requirements, and a full turbofan ADSG system model + Mode B design-space representation)
became **accepted spec** there. Since then: **Phase 1** (schema — new node/edge types, see Data
model below), **Phase 2** (the `adsg-core`/`SBArchOpt` sidecar spike, `packages/cem-archspace/`,
ADR-011 ratified), **Phase 3** (FR-COMP-01/02/05/06 seeded as real content on `Turbofan-Ref`,
FR-COMP-03 validated-but-unwired, FR-COMP-04 deferred to Phase 4 — see impl v5 §11), **Phase 4**
(the reconciled 5-subsystem turbofan system-model instance seeded — station 0–8 Ports, boundary
Functions, SelectionChoice/ConnectionChoice primitives, FR-COMP-04 unblocked, `REQ-THRUST` wired
into the seeded structure — see impl v5 §12), **Phase 5's "Foundation slice"** (canvas visual
types for Function/SelectionChoice/ConnectionChoice + ArchDerives/IncompatibleWith/ChoiceConstraint
edges, review-gate origin UI, `/parametrics/evaluate` + `/information/elements` +
`/collections/dynamic|freeze` — see impl v5 §13), **Phase 5's Export & Reporting**
(`/elements/:id/attachments` + `/export/table` (CSV) + `/export/report` (one HTML template,
`"risk-register"`) + a client-side "Export PNG" canvas button — see impl v5 §14; research found
none of the three mechanisms this group's own design assumed already existed actually did), and
**Phase 5's documents → draft model pipeline** (FR-CORE-14…18 — a real async job:
`POST /import/documents` uploads a PDF, real text extraction + deterministic segmentation + a real
Ollama-drafted Requirement per candidate, landing as a `document-import`-origin proposal
`mode_b.rs::accept_proposal` now genuinely materializes — see impl v5 §15; no OCR, no `llm-gateway`
second-caller abstraction), and **Phase 5's Interaction view + Swimlane mode** (`/interactions/*`,
a real Lifeline/Message diagram in `InteractionPanel.tsx`, ADR-009 ratified — messages/fragments
as plain content on the existing `:Interaction`/`:InteractionFragment` elements, not a new
`NodeKind`; Swimlane View as a manual-grid React Flow layout keyed on the newly-added `Allocate`
`EdgeKind`, click-to-allocate rather than drag-to-allocate this pass — see impl v5 §16; live
browser verification also found and fixed a missing `fitView` call, and separately found that
`/parametrics/evaluate`, `/information/elements`, `/collections/*`, and `/export/*` all still lack
a Next.js proxy route — real but not yet blocking, since none has a frontend caller yet, see impl
v5 §16.4) are all built and verified. This closes out Phase 5 entirely. A follow-up pass then
closed that flagged proxy/UI gap for real: `ParametricsPanel.tsx`, a toolbar Information Element
creator, `TraceabilityPanel`'s new Dynamic Collection save/freeze UI, `ElementInspector`'s new
Collection-members and Attachments sections, and toolbar Export Table/Report triggers — plus a
binary-safe fix to the shared `proxyRequest` helper (it built every response via `.text()`, which
would have corrupted non-UTF-8 attachment downloads) and a new `proxyMultipart` helper for real
file uploads — see impl v5 §17. **Phase 6** (test coverage — landing the `T-PARAM-*`/`T-INFO-*`/
`T-INTX-*`/`T-EXPORT-*`/`T-CORE-10/12-*`/`T-CORE-03-EXT`/`T-DOCIMPORT-01…07` test IDs) is now also
done — see impl v5 §18: of 19 IDs, 7 had a real gap and got 5 new tests (62/62 passing overall),
most were already covered by earlier passes' own tests (confirmed by reading the suite first, not
assumed), and several describe capabilities this session already, honestly scoped out (no general
algebraic Parametrics, no XLSX export, no server-side full-diagram render, no orphan-Action
rejection, no OCR) — flagged, not built just to make a spec sentence pass. **This closes out every
phase in `docs/IMPLEMENTATION_KICKOFF.md`.**
`docs/claude/*.md` holds the original amendment/analysis docs Phase 0
merged, kept for history.

## What this is
Axioma is a cloud-native model-based systems engineering platform, built around a SysML v2
model graph, shipped as **two independently-shippable products**:

- **Product 1 — MBSE Platform.** Requirements, architecture, safety, mission planning,
  traceability, simulation, collaboration. Standalone product, no CEM dependency.
- **Product 2 — Computational Engineering Model (CEM).** Sits on top of Product 1.
  - **Mode A** — grounded AI copilot (fast-follow bridge).
  - **Mode B** — deterministic architecture/trade-study optimizer (`cem-core`, never an LLM).
  - **Mode C** — manufacturable geometry synthesis + external solver validation. **Research-risk**,
    explicitly allowed to lag — it must never block Product 1.

Reference System-of-Interest for the pilot: a **turbofan engine**, decomposed into five
subsystems — Fan & LP Compression, Core (HP) Compressor, Combustor, Turbine (HP & LP),
Control (FADEC/EEC).

## Tech stack — ADR log is the single source of truth (implementation doc §2.5); don't substitute
| Layer | Technology | Notes |
|---|---|---|
| Frontend | **React 19 + Next.js + React Flow** | ADR-002. SvelteFlow references anywhere are void — do not use it. |
| Graphics | WebGPU (rendering) + viewport virtualization | NFR-PERF-01; off-screen elements clustered, not held live |
| Topology store | **Neo4j / Memgraph** | Relationships/topology **only** — kept lean (NFR-DATA-01) |
| Element/metadata store | **Postgres/JSONB** (or document store) | Element bodies, long text, large metadata (NFR-DATA-02) |
| Object store | S3-compatible | Geometry, meshes, solver result files — referenced by pointer, never inlined |
| Time-series store | — | Simulation/telemetry history, playback, dashboards |
| Backend | **Rust, Axum** | REST surface, query-budget enforcement, auth |
| Behavioral sim (execution) | **fUML Reference Implementation as a JVM sidecar** (`fuml-runtime`), driven over **gRPC** | ADR-005/008. CPL+Apache license, adoptable. |
| Behavioral sim (authoring) | **`alf-lite`** — in-house, clean-room, minimal Alf-subset compiler → fUML, Rust | ADR-005. Never link the GPL-v3 Alf RI. |
| Collaboration | CRDT (Yjs/Hocuspocus) for convergence **only** | Validity is a separate, server-side concern — see below |
| Versioning | Git-backed model storage (semantic diffs) | AI proposals land as branches/commits like human changes |
| LLM | Pluggable provider behind `llm-gateway`, local Ollama default, hosted optional | ADR-004. `cem-core` never uses an LLM to decide. |
| Package manager | pnpm workspaces | |
| Build system | Turborepo | |
| Lint/format | Biome | |
| UI components | Tailwind CSS 4 + shadcn/ui | |

## Monorepo layout
```
axioma/
├── .devcontainer/
├── apps/
│   ├── web/                 # React 19 + Next.js + React Flow
│   ├── api/                 # Rust (Axum) REST surface
│   └── docs/
├── packages/
│   ├── sysml-core/          # SysML v2 parse, KerML rules, semantic-validation layer
│   ├── cem-core/            # Mode B: deterministic optimizer (no LLM)
│   ├── cem-geometry/        # Mode C: geometry + manufacturing constraints
│   ├── cem-connectors/      # FEA/CFD adapters, Campaigns, result-state typing
│   ├── llm-gateway/         # Pluggable LLM provider interface
│   ├── scheduler/           # Governed Campaign job queue (concurrency/quota/cost)
│   ├── fuml-runtime/        # JVM sidecar (fUML RI), exposed over gRPC
│   ├── alf-lite/            # In-house minimal Alf-subset compiler → fUML (clean-room)
│   ├── diagram-engine/      # React Flow nodes/edges
│   ├── shared-types/        # Generated TS types from Rust structs
│   └── ui-components/       # Design system (shadcn/Tailwind 4)
├── infrastructure/          # Terraform/K8s, provider-parameterized
└── docker-compose.yml       # Local: Neo4j, Postgres, MinIO, Redis, collab, Vault
```

## Data model
A **directed property graph — not a DAG.** Acyclicity is enforced only on the
containment/composition hierarchy; traceability, `validatedBy`, and `Suspect` propagation
legitimately form cycles. All traversal must use visited-set cycle detection, never assume
global acyclicity.

- **Node labels:** `:Element` (base), `:Structure`, `:Requirement`, `:Port`, `:Hazard`,
  `:Control`, `:Mission`, `:Stakeholder`, `:SimulationRun`; plus (v5, `docs/IMPLEMENTATION_KICKOFF.md`
  Phase 1) `:Constraint`/`:Parameter` (Parametrics), `:InformationElement` (Info/Data
  Architecture), `:Interaction`/`:InteractionFragment` (ADR-009 ratified — see impl v5 §16.1;
  messages/fragments are plain content on these elements, not further node types),
  `:Collection` (Dynamic/Static element collections), `:CandidateStructureSuggestion`
  (proposal-scoped only, document-import), `:Function`/`:SelectionChoice`/`:ConnectionChoice`
  (Mode B architecture design-space, FR-ARCH).
- **Edges:** `contains` (acyclic only), `Satisfy`/`Verify`/`Refine`, `causes`/`mitigatedBy`,
  `validatedBy`, `Suspect`; plus (v5, Phase 1) `Bound`, `Derive`, `Copy`, `Member`, `ArchDerives`
  (cycles permitted — **renamed from the spec's own `derives`**, which collided with `Derive`
  above; see `packages/sysml-core/src/lib.rs::EdgeKind::ArchDerives`'s doc comment),
  `IncompatibleWith`, `ChoiceConstraint`; plus (v5, Phase 5) `Allocate` (Swimlane allocation,
  FR-CORE-12 — kind-unconstrained on both ends, same discipline as `ArchDerives` above). Edges
  carry metadata (stereotype, multiplicity, provenance).

All of the above are real `NodeKind`/`EdgeKind` variants in `packages/sysml-core/src/lib.rs` today
(with `sysml-textual`'s keyword mapping and `packages/shared-types` kept in sync) — not aspirational.
Full detail, including which of the new edges got a real endpoint-legality rule vs. stayed
deliberately kind-unconstrained, in `docs/Axioma_implementation_v5.md` §2.3. **Not yet done**:
FR-CORE-13 (orphan-Action rejection) — blocked on an Action/Activity `NodeKind` decision no
merged doc makes yet, flagged there rather than guessed. (FR-CORE-12, Swimlane allocation, is
built — see impl v5 §16.3 — and was never blocked on that same decision.)

## Non-negotiable architectural rules
1. **Convergence ≠ validity.** The CRDT layer only guarantees clients converge to the same
   state. A separate server-authoritative `sysml-core` pass validates every write — human
   CRUD, CRDT-converged change, or AI proposal — against the same rule set (type-legal
   relationship endpoints, containment acyclicity, parametric consistency, no dangling edges).
   An illegal converged state is **quarantined and surfaced as a conflict, never persisted to
   Main.**
2. **Query budgets are mandatory.** Every traversal/traceability endpoint enforces explicit
   `maxDepth`, `maxFanout`, and cursor-based pagination. Unbounded traversals are rejected,
   not merely discouraged.
3. **Solver results are typed, never blindly trusted.** A run resolves to one of `Converged`,
   `Diverged`, `Failed`, `Timeout`, `Suspect-Numerical`, `LicenceUnavailable`. A plausibility
   pass runs before any graph write. Only `Converged`-within-bounds can satisfy an autonomy
   gate — everything else drops to human review regardless of Autonomy Level.
4. **Autonomy levels (L0–L4)** govern how much CEM-generated change merges without review.
   **Hard exception, cannot be configured away:** any element linked to an unmitigated Hazard,
   or a High/Catastrophic Hazard, always requires individual human review, at every level.
5. **Campaigns must declare a budget.** A Campaign (multi-run solver batch) without a
   cost/quota ceiling is rejected outright — no unbounded L4 loops.
6. **Generative-path concurrency:** a human edit always wins over an in-flight autonomous
   write; the autonomous result is re-queued against the new state, never force-merged.
   Overlapping Campaigns on one subsystem are serialized by the scheduler.
7. **Persistence is deliberately polyglot** — never put large bodies/text in Neo4j, never
   inline geometry/meshes in the graph. Graph = topology only; Postgres = bodies/metadata;
   S3 = blobs referenced by pointer.
8. **`alf-lite` scope discipline:** only implement Alf constructs the pilot's models actually
   need. It's a compiler front-end only — compiles to fUML, which `fuml-runtime` executes.
   Never build a second execution path. Unsupported constructs must fail with a precise
   compile error, never a silent partial compile.
9. **Every AI-generated element carries full provenance** (model, version, prompt-hash,
   temperature/seed, context snapshot) — same rigor as `SimulationRun` provenance for solvers.

## Design system — "Structural Clarity"
Supersedes any earlier "Obsidian & Neon" reference. Two-color brand discipline (Cobalt +
Graphite) applies to **brand/identity surfaces only** (logo, nav, marketing); the product
UI's functional status system is a deliberately separate semantic layer and is not bound by it.

| Role | Value | Usage |
|---|---|---|
| Obsidian (ground) | `#07070C` | Primary background |
| Graphite | `#7C7C86` | Supporting structure, borders, secondary text |
| Cobalt (true brand) | `#052583` | Logo, wordmark, identity marks **only** |
| Cobalt-glow (UI accent) | `#3A5BFF` | Interactive elements: buttons, links, hover, node-lattice |
| Paper (inverse) | `#F3F4F8` | Light-mode surfaces |
| Alert (functional only, off-palette) | `#FF5C5C` | `Suspect` staleness pulse — deliberately outside the brand palette so "validated" and "needs review" are never confusable |

Typography: **Space Grotesk** (UI) + **JetBrains Mono** (technical annotation — IDs,
provenance, telemetry). 4px grid, glassmorphic panels, floating action dock with the
Autonomy selector (L0–L4).

Every element renders three orthogonal, filterable signals: **Origin** (human / AI-suggested
/ AI-auto-merged — border style), **Validation** (unverified / solver-validated /
test-validated — corner badge), **Staleness** (current / `Suspect` — red pulse overlay).

## Roadmap (build in this order — Product 2 never blocks Product 1)
**Product 1:**
1. **P1.1 Core Graph (Mo 1–2):** KerML/SysML v2 meta-model incl. Hazard/Control/Mission/
   Stakeholder; polyglot persistence wired; CRUD + semantic-validation layer; Git versioning;
   ReqIF/SysML v2 import. *Also here (v5): `:InformationElement`/`:Constraint`/`:Parameter`,
   `Derive`/`Copy` edges — pure data-model additions.*
2. **P1.2 IDE Experience (Mo 3–4):** Monaco + LSP; canvas with viewport virtualization;
   Hazard/Risk panel; Mission timeline; provenance visual language scaffolding. *Also here (v5):
   Dynamic/Static Collections (FR-CORE-10/11), Swimlane allocation (FR-CORE-12, built — see
   impl v5 §16.3) + orphan-Action validation (FR-CORE-13, still blocked on an Action/Activity
   `NodeKind` decision no merged doc makes yet).*
3. **P1.3 Digital Thread (Mo 5):** budgeted traceability + change-impact; standards-aligned
   safety reporting (ARP4761 / MIL-STD-882 / ISO 26262). *Also here (v5): diagram/table export
   (FR-EXPORT-01/02), the full documents→draft-model pipeline (FR-CORE-14…18).*
4. **P1.4 Behavioral Simulation + Pilot (Mo 6):** `fuml-runtime` execution via gRPC;
   `alf-lite` for the pilot's behaviors; 1M-element load fixture in CI. *Also here (v5):
   Interaction/timing modeling (FR-INTX, ADR-009 ratified — built, see impl v5 §16.1),
   Parametrics evaluation (FR-PARAM).*

**Mode A fast-follow** (after P1 stable): grounded copilot, part search, requirement linting.
*Also here (v5): report generation + file attachments (FR-EXPORT-03/04) — no hard phase
dependency, lowest urgency of the merged groups.*

**Product 2 (independent track):**
5. **P2.1 Mode B (Mo 7–9):** `cem-core` optimizer, trade-study runner, Interface Contract schema.
   **[v5, re-scoping flag — still open]** This estimate predates the FR-ARCH gap (Mode B's
   architecture design-space representation — what it actually searches over, reqs §5.17).
   **ADR-011 is now ratified**: `packages/cem-archspace/` (Python gRPC sidecar, `adsg-core` +
   `SBArchOpt`, MIT-licensed) is built and verified end-to-end — see its README and impl v5 §10.
   FR-ARCH-01…06 (wiring it into a real `apps/api` HTTP surface + `cem-core`'s own encode/decode
   logic) and FR-COMP-01…06 (compressor requirements) still belong in this phase's real build-out.
   **Don't assume the Mo 7–9 window absorbs that unchanged just because the spike is done** — the
   spike de-risked *whether* the approach works, not *how long* the real build takes.
6. **P2.2 Contract + Autonomy + Review (Mo 9–10):** proposal/branch workflow, L0–L4 autonomy.
   *Also here (v5): FR-ARCH-07/08 (architecture instance generation/comparison, non-convergent
   handling) alongside the proposal/branch workflow.*
7. **P2.3 Mode C — one subsystem (Mo 11–14, research-risk):** Fan & LP Compression casing/mounts
   (structural-only), one external FEA solver, end-to-end.
8. **P2.4 Expand (Mo 15+):** more subsystems, increasing physics complexity.

**FR-COMP/FR-ARCH have no test-spec rows yet** (unlike every other v5 addition) — authoring them
was never part of Phase 6's own 19 named test IDs (see impl v5 §18) and remains open; a distinct,
still-uncovered gap from the now-closed Phase 6 pass, not done as part of the Phase 0 doc merge.

## Working conventions
- Check the **ADR log** (implementation doc §2.5) before making any technology decision —
  it's the tiebreaker over anything else in the doc set.
- Every model write must be traceable to a Git-style commit — never a silent mutation.
- Validate all element creation against KerML rules (`sysml-core`) before persisting.
- Keep TypeScript types generated from Rust structs (`shared-types`) in sync — don't hand-edit.
- New React Flow node types belong in `packages/diagram-engine`, not inline in `apps/web`.
- `cem-connectors` and `fuml-runtime` both cross process boundaries via **gRPC** (ADR-008) —
  don't introduce bespoke REST for a new external-tool boundary.
- Run `cargo clippy` + `cargo fmt` and Biome checks before considering work done.

## Testing expectations (see `Axioma_test_specification_v4.md` for the full suite)
Tests run against a shared reference fixture, **`Turbofan-Ref`**, grown per phase, plus a
synthetic 1M-element **`Turbofan-Scale`** fixture for performance-only tests. Each test has
binary PASS/FAIL criteria with numeric thresholds where the source NFR defines one. Examples:
- 50k blocks / 100k relationships: load < 3s, pan/zoom > 50 FPS, search < 100ms.
- Element-create latency: p95 < 100ms, p50 < 50ms, zero errors, at `Turbofan-Scale`.
- A converged-but-illegal state must be quarantined, never persisted to Main.
- A `Diverged` solver run must drop to review even at Autonomy L4.
- A Campaign without a budget must be refused outright.
- `alf-lite`: each supported construct needs a golden test (source → fUML → executed trace);
  an out-of-subset construct must yield a precise compile error, never a silent partial compile.
- Deterministic replay: 100 identical simulation runs produce identical results.


