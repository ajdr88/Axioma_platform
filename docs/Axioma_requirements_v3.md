# Axioma: System Requirements Specification

**Version:** 3.0 (Post-Architecture-Review)
**Status:** Draft for Architecture Review — Rev B
**Supersedes:** Axioma_requirements.md v2.0
**Companion documents:** Axioma_implementation_v3.md, Axioma_architecture_review.md
**Change basis:** This revision folds in all twelve prioritized findings from the architecture review. Material changes are flagged inline as **[REV-B]** with the review section that motivated them.

---

## 1. Executive Summary

**Axioma** is a cloud-native model-based systems engineering platform for building complex hardware — go from first requirements to manufacturing specifications with one connected model. It is built around a SysML v2 model graph and delivered as **two products on independent tracks** rather than one monolithic release **[REV-B §A]**. This framing matches the live copy at axioma.systems verbatim; keep both in sync:

- **Product 1 — the MBSE Platform.** A next-generation, SysML v2-native model-based systems engineering tool: requirements, architecture, safety, mission planning, traceability, simulation, and collaboration. This is a complete, sellable product on its own, comparable in scope to what competitors (e.g. Dalus) ship today. It has no dependency on the Computational Engineering Model.
- **Product 2 — the Computational Engineering Model (CEM).** A generative, physics-validated design layer that sits on top of Product 1: system-level architecture optimization (Mode B) and manufacturable part/assembly geometry synthesis (Mode C), with validation delegated to established external FEA/CFD solvers. Mode A (a grounded AI copilot) is a fast-follow bridge between the two products.

The reason for the split is dependency honesty: Mode C is genuine research-risk work (see the Leap71/Noyron precedent — years of specialized effort in a single domain), and it must not block shipping the parts that are ready. Each product is independently valuable and independently shippable.

### Why "Axioma"

An axiom is a foundational statement from which everything else is derived. Axioma is a research project built on that idea: encoding engineering knowledge — physics, manufacturing constraints, safety logic, mission requirements — as computable first principles, and deriving design consequences directly from them. This exact framing is the live copy at axioma.systems; keep this section and the site in sync going forward.

### Strategic Objectives

* **Eliminate Information Silos:** A single-source-of-truth model graph, versioned like source code.
* **Accelerate Design Cycles:** AI-augmented modeling and real-time textual/graphical sync targeting a meaningful reduction in model-entry time (measured against a baseline established during the pilot, not assumed) **[REV-B §D1 — replaced the unfounded "40%" with a measured target]**.
* **Ensure High Fidelity:** Early behavioral simulation plus, in Product 2, physics-validated geometry before anything is built.
* **Be Deployment-Ready, Not Deployment-Locked:** Support the compliance/hosting postures aerospace/defense customers require (SOC 2, GovCloud/ITAR, on-prem, EU residency) as configuration, not re-architecture (§3.4).

### Product & Release Structure **[REV-B §A]**

| Track | Scope | Depends on | Risk class |
| :--- | :--- | :--- | :--- |
| **Product 1** | Phases 1–4: graph foundation, IDE experience, digital thread, behavioral simulation, safety, mission planning | — | Delivery-risk (known engineering) |
| **Mode A fast-follow** | Grounded AI copilot over the Product 1 graph | Product 1 stable | Delivery-risk |
| **Product 2 — Mode B** | System-level architecture synthesis & trade-study optimization | Product 1 graph + Interface Contract | Delivery-risk, harder |
| **Product 2 — Mode C** | Manufacturable geometry synthesis + external validation | Mode B + `cem-geometry` kernel | **Research-risk** (explicitly) |

### Success Metrics

* Product 1 usable as a standalone MBSE tool with no CEM present.
* 100% compliance with the OMG SysML v2 API specification.
* Change-impact analysis reduced from hours to seconds, measured at target scale (§3.1).
* Zero re-architecture required to enable any compliance/deployment mode (§3.4).
* All performance NFRs continuously verified against a representative load fixture in CI (§3.1) — asserted numbers must be measured, not aspirational **[REV-B §C1]**.

---

## 2. Functional Requirements (FR)

Requirement IDs are grouped by domain and are **stable identifiers, not an ordering** — new requirements append within their group and existing IDs are never renumbered **[REV-B §D1]**. Every FR carries a traceability triplet: it links to its design section (§5–§6 here or in the implementation doc) and its test scenario (implementation doc §5). A full traceability matrix is maintained in the implementation doc §6.

### 2.1 Core Platform (Product 1)

| ID | Requirement Name | Description | Design | Test |
| :--- | :--- | :--- | :--- | :--- |
| **FR-CORE-01** | Standardized API | 100% compliance with the OMG Systems Modeling API & Services v1.0 standard. | impl §1 | impl §5 |
| **FR-CORE-02** | Dual-Notation Sync | Real-time bi-directional sync between SysML v2 Textual Notation (LSP-based) and Graphical Diagrams. | impl §4.3 | impl §5 |
| **FR-CORE-03** | Graph Traceability | n-degree relationship maps (Satisfy, Verify, Refine) across the model, via graph-query languages, subject to the query budgets in NFR-PERF-04. | §5.3 | impl §5 |
| **FR-CORE-04** | Behavioral Simulation | Discrete-event simulation of State Machines and Activity Diagrams. Execution-engine build-vs-adopt is an open ADR (§7); full f-UML/Alf compliance is not assumed **[REV-B §D3]**. | impl §4.5 | impl §5 |
| **FR-CORE-05** | Semantic Validation | A server-authoritative validation pass enforces model invariants (valid relationship endpoints, containment acyclicity, parametric consistency) independently of the collaboration layer. See §5.1 **[REV-B §B1]**. | §5.1 | impl §5 |
| **FR-CORE-06** | Collaborative Editing | Conflict-free convergent editing (CRDT) with no package locking. Convergence is distinct from validity — see FR-CORE-05 and §5.1 **[REV-B §B1]**. | §5.1 | impl §5 |
| **FR-CORE-07** | Model Import / Interop | First-class import from ReqIF (requirements) and the SysML v2 standard API (model interchange), plus an AI-assisted "documents → draft model" path. Migration off Cameo is a named, designed capability, not an afterthought **[REV-B §E3]**. | impl §4.4 | impl §5 |
| **FR-CORE-08** | Provenance & Confidence Model | Every element records origin (human / AI-suggested / AI-auto-merged), validation state, and staleness, queryable graph-wide and surfaced visually. See §6.2 and FR-CEM-04 **[REV-B §E2]**. | §6, impl §6.x | impl §5 |
| **FR-CORE-09** | Alf Authoring (Minimal Subset) | Behavioral action code may be authored in a minimal, in-house subset of OMG Alf (`alf-lite`), compiled to fUML for execution. Clean-room (public spec only; no GPL Alf RI code); scoped to the pilot's constructs and grown on demand. See impl §9.6 **[REV-B §D3]**. | impl §9.6 | impl §5 |

### 2.2 Computational Engineering Model — CEM (Product 2)

The CEM is a two-tier generative system over the Product 1 graph: **Mode B** operates on the systems-architecture graph, **Mode C** generates and validates physical geometry. **Mode A** is the grounded copilot bridging them. Architecture in §5.

| ID | Requirement Name | Description | Design | Test |
| :--- | :--- | :--- | :--- | :--- |
| **FR-CEM-01** | Grounded Retrieval (Mode A) | Answer engineering questions from the model graph plus an external corpus, every claim citing a source. | §5.2 | impl §5 |
| **FR-CEM-02** | Architecture Synthesis (Mode B) | From top-level requirements, generate/optimize allocation of Blocks and interfaces across subsystems via 0D/1D performance and mass-budget models. | §5.2 | impl §5 |
| **FR-CEM-03** | Validation Gate | Every generated candidate passes an appropriate validation gate before being tagged `Validated`. The gate result is not binary — see the solver-result states in §5.4 and FR-CEM-13 **[REV-B §B3]**. | §5.4 | impl §5 |
| **FR-CEM-04** | Auto-Traceability & Provenance | Generated elements create `Satisfy`/`Verify`/`Refine` edges to their source, tagged `source: ai-generated`, and carry full generation provenance (FR-CEM-05). | §5.2 | impl §5 |
| **FR-CEM-05** | AI Generation Provenance | Every LLM-driven generation records model name, version, prompt-template hash, temperature/seed, and a context snapshot — the LLM analog of `SimulationRun` provenance **[REV-B §B4]**. | §5.2 | impl §5 |
| **FR-CEM-06** | Continuous Feedback Ingestion | Real test/simulation outcomes feed future generations for that element type. | §5.4 | impl §5 |
| **FR-CEM-07** | Configurable Review Gate | Review strictness before merge is governed by the Autonomy Level (FR-CEM-16); no level bypasses FR-CEM-03. | §5.6 | impl §5 |
| **FR-CEM-08** | Interface Contract | Mode B emits a structured Interface Contract per subsystem as the spec Mode C consumes. | §5.3 | impl §5 |
| **FR-CEM-09** | Geometry Synthesis (Mode C) | From an Interface Contract, generate manufacturable geometry, validated externally (§5.4). | §5.4 | impl §5 |
| **FR-CEM-10** | Assembly Composition | Mode C composes mating parts into valid assemblies/sub-assemblies respecting the Interface Contract's ports. | §5.4 | impl §5 |
| **FR-CEM-11** | Bidirectional MDO Feedback | Mode C actuals write back to Block properties; material deviation flags dependents `Suspect`. Whether that auto-triggers re-optimization is governed by the Autonomy Level (FR-CEM-16). | §5.6 | impl §5 |
| **FR-CEM-12** | FEA/CFD Connector Framework | Provider-agnostic interface for submitting jobs (structural FEA, CFD, thermal) to external solvers; no vendor hard-wired. Generation stays proprietary; validation is delegated. | §5.4 | impl §5 |
| **FR-CEM-13** | Solver Result States | Solver outcomes are typed (`Converged`, `Diverged`, `Failed`, `Timeout`, `Suspect-Numerical`, `LicenceUnavailable`), with a plausibility check before any graph write. Only `Converged`-within-bounds may satisfy an autonomy gate; all else drops to human review **[REV-B §B3]**. | §5.4 | impl §5 |
| **FR-CEM-14** | Multi-Run Simulation Campaigns | Batches of runs (varying geometry, BCs, or candidates) dispatched in parallel via a governed job scheduler (NFR-PERF-05) across configured solvers, at Part / Subsystem / System level. | §5.4 | impl §5 |
| **FR-CEM-15** | Comparative Design Evaluation | Campaign results compared against Interface Contract targets and each other (e.g. Pareto front on mass/stress/cost), surfaced or fed back to Mode B/C. | §5.4 | impl §5 |
| **FR-CEM-16** | Configurable Autonomy Levels | An ordered set of autonomy levels (L0–L4) for CEM-driven change, mirroring AI coding-assistant conventions. Defined in §5.6. | §5.6 | impl §5 |
| **FR-CEM-17** | Scoped Autonomy Configuration | Autonomy Level is set per project/branch and overridable per element-type/subsystem — not a single global switch. | §5.6 | impl §5 |
| **FR-CEM-18** | Safety Override (Non-Negotiable) | No autonomy level allows an element linked to an unmitigated Hazard, or a High/Catastrophic Hazard (FR-SAFE-02), to merge without individual human review. Cannot be configured away. | §5.6 | impl §5 |
| **FR-CEM-19** | Result Provenance & Traceability | Each solver run's metrics write back via a `validatedBy` edge to a `SimulationRun` node recording solver/version/settings/input-hash/timestamp; large result files referenced by pointer, not stored in the graph (NFR-DATA-02). | §5.4 | impl §5 |

### 2.3 System Safety & Hazards (Product 1)

| ID | Requirement Name | Description | Design | Test |
| :--- | :--- | :--- | :--- | :--- |
| **FR-SAFE-01** | Hazard Identification | `Hazard` nodes linked to Blocks/Functions/Requirements via `causes`/`mitigatedBy`. | impl §4.3 | impl §5 |
| **FR-SAFE-02** | Risk Assessment Matrix | Configurable Severity × Likelihood scales (e.g. 5×5, MIL-STD-882 / ARP4761 conventions), auto-computed Risk Index. | impl §4.3 | impl §5 |
| **FR-SAFE-03** | Mitigation/Control Tracking | Controls linked to Hazards; residual risk and status (Open / Mitigated / Accepted). | impl §4.3 | impl §5 |
| **FR-SAFE-04** | Safety Traceability | Hazards/Controls participate in the same traceability graph (FR-CORE-03). | §5.3 | impl §5 |
| **FR-SAFE-05** | Standards-Aligned Reporting | Export a Hazard/Risk register formatted for ARP4761 / MIL-STD-882 / ISO 26262. | impl §4.4 | impl §5 |

### 2.4 Mission Planning (Product 1)

| ID | Requirement Name | Description | Design | Test |
| :--- | :--- | :--- | :--- | :--- |
| **FR-MSN-01** | Mission/Use-Case Definition | `Mission`/`UseCase` elements at the top of the hierarchy, deriving Requirements. | impl §4.3 | impl §5 |
| **FR-MSN-02** | Stakeholder Management | Stakeholders, concerns, and links to owned Missions/Requirements. | impl §4.3 | impl §5 |
| **FR-MSN-03** | Program Phase Tracking | Lifecycle phases (Concept → Development → Production → Operations → Disposal) as a timeline overlay. | impl §4.3 | impl §5 |
| **FR-MSN-04** | Mission-to-Requirement Traceability | Traceability extends upward: Missions → Requirements → Blocks. | §5.3 | impl §5 |

---

## 3. Non-Functional Requirements (NFR)

### 3.1 Performance & Scale **[REV-B §C1]**

* **NFR-PERF-01 (Canvas rendering):** Render 10,000+ *visible* elements at 60 FPS via WebGPU. Beyond that, the canvas uses viewport virtualization and level-of-detail — off-screen subsystems are clustered/collapsed, not held as live nodes. "WebGPU" is the rendering backend; virtualization is the data-volume strategy (impl §4.3).
* **NFR-PERF-02 (Interaction latency):** UI feedback for element creation < 50 ms; backend partial-graph persistence p95 < 200 ms *at target scale*.
* **NFR-PERF-03 (Model scale):** Support > 1 million elements. This is the backend-traversal target and is meaningful only with NFR-PERF-04 in force.
* **NFR-PERF-04 (Query budgets):** Traversal/traceability endpoints enforce max depth, max fan-out, and result caps with cursor-based pagination. GDS projections are scoped to a subgraph, never the whole model. Each endpoint declares a p95 latency target *at 1M-element scale*.
* **NFR-PERF-05 (Campaign resource governance):** Solver Campaigns run under a job scheduler with per-project concurrency limits, quotas, cost ceilings, retry/back-off, and cancellation. An autonomous (L4) loop cannot launch an unbounded or unbudgeted Campaign **[REV-B §C3]**.
* **NFR-PERF-06 (Continuous load verification):** A synthetic 1M-element reference model is maintained as a CI fixture; NFR-PERF-01/02/03/04 are measured against it on every release, not merely asserted.

### 3.2 Reliability & Data Integrity **[REV-B §B]**

* **NFR-REL-01 (Convergence vs. validity):** The collaboration layer guarantees all clients converge to the same state; a separate server-authoritative pass (FR-CORE-05) guarantees that state is *valid*. An illegal converged state is quarantined and surfaced as a conflict — never silently persisted. §5.1 defines the policy.
* **NFR-REL-02 (Graph topology):** The model is a **directed property graph**, not a DAG. Acyclicity is enforced only on the containment/composition hierarchy; traceability, feedback (`validatedBy`), and `Suspect` propagation are expected to form cycles. Algorithms must not assume global acyclicity **[REV-B §B2]**.
* **NFR-REL-03 (Solver trust):** External solver results are never trusted blindly; FR-CEM-13's result states and plausibility checks gate every write from a solver.
* **NFR-REL-04 (Backup / DR):** The graph, the document store, and the Git-backed model store have defined backup cadence and stated RPO/RTO recovery objectives **[REV-B §F]**.
* **NFR-REL-05 (Schema migration):** Evolution of the KerML schema or node definitions (`Hazard`, `Mission`, etc.) is supported by a versioned, tested graph-migration mechanism; migrating millions of existing nodes is a planned capability, not an incident **[REV-B §F]**.

### 3.3 CEM-Specific

* **NFR-CEM-01 (Latency):** Mode A query < 5 s; Mode B trade studies evaluate many candidate architectures per session (funnel efficiency, NFR-CEM-05).
* **NFR-CEM-02 (Determinism, scoped):** The deterministic optimizer (`cem-core`) and pinned external solvers are reproducible given identical inputs; solver reproducibility is scoped to a pinned solver name+version+settings on the `SimulationRun`. LLM-driven output (Mode A, drafting) targets "explainably similar," not bit-identical, and records the provenance in FR-CEM-05 to make that auditable **[REV-B §B4]**.
* **NFR-CEM-03 (Data Isolation):** Zero training on customer/proprietary data by default; on-prem/local-LLM supported.
* **NFR-CEM-04 (Provenance):** Every AI-sourced edge or claim carries a citation — no exceptions.
* **NFR-CEM-05 (Funnel Efficiency):** Mode B stays cheap enough to explore broadly; Mode C is invoked only for narrowed subsystems.
* **NFR-CEM-06 (Autonomy Auditability):** Every change to an Autonomy Level is itself logged (actor, timestamp, old/new).

### 3.4 Compliance & Deployment Readiness

Positioning unchanged from v2.0: SOC 2 Type 2, AWS GovCloud (ITAR/EAR), on-prem, and EU Data Residency are **optional modes activated later**, but the architecture must not require a rewrite to enable them.

* **NFR-COMP-01 (Deployment Portability):** No hard dependency on a single cloud vendor's proprietary services in the core data/compute path.
* **NFR-COMP-02 (Data Residency):** Data layer supports per-tenant/per-project region pinning from day one.
* **NFR-COMP-03 (Auth Abstraction):** Authentication behind a provider-agnostic interface; the LLM provider is behind the same kind of interface (§5.5) so local vs. hosted is a config choice **[REV-B §D4]**.
* **NFR-COMP-04 (Audit-Readiness):** All writes logged with actor/timestamp/diff (the Git-backed MVS + change-tracking middleware). Distinct from operational observability (NFR-OPS-01).
* **NFR-COMP-05 (Isolation-Ready Segmentation):** Supports a fully isolated single-tenant instance per customer/project.

### 3.5 Operability & Security Defaults **[REV-B §F]**

* **NFR-OPS-01 (Observability):** Every service (`sysml-core`, `cem-core`, `cem-geometry`, `cem-connectors`, LSP, collaboration server) emits distributed traces, metrics, and structured logs. This is operational telemetry, distinct from the compliance audit log (NFR-COMP-04).
* **NFR-OPS-02 (Multi-tenancy default):** The *default* shared deployment specifies exactly how project A's data is isolated from project B's (row/graph-level tenancy keys, enforced in the data-access layer). This is a security-critical default, not an edge case; single-tenant isolation (NFR-COMP-05) is the stricter variant, not the only defined one.
* **NFR-OPS-03 (Rate & abuse limits):** LLM and solver endpoints are rate-limited and quota-bound, especially when invoked programmatically by autonomy loops.
* **NFR-OPS-04 (Generative-path concurrency):** A defined policy governs overlapping writes on the same subsystem — concurrent Campaigns, or Mode C writing while a human edits the same Block. §5.7.

---

## 4. Technology Stack (Summary)

Full detail and rationale in the implementation doc §2; **the ADR log (impl §2.5) is the single source of truth for technology choices** and supersedes any offhand mention elsewhere **[REV-B §D2]**.

| Layer | Technology | Notes |
| :--- | :--- | :--- |
| Frontend | React 19 + Next.js + React Flow | **Single frontend stack. All SvelteFlow references removed** — see ADR-002 **[REV-B §D2]**. |
| Graphics | WebGPU (rendering) + virtualization (data volume) | NFR-PERF-01 |
| Topology store | Neo4j / Memgraph | Relationships only — kept lean (NFR-DATA-01) |
| Element/metadata store | Document store or Postgres/JSONB | Element bodies, large metadata **[REV-B §C2]** |
| Object store | S3-compatible | Geometry, meshes, solver result files |
| Backend | Rust (Axum) | Services listed in impl §2 |
| Collaboration | CRDT (convergence) + server-side semantic validation | FR-CORE-05/06 |
| Versioning | Git-based model storage | — |
| LLM | Pluggable provider (local Ollama or hosted) behind an interface | NFR-COMP-03 |
| Optimizer | `cem-core` — deterministic math, **never an LLM** | **[REV-B §D4]** |
| CEM geometry | `cem-geometry` — solid-modeling + manufacturing constraints | Product 2 |
| CEM validation | `cem-connectors` — external-solver adapters + Campaigns | Product 2 |

### Persistence Split (NFR-DATA) **[REV-B §C2]**

* **NFR-DATA-01:** Neo4j stores topology and relationships only; it is kept lean so traversal stays fast at 1M elements.
* **NFR-DATA-02:** Element bodies, long requirement text, and large metadata live in a document/relational store; geometry, meshes, and solver outputs live in object storage and are referenced from the graph by pointer.

---

## 5. Architecture Reference

### 5.1 Collaboration & Semantic Validity **[REV-B §B1]**

Two layers, deliberately separated:

* **Convergence layer (CRDT):** guarantees every client eventually holds the *same* document state. It knows nothing about SysML semantics.
* **Semantic-validation layer (server-authoritative):** after convergence, the same `sysml-core` rule set used for CRUD validates the converged state. Invariants checked include: relationship endpoints are type-legal (a `Satisfy` targets a Requirement, not a Block); the containment hierarchy stays acyclic; parametric constraints are not mutually inconsistent; no edge references a concurrently-deleted node.

**Policy for an illegal converged state:** the offending change-set is *quarantined* (visible, marked invalid, not applied to Main) and surfaced to the involved editors as a conflict to resolve. Illegal states are never silently persisted. Convergence (everyone sees the same thing) and validity (the thing is legal) are distinct guarantees, and the spec treats them as such.

### 5.2 CEM Two-Tier Model

| Tier | Analog | Role |
| :--- | :--- | :--- |
| **Mode A — Copilot** | Leo AI's LMM | Grounded Q&A, part search, requirement linting. LLM-backed; provenance per FR-CEM-05. |
| **Mode B — Architecture Synthesis** | NPSS-style cycle/mass-budget tools | Allocates requirements across subsystems; explores architectures cheaply. Uses the **deterministic** `cem-core` optimizer — the LLM only drafts, it does not decide **[REV-B §D4]**. |
| **Mode C — Geometry Synthesis** | Leap71's Noyron | Generates/validates manufacturable geometry per subsystem; composes assemblies. **Research-risk** track. |

### 5.3 Traceability Engine

n-degree relationship traversal (`Satisfy`/`Verify`/`Refine`/`causes`/`mitigatedBy`) spanning Mission → Requirement → Block → Hazard → SimulationRun. Bounded by NFR-PERF-04 (depth/fan-out/pagination) so a dense-graph query cannot explode. The graph is cyclic in general (NFR-REL-02); traversal algorithms use visited-set cycle detection, never acyclic assumptions.

### 5.4 CEM Validation via External FEA/CFD (Open Decision #2 — resolved)

Generation and decision logic stay proprietary (Modes A/B/C). Physics validation is delegated to external solvers through `cem-connectors`, a thin provider-agnostic adapter layer — not a physics engine Axioma builds.

**Solver result states (FR-CEM-13) [REV-B §B3]:** a run resolves to one of `Converged`, `Diverged`, `Failed`, `Timeout`, `Suspect-Numerical`, or `LicenceUnavailable`. A plausibility pass (result-sanity heuristics — e.g. non-negative safety factors, physically-bounded stresses) runs between the solver and the graph write. Only `Converged`-within-bounds can satisfy an autonomy gate (§5.6); every other state drops the item to human review regardless of Autonomy Level.

**Interface Contract (FR-CEM-08):** the schema crossing between Mode B and Mode C in both directions.

| Field | Description |
| :--- | :--- |
| Performance targets | Functional requirements the subsystem must meet |
| Boundary conditions | Thermal/mechanical/aero environment |
| Geometric envelope | Space constraints from neighboring subsystems |
| Interface/port definitions | Mating geometry and connections |
| Mass/cost targets | Budget from the system-level model |
| Material/process constraints | Admissible processes and materials |

Return payload (Mode C → graph): actual mass, predicted performance, cost, and the governing `SimulationRun`(s) — confirming the Mode B estimate or triggering `Suspect` (FR-CEM-11).

### 5.5 Reference Subsystem Decomposition (Open Decision #1 — resolved)

Axioma uses the standard MBSE decomposition (BDD structural hierarchy, IBD ports/flows, Parametric Diagram constraint links). Mode B is a live, optimizing implementation of the Parametric Diagram concept, not a new artifact type. For a turbofan SoI, the five top-level subsystems — Fan & LP Compression, Core (HP) Compressor, Combustor, Turbine (HP & LP), Control (FADEC/EEC) — each become one Interface Contract. Finer-grained sub-contracts nest under a subsystem later, only as Mode C work justifies.

### 5.6 Autonomy Levels (Open Decision #3 — resolved)

| Level | Name | Behavior |
| :--- | :--- | :--- |
| **L0** | Manual / Suggest-Only | Nothing unprompted; each generation invoked and reviewed individually. |
| **L1** | Review Every Change | Full task runs unattended; every generated element queued for one-by-one accept/reject. |
| **L2** | Review Batch | Full task runs; the whole result presented as one consolidated diff for a single decision. |
| **L3** | Guardrailed Autonomy | Changes passing all validation gates (FR-CEM-03/13) and within configured thresholds merge automatically; anything outside drops to L1 for that item. |
| **L4** | Full Autonomy | The whole Mode B → Mode C funnel, including Suspect-triggered re-optimization, runs and merges without review — subject only to the hard gates and NFR-PERF-05 budget limits. Explicit opt-in. |

Scope per FR-CEM-17 (project/branch default, element-type overrides). Non-negotiable exception per FR-CEM-18 (Hazard-linked elements always get human review). No autonomy level bypasses FR-CEM-03/13 or NFR-PERF-05.

### 5.7 Generative-Path Concurrency **[REV-B §F]**

When a Mode C write and a human edit target the same Block, or two Campaigns target the same subsystem, the platform applies: (a) optimistic concurrency with version checks on the target Block; (b) human edits always win over an in-flight autonomous write — the autonomous result is re-queued against the new state, not force-merged; (c) overlapping Campaigns on one subsystem are serialized by the scheduler (NFR-PERF-05). Defined here so the generative path has an explicit answer, not an emergent one.

---

## 6. Usability Principles

### 6.1 Primary Task-Flow (not "seven panels") **[REV-B §E1]**

The platform is designed around an engineer's end-to-end task flow, and the surfaces (Monaco text, React Flow diagram, 3D geometry viewer, Safety panel, Mission timeline, Autonomy selector, AI Copilot) are wired to that flow — not simply co-located. The canonical flow and its surface transitions are specified in the implementation doc §6; the design principle here is that **transitions between surfaces are first-class features**, e.g. selecting a Block in the diagram highlights it in text, shows its Hazards in the inspector, and offers Mode C geometry — as one continuous action, not four separate lookups.

### 6.2 Provenance & Confidence Visual Language (FR-CORE-08) **[REV-B §E2]**

A consistent, graph-wide visual vocabulary conveys, for every element: **origin** (human / AI-suggested / AI-auto-merged), **validation state** (unverified / solver-validated / test-validated), and **staleness** (consistent vs. `Suspect`). These are filterable ("show everything auto-merged at L4 not yet human-reviewed"). This is both a usability feature and a trust/safety feature — in a safety-critical context a single accent color is insufficient signal.

### 6.3 Onboarding & Import (FR-CORE-07) **[REV-B §E3]**

The first-hour experience — bringing an existing model in from Cameo/ReqIF/documents — is a designed capability, since it determines adoption more than any single feature. The AI-assisted "documents → draft model" path doubles as the strongest Mode A demo.

---

## 7. Open Decisions (ADR candidates)

The three original open decisions are resolved (§5.4, §5.5, §5.6). The review surfaced additional decisions now tracked as ADRs in the implementation doc §2.5; the load-bearing ones:

1. **Behavioral-simulation engine: build vs. adopt [REV-B §D3] — survey complete, see impl §9.** No maintained *Rust* engine exists; the from-scratch Rust interpreter assumed in earlier drafts is withdrawn. Resolution: **adopt** the Java fUML Reference Implementation (CPL/Apache) as a JVM sidecar for *execution*, driven over gRPC (ADR-008); **build** a minimal, clean-room in-house Alf-subset compiler (`alf-lite`, FR-CORE-09) for *authoring*, compiling to the same fUML and scoped to only the pilot's constructs; **decline to link** the GPL-v3 Alf RI. Tracked as ADR-005 (recommended; spike to ratify the subset and sidecar latency).
2. **Persistence topology confirmation [REV-B §C2].** The polyglot split (§4 NFR-DATA) is adopted in principle; the specific document/relational store and object store are ADR-003.
3. **LLM provider strategy [REV-B §D4].** Local-first (Ollama) for privacy vs. hosted for capability, behind one interface — ADR-004.
