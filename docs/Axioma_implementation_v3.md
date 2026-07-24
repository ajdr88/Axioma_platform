# Axioma: Implementation Specification

**Version:** 3.0 (Post-Architecture-Review)
**Status:** Draft for Architecture Review — Rev B
**Companion document:** Axioma_requirements_v3.md
**Change basis:** Folds in all twelve prioritized architecture-review findings. Material changes flagged **[REV-B §x]**.

---

## 1. SysML v2 REST API Specification (Draft)

Services are structured around **Projects**, **Commits**, and **Elements**. All traversal endpoints enforce query budgets (NFR-PERF-04): explicit `maxDepth`, `maxFanout`, and cursor-based pagination; unbounded traversals are rejected **[REV-B §C1]**.

### 1.1 Core Endpoints (Product 1)

* **`GET /projects/{id}/commits/{id}/elements?cursor=&limit=`** — Paginated element list for a model version.
* **`POST /projects/{id}/elements`** — Create an element (Block, Part, Requirement, Hazard, Mission). Write passes the semantic-validation layer (§4.2) before commit.
* **`GET /elements/{id}/traceability?depth=&maxFanout=&cursor=`** — Impact path, bounded by query budget; returns a page plus a continuation cursor.
* **`POST /simulations/execute`** — Behavioral simulation run.
* **`POST /import/reqif`** / **`POST /import/sysml-v2`** / **`POST /import/documents`** — Model import/interop (FR-CORE-07) **[REV-B §E3]**.
* **`GET /elements/{id}/provenance`** — Origin, validation state, staleness for any element (FR-CORE-08) **[REV-B §E2]**.

### 1.2 CEM Endpoints (Product 2)

* **`POST /cem/mode-a/query`** — Grounded Q&A; response carries citations + LLM provenance (FR-CEM-05).
* **`POST /cem/mode-b/optimize`** — `{ topLevelRequirementIds, constraints }` → trade-study run; ranked candidate architectures.
* **`GET /cem/interface-contract/{subsystemBlockId}`** — Current Interface Contract.
* **`POST /cem/mode-c/synthesize`** — `{ interfaceContractId }` → geometry, validation status, actuals.
* **`GET /cem/proposals/{branchId}`** / **`POST /cem/proposals/{id}/accept|reject`** — Review gate (autonomy-governed).
* **`POST /cem/campaigns`** — `{ elementId, solverIds[], parameterSweep, budget }` → governed Campaign (NFR-PERF-05). `budget` is mandatory; a Campaign without a cost/quota ceiling is rejected **[REV-B §C3]**.
* **`GET /cem/campaigns/{id}`** — Status + comparative/Pareto results.
* **`GET /cem/simulation-runs/{elementId}`** — `SimulationRun` provenance for an element, including result state (FR-CEM-13).
* **`PUT /cem/autonomy-level`** / **`GET /cem/autonomy-level/{scope}`** — Autonomy config; changes logged (NFR-CEM-06).

### 1.3 Safety & Mission Endpoints (Product 1)

* **`POST /safety/hazards`**, **`POST /safety/hazards/{id}/mitigations`**, **`GET /safety/risk-register/{projectId}`** (exportable ARP4761 / MIL-STD-882 / ISO 26262).
* **`POST /mission/missions`** / **`/use-cases`**, **`GET /mission/{id}/traceability`**.

### 1.4 Operations Endpoints **[REV-B §F]**

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
| `cem-core` | Mode B: **deterministic** 0D/1D models, mass-budget solver, allocation optimizer. Never calls an LLM to decide **[REV-B §D4]**. |
| `cem-geometry` | Mode C: solid-modeling kernel + manufacturing constraints (Product 2, research-risk). |
| `cem-connectors` | External FEA/CFD adapters, Campaign scheduling, result-state typing (§4.6). |
| `fuml-runtime` | **JVM sidecar** wrapping the fUML Reference Implementation (CPL/Apache); driven from Rust over **gRPC** (ADR-005, ADR-008, §9). Behavioral execution only; isolated so the JVM/Java-8 dependency does not touch the Rust build. |
| `alf-lite` | **In-house Rust** compiler for a minimal Alf subset (§9.6). Clean-room (public OMG spec only; no GPL RI code). Compiles to fUML for execution by `fuml-runtime` — a front-end only, no second runtime. |
| `llm-gateway` | Pluggable LLM provider behind one interface (local Ollama or hosted) — mirrors the solver-connector pattern **[REV-B §D4]**. |
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

* **Node labels:** `:Element` (base), `:Structure`, `:Requirement`, `:Port`, `:Hazard`, `:Control`, `:Mission`, `:Stakeholder`, `:SimulationRun`.
* **Edges:** `contains` (acyclic), `Satisfy`/`Verify`/`Refine`, `causes`/`mitigatedBy`, `validatedBy`, `Suspect`. Edges carry metadata (stereotype, multiplicity, provenance).

### 2.4 CEM Integration Map

| Component | Role |
| :--- | :--- |
| Graph (topology) + document store (bodies) | Context for Mode A; write-surface for Mode B/C, split per §2.2. |
| `sysml-core` semantic validation | Gates every write, human or AI (§4.2). |
| Git-backed MVS | AI proposals land as branch/Commit like human changes. |
| `collab` | AI proposals behave as another convergent editor; validity still enforced server-side. |
| `llm-gateway` | Mode A + drafting only. `cem-core` decisions stay deterministic. |
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
| **ADR-008** | **gRPC** is the standard transport for external-tool/process boundaries — the Java fUML sidecar (§9.5) and the `cem-connectors` solver adapters both use it; no mixing with bespoke REST. | Accepted |

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

The roadmap is no longer a single linear sequence. Product 1 ships independently; Product 2 proceeds on its own track and cannot block Product 1.

**Product 1 — MBSE Platform (must stand alone, no CEM):**
* **P1.1 Core Graph (Mo 1–2):** KerML/SysML v2 meta-model incl. `Hazard`/`Control`/`Mission`/`Stakeholder`; polyglot persistence wired (graph+doc+object); CRUD + semantic-validation layer; Git versioning; import (ReqIF/SysML v2).
* **P1.2 IDE Experience (Mo 3–4):** Monaco + LSP; canvas with **viewport virtualization** (NFR-PERF-01); Hazard/Risk panel; Mission timeline; provenance visual language scaffolding.
* **P1.3 Digital Thread (Mo 5):** budgeted traceability + change-impact; standards-aligned safety reporting.
* **P1.4 Behavioral Simulation + Pilot (Mo 6):** fUML execution via the `fuml-runtime` sidecar (ADR-005) over gRPC; `alf-lite` minimal Alf-subset compiler (§9.6) for the pilot's behaviors; pilot on a representative model; 1M-element load fixture in CI.

**Mode A fast-follow (after P1 stable):** grounded copilot, part search, requirement linting, docs→draft-model import. Delivery-risk.

**Product 2 — CEM (independent track):**
* **P2.1 Mode B (Mo 7–9):** `cem-core` deterministic optimizer; trade-study runner; Interface Contract schema.
* **P2.2 Contract + Autonomy + Review (Mo 9–10):** proposal/branch workflow; L0–L4 autonomy with Hazard override; generative-path concurrency policy. Validate the Interface Contract manually (humans consume it) before automating Mode C.
* **P2.3 Mode C — one subsystem (Mo 11–14, research-risk):** `cem-geometry` + `cem-connectors` against the Fan & LP Compression subsystem's casing/mounts (coldest, structural-only); one external FEA solver end-to-end; `scheduler` + typed result states live.
* **P2.4 Expand (Mo 15+):** more subsystems in increasing physics complexity (structural → aero → thermal); more solvers.

### 4.2 Semantic-Validation Layer (P1.1) **[REV-B §B1]**

Every write — human CRUD, CRDT-converged change, or AI proposal — passes the same `sysml-core` rule set before commit. Checks: type-legal relationship endpoints; containment acyclicity; parametric consistency; no dangling edges to deleted nodes. A converged-but-illegal state is **quarantined and surfaced as a conflict**, never persisted to Main. Convergence (`collab`) and validity (`sysml-core`) are separate guarantees.

### 4.3 IDE, Safety & Mission (P1.2)

* Monaco + LSP; ELK auto-layout; canvas virtualization (only viewport + margin live, off-screen subsystems clustered).
* Hazard/Risk matrix panel (Severity × Likelihood, filterable); Mission timeline (Concept→Disposal).
* Custom nodes for Blocks/Ports/Requirements/Hazards/Missions; provenance chrome per §6.3.
* **Testing:** round-trip text↔diagram consistency (single transaction); auto-layout < 500 ms at 500 blocks; 60 FPS at 10k *visible* elements *with* virtualization active.

### 4.4 Digital Thread & Import (P1.3)

* Budgeted traceability matrix (rows/cols configurable); change-impact "blast radius" within NFR-PERF-04 limits.
* Import: ReqIF, SysML v2 API, and AI-assisted docs→draft-model (FR-CORE-07).
* **Testing:** change-impact at 1M-element scale returns a *paginated* affected-set within the endpoint's declared p95; import round-trips a reference Cameo/ReqIF export without semantic loss.

### 4.5 Behavioral Simulation (P1.4)

* Discrete-event State Machine / Activity simulation. **Engine per ADR-005:** fUML execution is *adopted* (`fuml-runtime`, Java RI as a gRPC sidecar); Alf *authoring* is *built in-house* as `alf-lite` — a minimal, clean-room Alf-subset compiler targeting the same fUML (§9.6), scoped to the pilot's constructs and grown only on demand **[REV-B §D3]**.
* Interactive player, debugger, dashboards (time-series store).
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
| FR-CEM-02 | reqs §5.2 | (Mode B trade study) | `cem-core` |
| FR-CEM-03/13 | reqs §5.4 | Solver Result Typing, Plausibility Gate | `cem-connectors` |
| FR-CEM-05 | reqs §5.2 | LLM Provenance | `llm-gateway` |
| FR-CEM-14/15 | reqs §5.4 | Campaign Governance | `cem-connectors`, `scheduler` |
| FR-CEM-16/17/18 | reqs §5.6 | Autonomy Enforcement | `api`, `cem-core` |
| FR-SAFE-01…05 | impl §4.3/4.4 | Hazard Register | `sysml-core`, `api` |
| FR-MSN-01…04 | impl §4.3 | Mission Traceability | `sysml-core`, `api` |
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

1. **Language mismatch.** All the maintained engines are **Java** (Moka is Java/Eclipse; the RI is Java 8). Axioma's backend is Rust. There is no maintained, production-grade Rust f-UML/Alf engine. Embedding a Java engine means either a JVM sidecar service (clean process boundary, IPC/REST cost, extra runtime to operate) or JNI-style bridging (tighter, far more fragile). The sidecar is the only sane option and fits the service topology (§2.1) — it becomes another backing service like a solver behind `cem-connectors`.

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

- **Mature on both sides.** Rust via `tonic` (with `tonic-build` for codegen) and Java via official gRPC support; neither side is a second-class citizen.
- **Single versioned contract.** One `.proto` is the source of truth for the boundary — matching how the platform already treats the Interface Contract and solver boundaries. Protobuf's field-numbering/back-compat rules let the fUML runtime (on its own upstream release cadence) and the Rust backend evolve semi-independently.
- **Streaming fits the payload.** A behavioral simulation emits an *execution trace* — a time-ordered sequence of state transitions / token flows — not a single value. gRPC **server-streaming** maps directly onto that and onto the interactive player (Play/Pause/Step) and timeline view in P1.4. Plain request/response REST would force either a block-until-done call or a hand-rolled polling protocol.

**Caveats, recorded honestly:**
- gRPC solves *transport and typing*, not *semantics*. The fUML RI ingests XMI and emits a Java object graph; a decision remains on what crosses the wire — wrap XMI as an opaque `bytes` payload (simplest; Rust cannot introspect it) vs. define protobuf messages mirroring the exchanged fUML subset (cleaner, more work). This is the same "design the contract" work `cem-connectors` faces with solver I/O.
- **Use one RPC convention for both external-tool boundaries.** `cem-connectors` (solvers) crosses a comparable boundary; standardize on gRPC there too rather than mixing gRPC for fUML with bespoke REST for solvers. Recorded as **ADR-008**.
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
