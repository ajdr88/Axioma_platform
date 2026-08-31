# Axioma: Implementation Specification

**Version:** 5.0
**Status:** Draft — Phase 0 doc-consolidation pass
**Companion documents:** Axioma_requirements_v5.md, Axioma_test_specification_v4.md, Axioma_stage_tracking_amendment.md
**Change basis:** Folds v4 (Rev C, carried forward unchanged) plus the six `docs/claude/` amendment/analysis documents merged in this pass — see `Axioma_requirements_v5.md`'s header for the full list and the two numbering collisions found and resolved (ADR-011, reqs §5.14). Material changes flagged **[REV-D]**; earlier **[REV-B §x]**/**[REV-C]** flags carried forward unchanged.

---

## 1. SysML v2 REST API Specification (Draft)

Services are structured around **Projects**, **Commits**, and **Elements**. All traversal endpoints enforce query budgets (NFR-PERF-04): explicit `maxDepth`, `maxFanout`, and cursor-based pagination; unbounded traversals are rejected **[REV-B §C1]**.

### 1.1 Core Endpoints (Product 1)

* **`GET /projects/{id}/commits/{id}/elements?cursor=&limit=`** — Paginated element list for a model version.
* **`POST /projects/{id}/elements`** — Create an element (Block, Part, Requirement, Hazard, Mission). Write passes the semantic-validation layer (§4.2) before commit.
* **`GET /elements/{id}/traceability?depth=&maxFanout=&cursor=`** — Impact path, bounded by query budget; returns a page plus a continuation cursor.
* **`POST /simulations/execute`** — Behavioral simulation run.
* **`POST /import/reqif`** / **`POST /import/sysml-v2`** — Model import/interop (FR-CORE-07) **[REV-B §E3]**.
* **`POST /import/documents`** — **[REV-D, revised]** now an async job — see §1.1a.
* **`GET /elements/{id}/provenance`** — Origin, validation state, staleness for any element (FR-CORE-08) **[REV-B §E2]**.
* **`GET /blocks/{id}/lifecycle-status`** — Current stage, status, and computed progress % for a subsystem `Block`, per the rules in reqs §5.8 (FR-PM-01/03) **[REV-C]**.
* **`GET /projects/{id}/program-phase`** — Computed program phase and rollup progress, derived per FR-PM-02 from all subsystem stages **[REV-C]**.

### 1.1a Documents → Draft Model Endpoints **[REV-D, new]**

Revises the previously bare, unspecified `POST /import/documents` entry into its full async-job shape (FR-CORE-14…18, reqs §5.14):

```
POST /import/documents                      — { fileRef } → { jobId }
GET  /import/documents/{jobId}               — status: Extracting | Segmenting | Drafting | Validating | AwaitingReview | Failed
GET  /import/documents/{jobId}/candidates    — per-candidate: text, confidence, citation, category, flags (FR-CORE-18)
GET  /import/documents/{jobId}/suggestions   — candidate structural nouns (FR-CORE-17), display-only
POST /import/documents/{jobId}/proposal      — materializes the validated set as a `document-import` proposal (FR-CORE-16)
```

`GET /cem/proposals/{branchId}` and `POST /cem/proposals/{id}/accept|reject` (§1.2) are reused unchanged for the resulting proposal — no new accept/reject endpoint, consistent with "one mechanism, three origins" (reqs §5.6).

### 1.2 CEM Endpoints (Product 2)

* **`POST /cem/mode-a/query`** — Grounded Q&A; response carries citations + LLM provenance (FR-CEM-05).
* **`POST /cem/mode-b/optimize`** — `{ topLevelRequirementIds, constraints }` → trade-study run; ranked candidate architectures.
* **`GET /cem/interface-contract/{subsystemBlockId}`** — Current Interface Contract.
* **`POST /cem/mode-c/synthesize`** — `{ interfaceContractId }` → geometry, validation status, actuals.
* **`GET /cem/proposals/{branchId}`** / **`POST /cem/proposals/{id}/accept|reject`** — Review gate (autonomy-governed). **[REV-D]** Accepts three proposal origins: `cem-generated`, `human-authored` (FR-PM-05), `document-import` (FR-CORE-16).
* **`POST /cem/campaigns`** — `{ elementId, solverIds[], parameterSweep, budget }` → governed Campaign (NFR-PERF-05). `budget` is mandatory; a Campaign without a cost/quota ceiling is rejected **[REV-B §C3]**.
* **`GET /cem/campaigns/{id}`** — Status + comparative/Pareto results.
* **`GET /cem/simulation-runs/{elementId}`** — `SimulationRun` provenance for an element, including result state (FR-CEM-13).
* **`PUT /cem/autonomy-level`** / **`GET /cem/autonomy-level/{scope}`** — Autonomy config; changes logged (NFR-CEM-06).

### 1.2a Mode B Design-Space Endpoints **[REV-D, new — FR-ARCH]**

```
POST /cem/mode-b/design-space              — define/version an ADSG-style design-space model (functions, choices, constraints)
GET  /cem/mode-b/design-space/{id}/stats   — IR / CR / CRF / MRD (FR-ARCH-06)
POST /cem/mode-b/design-space/{id}/resolve — resolve one selection/connection choice, returns updated (partial) instance
GET  /cem/mode-b/instances?runId=          — browsable, comparable architecture instances from a Mode B run (FR-ARCH-07)
POST /cem/mode-b/instances/{id}/propose    — enter an instance into the existing /cem/proposals/* review gate (FR-CEM-07)
```

Not yet implemented — depends on ADR-011 (§2.5) and the `cem-core`/design-space sidecar it resolves to.

### 1.3 Safety & Mission Endpoints (Product 1)

* **`POST /safety/hazards`**, **`POST /safety/hazards/{id}/mitigations`**, **`GET /safety/risk-register/{projectId}`** (exportable ARP4761 / MIL-STD-882 / ISO 26262).
* **`POST /mission/missions`** / **`/use-cases`**, **`GET /mission/{id}/traceability`**.

### 1.4 Parametrics, Information, Interaction, Export, Collections Endpoints **[REV-D, new]**

```
POST /parametrics/constraints                       — define a Constraint (FR-PARAM-01)
POST /parametrics/bindings                           — bind Constraint parameters to Value Properties (FR-PARAM-02)
POST /parametrics/evaluate                           — { constraintIds[] or elementId } → computed values (FR-PARAM-03)

POST /information/elements                           — create an InformationElement (FR-INFO-01)
POST /information/data-types                         — create a Data Type / Enumeration (FR-INFO-02)

POST /interactions                                    — create an Interaction (FR-INTX-01)
POST /interactions/{id}/messages                      — add a message/invocation step
POST /interactions/{id}/fragments                     — add alt/opt/par/loop sub-sequence (FR-INTX-03)

GET  /export/diagram/{diagramId}?format=png|svg       — FR-EXPORT-01
GET  /export/table/{tableId}?format=csv|xlsx          — FR-EXPORT-02
POST /export/report                                    — { templateId, scopeElementId } → PDF/HTML (FR-EXPORT-03)
POST /elements/{id}/attachments                        — attach a file, pointer stored, body in object store (FR-EXPORT-04)

POST /collections/dynamic                              — save a Dynamic Query (FR-CORE-10)
POST /collections/{id}/freeze                          — convert Dynamic → Static (FR-CORE-11)
```

All new write endpoints pass through the existing semantic-validation layer (§4.2) before commit — no new bypass path. All new traversal-shaped endpoints (`/parametrics/evaluate` over a subgraph, `/collections/dynamic` query definition) are subject to the existing query-budget enforcement (NFR-PERF-04) — a Dynamic Query without depth/fan-out bounds is rejected at save time.

### 1.5 Operations Endpoints **[REV-B §F]**

* **`GET /healthz`** / **`GET /readyz`** — Liveness/readiness per service.
* **`GET /metrics`** — Prometheus-format metrics (NFR-OPS-01). Traces exported via OpenTelemetry.

---

## 2. Technical Architecture & Tech Stack

### 2.1 Service Topology

| Service | Responsibility |
| :--- | :--- |
| `sysml-core` | SysML v2 parse, KerML rules, semantic-validation layer (§4.2). |
| `api` (Axum) | REST surface, query-budget enforcement, auth. |
| `collab` | CRDT convergence server (Yjs/Hocuspocus) — convergence only. |
| `cem-core` | Mode B: **deterministic** 0D/1D models, mass-budget solver, allocation optimizer. Never calls an LLM to decide **[REV-B §D4]**. **[REV-D]** Design-space representation per reqs §5.17 (FR-ARCH); the encoder/optimizer layer is a build-vs-adopt decision, ADR-011. |
| `cem-geometry` | Mode C: solid-modeling kernel + manufacturing constraints (Product 2, research-risk). |
| `cem-connectors` | External FEA/CFD adapters, Campaign scheduling, result-state typing (§4.6). |
| `fuml-runtime` | **JVM sidecar** wrapping the fUML Reference Implementation (CPL/Apache); driven from Rust over **gRPC** (ADR-005, ADR-008, §9). Behavioral execution only; isolated so the JVM/Java-8 dependency does not touch the Rust build. |
| **`cem-archspace`** *(proposed, not yet built — ADR-011)* | **[REV-D]** A **Python gRPC sidecar** wrapping `adsg-core` (MIT) + `SBArchOpt` (MIT) for Mode B's design-space encode/decode and hierarchical-BO/evolutionary optimization (reqs §5.17, FR-ARCH-05/06/08). Same shape as `fuml-runtime`: an external-tool boundary crossed over gRPC (ADR-008), not a Rust reimplementation. Name/scope (standalone service vs. folded into `cem-core`'s deployment unit) is not yet finalized — tracked as part of ADR-011's ratification spike. |
| `alf-lite` | **In-house Rust** compiler for a minimal Alf subset (§9.6). Clean-room (public OMG spec only; no GPL RI code). Compiles to fUML for execution by `fuml-runtime` — a front-end only, no second runtime. |
| `llm-gateway` | Pluggable LLM provider behind one interface (local Ollama or hosted) — mirrors the solver-connector pattern **[REV-B §D4]**. **[REV-D]** Confirmed Product-1-tier shared infrastructure, not CEM-exclusive (ADR-012) — the documents→draft-model pipeline (§5.14 reqs) depends on it directly. |
| `scheduler` | Governed job queue for Campaigns: concurrency, quota, cost ceiling, back-pressure (NFR-PERF-05). |

### 2.2 Persistence (Polyglot) **[REV-B §C2]**

| Store | Holds | Rationale |
| :--- | :--- | :--- |
| **Neo4j / Memgraph** | Topology and relationships **only** | Kept lean so 1M-element traversal stays fast (NFR-DATA-01). |
| **Postgres / JSONB (or document store)** | Element bodies, long requirement text, large metadata | Graph DBs are poor at large payloads; keep them out of the graph. |
| **S3-compatible object store** | Geometry, meshes, solver result files | Referenced from the graph by pointer, never inlined (NFR-DATA-02). |
| **Time-series store** | Simulation/telemetry history | Behavioral-sim playback, dashboards. |
| **Redis** | Ephemeral cache, session, short-lived queue signalling | Dev-stack default; the durable job queue is the `scheduler` service, not Redis alone. |

### 2.3 Data Model

A **directed property graph** — *not* a DAG **[REV-B §B2]**. Acyclicity is enforced only on the containment/composition hierarchy. Traceability, `validatedBy`, and `Suspect` propagation legitimately form cycles; all traversal uses visited-set cycle detection.

* **Node labels (Product 1 core):** `:Element` (base), `:Structure`, `:Requirement`, `:Port`, `:Hazard`, `:Control`, `:Mission`, `:Stakeholder`, `:SimulationRun`.
* **Node labels — [REV-D, implemented `docs/IMPLEMENTATION_KICKOFF.md` Phase 1]:** `packages/sysml-core/src/lib.rs`'s `NodeKind` enum, `apps/api/src/store/neo4j.rs`'s `ALL_LABELS` (indexing), `packages/sysml-textual`'s keyword mapping (a real compiler-caught spot the Phase 1 plan itself didn't enumerate — found only by rebuilding the workspace after the enum changed, not predicted in advance), and `packages/shared-types/src/index.ts` all now carry these ten:
  - `:Constraint`, `:Parameter` — Parametrics (FR-PARAM, reqs §5.9). Body (equation text) in the document store; graph holds topology only (NFR-DATA-01).
  - `:InformationElement` — Information/Data Architecture (FR-INFO, reqs §5.10). Participates in existing containment/traceability rules unchanged.
  - `:Interaction`, `:InteractionFragment` — Interaction/timing modeling (FR-INTX, reqs §5.11). Underlying SysML v2 mapping: ADR-009, ratified — see §16.1.
  - `:Collection` — Dynamic/Static Element Collections (FR-CORE-10/11, reqs §5.13).
  - `:CandidateStructureSuggestion` — Document-import pipeline (FR-CORE-17, reqs §5.14). **Proposal-scoped only** — never promoted automatically to `:Structure`; discarded once a human converts a suggestion into a real Block.
  - `:Function`, `:SelectionChoice`, `:ConnectionChoice` — Mode B architecture design-space (FR-ARCH, reqs §5.17). `:Function` is distinct from an executable fUML Action (FR-CORE-04). Choice nodes carry a resolution-state property (unresolved/partial/resolved).
* **Edges (Product 1 core):** `contains` (acyclic), `Satisfy`/`Verify`/`Refine`, `causes`/`mitigatedBy`, `validatedBy`, `Suspect`.
* **Edges — [REV-D, implemented Phase 1]:** `Derive`, `Copy` (extends the FR-CORE-03 traceability taxonomy, reqs §5.3 — also threaded into `neo4j.rs`'s `trace_neighbors`, required for T-CORE-03-EXT's own PASS criterion to actually hold); `Bound` (Constraint parameter ↔ Value Property, FR-PARAM-02); `Member` (Collection membership — explicitly distinct from `contains`, so a Collection referencing elements from anywhere in the graph never threatens NFR-REL-02's containment-acyclicity guarantee); `ArchDerives` (DSG derivation edge, cycles explicitly permitted per NFR-REL-02 — **renamed from the spec's own `derives`** to resolve a real naming collision found while implementing this: reqs v5 §5.3 separately names an unrelated Requirements-traceability edge `Derive`, singular; see `EdgeKind::ArchDerives`'s own doc comment); `IncompatibleWith` (undirected-semantics incompatibility constraint); `ChoiceConstraint` (hyper-edge over ≥2 choices, carrying a Linked/Permutations/Unordered [non-]replacing type — mirrors `adsg_core.ChoiceConstraintType` exactly, confirmed during the Phase 2 spike). `ArchDerives`/`IncompatibleWith`/`ChoiceConstraint` are deliberately kind-unconstrained in `sysml-core`'s semantic-validation layer — the spec keeps them generic on purpose; `Bound`/`Derive`/`Copy`/`Member` each got a real endpoint-legality rule.
* **New properties — [REV-D, `citation`/`confidence`/role-tags documented as `ElementBody.properties` JSONB conventions this pass — no schema change needed, same as every other body property in this codebase]:** `citation`, `confidence` on `:Requirement` (document-import provenance, FR-CORE-15; `confidence` is proposal-scoped/transient, discarded on accept — it describes the draft, not the accepted content); an optimization-role tag (`objective`/`constraint`/`generic`, plus a permanence flag) on `:Requirement`/`:Constraint`, for FR-ARCH's metric modeling; a fulfillment-mechanism tag (`DE`/`MULTI`/`NOF`/`CON`/direct `COMP`) on `:Function`↔`:Structure` edges. **`origin` on a `Proposal` (Postgres, not the graph) is genuinely implemented this pass** — `apps/api/src/store/versioning.rs`'s `proposals` table gained a real `origin TEXT NOT NULL DEFAULT 'cem-generated'` column; this closes a real gap found while implementing, not assumed: the `proposalOrigin` field the document-import amendment's `document-import` value was meant to extend **did not exist anywhere in the real code** before this pass — `mode_b.rs::propose` (P2.2, the only real caller) now passes `"cem-generated"` explicitly at its `create_proposal` call site, giving the still-unbuilt `human-authored`/`document-import` origins (FR-PM-05/FR-CORE-16) a real column to land in later.

**[REV-D] A real gap found, flagged rather than guessed**: FR-CORE-12/13 (Swimlane allocation,
orphan-Action rejection) need an Action/Activity-node concept **no merged v5 doc actually names as
a `NodeKind`** — `:Structure` is the closest existing generic modeling kind, but treating every
`:Structure` with zero non-containment edges as an "orphan Action" would misfire on legitimate
top-level Blocks (e.g. `Engine` itself). Not implemented this Phase 1 pass — inventing a modeling
primitive the spec doesn't authorize would be exactly the "guessing ahead of the spec" the
semantic-validation layer's own existing discipline (`check_relationship_endpoints`'s doc comment)
already commits against. **Recommendation**: resolve with a small spec amendment (does an
`:Action`/`:Activity` `NodeKind` exist, and what's its containment/allocation relationship to
`:Structure`?) before Phase 5's Swimlane UI work comes to depend on it — not blocking anything else
in Phase 1, which is otherwise fully landed.

All new write paths pass through the existing semantic-validation layer (§4.2) before commit — no new bypass, consistent with how every other write path in this codebase already works.

### 2.4 CEM Integration Map

| Component | Role |
| :--- | :--- |
| Graph (topology) + document store (bodies) | Context for Mode A; write-surface for Mode B/C, split per §2.2. |
| `sysml-core` semantic validation | Gates every write, human or AI (§4.2). |
| Git-backed MVS | AI proposals land as branch/Commit like human changes. **[REV-D]** So do human-authored and document-import proposals — same mechanism, three origins (reqs §5.6). |
| `collab` | AI proposals behave as another convergent editor; validity still enforced server-side. |
| `llm-gateway` | Mode A, drafting, **[REV-D]** and the documents→draft-model pipeline (§5.14 reqs). `cem-core` decisions stay deterministic. |
| `cem-connectors` | Receives geometry/BC packages; runs external solvers; writes typed `SimulationRun`s. |
| React Flow canvas + 3D viewer | Diagram + separate geometry viewport; provenance visual language (§6.3). |

### 2.5 Architecture Decision Records (ADR log) **[REV-B §D2]**

This log is the **single source of truth for technology choices**; any conflicting mention elsewhere defers to it.

| ADR | Decision | Status |
| :--- | :--- | :--- |
| **ADR-001** | Product split: MBSE platform (P1) vs. CEM (P2) on independent tracks; Mode C flagged research-risk. | Accepted |
| **ADR-002** | **Single frontend stack: React 19 + Next.js + React Flow. SvelteFlow is not used anywhere.** All prior "SvelteFlow / React Flow" mentions are void. | Accepted |
| **ADR-003** | Polyglot persistence: Neo4j (topology) + Postgres/JSONB (bodies) + S3 (blobs). Specific vendors TBD but the split is fixed. | Accepted |
| **ADR-004** | LLM behind a provider-agnostic `llm-gateway`; local-first default, hosted optional. `cem-core` never uses an LLM for decisions. | Accepted |
| **ADR-005** | Behavioral-simulation: **ADOPT the Java fUML RI (CPL/Apache) as a JVM sidecar** for execution; **BUILD `alf-lite`**, a clean-room minimal Alf-subset compiler → fUML, for authoring; **DECLINE to link the GPL-v3 Alf RI**. See §9. | Recommended — spike to ratify subset |
| **ADR-006** | Graph is a directed property graph, not a DAG; acyclicity scoped to containment only. | Accepted |
| **ADR-007** | External-solver validation with typed result states + plausibility gate; no blind trust. | Accepted |
| **ADR-008** | **gRPC** is the standard transport for external-tool/process boundaries — the Java fUML sidecar (§9.5), the `cem-connectors` solver adapters, and **[REV-D]** the proposed `cem-archspace` sidecar (ADR-011) all use it; no mixing with bespoke REST. | Accepted |
| **ADR-009** **[REV-D]** | SysML v2 metaclass mapping for Interaction/timing modeling (FR-INTX group, reqs §5.11). Option 2 (Lifeline/Message *view* as a pure `diagram-engine` concern, decoupled from storage) ratified: messages/fragments are plain content on the existing `:Interaction`/`:InteractionFragment` elements; the Lifeline diagram is a new, storage-agnostic `InteractionPanel` renderer. | **Ratified — built, see §16.1** |
| **ADR-010** **[REV-D]** | Report-template mechanism for FR-EXPORT-03 (reqs §5.12) — generalize FR-SAFE-05's existing safety-register template pipeline rather than building a second one, mirroring the FR-PM-05/FR-CORE-16 "one mechanism, multiple origins" precedent. | Proposed |
| **ADR-011** **[REV-D]** | Mode B design-space representation (FR-ARCH group, reqs §5.17): **ADOPT `adsg-core` + `SBArchOpt`** (both MIT-licensed) behind a Python gRPC sidecar (`cem-archspace`, §2.1) as the encoder/optimizer foundation, mirroring the `fuml-runtime` sidecar pattern — rather than a ground-up Rust reimplementation. Spike (§10) confirmed both libraries compose end-to-end over the real gRPC boundary: all four design-space primitives (selection choice, connection choice, incompatibility constraint, `LINKED` choice constraint) round-trip correctly, a real Imputation Ratio (1.333 on the spike's test problem) comes back, and SBArchOpt's NSGA-II genuinely drives an adsg-core-built evaluation loop to a real optimized result. | **Ratified — spike complete, see §10** |
| **ADR-012** **[REV-D]** | Confirm `llm-gateway` as a Product-1-tier shared dependency (not CEM-exclusive, §2.1), and confirm local-Ollama-by-default satisfies a CEM-absent, Product-1-only deployment's need for the documents→draft-model pipeline (reqs §5.14). | Proposed |

*(This revision resolves a numbering collision in its source material: two independently-drafted amendments each proposed their own "ADR-011" — one for the adsg-core/SBArchOpt decision above, one for the llm-gateway confirmation now at ADR-012. See `Axioma_requirements_v5.md`'s header.)*

### 2.6 Interface Contract Schema (Mode B ↔ Mode C)

| Field | Example (turbofan HPT stage) |
| :--- | :--- |
| Performance targets | Inlet temp 1650K, pressure ratio 4.2, 12,000 RPM |
| Boundary conditions | Gas-path temp profile, centrifugal load at max RPM, vibration spectrum |
| Geometric envelope | Max outer diameter, axial length budget, hub/tip radius |
| Interface/port definitions | Disc bore diameter, blade root attachment, cooling-air feed port |
| Mass/cost targets | Stage mass ≤ 45 kg, unit cost envelope |
| Material/process constraints | Single-crystal Ni superalloy, casting or additive |

Return payload includes the governing `SimulationRun`(s) and their result states (FR-CEM-13/19).

**[REV-D] Extended worked examples — Fan & LP Compression / Core (HP) Compressor** (FR-COMP, reqs §5.15/§5.16):

| Field | Fan & LP Compression | Core (HP) Compressor |
| :--- | :--- | :--- |
| Performance targets | Design weight flow, BPR, FPR [1.1–1.8], design equivalent speed, target η_poly, high-η range = 70–105% N/√θ | Design weight flow, OPR contribution, design equivalent speed, target η_poly, high-η range |
| Off-design map | PR vs. w√θ/δ map, parametrized by N/√θ, with stall line | Same, for the core spool |
| Boundary conditions | Inlet distortion tolerance, altitude/Mach envelope, Reynolds-number floor at altitude | Combustor-inlet temperature/pressure environment |
| Geometric envelope | Fan diameter, LP-spool axial length budget, hub/tip ratio floor (~0.35) | Core diameter, HP-spool axial length budget |
| Interface/port definitions | Bypass duct port (to nozzle/mixer), LP-shaft coupling (to LP Turbine), gearbox port (if `IncludeGearbox`) | Bleed-air offtake port (location per `BleedOfftakeStage`), HP-shaft coupling (to HP Turbine), combustor-inlet port |
| Mass/cost targets | Stage/blade mass ≤ budget, unit cost envelope | Same, core-spool scope |
| Material/process constraints | Blade material vs. relative-Mach/thermal duty (FR-COMP-03 bound) | Same, higher-temperature duty than the LP spool |

---

## 3. Project Infrastructure & Dev-Ops

### 3.1 Monorepo

`pnpm` workspaces + `Turborepo` + `Biome`.

```text
axioma/
├── .devcontainer/
├── apps/
│   ├── web/                 # React 19 + Next.js + React Flow (ADR-002)
│   ├── api/                 # Rust (Axum) REST surface
│   └── docs/
├── packages/
│   ├── sysml-core/          # SysML v2 parse, KerML rules, semantic validation
│   ├── cem-core/            # Mode B: deterministic optimizer (no LLM)
│   ├── cem-geometry/        # Mode C: geometry + manufacturing constraints
│   ├── cem-connectors/      # FEA/CFD adapters, Campaigns, result-state typing
│   ├── llm-gateway/         # Pluggable LLM provider interface
│   ├── scheduler/           # Governed Campaign job queue
│   ├── fuml-runtime/        # JVM sidecar (fUML RI) exposed over gRPC — ADR-005/008
│   ├── alf-lite/            # In-house minimal Alf-subset compiler → fUML (§9.6, clean-room)
│   ├── diagram-engine/      # React Flow nodes/edges
│   ├── shared-types/        # Generated TS types from Rust structs
│   └── ui-components/       # Design system (Shadcn/Tailwind 4)
├── infrastructure/          # Terraform/K8s — provider-parameterized (NFR-COMP-01)
└── docker-compose.yml       # Local: Neo4j, Postgres, MinIO, Redis, collab, Vault
```

**[REV-D]** Not yet added: a `packages/cem-archspace/` (or equivalent) Python sidecar per ADR-011 — its exact location/packaging is part of that ADR's own ratification spike, not decided here.

### 3.2 Dev Environment

Dev Container pre-installs Rust, Node.js 24+, and DB CLIs. Local stack (Docker Compose): Neo4j (GDS), Postgres, MinIO (object store), Redis, `collab`, Vault. **The local stack mirrors the polyglot production topology** so persistence-split bugs surface in dev, not staging.

### 3.3 CI/CD & Guardrails

1. **Inner Loop:** Biome (<2 s); `cargo clippy`/`fmt`; Type-Gen (Rust→TS).
2. **Functional Validation:** OMG SysML v2 validation suite; ephemeral Neo4j+Postgres+MinIO for integration; Playwright visual regression on the canvas.
3. **Performance Gate [REV-B §C1/§C6]:** the synthetic **1M-element reference fixture** (NFR-PERF-06) runs in CI; traversal p95, canvas FPS, and persistence latency are asserted against budgets — a regression fails the build.
4. **Delivery:** Frontend to edge; backend as Distroless images to K8s (EKS / Cloud Run / on-prem, per NFR-COMP-01).
5. **Guardrails:** Canvas > 55 FPS under stress; LLM-based INCOSE PR review.

### 3.4 Deployment-Mode Readiness (NFR-COMP-01…05)

Optional modes, architected now, activated later: cloud portability (provider-parameterized Terraform), per-project region pinning, auth abstraction (OIDC broker), audit logging (Git-Sync middleware), single-tenant isolation. Default multi-tenant isolation (NFR-OPS-02) is enforced via tenancy keys in the data-access layer — specified, not implicit **[REV-B §F]**.

### 3.5 Observability **[REV-B §F]**

OpenTelemetry traces across all services; Prometheus metrics; structured logs. This is operational telemetry (NFR-OPS-01) and is explicitly distinct from the compliance audit log (NFR-COMP-04) — they have different retention, access, and purpose.

---

## 4. Implementation Plan

### 4.1 Roadmap — Two Product Tracks **[REV-B §A]**

The roadmap is no longer a single linear sequence. Product 1 ships independently; Product 2 proceeds on its own track and cannot block Product 1. **[REV-D]** Phase annotations below record where each newly-merged FR group belongs, per the amendments' own recommended placements — a sequencing suggestion, not a rewrite of the phase structure.

**Product 1 — MBSE Platform (must stand alone, no CEM):**
* **P1.1 Core Graph (Mo 1–2):** KerML/SysML v2 meta-model incl. `Hazard`/`Control`/`Mission`/`Stakeholder`; polyglot persistence wired (graph+doc+object); CRUD + semantic-validation layer; Git versioning; import (ReqIF/SysML v2). **[REV-D]** Also lands here: `:InformationElement`, `:Constraint`, `:Parameter`, `Derive`/`Copy` edges (FR-INFO, FR-PARAM's data-model half, FR-CORE-03's amended taxonomy) — pure data-model additions, cheapest before the meta-model is load-bearing elsewhere.
* **P1.2 IDE Experience (Mo 3–4):** Monaco + LSP; canvas with **viewport virtualization** (NFR-PERF-01); Hazard/Risk panel; Mission timeline; provenance visual language scaffolding. **[REV-D]** Also lands here: FR-CORE-10/11 (Dynamic Collections) and FR-CORE-12/13 (Swimlane allocation UI + Control/Object Flow validity) — both are canvas/Browser features, natural fits alongside the virtualization work already scheduled.
* **P1.3 Digital Thread (Mo 5):** budgeted traceability + change-impact; standards-aligned safety reporting. **[REV-D]** Also lands here: FR-EXPORT-01/02 (image/tabular export, a natural extension of "here's a table/matrix, now get it out of the tool") and FR-CORE-14…18 (the full documents→draft-model pipeline, filling in a line item this phase already claimed but never specified).
* **P1.4 Behavioral Simulation + Pilot (Mo 6):** fUML execution via the `fuml-runtime` sidecar (ADR-005) over gRPC; `alf-lite` minimal Alf-subset compiler (§9.6) for the pilot's behaviors; pilot on a representative model; 1M-element load fixture in CI. **[REV-D]** Also lands here: FR-INTX-01…04 (Interaction modeling — a behavioral-modeling capability, and this phase already stands up the fUML/Alf execution path it may eventually interoperate with) and FR-PARAM-01…04 (Parametrics — architecturally closest to "compute a value from model state," which this phase's Constraint/Value Property machinery already touches).

**Mode A fast-follow (after P1 stable):** grounded copilot, part search, requirement linting, docs→draft-model import. Delivery-risk. **[REV-D]** FR-EXPORT-03/04 (report generation, attachments) can land opportunistically anywhere after P1.3 — no hard phase dependency, lowest-risk/lowest-urgency of the merged groups.

**Product 2 — CEM (independent track):**
* **P2.1 Mode B (Mo 7–9):** `cem-core` deterministic optimizer; trade-study runner; Interface Contract schema. **[REV-D — re-scoping flag, still open]** This estimate was set before the FR-ARCH gap (design-space/optimizer representation, reqs §5.17) was identified. ADR-011 is now ratified (§10) — the sidecar spike is done, `adsg-core`/`SBArchOpt` are confirmed to work — but re-scoping the Mo 7–9 *estimate* itself is a separate, still-undone task: FR-ARCH-01…06 (wiring `archspace_client.rs` into a real HTTP surface, `cem-core`'s own encode/decode logic against the sidecar) and FR-COMP-01…06 (compressor-subsystem requirements, reqs §5.15/§5.16) both still belong in this phase's actual build-out. **Do not assume the original Mo 7–9 window absorbs this unchanged just because the spike is complete** — the spike de-risked *whether* the approach works, not *how long* building the real thing takes.
* **P2.2 Contract + Autonomy + Review (Mo 9–10):** proposal/branch workflow; L0–L4 autonomy with Hazard override; generative-path concurrency policy. Validate the Interface Contract manually (humans consume it) before automating Mode C. **[REV-D]** FR-ARCH-07/08 (architecture instance generation/comparison, non-convergent-evaluation handling) are a natural fit alongside the proposal/branch workflow already scheduled here; the `diagram-engine` additions for choice-node interaction/design-space-stats sidebar/instance comparison follow once the schema (§2.3) and API (§1.2a) exist, sequenced within this same window rather than a separate phase.
* **P2.3 Mode C — one subsystem (Mo 11–14, research-risk):** `cem-geometry` + `cem-connectors` against the Fan & LP Compression subsystem's casing/mounts (coldest, structural-only); one external FEA solver end-to-end; `scheduler` + typed result states live.
* **P2.4 Expand (Mo 15+):** more subsystems in increasing physics complexity (structural → aero → thermal); more solvers.

None of the Product 1 placements above are load-bearing for Product 2 — they're a sequencing suggestion, not a new dependency into the CEM track.

### 4.2 Semantic-Validation Layer (P1.1) **[REV-B §B1]**

Every write — human CRUD, CRDT-converged change, or AI proposal — passes the same `sysml-core` rule set before commit. Checks: type-legal relationship endpoints; containment acyclicity; parametric consistency; no dangling edges to deleted nodes. **[REV-D]** Extended per FR-CORE-13: an Action with no incoming/outgoing flow path is rejected ("orphan Action"), and a Decision node with more than one simultaneously-`True`-evaluable outgoing guard is rejected. A converged-but-illegal state is **quarantined and surfaced as a conflict**, never persisted to Main. Convergence (`collab`) and validity (`sysml-core`) are separate guarantees.

### 4.3 IDE, Safety & Mission (P1.2)

* Monaco + LSP; ELK auto-layout; canvas virtualization (only viewport + margin live, off-screen subsystems clustered).
* Hazard/Risk matrix panel (Severity × Likelihood, filterable); Mission timeline (Concept→Disposal).
* Custom nodes for Blocks/Ports/Requirements/Hazards/Missions; provenance chrome per §6.3.
* **[REV-D]** New canvas capabilities per §4.1's P1.2 annotation: Dynamic/Static Collections pinned into the Browser/navigation tree (FR-CORE-10/11); Swimlane/Partition allocation mode for Activity-equivalent diagrams (FR-CORE-12), a new React Flow layout mode (vertical/horizontal partitions) — no backend data-model change beyond the existing `Allocate` dependency stereotype. **Built, see impl v5 §16.3** — allocation ships as a click-to-allocate dropdown this pass, not the "drag-to-allocate headers" named here; native drag-and-drop is a separate, larger effort, not yet attempted.
* **Testing:** round-trip text↔diagram consistency (single transaction); auto-layout < 500 ms at 500 blocks; 60 FPS at 10k *visible* elements *with* virtualization active.

### 4.4 Digital Thread & Import (P1.3)

* Budgeted traceability matrix (rows/cols configurable); change-impact "blast radius" within NFR-PERF-04 limits.
* Import: ReqIF, SysML v2 API, and AI-assisted docs→draft-model (FR-CORE-07, fully specified per §1.1a/reqs §5.14).
* **[REV-D]** Export: `/export/diagram` (client-side viewport export via React Flow's own utilities; a server-side headless-render path for full-diagram export at any size, reusing the virtualization/clustering machinery in reverse) and `/export/table` (streams CSV directly, or XLSX via a lightweight server-side writer — a transform over an existing query result, no new persistence).
* **Testing:** change-impact at 1M-element scale returns a *paginated* affected-set within the endpoint's declared p95; import round-trips a reference Cameo/ReqIF export without semantic loss. **[REV-D]** Also: a document-extraction job completes successfully against a local-Ollama-only deployment with zero CEM services running (T-DOCIMPORT-06) — confirms FR-CORE-07 has no hidden Product-2 dependency.

### 4.5 Behavioral Simulation (P1.4)

* Discrete-event State Machine / Activity simulation. **Engine per ADR-005:** fUML execution is *adopted* (`fuml-runtime`, Java RI as a gRPC sidecar); Alf *authoring* is *built in-house* as `alf-lite` — a minimal, clean-room Alf-subset compiler targeting the same fUML (§9.6), scoped to the pilot's constructs and grown only on demand **[REV-B §D3]**.
* Interactive player, debugger, dashboards (time-series store).
* **[REV-D]** Interaction/timing modeling (FR-INTX, ADR-009 ratified — built, see impl v5 §16.1) and Parametrics evaluation (FR-PARAM, reqs §5.9) — the latter is a **pure synchronous computation**, must never dispatch to `cem-core`/`cem-connectors`/`scheduler` (verified via trace: zero spans from those services on a Parametric evaluation call).
* **Testing:** deterministic replay (100 identical runs identical); 1M-element load fixture green in CI.

### 4.6 Mode C + Connector Framework (P2.3)

* `cem-geometry`: solid-modeling + manufacturing constraints, gated by the Interface Contract.
* `cem-connectors`: solver-neutral job package → adapter → external solver; result typed (FR-CEM-13) and plausibility-checked before any graph write; metrics land as `SimulationRun` with `validatedBy` edge; large files to object store by pointer.
* `scheduler`: Campaigns dispatched with concurrency/quota/cost ceilings; L4 loops cannot exceed budget (NFR-PERF-05).
* **First target:** Fan & LP Compression casing/mount — structural FEA only, one solver, full pipeline end-to-end before adding complexity.
* **Testing:** connector round-trip creates a correctly-typed `SimulationRun`; a `Diverged`/`Timeout` result is *not* auto-merged even at L4; Campaign of 5+ variants ranks correctly against contract targets; budget-exceeding Campaign is refused.

---

## 5. Testing Scenarios & QA (Platform-Wide)

| Scenario | Asserts | Traces to |
| :--- | :--- | :--- |
| **Perf Stress** | 50k blocks / 100k rels: load < 3 s, pan/zoom > 50 FPS, search < 100 ms | NFR-PERF-01/02 |
| **1M Load Fixture (CI)** | Traversal p95, canvas FPS, persistence latency within budget | NFR-PERF-03/04/06 |
| **Query Budget** | An unbounded traversal request is rejected; a deep query paginates | NFR-PERF-04 |
| **Convergence vs. Validity** | Two edits producing an illegal converged state → quarantined, not persisted | FR-CORE-05/06, NFR-REL-01 |
| **Cyclic Traversal** | Traceability across a known cycle terminates (visited-set), no infinite loop | NFR-REL-02 |
| **Solver Result Typing** | A `Diverged` solver run drops the item to review even at L4 | FR-CEM-13, NFR-REL-03 |
| **Plausibility Gate** | A physically-impossible "pass" (negative safety factor) is caught pre-write | FR-CEM-13 |
| **LLM Provenance** | Every AI-generated element carries model/version/prompt-hash/seed | FR-CEM-05, NFR-CEM-02 |
| **Campaign Governance** | Budget-less Campaign refused; over-budget L4 loop halted | NFR-PERF-05 |
| **Generative Concurrency** | Human edit beats in-flight Mode C write; work re-queued | NFR-OPS-04, §5.7 (reqs) |
| **Autonomy Enforcement** | L3 over-threshold change drops to review; L4 Hazard-linked forced to review | FR-CEM-16/17/18 |
| **Autonomy Audit** | Autonomy-level change logged (actor/time/old/new) | NFR-CEM-06 |
| **Hazard Register** | Every Hazard has ≥1 Control before "Safety Reviewed" | FR-SAFE-01/03 |
| **Mission Traceability** | Every top-level Requirement traces to a Mission/UseCase; orphans flagged | FR-MSN-04 |
| **Alf Subset Conformance** | Each `alf-lite` construct: source → fUML → `fuml-runtime` trace matches golden; out-of-subset construct gives a precise compile error | FR-CORE-09, §9.6 |
| **Import Fidelity** | Cameo/ReqIF round-trip without semantic loss | FR-CORE-07 |
| **Deployment-Mode Switch** | EU residency / single-tenant toggled by config, no migration | NFR-COMP-01/02/05 |
| **Schema Migration** | A node-schema version bump migrates a large fixture without data loss | NFR-REL-05 |
| **Backup/Restore** | Restore to stated RPO/RTO from backup | NFR-REL-04 |
| **Multi-tenant Isolation** | Project A cannot read Project B in the shared deployment | NFR-OPS-02 |
| **Observability** | A cross-service request produces a single correlated trace | NFR-OPS-01 |
| **[REV-D] Parametric Evaluation Isolation** | A Constraint evaluation never dispatches to `cem-core`/`cem-connectors`/`scheduler` | FR-PARAM-03 |
| **[REV-D] Document-Import Product-2 Independence** | The documents→draft-model pipeline completes with zero CEM services running | FR-CORE-14, NFR-CEM-03 |
| **[REV-D] Swimlane Object-Flow Validity** | An orphan Action (no in/out flow) is rejected | FR-CORE-12/13 |
| **[REV-D] Dynamic Collection Live Re-evaluation** | A newly-matching element appears in a saved Dynamic Query without manual action | FR-CORE-10 |
| **[REV-D] Requirements Dependency Taxonomy** | `Derive`/`Copy` edges are queryable and distinguishable from `contains` | FR-CORE-03 (amended) |

**[REV-D] Note:** the full per-scenario test rows for the above (`T-PARAM-*`, `T-INFO-*`, `T-INTX-*`, `T-EXPORT-*`, `T-CORE-10/12-*`, `T-CORE-03-EXT`, `T-DOCIMPORT-01…07`) are now in `Axioma_test_specification_v4.md`. **FR-COMP-01…06 and FR-ARCH-01…08 currently have no concrete test-spec rows anywhere** — the turbofan system-model amendment that introduced them does not specify test scenarios the way the other two merged amendments do. Authoring those is Phase 6 work (`docs/IMPLEMENTATION_KICKOFF.md`), not done in this doc-consolidation pass.

---

## 6. Usability & Design

### 6.1 Primary Task-Flow **[REV-B §E1]**

The canonical engineer workflow the UI is designed around, with the surface transitions treated as first-class features:

1. **Frame** — define/import Missions & Requirements (Mission timeline + text editor).
2. **Architect** — decompose into subsystems on the canvas; selecting a Block *simultaneously* highlights its text, shows its Hazards in the inspector, and reveals its traceability — one action, not four lookups.
3. **Assess safety** — hazards surface inline on the affected Block, not in a disconnected register.
4. **Optimize (Mode B)** — run a trade study; candidate architectures appear as reviewable proposals with provenance chrome.
5. **Realize (Mode C)** — for a chosen subsystem, generate geometry; the 3D viewer opens beside the diagram, results carry solver provenance.
6. **Verify** — Campaign results compare against contract targets; `Suspect` flags propagate visibly upstream.

The point: these are wired transitions, not seven independent panels a user must manually reconcile.

### 6.2 "Structural Clarity" Design System **[Corporate identity update — supersedes the earlier placeholder "Obsidian & Neon" system]**

The platform's visual identity is now the one actually adopted and shipped (Axioma_design_philosophy.md, and live at axioma.systems): a graph-theory-literal mark — five nodes resolving into the letter A, bound by edges, with a crossing diagonal — and a deliberately restrained two-color brand palette. Any earlier reference to Indigo/Teal/Violet/Coral tokens elsewhere in this document set is void.

**Brand palette** (measured directly from the adopted logomark, not invented):

| Role | Value | Usage |
| :--- | :--- | :--- |
| Obsidian (ground) | `#07070C` | Primary background |
| Graphite (secondary) | `#7C7C86` (logo-measured: `#787878`) | Supporting structure, borders, secondary text |
| Cobalt — true brand | `#052583` (logo-measured: `#042582`) | Logo, nav wordmark accent, identity marks **only** |
| Cobalt-glow — UI accent | `#3A5BFF` | Interactive elements: buttons, links, hover states, background node-lattice — the true cobalt above is too low-contrast for interactive affordances against the obsidian ground |
| Paper (inverse contexts) | `#F3F4F8` | Light-mode surfaces where used |

**Logomark:** the five-node A-lattice (apex node; two mid-nodes bridged horizontally; two base nodes; crossing diagonal) is the adopted brand mark — white/graphite on obsidian, or cobalt/graphite on paper. Full rationale in Axioma_design_philosophy.md ("Structural Clarity").

**Typography:** Space Grotesk (UI, wide letterspacing on the wordform) + JetBrains Mono (technical annotation — IDs, provenance, telemetry labels). Supersedes the earlier placeholder "Inter."

**A deliberate scope boundary:** the two-color discipline above is a *brand-identity* rule (marketing surfaces, logo, nav) — the product UI's functional status system (§6.3) is a separate semantic layer and is not required to stay within it, for the reasons given there.

4px grid. Glassmorphic panels; floating action dock; Autonomy selector (L0–L4) in the dock with a lock glyph on Hazard-overridden elements.

### 6.3 Provenance & Confidence Visual Language **[REV-B §E2]**

Beyond a single accent color, every element renders three orthogonal signals, filterable graph-wide:

| Dimension | States | Encoding |
| :--- | :--- | :--- |
| **Origin** | human / AI-suggested / AI-auto-merged | Border style (solid / dashed / dashed-glow), using Graphite → Cobalt-glow |
| **Validation** | unverified / solver-validated / test-validated | Corner badge (none / Cobalt-glow check / Paper check) |
| **Staleness** | current / `Suspect` | Warm alert-red pulse overlay (`#FF5C5C`) when Suspect |

**Why Suspect gets a color outside the brand palette:** §6.2's two-color brand discipline (Cobalt + Graphite) is a marketing/identity rule. Reusing Cobalt-glow for both "validated" and "Suspect" would mean the same hue signals both "confirmed good" and "needs review" — acceptable friction on a marketing page, not acceptable in a safety-critical engineering tool where FR-CEM-13 and NFR-REL-03 depend on a reviewer distinguishing these states at a glance. The functional status layer is intentionally a separate semantic system from the brand palette, not an oversight.

Filter examples: "everything auto-merged at L4 not yet human-reviewed"; "all `Suspect` elements downstream of Requirement X." This is a trust/safety feature as much as a usability one.

### 6.4 Component Example (React Flow node)

```tsx
// Tailwind config maps: cobalt-glow -> #3A5BFF, graphite -> #7C7C86,
// obsidian -> #07070C, alert -> #FF5C5C (functional layer, see §6.3)
export function AxiomaBlockNode({ data }) {
  return (
    <div className={`rounded-xl border bg-obsidian/80 p-0 shadow-2xl backdrop-blur-md
      ${data.origin === "human" ? "border-white/10"
        : data.origin === "ai-suggested" ? "border-dashed border-cobalt-glow/40"
        : "border-dashed border-cobalt-glow/70 shadow-cobalt-glow/20"}`}>
      <div className="flex items-center gap-2 border-b border-white/5 p-3 bg-cobalt-glow/10 rounded-t-xl">
        <BoxIcon className="w-4 h-4 text-cobalt-glow" />
        <span className="text-sm font-semibold text-white/90">{data.label}</span>
        {data.validation === "solver-validated" && <CheckIcon className="w-3 h-3 text-cobalt-glow ml-auto" />}
        {data.suspect && <span className="ml-auto w-2 h-2 rounded-full bg-alert animate-pulse" />}
      </div>
      <div className="p-3 space-y-1">
        <p className="text-[10px] uppercase tracking-widest text-white/40 mb-1">Properties</p>
        {data.properties.map(p => (
          <div key={p.id} className="text-xs text-white/70 font-mono">
            {p.name}: <span className="text-graphite">{p.type}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
```

---

## 7. Requirements Traceability Matrix **[REV-B §D1]**

Each requirement links to its design section, test scenario, and implementing module. Maintained as the authoritative cross-reference; abbreviated here.

| Requirement | Design | Test (§5 row) | Module |
| :--- | :--- | :--- | :--- |
| FR-CORE-01 | impl §1 | Import Fidelity | `sysml-core`, `api` |
| FR-CORE-05/06 | reqs §5.1 | Convergence vs. Validity | `sysml-core`, `collab` |
| FR-CORE-07 | impl §4.4 | Import Fidelity | `api` (import) |
| FR-CORE-08 | reqs §6.2 | LLM Provenance | `api`, `web` |
| FR-CORE-09 | impl §9.6 | Alf Subset Conformance | `alf-lite`, `fuml-runtime` |
| **FR-CORE-10/11** | reqs §5.13 | Dynamic Collection Live Re-evaluation | `api`, `web` |
| **FR-CORE-12/13** | reqs §5.11, impl §4.3 | Swimlane Object-Flow Validity | `sysml-core`, `diagram-engine` |
| **FR-CORE-14…18** | reqs §5.14 | Document-Import Product-2 Independence | `api`, `llm-gateway`, `sysml-core` |
| FR-CEM-02 | reqs §5.2 | (Mode B trade study) | `cem-core` |
| FR-CEM-03/13 | reqs §5.4 | Solver Result Typing, Plausibility Gate | `cem-connectors` |
| FR-CEM-05 | reqs §5.2 | LLM Provenance | `llm-gateway` |
| FR-CEM-14/15 | reqs §5.4 | Campaign Governance | `cem-connectors`, `scheduler` |
| FR-CEM-16/17/18 | reqs §5.6 | Autonomy Enforcement | `api`, `cem-core` |
| **FR-PARAM-01…04** | reqs §5.9 | Parametric Evaluation Isolation | `sysml-core`, `api` |
| **FR-INFO-01…04** | reqs §5.10 | *(T-INFO-01/02, test spec v4)* | `sysml-core`, `api` |
| **FR-INTX-01…04** | reqs §5.11 | *(T-INTX-01/02, test spec v4)* | `diagram-engine`, `api` |
| **FR-EXPORT-01…04** | reqs §5.12 | *(T-EXPORT-01/02/03, test spec v4)* | `api`, `web` |
| **FR-COMP-01…06** | reqs §5.15, §5.16 | *not yet specified — Phase 6* | `sysml-core`, `cem-core` |
| **FR-ARCH-01…08** | reqs §5.17 | *not yet specified — Phase 6* | `cem-core`, `cem-archspace` (proposed) |
| FR-SAFE-01…05 | impl §4.3/4.4 | Hazard Register | `sysml-core`, `api` |
| FR-MSN-01…04 | impl §4.3 | Mission Traceability | `sysml-core`, `api` |
| FR-PM-01…05 | reqs §5.8 | Stage Rollup, Computed Progress, Testing Derivation, Unified Review Gate | `sysml-core`, `api`, `cem-connectors` |
| NFR-PERF-01…06 | impl §3.3, §4.3 | Perf Stress, 1M Load Fixture, Query Budget | `web`, `api`, Neo4j |
| NFR-REL-01…05 | reqs §5.1, §3.2 | Convergence, Cyclic Traversal, Schema Migration, Backup | multiple |
| NFR-OPS-01…04 | impl §3.5, §3.4 | Observability, Multi-tenant Isolation, Generative Concurrency | all services |
| NFR-COMP-01…05 | impl §3.4 | Deployment-Mode Switch | `infrastructure`, `auth` |

---

## 8. Team Onboarding (P1.1)

* **Prerequisites:** Docker Desktop, VS Code + Remote-Containers, `pnpm`.
* **One-command start:** `git clone … && cd core && code .` → "Reopen in Container".
* **Local stack mirrors production topology:** Neo4j + Postgres + MinIO + Redis + `collab` + Vault.
* **Definition of Done (P1.1):** p95 element-create < 100 ms; ≥ 85% Rust unit coverage; OpenAPI docs complete; 10k-node import leak-free; semantic-validation layer rejects a crafted illegal state in an integration test.

| Task | Component | Description |
| :--- | :--- | :--- |
| AX-101 | `api` | `/healthz`, `/readyz`, `/metrics` |
| AX-102 | `sysml-core` | KerML constraints + semantic-validation rule set |
| AX-103 | `mvs` | Git repo per Model Project |
| AX-104 | `auth` | OIDC behind provider-agnostic abstraction (NFR-COMP-03) |
| AX-105 | `persistence` | Polyglot wiring: graph↔doc↔object with pointer references (ADR-003) |
| AX-106 | `platform` | OpenTelemetry + Prometheus baseline across services (NFR-OPS-01) |

---

## 9. ADR-005 Resolution Input: Open-Source f-UML/Alf Engines

This appendix records the survey that ADR-005 (§2.5) called for. It is decision *input*, not yet a final decision — but it materially narrows the options.

### 9.1 What exists

| Component | Project | Language | Latest | License | Notes |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **fUML engine** | ModelDriven fUML Reference Implementation | Java (Java 8 only; not JDK 9+) | 1.4.1 on Maven Central (conforms to fUML 1.3) | **CPL 1.0 + Apache 2.0** (dual) | Explicitly designed to be *used as a library* in other software; takes XMI in, produces an execution trace. Originally Lockheed Martin-funded, maintained by Model Driven Solutions. |
| **Alf compiler + engine** | ModelDriven Alf Reference Implementation | Java | 1.1-conformant | **GPL v3** | Compiles Alf text → fUML, targeting either the fUML RI engine or Papyrus/Moka. Handles full Alf at Extended conformance. |
| **fUML engine (alt.)** | Eclipse Papyrus **Moka** | Java (Eclipse/EMF) | v3.1.x | **EPL** | Execution engine inside Papyrus; the Alf RI can target it. Heavy Eclipse/EMF dependency. |
| **fUML engine (alt.)** | Model-driven C++ fUML engine (Ilmenau/academic) | C++ | research | varies/unclear | Better memory/perf than the Java RI in some conditions; not a maintained product. |

### 9.2 The two findings that actually drive the decision

1. **Language mismatch.** All the maintained engines are **Java** (Moka is Java/Eclipse; the RI is Java 8). Axioma's backend is Rust. There is no maintained, production-grade Rust f-UML/Alf engine. Embedding a Java engine means either a JVM sidecar service (clean process boundary, IPC/REST cost, extra runtime to operate) or JNI-style bridging (tighter, far more fragile). The sidecar is the only sane option and fits the service topology (§2.1) — it becomes another backing service like a solver behind `cem-connectors`. **[REV-D]** ADR-011's `cem-archspace` sidecar (§2.1) is the same shape, one abstraction level later — Python this time instead of Java, but the same "wrap the external tool as a sidecar over gRPC" resolution.

2. **License split is the real gate.** The **fUML RI is CPL-1.0 + Apache-2.0** — permissive/weak-copyleft, usable as a library in a commercial/proprietary product with attention to CPL's terms. But the **Alf RI is GPL v3**, which is a genuine problem for Axioma's proprietary, on-prem-distributed posture (§3.4): linking GPL-v3 code into distributed software imposes copyleft obligations the business model almost certainly cannot accept. **The fUML layer is adoptable; the Alf-text layer is not, at least not by linking.**

### 9.3 Recommended shape for ADR-005

Given the above, the pragmatic decision is a **split**, not an all-or-nothing build:

- **Adopt for fUML execution:** wrap the fUML Reference Implementation (CPL/Apache) as a **JVM sidecar service** — call it `fuml-runtime` — that Axioma's Rust backend drives over a thin RPC boundary, exactly like an external solver. This gets standards-conformant behavioral execution *without* building an execution engine from scratch, and the permissive license clears commercial use. Pin to the Maven Central artifact (`org.modeldriven:fuml`).
- **Do NOT link the Alf Reference Implementation (GPL v3).** Its copyleft is incompatible with Axioma's proprietary, on-prem-distributed posture (§3.4).
- **DECISION: write a minimal in-house Alf subset (`alf-lite`), scoped to only the constructs the pilot needs.** Rather than deferring Alf entirely or shipping the GPL RI as a bolt-on tool, Axioma implements a small, clean-room Alf-subset compiler in Rust that emits the **same fUML** the `fuml-runtime` sidecar already executes (§9.6). This keeps Alf-text authoring inside the product, under a license Axioma controls, with no GPL exposure and no second execution path — `alf-lite` compiles to fUML; `fuml-runtime` runs it. Scope is deliberately minimal (see §9.6) and grows only as real pilot usage demands, exactly the discipline the review urged for build decisions.
- **Keep Moka in reserve** as an alternate fUML backend if the RI proves too limited, but note its heavy Eclipse/EMF footprint makes it a poorer sidecar than the standalone RI.

### 9.4 Consequences for the spec

- FR-CORE-04 ("Behavioral Simulation") is satisfiable by **adopting** (fUML RI sidecar), not building — this removes the largest hidden build-cost flagged in the review (§D3) from the Product 1 critical path.
- The earlier assumption of a "Rust f-UML/Alf interpreter" (inherited from v1/v2 text) is **withdrawn**: there is no maintained Rust engine to adopt, and re-implementing the Java RI in Rust is exactly the kind of non-differentiating multi-year effort the review warned against. The sidecar approach is preferred.
- A new backing service, `fuml-runtime` (JVM), joins the service topology (§2.1). It carries a JVM/Java-8 operational dependency — noted as a tradeoff, but isolated behind a process boundary so it does not contaminate the Rust build or the rest of the stack.
- A new in-house component, **`alf-lite`** (Rust), joins the workspace: a minimal Alf-subset compiler targeting fUML (§9.6, FR-CORE-09). It is clean-room — it reads the public OMG Alf spec, it does **not** derive from or link the GPL Alf RI — so its provenance must be documented for the eventual compliance/audit trail (NFR-COMP-04).
- **ADR-005 status → recommended: ADOPT fUML RI as a JVM sidecar for execution; BUILD `alf-lite`, a minimal in-house Alf-subset compiler, for authoring; DECLINE to link the GPL Alf RI.** Final ratification pending a short spike to confirm the RI handles the pilot's State Machine constructs and to measure sidecar RPC overhead against NFR-CEM-01 latency.

### 9.5 Transport for the sidecar: gRPC

The Rust-backend ↔ Java-fUML-sidecar boundary is a **process boundary regardless** (the GPL isolation and JVM runtime reasons force it), so the only question is which RPC mechanism crosses it. **gRPC is the recommended transport**, for reasons specific to this boundary:

- **Mature on both sides.** Rust via `tonic` (with `tonic-build` for codegen) and Java via official gRPC support; neither side is a second-class citizen. **[REV-D]** Python (for ADR-011's `cem-archspace`) is equally mature on gRPC's side, via the standard `grpcio`/`grpc.aio` bindings — the same transport choice generalizes cleanly to a third language, not just the two already in play.
- **Single versioned contract.** One `.proto` is the source of truth for the boundary — matching how the platform already treats the Interface Contract and solver boundaries. Protobuf's field-numbering/back-compat rules let the fUML runtime (on its own upstream release cadence) and the Rust backend evolve semi-independently.
- **Streaming fits the payload.** A behavioral simulation emits an *execution trace* — a time-ordered sequence of state transitions / token flows — not a single value. gRPC **server-streaming** maps directly onto that and onto the interactive player (Play/Pause/Step) and timeline view in P1.4. Plain request/response REST would force either a block-until-done call or a hand-rolled polling protocol.

**Caveats, recorded honestly:**
- gRPC solves *transport and typing*, not *semantics*. The fUML RI ingests XMI and emits a Java object graph; a decision remains on what crosses the wire — wrap XMI as an opaque `bytes` payload (simplest; Rust cannot introspect it) vs. define protobuf messages mirroring the exchanged fUML subset (cleaner, more work). This is the same "design the contract" work `cem-connectors` faces with solver I/O, and that ADR-011's design-space sidecar will face too (functions/choices/constraints, not a single opaque blob, per reqs §5.17).
- **Use one RPC convention for every external-tool boundary.** `cem-connectors` (solvers) crosses a comparable boundary; standardize on gRPC there too rather than mixing gRPC for fUML with bespoke REST for solvers. Recorded as **ADR-008**. **[REV-D]** ADR-011's `cem-archspace` sidecar follows the same standardization.
- If the boundary were in-process, gRPC would be the wrong tool (JNI would) — but JNI was already ruled out as too fragile (§9.2), so this does not apply.

### 9.6 `alf-lite` — the minimal in-house Alf subset

`alf-lite` is a small, clean-room Rust compiler for a **deliberately restricted subset** of the OMG Alf language, emitting the same fUML that `fuml-runtime` executes. It is written against the published Alf specification and shares no code with the GPL-v3 Alf Reference Implementation.

**Design rules (to keep it "minimal" and prevent scope creep):**
- **Spec-driven scope, pilot-gated growth.** Only constructs exercised by the pilot's State Machine / Activity behaviors are implemented. A construct is added *when a pilot model needs it*, not speculatively. The supported-subset list is a living, versioned document, not "eventually all of Alf."
- **Compile-to-fUML, never a second runtime.** `alf-lite` produces fUML that `fuml-runtime` runs. There is exactly one execution path; `alf-lite` is a front-end only. This avoids the classic trap of a home-grown language accreting its own divergent interpreter.
- **Clean-room provenance.** Derived solely from the public OMG Alf spec; no inspection or reuse of the GPL RI's source. Provenance recorded for audit (NFR-COMP-04).
- **Explicit "unsupported" behavior.** Any Alf construct outside the subset is a clear compile-time error naming the unsupported feature — never a silent partial compile that would produce a subtly wrong model.

**Initial target subset (indicative, ratified during the P1.4 spike):** value specifications and local names; feature/property access; basic arithmetic/boolean/comparison operators; `if`/`else` and simple loops; behavior (activity) invocation; signal send/accept for State Machine transitions. **Explicitly out of initial scope:** collection/sequence expression operators, the full standard model library, generics/templates, and the advanced multiplicity/typing rules of Extended-conformance Alf — any of which is added only if a pilot model requires it.

**Testing:**
- *Subset conformance:* each supported construct has a golden test — Alf source → expected fUML → executed by `fuml-runtime` → expected trace.
- *Unsupported-construct safety:* an out-of-subset construct yields a precise compile error identifying the feature, never a partial/incorrect compile.
- *Round-trip with the sidecar:* an `alf-lite`-compiled behavior and the same behavior authored directly as fUML produce identical execution traces (confirms `alf-lite` is a faithful front-end, not a divergent semantics).

---

## 10. ADR-011 Resolution: `cem-archspace` Spike Findings **[REV-D]**

`docs/IMPLEMENTATION_KICKOFF.md` Phase 2 called for a spike proving `adsg-core`/`SBArchOpt` can be
wrapped as a gRPC sidecar (`cem-archspace`, `packages/cem-archspace/`) and round-trip a small
synthetic design space — the same "spike to ratify" shape ADR-005 used for fUML/Alf (§9). This
records what was actually built and verified, not what was planned; see
`packages/cem-archspace/README.md` for the full detail this section summarizes.

### 10.1 What was proven, with real numbers

Every claim below was exercised twice: first directly against the installed libraries in a Python
REPL, then a second time over the real gRPC wire against the running sidecar (and a third time
from a Rust integration test, `apps/api/src/main.rs`'s
`archspace_design_space_round_trips_through_the_sidecar`, against the Dockerized service).

- **All four design-space primitives round-trip correctly**: a selection choice, a connection
  choice, an incompatibility constraint, and a `LINKED` choice constraint — using the exact test
  problem `Axioma_sysml_tool_landscape_evaluation.md` itself suggested (Core (HP) Compressor /
  Turbine stage-count and bleed-offtake choices, reqs v5 §5.16, FR-COMP-04).
- **The `LINKED` constraint does exactly what FR-COMP-04 needs**: two linked design-variable nodes
  collapse into one shared search axis (`GraphProcessor.des_vars` reports 4, not 5, for a
  definition with 2 linked DVs + 3 selection choices) — Mode B only ever has to search one stage-
  count axis for a linked spool, not two independently.
- **A real Imputation Ratio**: `1.333...` (32 declared, 24 valid) on this spike's design space —
  FR-ARCH-06's IR half genuinely computed, not asserted. Correction Ratio/Correction Fraction/Max
  Rate Diversity are real `adsg-core` concepts too, just not wired into this pass's minimal
  `DesignSpaceStats` message (see §10.3).
- **`DecodeInstance` returns a real, internally-consistent architecture instance** — a design
  vector, an activeness mask, and the actual resolved node names, not merely "no error."
  Confirms FR-ARCH-05's decode half.
- **SBArchOpt genuinely drives adsg-core's evaluation loop** — wrapping a `DSGEvaluator` in
  `adsg_core.optimization.problem.DSGArchOptProblem` and running SBArchOpt's own
  `get_nsga2()` wrapper for 3 generations converged to a best objective of `1.0688` against a
  placeholder "minimize stage count" objective whose true minimum is `1.0`. This is ADR-011's
  *other* half — SBArchOpt actually consumes an adsg-core-built problem, not just "the two
  libraries are compatible on paper."
- **Errors surface loudly, not silently**: a bogus design-space handle returns gRPC `NOT_FOUND`;
  `adsg-core` itself rejecting a malformed definition (e.g. a name referenced before it's declared)
  surfaces as `INVALID_ARGUMENT` — same "reject, don't silently accept" discipline `sysml-core`'s
  own semantic-validation layer already follows.

### 10.2 A real, non-obvious finding about `adsg-core`'s actual API

**There is no literal `FUN`/`COMP`/`MULTI`/`NOF`/`DE`/`CON` class hierarchy in `adsg-core`** —
confirmed by reading `adsg_core/graph/adsg_nodes.py` directly. The library's real node vocabulary
is `NamedNode`/`ConnectorNode`/`DesignVariableNode`/`MetricNode`; that FUN/COMP/etc. vocabulary is
Bussemaker's thesis terminology for graph *topology* (derivation edges, choices), not distinct
Python types. This **validates, rather than contradicts**, reqs v5 §5.17's own design (a
fulfillment-mechanism *tag* on `:Function`↔`:Structure` edges, not a type hierarchy) — it was the
right call going in, not a gap this spike needed to fix.

Also found only by building something real, not documented anywhere in the public guide/API-
reference pages: a selection choice's option nodes (and a connection choice's connectors) must be
derived **only** through the choice/connection call itself — pre-wiring them with a plain
derivation edge first, then also passing them as choice options, makes `GraphProcessor` reject the
whole graph as infeasible.

### 10.3 What's still open (not resolved by this spike, on purpose)

- **Native storage vs. sidecar-owned working representation** — reqs v5 §5.17 and impl v5 §2.3
  both flagged this as an open question before this spike; it's still open after it. This spike's
  `cem-archspace` holds every `BasicDSG` in-memory only, keyed by a process-lifetime handle, with
  no persistence and no sync back into Axioma's own Neo4j graph. **Recommendation based on what
  actually got built**: keep the sidecar as the working representation for active design-space
  editing/optimization (matching how `fuml-runtime` never persists execution state either), and
  only materialize a *resolved architecture instance* (FR-ARCH-07, a chosen candidate) into
  Axioma's graph as a `:Structure` subgraph — mirroring how Mode B's existing `accept`/`propose`
  flow (`mode_b.rs`) already treats an accepted candidate, not a live search state, as the thing
  that gets written to `main`. The unresolved *search space itself* stays sidecar-side.
- **Correction Ratio / Correction Fraction / Max Rate Diversity** (the other three of FR-ARCH-06's
  four health metrics) are real `adsg-core` capability (behind `GraphProcessor.get_statistics()`'s
  richer, `pandas`-DataFrame-shaped output) not yet mapped into `DesignSpaceStats`'s proto fields.
- **Multi-objective/constraint metrics, hierarchical-BO (`ArchSBO`) instead of plain NSGA-II,
  Probability-of-Viability hidden-constraint handling (FR-ARCH-08's fuller form)** — all real
  SBArchOpt/adsg-core capability this spike's single-objective NSGA-II smoke test didn't need to
  exercise to prove the pipeline works.
- **Not wired into Mode B's real `optimize`/`propose` flow** — `cem-core` remains untouched by
  this pass, deliberately: it stays "pure computation, no I/O" (its own README's existing claim).
  Wiring `archspace_client.rs` into a real `/cem/mode-b/design-space/*` HTTP surface (§1.2a) and
  building `cem-core`'s own encode/decode logic against it is P2.1 proper's job, now re-scoped
  (impl §4.1) on the strength of this ratified ADR rather than an unresolved "Proposed" one.

---

## 11. Phase 3: FR-COMP Content Landed **[REV-D]**

`docs/IMPLEMENTATION_KICKOFF.md` Phase 3 called for landing FR-COMP-01…06 as actual content
against `Turbofan-Ref`, plus the Interface Contract worked examples. Recorded here is what was
actually seeded and verified, not what was planned.

### 11.1 What was seeded, with real element ids

`apps/api/src/main.rs::seed_turbofan_ref` now calls a new `seed_fr_comp_content` step (part of the
same genesis commit, so it's covered by that function's own existing diff-accuracy discipline) that
creates, for each of `FanLpCompression`/`CoreHpCompressor`:

- **FR-COMP-01** — a real `:Requirement` (`REQ-FAN-SPEC`/`REQ-CORE-SPEC`), `Satisfy`-linked from
  its subsystem, carrying the 9-field structured spec as body properties. Numeric values reuse
  `cem-core`'s own reference constants where they overlap (bypass ratio = 5.0, matching
  `cem_core::REFERENCE_BYPASS_RATIO`) — duplicated as a literal rather than exposing that constant
  publicly, since `cem-core` deliberately stays a zero-I/O, self-contained crate.
- **FR-COMP-02** — the **first-ever real instantiation** of Phase 1's `:Constraint`/`:Parameter`
  `NodeKind`s: one `:Constraint` per subsystem (`FanPerformanceMapConstraint`/
  `CorePerformanceMapConstraint`) carrying an explicitly-flagged-illustrative sampled-points
  property (`sourceNote: "illustrative shape only -- real constitutive equations not yet sourced"`
  — reqs v5 §5.15 itself says the real off-design equations aren't sourced yet, so this doesn't
  pretend otherwise), plus two `:Parameter` elements per subsystem (equivalent weight flow, speed),
  each `Bound`-edged `Parameter --Bound--> subsystem` (the real `EdgeKind::Bound` endpoint rule:
  source must be a `Parameter`). The Constraint's body lists which Parameter ids it uses as a plain
  JSON array — there's no dedicated "constraint uses parameter" edge in the Phase 1 schema, and
  inventing one mid-content-authoring would be designing schema outside its own phase; flagged as a
  real, honest gap for whenever Parametrics evaluation (§4.1, Phase 5) actually gets built.
- **FR-COMP-03** — landed as a **pure, tested `sysml-core` function only, not wired into any HTTP
  endpoint this pass**: `sysml_core::check_compressor_blade_loading` (+ a new
  `ValidationError::CompressorLoadingOutOfBounds` variant), exercising the exact thresholds reqs v5
  already cites (diffusion factor > 0.4 needs override; relative Mach > 1.35 is never accepted even
  with one; 1.2 < Mach ≤ 1.35 needs override). Confirmed via 4 unit tests covering each boundary.
  **Why not wired in yet**: the only generic body-mutation endpoint (`PATCH
  .../elements/:id/body`) validates nothing kind-specific for *any* element kind today — Hazard
  severity, Stage-Tracking status, etc. are all unvalidated JSONB-bag conventions. Bolting a
  diffusion-factor/Mach check onto that one endpoint would make it inconsistently stricter than
  every other kind-specific property already flowing through it. `traceability.rs`'s
  `?acknowledge=true`/`409` pattern is the existing human-override REST convention, noted here for
  whenever a real kind-specific validation calling convention gets built (Phase 5 API-surface work,
  §1.2a) — matching how P2.2's own `autonomy::decide` stayed pure-and-tested before `mode_b.rs`
  wired it in once a real caller existed.
- **FR-COMP-05** — two real `:Port` elements per subsystem (Fan: station 1 inlet / station 2 exit;
  Core: station 2 inlet / station 3 exit — station 2 legitimately shared between them, a real
  system-model detail per §5.16's convention, not an inconsistency), `Contains`-edged from their
  subsystem, carrying equivalent weight flow/speed (and `bleedFractionB` on Core's exit port only,
  per the reconciliation table's bleed-origin assignment) as body properties.
- **FR-COMP-06** — landed as a body-property convention only (`negotiable: true`, `flagged: false`
  on the two spec Requirements) — a real automatic incompatibility *detector* needs formulas this
  codebase doesn't have yet; this is the hook a future real check sets, not an active check itself.
- **Interface Contract worked examples** (§2.6's table) merged (read-then-write,
  `mode_b.rs::upsert_subsystem_contract`'s exact pattern, so this never clobbers or is clobbered by
  a real Mode B `accept` run touching the same subsystem body later) onto `FanLpCompression`/
  `CoreHpCompressor`'s existing bodies using `cem_core::build_interface_contract`'s real six
  camelCase keys, tagged `specProvenance: "docs-worked-example"` so it's never confused with a real
  run's `modeBProvenance` tag.

**FR-COMP-04 (stage-count consistency) is explicitly deferred to Phase 4** — it needs Turbine-side
content (stage-count Parameters, a `ChoiceConstraint` edge) outside this phase's compressor-only
scope; a lopsided compressor-only half of a cross-subsystem constraint would be worse than clearly
deferring it.

### 11.2 A real, pre-existing bug found and fixed while seeding this content

`apps/api/src/store/neo4j.rs`'s `row_to_element` resolved an element's `NodeKind` by scanning
`labels(n)` and returning the first label `NodeKind::from_label` recognized. **Neo4j's `labels(n)`
order is not the `MERGE` clause's label order** — it's determined by each label's internal token
id, which depends on which label was first created *anywhere in the database*, not per-node. Since
every element carries both the generic `:Element` label and its specific one, and `:Element` is
itself a valid `NodeKind`, a freshly-introduced label (here, `:Parameter` — genuinely never
instantiated before this pass) could sort *after* `:Element` in `labels(n)`, causing `row_to_element`
to silently resolve the element's kind as the generic `Element` instead of `Parameter`. Confirmed
empirically against the live Neo4j instance: `FanLpCompression` (a long-lived `:Structure`) returned
`["Structure", "Element"]`, while `FanEquivalentWeightFlowParam` (brand new) returned `["Element",
"Parameter"]` — same function, opposite label order. This broke `create_edge`'s
`check_relationship_endpoints` call for the new `Parameter --Bound--> subsystem` edges (source kind
read back as `Element`, not `Parameter`, so the real endpoint rule rejected it) — caught by the new
integration test, not anticipated by the plan. Fixed by explicitly filtering out the generic
`:Element` label before matching, rather than relying on encountering the specific label first.
This was latent for every `NodeKind` this whole codebase has ever created, not something Phase 3
introduced — it simply took a genuinely new label to expose it, since every previously-existing
label happened to have an internal token id lower than `:Element`'s.

### 11.3 Verification

- `cargo test -p sysml-core`: 28/28 passing (incl. the 4 new `check_compressor_blade_loading` tests).
- `cargo build/clippy/fmt --workspace`: clean.
- Full `apps/api` `--ignored` suite against the live Docker stack: 45/45 passing, including the new
  `seed_turbofan_ref_lands_fr_comp_content_for_both_compressor_subsystems` test and, critically,
  `mode_b_accept_wires_traceability_and_interface_contract_is_fully_populated` — confirming the
  Interface Contract merge composes correctly with Mode B's own `accept`-path writes to the same
  subsystem bodies rather than clobbering them.

---

## 12. Phase 4: Turbofan System-Model Instance Seeded **[REV-D]**

`docs/IMPLEMENTATION_KICKOFF.md` Phase 4 called for seeding an actual instance of the reconciled
5-subsystem engine model (reqs v5 §5.16/§5.17) — boundary functions, per-subsystem breakdown,
cross-cutting connection choices/constraints, station 0–8 numbering, and `Satisfy`/`Verify` edges
from existing Requirements into the seeded structure. Recorded here is what was actually seeded and
verified, `apps/api/src/main.rs::seed_fr_arch_system_model`, called from `seed_turbofan_ref` right
after Phase 3's `seed_fr_comp_content`.

### 12.1 What was seeded

- **Station 0–8 gas-path Ports**, extending Phase 3's Fan/Core ports (stations 1/2, 2/3) across the
  rest of the chain: `CombustorInletPort`/`CombustorExitPort` (stations 3/4 — station 3 shared with
  `CoreExitPort`, station 4 shared with `TurbineHpInletPort`, the same documented shared-station
  pattern §5.16 already establishes for stations 2/3), `TurbineLpInterstagePort` (station 5),
  `TurbineExitPort` (station 6), `NozzleInletPort`/`NozzleExitPort` (stations 7/8, on `TurbineHpLp`
  per the ratified "Nozzle folded into Turbine's exit port" reconciliation decision — not a 6th
  subsystem), plus three non-station-numbered ports the per-subsystem breakdown names explicitly:
  `FanBypassDuctExitPort`, `CoreBleedOfftakePort` (appended into Core's existing Interface Contract
  `ports` array via the same read-merge pattern Phase 3 established), and `ControlAccessoryPort`.
  `CombustorFuelInjectorPort` is a documented property only, no edge — it's "fixed, not a choice"
  per the doc's own words, and no fixed non-choice port-to-port edge type exists in this schema.
- **5 `:Function` elements — first-ever instantiation of this `NodeKind`**: `GenerateThrust`
  (permanent, `DE`, `ArchDerives`-linked to the four gas-path subsystems), `ProvideBleedAir`/
  `ProvideAccessoryShaftPower` (conditional, `NOF`, `ArchDerives`-linked to the `ConnectionChoice`
  that resolves their fulfillment rather than a fixed Structure), `RegulateEngineOperation`/
  `MeterFuelFlow` (permanent, `COMP`, `ArchDerives`-linked to `ControlFadecEec`). The
  `permanence`/`fulfillmentMechanism` body tags match this doc's own §2.3 convention exactly.
- **4 `:SelectionChoice` + 2 `:ConnectionChoice` elements — first-ever instantiation of either
  `NodeKind`**: `IncludeGearbox`, `BleedOfftakeStage`, `PowerOfftake`, `MixedNozzle` (each
  `ArchDerives`-linked to its owning subsystem, `options` as a plain JSON array, `resolutionState:
  "unresolved"` per §5.17's own node-property-state-machine spec); `BleedAirRouting`/
  `PowerOfftakeRouting` (source/target port ids and cardinality as body properties — no dedicated
  port-to-port connection edge exists in this schema, same "plain JSON, gap flagged" precedent
  Phase 3 already established for Constraint→Parameter usage).
- **1 `IncompatibleWith` edge** (FR-ARCH-04): `MixedNozzle` → `FanBypassDuctExitPort`, the two
  elements §5.16's own prose says the nozzle-flow-exclusivity constraint "spans."
- **FR-COMP-04 (stage-count consistency), unblocked and landed** — explicitly deferred from Phase 3
  pending Turbine-side content, which this phase now provides: 4 new `:Parameter`s
  (`FanLpStagesParam`/`CoreHpStagesParam`/`TurbineHpStagesParam`/`TurbineLpStagesParam`,
  `Bound`-linked to their subsystem) plus 2 `ChoiceConstraint` edges linking each compressor's stage
  count to its driving turbine section's.
- **The remaining named per-subsystem design variables** (`GearRatioParam`, `BprParam`, `FprParam`,
  `OprCoreParam`, and Combustor's `ChamberSizeParam`/`FlameTemperatureParam`/`PressureLossParam`/
  `NOxParam`) as `:Parameter`s, `Bound`-linked, carrying §5.16's own stated bounds where given
  (BPR/FPR/GearRatio) or `illustrative: true` where the doc names a design variable with no numeric
  target at all (Combustor's four — it deliberately has no architecture choice modeled this pass).
- **`REQ-THRUST` wired into the seeded structure** — previously a fully disconnected element in
  this fixture (confirmed by reading `seed_turbofan_ref` before this pass: it was created with zero
  edges anywhere). Now `Satisfy`-linked from the four gas-path subsystems `GenerateThrust`
  decomposes into, and confirmed reachable from `REQ-THRUST` via `traceability::run_traversal`
  (direction `Incoming`) in the new integration test — the literal "exercising the traceability
  machinery end-to-end" check this phase's own kickoff text asks for.

### 12.2 Two real findings, flagged rather than resolved silently

- **A minor doc inconsistency**: §5.16's own prose calls `GenerateThrust`'s decomposition a
  "five-subsystem gas-path chain," but its own mermaid diagram's `GT` subgraph includes only four
  (Fan & LP Compression, Core (HP) Compressor, Combustor, Turbine (HP & LP) — Control (FADEC/EEC)
  is not gas-path). The diagram was treated as authoritative for this seed; the prose mismatch is
  flagged here rather than silently resolved by guessing which is right.
- **A real, confirmed schema gap**: `EdgeKind::ChoiceConstraint`'s own doc comment says it "carries
  a Linked/Permutations/Unordered [non-]replacing type" (mirroring `adsg_core.ChoiceConstraintType`,
  confirmed during the Phase 2 spike), but `Edge` (`packages/sysml-core/src/lib.rs`) is
  `{source, target, kind}` only — no properties field exists to persist that type anywhere. The two
  `ChoiceConstraint` edges this phase creates are real, but their "Linked" type lives only in code
  comments and this write-up, not in the graph. Not invented around (e.g. encoding it into an id) —
  flagged as a real gap for whenever a design-space-definition property mechanism gets built.

### 12.3 Explicitly not attempted this pass

Per Phase 2's own ratified recommendation (§10.3) that the *unresolved* search space stays
sidecar-side and only a *resolved* architecture instance materializes into Axioma's graph: no
wiring into `cem-archspace`/`archspace_client.rs`, no live Mode B search over this design space —
everything seeded here is a static, versioned *description* of the design space's fixed structure,
not a live search state. No `diagram-engine`/UI (Phase 5). No full Metrics-table objective/
constraint modeling beyond what diffusion-factor/Mach (Phase 3's `check_compressor_blade_loading`)
and the two stage-count `ChoiceConstraint`s cover — a real Parametrics evaluation engine that could
meaningfully consume TSFC/Thrust/Weight/Jet-Mach/etc. tags doesn't exist yet (Phase 5).

### 12.4 Verification

- `cargo build/clippy/fmt --workspace`: clean.
- Full `apps/api` `--ignored` suite against the live Docker stack: 46/46 passing, including the new
  `seed_turbofan_ref_lands_fr_arch_system_model_across_all_five_subsystems` test. Phase 3's own
  `seed_turbofan_ref_lands_fr_comp_content_for_both_compressor_subsystems` needed one real update
  (not a regression): it had asserted *exactly* 2 Contains-linked ports per compressor subsystem,
  which this phase's new `FanBypassDuctExitPort`/`CoreBleedOfftakePort` correctly breaks — fixed to
  assert the two named compressor ports are present among the subsystem's children, not an exact
  count, so a future phase adding one more port doesn't re-break the same assertion again.

---

## 13. Phase 5: Foundation Slice — Canvas Visual Types, Review-Gate Origin, Parametrics/
## Information/Collections **[REV-D]**

`docs/IMPLEMENTATION_KICKOFF.md` Phase 5 spans six new REST endpoint groups and three
`diagram-engine` efforts of very different size and risk. Presented with the real scope (research
below), the user chose the **Foundation slice**: the review-gate origin UI, ADSG canvas visual
types, and the Parametrics/Information/Collections backend surface — the fully-unblocked subset.
Document-import's async pipeline, Export/Reporting, and the Interaction view + Swimlane mode are
explicitly deferred (see §13.4).

### 13.1 What was found before designing anything

- **`diagram-engine` had exactly one node renderer for every `NodeKind`** (`AxiomaBlockNode`) and
  one edge renderer (`AxiomaEdge`). Phase 4's new `:Function`/`:SelectionChoice`/`:ConnectionChoice`
  content rendered as plain generic cards, indistinguishable from a `:Structure`.
- **The main canvas only ever rendered `Contains` edges** — `Causes`/`MitigatedBy`/`Concerns` are
  fetched into their own React state, consumed only by side panels, never added to the canvas's own
  edge array. Rendering `ArchDerives`/`IncompatibleWith`/`ChoiceConstraint` on the canvas was
  therefore new territory, not a style tweak to something already rendered.
- **`AutonomyPanel.tsx`'s `Proposal` interface didn't declare an `origin` field at all**, even
  though `apps/api` has returned one on every proposal since Phase 1 — the client silently
  discarded it. Only `cem-generated` is ever actually produced (`mode_b.rs`'s sole
  `create_proposal` call); `human-authored`/`document-import` have no real producer yet.
- **The generic `POST /elements` (any `NodeKind`) and `POST/GET/DELETE /edges` (any `EdgeKind`)
  endpoints already cover most of what §1.4 asks Parametrics/Information/Collections to add** —
  `/parametrics/constraints`, `/parametrics/bindings`, `/information/data-types` would be pure
  wrapper duplicates of endpoints that already exist. What's genuinely new: evaluating a Constraint
  (no expression/lookup evaluator exists anywhere) and Dynamic Query definition + execution (no
  stored-query concept exists).

### 13.2 What was built

- **Canvas visual types** — deliberately **not** three new bespoke node components (the turbofan
  amendment's literal §3.5 suggestion). `AxiomaBlockNode`'s existing `kindAccent` map is already the
  per-kind visual-differentiation extensibility point; duplicating its card-chrome logic three times
  over would be premature abstraction. Extended `kindAccent` with `Function`/`SelectionChoice`/
  `ConnectionChoice` entries plus a new parallel `kindGlyph` map (a single-character shape cue — ƒ /
  ◈ / ⇄ — next to the existing dot, in the same slot the Hazard-linkage badge already uses).
  `ArchDerives`/`IncompatibleWith`/`ChoiceConstraint` are now fetched via the existing generic
  `GET /edges?kind=X` surface and merged into the canvas's own edge array as read-only `axiomaEdge`s
  (distinct dashed color per kind, `reconnectable: false`, an id prefix that can't collide with
  `Contains`'s own convention) — confirmed live in a Playwright check against the real seeded
  fixture: exactly 5 Function/4 SelectionChoice/2 ConnectionChoice glyphs rendered, and edge stroke
  colors matched 12 `ArchDerives`/1 `IncompatibleWith`/2 `ChoiceConstraint` exactly.
- **Review-gate origin UI** — `Proposal` gained `origin`, rendered as an inline badge, plus an
  origin `<select>` filter matching the canvas's own existing origin-filter dropdown's visual
  convention (no `Tabs` component exists anywhere in `@axioma/ui-components` to justify inventing
  one). No backend change — the field was already there.
- **`POST /parametrics/evaluate`** (`apps/api/src/parametrics.rs`) — deliberately **not** a general
  arithmetic-expression parser (reqs v5 doesn't concretely specify one); evaluates the one shape
  §5.15 already gives real content for — linear interpolation over a Constraint's
  `sampledPointsAtDesignSpeed` tabulated curve. Tested directly against Phase 3's real
  `FanPerformanceMapConstraint` (interpolates to exactly `1.325` at input `275.0`, a deterministic
  value derived from that Constraint's own literal seed data), plus out-of-range and unknown-id
  cases, both typed "not evaluable" reasons rather than a silent/wrong extrapolation or a 500.
- **`POST /information/elements`** (`apps/api/src/information.rs`) — a real `:InformationElement`
  with `abstractionLevel` (FR-INFO-03) set in the same call/commit. `/information/data-types` isn't
  built separately — no `:DataType` `NodeKind` exists; a Data Type is itself just an
  `:InformationElement`.
- **`POST /collections/dynamic` + `POST /collections/{id}/freeze`**
  (`apps/api/src/collections.rs`) — a new Postgres `dynamic_collections` table stores a query
  *definition* (`rootId`/`depth`/`maxFanout`/`direction`, the exact shape
  `traceability::run_traversal` already takes); `freeze` actually re-runs that traversal (reused,
  not reimplemented) and materializes a real `:Collection` + one `Member` edge per visited element.
  Save-time budget rejection reuses a new `traceability::validate_budget`, factored out of the
  traceability endpoint's own previously-inline ceiling check so both call sites enforce one
  ceiling, not two. Confirmed the frozen membership matches `run_traversal`'s own result for
  identical parameters, and that an over-ceiling save is rejected (NFR-PERF-04).

### 13.3 A real, unrelated finding hit during manual verification

Restarting the API to manually verify the frontend changes in a browser surfaced that the live dev
Postgres had **484 accumulated test/throwaway projects**, and the long-lived "Turbofan Reference"
project had been seeded before Phases 3–5 existed — `ensure_seeded()` only runs once, at genesis, so
none of this session's new content had ever actually reached it. This is the same gotcha this
session's own memory already documents (test pollution silently defeating the genesis-seed gate);
recovered the same documented way — truncated the versioning/content tables and let
`ensure_seeded()` reseed fresh — after explicit confirmation, since it's a destructive local-dev-DB
action. Not a Phase 5 defect; a pre-existing dev-environment hygiene issue this pass's manual
verification happened to surface again.

### 13.4 Explicitly not attempted this pass

`/parametrics/constraints`/`/parametrics/bindings`/`/information/data-types` (already covered by the
generic endpoints, §13.1). The document-import async job pipeline (§1.1a — needs an LLM-structuring
capability that doesn't exist). `/export/*` **was subsequently built — see §14**.
`/interactions/*` and its `diagram-engine` counterpart, and Swimlane mode (FR-CORE-12) — the
latter's own spec text assumes an `Allocate` `EdgeKind` that Phase 1 never actually created, a real
gap flagged here rather than silently invented around — **were subsequently built, ADR-009
ratified — see §16**. The choice-
resolution click-to-resolve interaction and design-space stats sidebar from the turbofan amendment
§3.5 — both depend on the still-unbuilt Mode B design-space HTTP surface (§1.2a).

---

## 14. Phase 5 continued: Export & Reporting (FR-EXPORT-01..04) **[REV-D]**

The user asked to continue working through Phase 5's deferred verticals in sequence, starting with
**Export & Reporting**. Reqs v5 §5.12 frames all four FR-EXPORT items as reusing existing
mechanisms; research before designing anything found **none of the three referenced mechanisms
actually existed in reusable form**, which materially changed this work's real scope.

### 14.1 What was found before designing anything

- **FR-SAFE-05's risk-register export (`traceability.rs::get_risk_register`) was a single
  hardcoded query→struct→JSON pipeline**, not a template+scope engine — its own doc comment
  already said so ("no literal ARP4761 template exists... this is this project's own reasonable
  field layout"). `format: "ARP4761"` was a hardcoded literal. There was nothing to "generalize."
- **No "Generic Table view" exists anywhere in `apps/web`** — FR-EXPORT-02's premise references a
  UI component that was never built.
- **`GeometryPointer` (`packages/sysml-core/src/lib.rs`) was dead code** — never constructed or
  used anywhere except its own round-trip unit test. The real pattern at the one call site
  (`seed_turbofan_ref`) was a plain string URI embedded in `ElementBody.properties`.
  **`ObjectStore` had a write method (`put_placeholder`) and no read method at all** — a real gap,
  since an attachment necessarily needs a download path.

### 14.2 What was built

- **FR-EXPORT-04 (attachments)** — `ObjectStore::get_object` (the missing read half;
  `put_placeholder` renamed to `put_object` to match, its one call site — `seed_turbofan_ref` —
  updated). New Postgres `attachments` table (mirrors `dynamic_collections`'s creation pattern).
  Three endpoints (`POST/GET .../elements/:id/attachments`, `GET .../attachments/:id`) — multipart
  upload (`axum`'s `multipart` feature, not previously enabled), metadata list, byte-stream
  download. No Neo4j write, no `record_commit` — an attachment references an existing element by
  id without creating/modifying any graph node/edge, matching reqs v5 §5.12's "none of them write
  to the graph" framing read as "the topology store specifically."
- **FR-EXPORT-02 (tabular)** — `GET .../export/table` (CSV only; XLSX needs a new crate not
  justified this pass), scoped by `?kind=X` (a `NodeKind` filter) or `?collectionId=Y` (a frozen
  `/collections/{id}/freeze` result's real membership — Phase 5's own Collections feature standing
  in for the nonexistent Generic Table view's "scope," a real non-duplicative connection rather
  than a second invented scoping mechanism). Fixed baseline columns; hand-rolled RFC4180 CSV
  writer (no new crate justified for ~10 lines of well-known escaping logic).
- **FR-EXPORT-03 (report)** — `traceability::get_risk_register`'s data-gathering was split into
  `pub(crate) build_risk_register`, now called by both the unchanged existing JSON endpoint and a
  new `POST .../export/report { templateId, scopeElementId? }` (`export.rs`). Exactly one template
  is registered — `"risk-register"`, rendered as a plain HTML table (no PDF crate justified for one
  template; reqs v5 explicitly accepts "PDF/HTML"). Any other `templateId` is a precise 400 naming
  what's missing, never a silent fallback — same discipline as `sysml-core`'s own validation layer.
- **FR-EXPORT-01 (diagram image)** — client-side only. `html-to-image`'s `toPng` captures
  `Canvas`'s existing `canvasWrapperRef` (the same DOM node the clustering margin math already
  measures) on a new "Export PNG" toolbar button in `page.tsx`, triggering a normal browser
  download. Confirmed via a live Playwright check: a real ~190KB PNG (correct magic bytes)
  downloads with the project id in its filename, no console errors. The **server-side
  headless-render path for full-diagram export "at any size"** (reqs' other named half, reusing
  the virtualization/clustering machinery in reverse) is real, separate new capability — not
  attempted this pass.

### 14.3 Explicitly not attempted this pass

XLSX writing; PDF generation; the server-side headless diagram render; any MIL-STD-882/ISO-26262
report template variant — none has a concrete spec to build against, the same gap
`get_risk_register`'s own pre-existing doc comment already flagged before this pass touched it.

### 14.4 Verification

- `cargo build/clippy/fmt --workspace`; `pnpm --filter @axioma/web exec tsc --noEmit`/Biome; a real
  `next build` (catches any SSR/bundling issue the new `html-to-image` dependency could introduce —
  none found).
- Full `apps/api` `--ignored` suite against the live Docker stack: 52/52 passing, including three
  new tests (attachment upload→list→download round-trip with real bytes; table export scoped both
  ways; report export producing HTML that contains the same hazard data
  `risk_register_reflects_hazard_severity_and_mitigated_control` already proves the JSON endpoint
  computes) and confirming that existing test itself is unaffected by the `build_risk_register`
  refactor.
- Live Playwright verification of the "Export PNG" button (§14.2) — not just passing tests.

---

## 15. Phase 5 continued: Documents → Draft Model Pipeline (FR-CORE-14..18) **[REV-D]**

The third of Phase 5's deferred verticals the user asked to work through in sequence (after Export
& Reporting), and the biggest/most novel one — a real async job pipeline turning an uploaded PDF
into reviewable draft Requirements.

### 15.1 What was found before designing anything

- **Mode A's Ollama-calling pattern is directly reusable** — a plain `reqwest` call to
  `{OLLAMA_URL}/api/generate` (model `qwen2.5:1.5b`), `modelVersion` from `/api/tags`'s digest
  field, `promptTemplateHash` = SHA-256 of the fixed template. `packages/llm-gateway` is still "Not
  started" — Mode A already set the precedent of hard-wiring Ollama directly rather than building
  the abstraction for its first caller; this pipeline does the same for its second, deliberately
  duplicating the ~30-line request/response shape rather than sharing it (two callers doesn't
  justify extraction, same reasoning already applied to `fuml_client`/`archspace_client`).
- **No async-job infrastructure existed anywhere in this codebase** — no jobs table, no
  background-worker loop, no polling-by-id pattern (confirmed via search).
- **No PDF/OCR crate existed.** Added `pdf-extract` 0.12 (MIT), verified directly against docs.rs
  before adding: `extract_text_from_mem_by_pages(&[u8]) -> Result<Vec<String>, OutputError>` — real
  per-page text extraction from in-memory bytes, exactly what FR-CORE-15's page-number citation
  needs. Text-layer extraction only — **no OCR is attempted this pass**; an all-pages-empty
  extraction is detected and fails the job with a precise reason.
- **`create_proposal`'s schema (`store/versioning.rs`) is shaped entirely around Mode B's "propose
  one subsystem candidate" concept.** Reqs v5 §5.6's own amendment explicitly allows "individual or
  **consolidated-batch** accept/reject" for `document-import`, so one proposal per completed job —
  `candidate` holding the full drafted-requirements array — is spec-conformant, not a workaround.
  `subsystem_id` is repurposed to hold the job id (documented, not renamed).
- **A more consequential finding**: `mode_b.rs::accept_proposal` unconditionally deserialized
  `proposal.candidate` as Mode B's `Candidate` struct — this would hard-fail on a document-import
  proposal's real candidate shape (an array of drafted requirements). Reqs v5's "reused unchanged"
  claim for `GET/POST /cem/proposals/*` is true at the routing/HTTP-shape level, but the *internal
  accept-time materialization* needed to branch on `proposal.origin` — a real, necessary extension,
  documented directly in `accept_proposal`'s own updated doc comment.
- **`mode_b.rs::propose`'s established branch-without-commit pattern is followed identically**: a
  proposal's branch never actually receives a `commit()` — content lives entirely in
  `proposals.candidate`, materialized only at accept time. Impl v5 §5.14 stage 5's prose says
  "lands as a Git branch/commit," but the already-shipped mechanism doesn't literally commit
  either; diverging from it for one origin would be inconsistent, not more correct.

### 15.2 What was built

`apps/api/src/document_import.rs` (new) implements all five stages and four endpoints:

- **`POST /import/documents`** — multipart PDF upload, inserts the job row (`status: Extracting`),
  `tokio::spawn`s the pipeline against a cloned `AppState`, returns `{jobId}` before the spawned
  task necessarily finishes — real asynchrony, confirmed live (status observed `Extracting` →
  `AwaitingReview` across real HTTP requests, not just internally).
- **Extraction** → all-pages-empty is `Failed` ("no extractable text layer -- OCR not
  implemented"). **Segmentation** (deterministic — no LLM call, per reqs' own explicit "not an LLM
  call" text): any sentence containing "shall" is a candidate, tagged with its page index; a
  length heuristic flags low confidence, never dropped (FR-CORE-18). Zero candidates → `Failed`
  ("no candidate requirement statements found") — FR-CORE-18's "reported failure state, not an
  empty successful import," implemented literally. **Structuring**: one Ollama call per candidate,
  drafting `{name, shallText, category}`; a parse failure falls back to the raw candidate text
  rather than dropping it. **Grounding & Provenance**: citation/confidence/`ImportProvenance`
  stamped per candidate. **Validation**: an internal invariant check (should never trigger, given
  the same code stamps these fields unconditionally) before `AwaitingReview`.
- **FR-CORE-17 suggestions**: a simple heuristic (consecutive Title-Case word runs), explicitly
  documented as not real NLP, computed alongside segmentation and stored separately.
- **`GET /import/documents/:jobId`** (status+error), **`GET .../candidates`**,
  **`GET .../suggestions`**, **`POST .../proposal`** (only from `AwaitingReview`; creates one branch
  + one `document-import`-origin proposal batching every drafted candidate).
- **`mode_b.rs::accept_proposal`** now branches on `proposal.origin`: `document-import` calls the
  new `document_import::materialize_proposal` (creates one real `:Requirement` per drafted
  candidate — name, `shallText`/citation/confidence/provenance as body properties, `Origin::
  AiSuggested`, one commit for the batch) instead of `apply_candidate_to_main`.

### 15.3 Verification

- A hand-built minimal PDF (byte offsets computed programmatically, not a static literal with
  manually-counted offsets — no test fixture ships with `pdf-extract`'s crates.io package) drives
  three new integration tests: the full pipeline end-to-end through accept (drafted candidate's
  citation/confidence checked, then a real `:Requirement` element confirmed after accept); the
  no-extractable-text `Failed` case; the no-"shall"-sentences `Failed` case. All passed on the
  first real run, including the live Ollama call (full suite run: 55/55 passing, was 52 going in).
- **Live HTTP verification beyond the tests** (which call handlers directly, not through the real
  router): a real `curl` multipart upload against a running dev server, end-to-end through accept.
  The model drafted a genuinely sensible title ("Manual Curl Verification") from the raw candidate
  sentence, correct citation (`page: 1`), correct provenance, and a real `Origin::AiSuggested`
  `:Requirement` element existed in the graph afterward. This left one real proposal/branch/element
  in the live "Turbofan Reference" project — same no-cleanup precedent as every other manual
  verification pass this session (T-P1.4-05, Mode A, the Autonomy panel's own P2.2 pass); no
  delete-project/proposal endpoint exists to clean it up with anyway.
- `cargo build/clippy/fmt --workspace` clean.

### 15.4 Explicitly not attempted this pass

OCR (§15.1); `llm-gateway` (Mode A's own precedent stands — no second-caller abstraction yet);
parallelizing the per-candidate LLM calls (sequential is fine for a background job); a real
NLP-based structural-noun extractor; any rename of `create_proposal`'s columns (repurposed, not
renamed, to avoid touching every Mode-B call site).

---

## 16. Phase 5 continued: Interaction View + Swimlane Mode (FR-INTX-01..04, FR-CORE-12) —
## ADR-009 Ratified **[REV-D]**

The last of Phase 5's three deferred verticals. Two known blockers going in: **ADR-009** (the
SysML v2 metaclass mapping for Interactions) was still "Proposed," and FR-CORE-12's own design
text assumed an `Allocate` `EdgeKind` that §2.3's real enum never actually had.

### 16.1 ADR-009 ratified — option 2, as its own text recommended

`:Interaction`/`:InteractionFragment` (Phase 1 placeholders) stay exactly what they were —
`packages/sysml-core/src/lib.rs::EdgeKind`/`NodeKind` gained **nothing** for this vertical beyond
the unrelated `Allocate` fix below. Messages live as a plain JSON array
(`{order, from, to, text, kind, fragmentId?, refInteractionId?, timingConstraint?}`) on the
`:Interaction` element's own body; a fragment is a real `:InteractionFragment` element,
`Contains`-edged from its parent, with `{fragmentKind, guard?}` as its body. The Lifeline/Message
diagram (`apps/web/src/components/InteractionPanel.tsx`) is pure rendering over that content — a
hand-rolled SVG, not a new `diagram-engine` node/edge type — decoupled from storage exactly as
ADR-009's recommendation said it should be. FR-INTX-02 (timing) and FR-INTX-04
(`refInteractionId` sub-interaction reuse) are both just message fields, captured and displayed;
no latency-analysis engine or ref-resolution UI was built — neither has spec text asking for one.
**ADR-009 status: Ratified** (flipped in reqs v5 §7 and impl v5 §2.5's own ADR table, both below).

Three endpoints, matching impl v5 §1.4's own sketch exactly: `POST .../interactions`
(`interactions.rs::create_interaction`, validates every `participantIds` entry exists first),
`POST .../interactions/:id/messages` (read-merge append, server-assigned `order`), `POST
.../interactions/:id/fragments` (creates the element + `Contains` edge). No dedicated UI exists
to *create* an Interaction with participants (`page.tsx`'s "+ Add Node" is hardcoded to
`Structure` — unchanged this pass); one is expected to be seeded or created directly against the
endpoint, then opened by clicking it on the canvas like any other element.

### 16.2 `EdgeKind::Allocate` — a straightforward gap-closure, not a design call

Unlike ADR-009, FR-CORE-12's own text already treats this as decided ("no backend data-model
change beyond the *existing* `Allocate` dependency stereotype") — it just wasn't actually there.
Added kind-unconstrained on both ends (Block/Actor/Interface are all plain `:Structure` today, no
separate kind exists to constrain against — same discipline already applied to
`ArchDerives`/`IncompatibleWith`/`ChoiceConstraint`). Reuses the existing generic `POST/GET/DELETE
/edges` endpoints — no dedicated endpoint needed.

### 16.3 Swimlane mode — a real library primitive, previously unused

`@xyflow/react` 12.11 (already pinned) ships `parentId`/`extent: "parent"` node properties for
nested/contained children — confirmed real and shipped, and unused anywhere in this codebase
before this pass. `packages/diagram-engine/src/swimlane.ts::computeSwimlaneLayout` groups every
element by its outgoing `Allocate` edge target into a lane (a real `Unallocated` catch-all lane
for anything with none — never silently hidden, since FR-CORE-12's own text never says allocation
is mandatory); each lane is a `SwimlaneLaneNode` (a real React Flow parent node), each member gets
`parentId`/`extent: "parent"` plus a computed relative position. **A manual grid, not ELK** — ELK's
flat `layered` algorithm (`layout.ts`) has no partition-aware mode wired in, and building one is a
bigger lift than FR-CORE-12's own "vertical/horizontal partitions" ask needs. `page.tsx`'s
Swimlane View toggle swaps this layout in wholesale in place of the normal ELK/clustering path
(mutually exclusive, not layered — Contains edges aren't drawn in this view; they'd cross lane
boundaries in a way that fights the partitioning that's the point of the view) and calls
`reactFlowInstance.fitView()` on entry.

**Allocation is click-to-allocate, not drag-to-allocate — a real, flagged scope-down.**
FR-CORE-12's own text says "drag-to-allocate headers." This pass ships a dropdown in
`ElementInspector.tsx` (visible only in Swimlane View) instead: a real, working allocation action
— removes any prior `Allocate` edge from the element first, then creates the new one (so an
element is allocated to at most one lane, matching "each partition allocated to exactly one
structural element") — just not the literal drag gesture. Native drag-and-drop-into-a-React-Flow-
group is a separate, larger interaction-design effort, not attempted here.

### 16.4 A real, unrelated gap found during live browser verification: the Next.js proxy layer

`apps/web` has **no rewrites/catch-all config** (`next.config.mjs` only sets
`transpilePackages`) — every backend route needs its own explicit
`apps/web/src/app/api/.../route.ts` calling the shared `proxyRequest` helper
(`apps/web/src/lib/api-proxy.ts`), one file per path, matching e.g. `edges/route.ts`. Live curl
verification of `InteractionPanel` against the running dev server (not just the Rust API directly)
found **none of the three `/interactions/*` endpoints had one** — a real 404 from the browser's
own point of view despite the backend being fully correct and its own integration tests all
passing. Added the three missing route files this pass (§16.1's endpoints).

**This is not unique to Interactions.** The same check found `/parametrics/evaluate`,
`/information/elements`, `/collections/dynamic`, `/collections/:id/freeze` (Phase 5's Foundation
slice, §13), and `/export/table`, `/export/report`, `/elements/:id/attachments`,
`/attachments/:id` (§14) are **also** missing a Next.js proxy route — real, confirmed via the same
`find apps/web/src/app/api` sweep. None of those five have a frontend caller today (grepped for
`fetch(` against every one of those paths in `apps/web/src`; zero hits), which is why their own
passes' "live verification" never caught it — each curled the Rust API on port 8080 directly, not
through the Next.js dev server. **Not fixed this pass** — there's no UI to unblock yet, and adding
unused proxy plumbing isn't this vertical's job. Flagged here so the first PR that builds a UI for
any of those five knows to add its `route.ts` alongside it, not discover the same 404 fresh.

### 16.5 Explicitly not attempted this pass

FR-CORE-13 (orphan-Action rejection) — still blocked on the separate, undecided Action/Activity
`NodeKind` question Phase 1 already flagged, untouched by Swimlane's own "partition by `Allocate`
target" mechanism. Native drag-and-drop lane reassignment (§16.3). A latency/timing-analysis engine
consuming FR-INTX-02's captured constraints. ELK-aware partition layout (§16.3). A "create
Interaction with participants" UI flow (§16.1). Proxy routes for the five unrelated already-built,
never-yet-UI'd endpoints named in §16.4.

### 16.6 Verification

- `cargo build/clippy/fmt --workspace`; full `apps/api` `--ignored` suite against the live Docker
  stack: 57/57 passing, including two new tests (`interaction_pipeline_stores_messages_and_
  fragments_correctly`, `allocate_edge_round_trips_through_the_generic_edge_endpoint`).
- `pnpm --filter @axioma/web exec tsc --noEmit` / `pnpm --filter @axioma/diagram-engine exec tsc
  --noEmit` / Biome, all clean.
- Live verification against the real running dev server (not just `cargo test`) via a headless
  Chromium driver (`playwright`, fetched on demand — not a new repo dependency): created a real
  Interaction with participants, two messages (one `sync`, one `reply`), and an `opt` fragment
  through the actual browser-facing proxy path; opened it on canvas and confirmed the lifeline SVG
  renders both messages and the participant boxes correctly. Toggled Swimlane View with three
  seeded `Allocate` edges across an otherwise-unallocated 50-element project; confirmed three real
  lanes render (two real + `Unallocated`), `fitView` brings them on screen, and reallocating an
  element via `ElementInspector`'s dropdown round-trips (delete old edge → create new → reload)
  and is reflected back in the dropdown's own next render. This pass caught and fixed one real bug
  this way (Swimlane View not calling `fitView` on entry, leaving the lane grid off-screen behind
  the toolbar panel) beyond what `cargo test`/`tsc` alone could have caught.

---

## 17. Frontend UI for Parametrics, Information Elements, Collections, Export & Attachments
## **[REV-D]**

§16.4 flagged that five already-built backend capability groups — Parametrics evaluate,
Information Elements, Dynamic/Static Collections, Export & Reporting, and Element Attachments —
had zero frontend callers and zero Next.js proxy routes. This pass closes that gap with real UI,
not just routes, for all five, and closes out `docs/IMPLEMENTATION_KICKOFF.md` Phase 5 entirely.

### 17.1 The proxy layer needed a real fix, not just eight new files

`apps/web/src/lib/api-proxy.ts`'s `proxyRequest` built every response via `await upstream.text()`.
That's a real correctness bug for the new attachment-download route: an arbitrary uploaded file
(image, PDF, anything non-UTF-8) would be corrupted by a decode/re-encode round-trip through a JS
string. Fixed by switching the shared relay to `arrayBuffer()` — confirmed behavior-preserving for
every existing JSON/CSV/HTML caller (all 28 pre-existing proxy routes just do a thin
`return proxyRequest(...)`, none inspect the body as a string) and now correct for binary content.

A new `proxyMultipart(path, request)` forwards a browser `FormData` upload's exact bytes and
original `Content-Type` (boundary included) straight through — passing a raw `ArrayBuffer` as
`fetch`'s body, not a reconstructed `FormData`, is what makes the manually-copied header survive
(`fetch` only auto-generates a new, mismatched-boundary Content-Type when the body is `FormData`
itself). Verified for real: uploaded a file through the browser, downloaded it back through the
same proxy, and confirmed the bytes are identical — the one thing `tsc`/`cargo test` can't catch on
their own.

Eight new route files (`parametrics/evaluate`, `information/elements`, `collections/dynamic`,
`collections/:id/freeze`, `elements/:id/attachments` GET+POST, `attachments/:id`, `export/table`,
`export/report`) — `elements/:id/attachments` reuses the existing `[id]` segment name (not
`[elementId]`), matching every sibling route already under `elements/[id]/`; Next.js requires one
consistent dynamic-segment name per path position.

### 17.2 What got built, per capability

- **Parametrics (FR-PARAM-03)** — new `ParametricsPanel.tsx`: checkbox-select any `Constraint`
  element, one numeric input for `equivalentWeightFlowLbPerSec`, Evaluate, per-constraint
  `pressureRatio` or a typed error rendered inline. No UI for FR-PARAM-01/02 (Constraint/Binding
  authoring) — those already go through the generic `POST /elements`/`POST /edges` endpoints, which
  already have UI (the toolbar's "+ Add Node" and the canvas's own edge-drag), so nothing new was
  needed there.
- **Information Elements (FR-INFO-01/03)** — the toolbar's "+ Add Node" gained a kind selector
  (`Structure` / `Information Element`) and, for the latter, an abstraction-level selector
  (Conceptual/Logical/Physical). A new `createInformationElement` posts to the dedicated endpoint
  (atomic `abstractionLevel` write) and shares its node-placement tail with the existing
  `createElement` via a new `placeNewElement` helper. No new UI needed to *view*
  `abstractionLevel` afterward — it lands in the body's `properties`, already shown/editable by
  `ElementInspector`'s existing generic property-row editor.
- **Dynamic/Static Collections (FR-CORE-10/11)** — reused `TraceabilityPanel.tsx`'s existing
  root/depth/maxFanout/direction state (exactly what `POST /collections/dynamic` needs) rather than
  building a second form: a "Save as Dynamic Collection" name field + button, and a "Freeze" button
  per saved collection. Saved collections are `Home`-level state in `page.tsx`, not local to the
  panel — `TraceabilityPanel` unmounts on close (conditionally rendered), so panel-local state would
  be lost every close, not just on reload; there's still no LIST endpoint, so a full page reload
  does lose them — a real, accepted gap, not silently hidden. `ElementInspector` gained a
  `elementKind === "Collection"` section listing real member names (fetched via `GET
  /edges?kind=Member`, filtered client-side to the collection's own id — no source-filter param
  exists on that endpoint, same pattern `InteractionPanel` already uses for `Contains`) plus an
  "Export as Table (CSV)" link scoped by `?collectionId=`.
- **Export & Reporting (FR-EXPORT-02/03)** — a `NodeKind` selector + "Export Table" link next to
  the existing "Export PNG" button in the main toolbar (a GET with `Content-Disposition` needs no
  JS, matching the anchor-download precedent already in `HazardRiskPanel`). `HazardRiskPanel`'s
  pre-existing "Export Risk Register (ARP4761)" link — which pointed at `/safety/risk-register`, a
  JSON endpoint, and so just navigated the browser to raw JSON despite its "Export" label — is
  replaced with a real "Export Report (HTML)" button using the new `/export/report` endpoint
  (`fetch` → `blob()` → `URL.createObjectURL` → synthetic click, mirroring `handleExportPng`'s
  existing download pattern). A real, deliberate correction alongside the new capability, not a
  silent behavior change left unmentioned.
- **Attachments (FR-EXPORT-04)** — `ElementInspector` gained an unconditional "Attachments"
  section (every element kind, not gated): list with per-file download links, a file input +
  Upload button using `FormData` (the browser sets the multipart boundary; `proxyMultipart`
  forwards it untouched). axum has no `DefaultBodyLimit` layer configured anywhere — uploads
  default-cap at axum's built-in 2MB. Not changed this pass; flagged here as a known limit.

### 17.3 A real, pre-existing data-fixture gap found during live verification (not a code bug)

Live-evaluating the seeded `FanPerformanceMapConstraint` through the new `ParametricsPanel`
returned `"no such Constraint"` — `parametrics.rs::evaluate_one`'s own correct, typed error for a
missing Postgres body. Direct `GET .../elements/FanPerformanceMapConstraint/body` on the live
"Turbofan Reference" project confirmed it: 404, no body row, despite `main.rs`'s
`seed_fr_comp_content` correctly writing one (confirmed by reading that code — the
`sampledPointsAtDesignSpeed` write is really there). `ensure_seeded()`'s gate is "skip entirely if
any project already exists" (`state.versioning.count_projects().await? > 0`) — this session's own
long history of manual Postgres-table resets (recorded in the auto-memory file, done to clear test
pollution) drifted Postgres and Neo4j out of sync for this one long-lived shared "Turbofan
Reference" fixture: the Neo4j Constraint node survived a reset that the Postgres body row didn't.
**Not a `parametrics.rs` bug** — verified by backfilling the one element's body through the
existing generic `PUT .../elements/:id/body` endpoint (the same path `ElementInspector`'s own Save
button uses) and re-running the evaluation, which then correctly interpolated
(`pressureRatio = 1.3` at the sampled boundary). Flagged here rather than "fixed" broadly — a
general reseed-drift fix is a separate, larger concern than this vertical's own scope.

### 17.4 Explicitly not attempted this pass

FR-INFO-02 (Data Type/Enumeration authoring UI — no dedicated backend endpoint exists either, per
§13.1's own finding); FR-EXPORT-01's server-side full-diagram headless render (client-side PNG
export already covers the other half, §14); XLSX export (CSV only, matching `export.rs`'s own
scope); column-selection on table export; manual add/remove-by-hand editing of a frozen Static
Collection (FR-CORE-11's other named capability — this pass's Collection view is read-only);
raising axum's request body limit for larger attachment uploads.

### 17.5 Verification

- `cargo build/clippy/fmt --workspace` clean (no Rust changes this pass); `pnpm --filter @axioma/web
  exec tsc --noEmit` clean; Biome clean; a real `next build` succeeded and listed all eight new
  proxy routes in its route table.
- Live verification against the real running dev server via a headless Chromium driver
  (Playwright, fetched on demand — not a new repo dependency): evaluated a real Constraint and got
  a real interpolated `pressureRatio`; created an Information Element via the toolbar and confirmed
  `abstractionLevel: "Conceptual"` landed in its body via a direct API check; saved and froze a
  Dynamic Collection, confirmed the new `:Collection` element and its 6 real `Member` edges render
  correctly in `ElementInspector` after allowing the fetch to resolve (an initial screenshot raced
  the fetch and showed "No members." — re-checked with a longer wait and confirmed it was a
  screenshot-timing artifact of the verification script, not a real bug); uploaded a real file and
  downloaded it back, confirming byte-identical content; downloaded a real CSV via Export Table
  with the correct header and rows; downloaded a real HTML file via Export Report.

---

## 18. Phase 6: Test Coverage **[REV-D]**

`docs/IMPLEMENTATION_KICKOFF.md` Phase 6: land the 19 test IDs named in each amendment's own §4 —
`T-PARAM-*`, `T-INFO-*`, `T-INTX-*`, `T-EXPORT-*`, `T-CORE-10/12-*`, `T-CORE-03-EXT`,
`T-DOCIMPORT-01…07` — as they become exercisable, prioritizing `T-DOCIMPORT-06` and "the Phase 2
sidecar's own round-trip tests." This closes out the phase-numbered roadmap in
`docs/IMPLEMENTATION_KICKOFF.md` entirely.

### 18.1 What was found before writing anything

- **The Phase 2 sidecar round-trip is already done, not a gap.**
  `archspace_design_space_round_trips_through_the_sidecar` (already in `main.rs`'s test module)
  already exercises the real gRPC boundary against the Dockerized `cem-archspace` service — see
  §10.1's own text. Nothing to add there.
- **Several of the 19 definitions describe capabilities this session's own earlier passes already,
  honestly scoped out** — writing a test against their literal wording would either fail against
  real code (not useful CI hygiene for a documented, deliberate scope-down) or require quietly
  building new production features mid-"test coverage" pass (scope creep). Found by reading the
  actual code, not assumed: no general algebraic Parametrics evaluator exists (§1's own doc
  comment — only linear interpolation over tabulated points); no XLSX/column-selection export
  (CSV, fixed columns only, §14); no server-side full-diagram headless render (§14, client-side
  viewport PNG only); no orphan-Action rejection (FR-CORE-13, blocked on an undecided
  Action/Activity `NodeKind` question); no OCR (§15, deliberately not built); no Object Flow/Action
  model at all (same blocker as FR-CORE-13); no dedicated Data Type/Enumeration endpoint
  (FR-INFO-02, confirmed unbuilt in §17.2).
- **Several of the 19 were already substantially covered by existing tests from earlier passes**,
  confirmed by reading the test module before writing anything new — a real, non-obvious finding
  that shrank this pass's actual scope: `dynamic_collection_freeze_matches_the_traversal_it_reruns`
  already asserted save-time budget rejection (T-CORE-10-01's other half);
  `interaction_pipeline_stores_messages_and_fragments_correctly` already exercises a
  `timingConstraint`, a fragment with a `guard`, and `refInteractionId` (T-INTX-01/02, near-fully);
  `parametrics_evaluate_interpolates_fan_performance_map_and_rejects_out_of_range` already covers
  T-PARAM-02 in full; `allocate_edge_round_trips_through_the_generic_edge_endpoint` already covers
  T-CORE-12-01's allocation half; `export_table_as_csv_scoped_by_kind_and_by_collection` and
  `export_report_renders_risk_register_html_matching_the_json_endpoint` already cover T-EXPORT-02/
  03 to the extent they're built; `document_import_pipeline_drafts_and_accepts_a_real_requirement`
  already runs the full pipeline successfully, the behavioral half of T-DOCIMPORT-06.

### 18.2 What was actually new — 5 tests, not 19

Given the above, only 7 of the 19 IDs had a real, previously-uncovered gap worth a new test, landed
as 5 test functions (some share setup across adjacent IDs rather than re-running an expensive real
Ollama-backed pipeline per ID):

- **`bound_edge_endpoint_rejects_a_non_parameter_source` (T-PARAM-01, adapted)** — the real
  "type-checked binding" this system enforces is `EdgeKind::Bound`'s kind-based endpoint rule
  (`sysml-core`: source must be `NodeKind::Parameter`), not unit/value-type checking, which doesn't
  exist. Confirms a non-Parameter source is rejected and a real Parameter source succeeds.
- **`dynamic_collection_reflects_a_newly_added_element_on_refreeze` (T-CORE-10-01, other half)** —
  freeze once, add a new element reachable from the same root, freeze the same saved definition
  again, confirm the new element appears. Each freeze creates a new `:Collection` snapshot rather
  than updating one in place — "re-evaluation" here means calling freeze again, not a live-updating
  single element. **A real test-authoring mistake found and fixed while writing this**: the first
  draft used `Contains` edges for the fixture and got an empty traversal back —
  `Neo4jStore::trace_neighbors`'s Cypher query only matches `SATISFY|VERIFY|REFINE|DERIVE|COPY`
  relationship types, `Contains` is a structurally separate edge never included in traceability/
  collection traversals. Fixed by using `Refine` instead, matching every other traceability test's
  own established fixture convention.
- **`derive_and_copy_edges_round_trip_and_are_traceability_distinguishable_from_contains`
  (T-CORE-03-EXT)** — confirmed `Derive`/`Copy` are already in that same relationship-type set
  (real, pre-existing, just never asserted by a test), so both are queryable via the real
  `GET .../traceability` endpoint today. "Distinguishable from Containment" is verified by
  construction (no `Contains` edge exists in the fixture at all, so every `viaEdgeKind` result can
  only be `Derive`/`Copy`).
- **`document_import_produces_multiple_candidates_with_full_provenance_and_surfaces_via_the_real_
  proposals_endpoint` (T-DOCIMPORT-01/02/03, strengthened)** — a 3-"shall"-sentence document
  produces 3 candidates (the existing acceptance test only ever used one), every candidate's full
  `provenance` block is asserted (never checked before), and the resulting proposal is confirmed
  visible through the real `GET /cem/proposals/:branchId` list endpoint — the same one human-
  authored/cem-generated proposals use — not just a direct Postgres read.
- **`document_import_surfaces_structure_suggestions_and_low_confidence_candidates_without_
  dropping_either` (T-DOCIMPORT-04/05, adapted)** — neither the suggestion path nor the
  low-confidence path had any test before this. The spec's literal T-DOCIMPORT-05 scenarios
  (a requirement split across a page break, one embedded in a table) aren't what the built
  heuristic detects (`segment()`'s confidence is a pure sentence-length threshold, 20–400 chars);
  tested with a deliberately short "shall" sentence instead, against the same real PASS criterion
  (surfaced with `confidence: "low"`, never dropped). **A second test-authoring mistake found and
  fixed**: the first draft matched the low-confidence candidate by its exact extracted sentence
  text — but `shall_text` is the LLM's *drafted* wording from the Structuring stage, not the raw
  extracted sentence verbatim (segmentation/confidence-scoring happens before the LLM call and is
  deterministic; the LLM's rewording of that sentence is not). Fixed by matching on `confidence ==
  Low` instead of exact text.

### 18.3 Explicitly not attempted this pass

Building any of the unbuilt capabilities named in §18.1's first bullet to make a spec sentence
literally pass — none has a concrete implementation ask beyond "make this specific test scenario
work," which is exactly the kind of guessing-ahead-of-the-spec this session's own established
discipline commits against. Dynamic/scheduled re-evaluation triggers for Dynamic Collections
(still on-demand only, matching `collections.rs`'s own documented scope). Physically stopping the
`cem-archspace` Docker container mid-test to verify T-DOCIMPORT-06's Product-2 independence at
runtime — no precedent for Docker orchestration from within this codebase's tests; verified by
construction instead (§18.1).

### 18.4 Verification

- `cargo build/clippy/fmt --workspace` clean.
- Full `apps/api` `--ignored` suite against the live Docker stack: **62/62 passing** (57 existing +
  5 new), zero regressions. Both test-authoring mistakes in §18.2 were caught by actually running
  the new tests against the real stack before considering them done, not just by getting them to
  compile.
- No frontend changes this pass — no `tsc`/Biome/`next build` needed.
