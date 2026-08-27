# Axioma: Test Specification (Turbofan Pilot)

**Version:** 4.0
**Status:** Draft — Phase 0 doc-consolidation pass
**Supersedes:** Axioma_test_specification_v3.md
**Companion documents:** Axioma_requirements_v5.md, Axioma_implementation_v5.md
**Scope:** One acceptance-level test per implementation step, expressed against a single running example — the **turbofan engine** SoI and its five reference subsystems (Fan & LP Compression, Core/HP Compressor, Combustor, Turbine HP&LP, Control/FADEC). Each test states a concrete setup, the action, and **binary PASS/FAIL criteria** with numeric thresholds where the source NFR defines one.
**Change basis (v4):** Renamed from v3 per `docs/IMPLEMENTATION_KICKOFF.md` Phase 0's own recommendation ("candidate for a v4 rename... given the volume of additions"). All v3 tests carried forward unchanged; new tests appended for every FR group merged into `Axioma_requirements_v5.md` that already specifies concrete test scenarios (`FR-PARAM`, `FR-INFO`, `FR-INTX`, `FR-EXPORT`, `FR-CORE-10/11/12/13`, the amended `FR-CORE-03`, `FR-CORE-14…18`). **`FR-COMP-01…06` and `FR-ARCH-01…08` have no test rows yet** — the amendment that introduced them didn't specify test scenarios the way the others did; authoring those is `IMPLEMENTATION_KICKOFF.md` Phase 6 work, not done in this pass.

---

## 0. Conventions

* **Running fixture — `Turbofan-Ref`.** A shared, versioned reference model reused across phases and grown as phases unlock features:
  - **P1.1+** structural: `Engine` block composed of the five subsystem blocks, each with the ports from Axioma_requirements_v5.md §5.5.
  - **P1.2+** a top requirement `REQ-THRUST` ("Engine shall provide ≥ 30,000 lbf takeoff thrust") and `REQ-SFC` ("... below a specified fuel rate"), plus one `Hazard` (`HAZ-OVERSPEED`) and one `Mission` (`MSN-CLIMB`).
  - **P1.4+** a State Machine for the Control subsystem (`Idle → Armed → Running → Shutdown`).
  - **P2+** the `Turbine` and `Fan` Interface Contracts.
* **`Turbofan-Scale`.** A synthetic 1,000,000-element engine model (five subsystems recursively elaborated to part level) used only for performance/scale tests (NFR-PERF-06).
* **Result states.** A test is **PASS** only if *all* listed criteria hold; any single failed criterion is **FAIL**. "Observed via" names where evidence is captured (API response, audit log, trace, screenshot-diff).
* **Traceability.** Each test cites the requirement(s) it verifies; the reverse mapping lives in Axioma_implementation_v5.md §7.

---

## Phase P1.1 — Core Graph

### T-P1.1-01 — Element CRUD & KerML typing
**Verifies:** FR-CORE-01
**Setup:** Empty project.
**Action:** `POST /projects/{id}/elements` to create the `Engine` block and its five subsystem blocks; read each back.
**PASS:**
- All six elements created with unique UUIDs; each `GET` returns the stored element unchanged.
- Node labels are KerML-correct (`:Structure` for blocks); response schema validates against the OMG SysML v2 API contract.
**FAIL:** any element missing/altered on read-back, a duplicate UUID accepted, or a schema-invalid response.

### T-P1.1-02 — Semantic-validation layer rejects an illegal relationship
**Verifies:** FR-CORE-05, NFR-REL-01
**Setup:** `Turbofan-Ref` structural.
**Action:** Attempt to create a `Satisfy` edge from the `Combustor` block to the `Turbine` block (Satisfy must target a Requirement, not a Block).
**PASS:**
- Request rejected with `400`, error names the violated KerML rule and the offending endpoint types.
- No partial write: a follow-up read shows no `Satisfy` edge and no orphaned nodes.
**FAIL:** edge persisted, generic/no error code, or any residual partial state.

### T-P1.1-03 — Containment acyclicity enforced; traceability cycles allowed
**Verifies:** NFR-REL-02
**Setup:** `Turbofan-Ref`.
**Action (a):** Attempt to make `Engine` a containment child of its own child `Turbine` (would cycle the containment hierarchy). **Action (b):** Create a legitimate cycle across *non-containment* edges: `REQ-THRUST` —Satisfy→ `Turbine` and `Turbine` —Refine→ `REQ-THRUST`.
**PASS:**
- (a) rejected as a containment-cycle violation.
- (b) accepted; a traceability query across the cycle terminates (visited-set) and returns without error or infinite loop.
**FAIL:** (a) accepted, or (b) rejected/hangs.

### T-P1.1-04 — Polyglot persistence split
**Verifies:** NFR-DATA-01, NFR-DATA-02
**Setup:** Create `REQ-THRUST` with a 20 KB rationale text body; attach a placeholder geometry pointer to `Turbine`.
**Action:** Inspect where each datum lands.
**PASS:**
- Neo4j holds the node + relationships but **not** the 20 KB body; the body is in the document store; the geometry reference in the graph is a pointer to object storage, not inlined bytes.
**FAIL:** large body or binary stored inside the graph.

### T-P1.1-05 — Git-backed versioning (branch/commit)
**Verifies:** FR-CORE-01 (versioning), NFR-COMP-04
**Setup:** `Turbofan-Ref` on `main`.
**Action:** Branch `lightweight-fan`; change the Fan block's `mass` property; commit; diff against `main`.
**PASS:**
- Branch/commit succeed; diff reports exactly the one changed property with old/new values, actor, and timestamp.
- The write appears in the audit log with actor/timestamp/diff.
**FAIL:** diff misses or over-reports the change, or the audit entry lacks any of actor/timestamp/diff.

### T-P1.1-06 — ReqIF / SysML v2 import fidelity
**Verifies:** FR-CORE-07
**Setup:** A reference ReqIF export of 50 turbofan requirements and a SysML v2 API export of the structural model.
**Action:** `POST /import/reqif` then `/import/sysml-v2`; round-trip export and compare.
**PASS:**
- All 50 requirements imported with IDs/text/attributes intact; structural import reproduces the five-subsystem hierarchy and all ports.
- Round-trip export is semantically equal to input (no lost elements, relationships, or attributes).
**FAIL:** any dropped/altered element, relationship, or attribute.

### T-P1.1-07 — Element-create latency (DoD gate)
**Verifies:** NFR-PERF-02
**Setup:** `Turbofan-Scale` loaded.
**Action:** 1,000 sequential element creates; record latency distribution.
**PASS:** p95 element-create < 100 ms; p50 < 50 ms; zero errors.
**FAIL:** p95 ≥ 100 ms or any error.

### T-CORE-03-EXT — Requirements dependency taxonomy (`Derive`/`Copy`) **[REV-D]**
**Verifies:** FR-CORE-03 (amended)
**Setup:** `Turbofan-Ref` with `REQ-THRUST` and a lower-level requirement `REQ-LOW` feeding into it.
**Action:** Create a `Derive` edge from `REQ-LOW` to `REQ-THRUST`, and a `Copy` edge duplicating `REQ-LOW` into another scope.
**PASS:** Both edges queryable via the same traceability endpoint as Satisfy/Verify/Refine; `Derive` is distinguishable from a Containment edge in query results.
**FAIL:** Edge type not persisted, or indistinguishable from Containment.

### T-INFO-01 — Information element modeling & specialization **[REV-D]**
**Verifies:** FR-INFO-01, FR-INFO-02
**Setup:** Empty information-architecture view.
**Action:** Create an Information Element `ControlData` at Conceptual level; specialize to a Logical element `PowerLevelParam` with a custom Data Type `Watts`.
**PASS:** Specialization traceable; `Watts` Data Type usable to type a Value Property elsewhere in the model.
**FAIL:** Specialization edge missing, or Data Type not reusable outside its defining scope.

### T-INFO-02 — Information flow typing **[REV-D]**
**Verifies:** FR-INFO-04
**Setup:** Two Actions connected by an Object Flow.
**Action:** Type the flow using an Information Element.
**PASS:** Flow's typed content is queryable back to the Information Element (not an untyped string label).
**FAIL:** Flow content is a free-text label with no graph link.

---

## Phase P1.2 — IDE Experience

### T-P1.2-01 — Text ↔ diagram round-trip, single transaction
**Verifies:** FR-CORE-02
**Setup:** `Turbofan-Ref` open in split-pane.
**Action:** Rename `Combustor` → `AnnularCombustor` by editing the label on the canvas.
**PASS:**
- Monaco text reflects `AnnularCombustor` in < 50 ms; backend records **exactly one** transaction; no duplicate/ghost element.
**FAIL:** text not updated, > 50 ms, or > 1 transaction.

### T-P1.2-02 — Canvas virtualization at scale
**Verifies:** NFR-PERF-01
**Setup:** `Turbofan-Scale` open; zoom to a region showing ~8,000 elements.
**Action:** Continuous pan/zoom for 30 s while sampling frame rate; inspect what is instantiated.
**PASS:**
- Sustained ≥ 60 FPS with ≥ 10,000 visible elements; off-screen subsystems are clustered/collapsed, **not** held as live nodes (verified via render-node count ≪ total model size).
**FAIL:** FPS drops below 55 sustained, or the client instantiates the full model.

### T-P1.2-03 — Auto-layout quality/latency
**Verifies:** FR-CORE-02 (graphical parity)
**Setup:** Expand `Turbine` to 500 sub-parts with 1,000 specialization edges.
**Action:** Trigger ELK auto-layout.
**PASS:** layout completes < 500 ms; no edge routes through a node's interior (automated overlap check = 0 violations).
**FAIL:** ≥ 500 ms or ≥ 1 line-through-node.

### T-P1.2-04 — Hazard/Risk matrix panel
**Verifies:** FR-SAFE-01, FR-SAFE-02
**Setup:** `HAZ-OVERSPEED` linked to the `Turbine` block via `causes`.
**Action:** Score it Severity=Catastrophic, Likelihood=Remote in the panel.
**PASS:**
- Risk Index auto-computed per the configured 5×5 matrix; the `Turbine` block shows a hazard indicator; the hazard is filterable by subsystem.
**FAIL:** wrong/absent Risk Index, or no visible linkage on the block.

### T-P1.2-05 — Mission timeline
**Verifies:** FR-MSN-01, FR-MSN-03
**Setup:** `MSN-CLIMB` defined; tag `REQ-THRUST` to the "Operations" phase.
**Action:** Open the Mission timeline.
**PASS:** phases render Concept→Disposal; `REQ-THRUST` appears under Operations; retagging moves it live.
**FAIL:** phase missing, or tag not reflected.

### T-P1.2-07 — Mitigation/Control tracking & residual risk
**Verifies:** FR-SAFE-03
**Setup:** `HAZ-OVERSPEED` (Severity=Catastrophic) on the `Turbine`.
**Action:** Add a Control ("FADEC overspeed cutoff") linked via `mitigatedBy`; set its status to Mitigated.
**PASS:** the Control links to the hazard; residual risk recomputes and drops; status shows Mitigated; an unmitigated hazard with no Control shows residual = raw risk.
**FAIL:** residual risk unchanged after mitigation, or status not tracked.

### T-P1.2-08 — Stakeholder management
**Verifies:** FR-MSN-02
**Setup:** `MSN-CLIMB` defined.
**Action:** Create a Stakeholder ("Propulsion Chief Engineer") with a concern ("climb-rate margin") linked to `MSN-CLIMB` and `REQ-THRUST`.
**PASS:** stakeholder, concern, and both links persist and are traversable from either the mission or the requirement.
**FAIL:** any link missing or not traversable.

### T-P1.2-06 — Provenance visual language (scaffolding)
**Verifies:** FR-CORE-08
**Setup:** Create one block by hand; mark a second as `ai-suggested` via the API.
**Action:** Render both; apply the filter "AI-suggested only".
**PASS:** the two render with distinct origin encodings (border style per §6.3); the filter shows only the AI-suggested block.
**FAIL:** indistinguishable rendering or incorrect filter result.

### T-CORE-10-01 — Dynamic Query live re-evaluation **[REV-D]**
**Verifies:** FR-CORE-10
**Setup:** `Turbofan-Ref`.
**Action:** Save a Dynamic Query for "all Blocks with stereotype `subsystem`"; add a new matching Block after save.
**PASS:** New Block appears in the collection on next re-evaluation without manual action.
**FAIL:** New Block absent, or the query required unbounded traversal (should be rejected per NFR-PERF-04, not silently allowed).

### T-CORE-12-01 — Swimlane allocation & orphan-Action rejection **[REV-D]**
**Verifies:** FR-CORE-12, FR-CORE-13
**Setup:** An Activity-equivalent diagram with 3 Swimlanes allocated to 3 of the five subsystem Blocks.
**Action:** Add an Action with no incoming/outgoing flow.
**PASS:** Orphan Action rejected per FR-CORE-13; allocated Actions show the correct owning-Block stereotype.
**FAIL:** Orphan Action silently accepted, or allocation not visible/queryable.

---

## Phase P1.3 — Digital Thread

### T-P1.3-01 — Change-impact "blast radius" at scale, budgeted
**Verifies:** FR-CORE-03, NFR-PERF-04
**Setup:** `Turbofan-Scale`; `REQ-THRUST` linked (directly/indirectly) to ~1,200 downstream elements.
**Action:** `GET /elements/REQ-THRUST/traceability?depth=5&maxFanout=…`; change `REQ-THRUST` and request the affected set.
**PASS:**
- Affected set returned **paginated** (cursor), within the endpoint's declared p95 at 1M scale (target < 2 s for the first page); all ~1,200 true dependents recovered across pages; no dependent missed, none spurious.
**FAIL:** unpaginated dump, p95 over target, or any missed/spurious dependent.

### T-P1.3-02 — Query-budget enforcement
**Verifies:** NFR-PERF-04
**Action:** Issue a traceability request with no depth/fan-out limit (or above the cap).
**PASS:** request rejected (or clamped with an explicit notice); server never attempts an unbounded traversal.
**FAIL:** server runs the unbounded query.

### T-P1.3-03 — Traceability breach on delete
**Verifies:** FR-CORE-03, FR-SAFE-04
**Setup:** `REQ-THRUST` with 10 `Satisfy` dependents.
**Action:** Delete `REQ-THRUST`.
**PASS:** a "Traceability Breach" warning lists all 10 now-orphaned dependents; deletion requires acknowledge/reassign.
**FAIL:** silent delete, or incomplete orphan list.

### T-P1.3-04 — Safety register export (standards format)
**Verifies:** FR-SAFE-05
**Setup:** `HAZ-OVERSPEED` with one Control (status Mitigated).
**Action:** `GET /safety/risk-register/{projectId}` in ARP4761 format.
**PASS:** export includes hazard, severity/likelihood/Risk Index, linked Control, residual status; structure matches the ARP4761 template.
**FAIL:** any field missing or template mismatch.

### T-P1.3-05 — Mission traceability completeness
**Verifies:** FR-MSN-04
**Setup:** `Turbofan-Ref` where every requirement but one is linked to a Mission/UseCase.
**Action:** Run the mission-coverage check.
**PASS:** the one orphan requirement is flagged; all Mission→Requirement→Block chains resolve.
**FAIL:** orphan not flagged, or a valid chain not resolved.

### T-EXPORT-01 — Full-diagram image export at scale **[REV-D]**
**Verifies:** FR-EXPORT-01
**Setup:** A 500-element diagram (the `Turbine` expansion fixture from T-P1.2-03).
**Action:** Export as PNG at full extent, not just viewport.
**PASS:** Full diagram rendered server-side; client does not need to instantiate all 500 elements to trigger it.
**FAIL:** Export limited to visible viewport, or client-side memory spike.

### T-EXPORT-02 — Filtered tabular export **[REV-D]**
**Verifies:** FR-EXPORT-02
**Setup:** A 50-requirement Requirements Table (T-P1.1-06's imported set).
**Action:** Filter to 10 requirements and 4 of 8 columns; export to XLSX.
**PASS:** Exported file matches exactly the filtered/column-selected view.
**FAIL:** Export includes unfiltered rows/columns.

### T-EXPORT-03 — Model report generation, shared template mechanism **[REV-D]**
**Verifies:** FR-EXPORT-03
**Setup:** `MSN-CLIMB` with its linked Requirements.
**Action:** Generate a report scoped to the Mission.
**PASS:** Includes the Mission's Requirements and scenario reference; uses the same template mechanism as FR-SAFE-05 (verified: no second, divergent template engine in the codebase).
**FAIL:** Missing content, or a parallel/divergent report pipeline exists.

### T-DOCIMPORT-01 — Document requirement extraction **[REV-D]**
**Verifies:** FR-CORE-14
**Setup:** Upload a 10-page turbofan requirements PDF with 20 clear "shall" statements.
**Action:** `POST /import/documents`; poll job status.
**PASS:** Job completes; 20 `Requirement` candidates produced, each with name/ID/text/category.
**FAIL:** Any statement dropped without being surfaced as low-confidence (FR-CORE-18), or the job blocks synchronously instead of running as a job.

### T-DOCIMPORT-02 — Citation & generation provenance **[REV-D]**
**Verifies:** FR-CORE-15
**Setup:** A completed extraction job from T-DOCIMPORT-01.
**Action:** Inspect a drafted Requirement's provenance via `GET /import/documents/{jobId}/candidates`.
**PASS:** Citation present (page + offset where extractable); LLM generation provenance fields all present; missing any one of these fails validation before the proposal is created.
**FAIL:** Requirement reaches the review UI with a missing citation or missing generation provenance.

### T-DOCIMPORT-03 — Review-gate reuse (`document-import` origin) **[REV-D]**
**Verifies:** FR-CORE-16
**Setup:** A completed extraction job.
**Action:** `POST /import/documents/{jobId}/proposal`; open the resulting proposal.
**PASS:** Proposal appears via the existing `GET /cem/proposals/{branchId}` endpoint with `origin: document-import`, `autonomyLevel: n/a`; accept/reject works identically to a `human-authored` or `cem-generated` proposal.
**FAIL:** A second, divergent review UI/endpoint exists for document-import proposals.

### T-DOCIMPORT-04 — Candidate structure suggestions, non-binding **[REV-D]**
**Verifies:** FR-CORE-17
**Setup:** Upload a document that mentions "the Combustor assembly" in prose with no prior Combustor Block in the model.
**Action:** Inspect `GET /import/documents/{jobId}/suggestions`.
**PASS:** "Combustor" appears as a candidate structure suggestion, display-only; no `:Structure` element is created automatically.
**FAIL:** A Block is auto-created without human action.

### T-DOCIMPORT-05 — Low-confidence / malformed extraction handling **[REV-D]**
**Verifies:** FR-CORE-18
**Setup:** Upload a document with one requirement split across a page break and one requirement embedded in a table.
**Action:** Inspect candidates.
**PASS:** Both are surfaced with a lower confidence score and a flag explaining why, not silently merged incorrectly or dropped.
**FAIL:** Either statement is missing from the candidate list with no failure/flag recorded.

### T-DOCIMPORT-06 — Product-2 independence **[REV-D]**
**Verifies:** FR-CORE-14, NFR-CEM-03
**Setup:** A local-Ollama-only deployment with no CEM services running (`cem-core`/`cem-connectors`/`scheduler` absent).
**Action:** Run the full pipeline against a reference requirements PDF.
**PASS:** Pipeline completes successfully — confirms FR-CORE-07 has no hidden Product-2 dependency.
**FAIL:** Pipeline fails or silently calls a CEM-tier service.

### T-DOCIMPORT-07 — Scanned PDF / OCR **[REV-D]**
**Verifies:** FR-CORE-14
**Setup:** A scanned (image-only) PDF of requirements.
**Action:** Upload; run extraction.
**PASS:** OCR runs automatically; extraction proceeds without a separate user action.
**FAIL:** Job fails or requires a manual OCR step outside the platform.

---

## Phase P1.4 — Behavioral Simulation & Pilot

### T-P1.4-01 — fUML sidecar execution over gRPC (deterministic)
**Verifies:** FR-CORE-04, NFR-CEM-02
**Setup:** Control State Machine `Idle → Armed → Running → Shutdown`, driven by signals (`arm`, `ignite`, `cutoff`).
**Action:** Execute via `fuml-runtime` over gRPC, streaming the trace; run 100 times with identical inputs.
**PASS:**
- The transition sequence and final state are identical across all 100 runs; the trace streams incrementally (server-streaming), not one terminal blob.
**FAIL:** any run diverges, or the trace only arrives on completion.

### T-P1.4-02 — `alf-lite` subset conformance
**Verifies:** FR-CORE-09
**Setup:** An Alf action on the `Armed→Running` transition using only in-subset constructs (a guard comparison + a behavior invocation setting `Turbine.rpm`).
**Action:** Compile with `alf-lite` → fUML → execute in `fuml-runtime`.
**PASS:** compiles without error; produced fUML executes to the golden trace (`Turbine.rpm` set as specified).
**FAIL:** compile error on an in-subset construct, or trace mismatch.

### T-P1.4-03 — `alf-lite` unsupported-construct safety
**Verifies:** FR-CORE-09, §9.6
**Action:** Compile an Alf action using an out-of-subset construct (e.g. a collection-sequence operator).
**PASS:** precise compile-time error naming the unsupported construct; **no** partial/incorrect fUML emitted.
**FAIL:** silent partial compile, generic error, or any emitted output.

### T-P1.4-04 — `alf-lite` ↔ direct-fUML equivalence
**Verifies:** FR-CORE-09
**Action:** Author the `Armed→Running` behavior two ways — via `alf-lite`, and hand-authored as fUML — and execute both.
**PASS:** identical execution traces.
**FAIL:** any divergence (indicates `alf-lite` is not a faithful front-end).

### T-P1.4-05 — Pilot trade study end-to-end
**Verifies:** FR-CORE-02/03/04 integration
**Setup:** `Turbofan-Ref` with the Control behavior and `REQ-THRUST`.
**Action:** Branch; swap a Fan variant (different bypass ratio); run the behavioral sim; generate a comparison report — timed.
**PASS:** full loop (branch → swap → simulate → report) completes in < 30 min by a pilot engineer; report shows the thrust-relevant delta between variants.
**FAIL:** loop exceeds 30 min or the report omits the comparison.

### T-P1.4-06 — 1M-element load fixture (CI gate)
**Verifies:** NFR-PERF-03, NFR-PERF-06
**Setup:** `Turbofan-Scale`.
**Action:** CI loads the fixture and runs the P1.2/P1.3 perf assertions.
**PASS:** load < 5 s; client memory < 2 GB; all referenced perf budgets green; a regression fails the build.
**FAIL:** any budget breached or the gate not wired into CI.

### T-INTX-01 — Message-based interaction with timing constraint **[REV-D]**
**Verifies:** FR-INTX-01, FR-INTX-02
**Setup:** Control (FADEC/EEC) and Turbine (HP & LP) as the two participating Blocks.
**Action:** Model a 3-step message exchange (arm → ignite → cutoff acknowledgment) with a Duration Constraint between steps 1 and 3.
**PASS:** Exchange renders in time order; Duration Constraint evaluable/checkable against a supplied timing value.
**FAIL:** Steps unordered, or constraint not attached to the correct pair of points.

### T-INTX-02 — Bounded loop sub-interaction **[REV-D]**
**Verifies:** FR-INTX-03
**Setup:** The Interaction from T-INTX-01.
**Action:** Add a loop sub-sequence (a repeated sensor poll) with a guard condition.
**PASS:** Guard editable and evaluated at simulation/evaluation time; loop bounded (no infinite-loop acceptance without an explicit bound or budget).
**FAIL:** Unbounded loop accepted with no safeguard.

### T-PARAM-01 — Constraint definition & binding **[REV-D]**
**Verifies:** FR-PARAM-01, FR-PARAM-02
**Setup:** The `Turbine` block, with two Value Properties (`PowerLevel`, `TimeValue`).
**Action:** Define Constraint `CookEnergy = PowerLevel * TimeValue`; bind to both properties.
**PASS:** Binding succeeds with type-checked parameters; Constraint appears in the `Turbine` block's Relations.
**FAIL:** Binding accepted with mismatched types, or Constraint not visible in traceability queries.

### T-PARAM-02 — Parametric evaluation, no CEM dispatch **[REV-D]**
**Verifies:** FR-PARAM-03
**Setup:** The Constraint from T-PARAM-01.
**Action:** Set `PowerLevel=750`, `TimeValue=120`; evaluate.
**PASS:** Returns `CookEnergy=90000` without invoking `cem-core`/`cem-connectors`/`scheduler` (verified via trace — no spans from those services).
**FAIL:** Wrong value, or evaluation dispatches to a Product-2 service.

---

## Phase P2.1 — CEM Mode B (System-Level Synthesis)

### T-P2.1-01 — Deterministic trade-study optimizer
**Verifies:** FR-CEM-02, NFR-CEM-02
**Setup:** `REQ-THRUST` + `REQ-SFC` as top-level inputs.
**Action:** `POST /cem/mode-b/optimize` twice with identical inputs and graph state.
**PASS:**
- Returns ranked candidate allocations across the five subsystems; the two identical runs produce **identical** rankings (deterministic optimizer, no LLM in the decision path).
**FAIL:** non-identical results on identical input, or an LLM call in the decision path (verified via `cem-core` having no `llm-gateway` dependency).

### T-P2.1-02 — Funnel efficiency (breadth)
**Verifies:** NFR-CEM-05, NFR-CEM-01
**Action:** Run a trade study exploring fan/compressor/turbine allocation variants.
**PASS:** evaluates ≥ dozens of candidate architectures within one interactive session; Mode C is **not** invoked during Mode B exploration.
**FAIL:** Mode C triggered speculatively, or the study cannot explore at breadth within a session.

### T-P2.1-03 — Interface Contract emission
**Verifies:** FR-CEM-08
**Setup:** A chosen candidate architecture.
**Action:** `GET /cem/interface-contract/Turbine`.
**PASS:** contract contains all six fields (performance targets, boundary conditions, geometric envelope, interface/port defs, mass/cost targets, material/process constraints) populated for the Turbine subsystem.
**FAIL:** any field missing or empty for a subsystem that requires it.

### T-P2.1-04 — LLM generation provenance (Mode A drafting)
**Verifies:** FR-CEM-05, NFR-CEM-04
**Setup:** Ask Mode A to draft a "Shall" statement for a turbine life requirement.
**Action:** Inspect the produced element's provenance.
**PASS:** element records model name+version, prompt-template hash, temperature/seed, context snapshot; every factual claim carries a citation.
**FAIL:** any provenance field absent, or an uncited claim.

### T-P2.1-05 — Mode A grounded retrieval with citations
**Verifies:** FR-CEM-01, NFR-CEM-04
**Setup:** `Turbofan-Ref` with `REQ-THRUST`, the Turbine block, and its `SimulationRun` history in the graph.
**Action:** `POST /cem/mode-a/query` — "What verifies the takeoff-thrust requirement, and what is the current turbine stage mass?"
**PASS:**
- Answer is grounded in graph facts (correct verifying element and current mass); **every** claim cites a source element ID or document; no un-sourced assertion; a question with no graph support returns "not found," not a fabrication.
**FAIL:** any uncited claim, a wrong/hallucinated fact, or a confident answer where the graph has no basis.

### T-P2.1-06 — Auto-traceability of generated elements
**Verifies:** FR-CEM-04
**Action:** At L1, accept a Mode B-generated turbine-stage block created to satisfy a turbine sub-requirement.
**PASS:** on acceptance, a `Satisfy` edge to the source requirement is created automatically, tagged `source: ai-generated`, and the element carries its generation provenance (FR-CEM-05 link).
**FAIL:** missing/incorrect `Satisfy` edge, or missing `source: ai-generated` tag.

---

## Phase P2.2 — Interface Contract, Autonomy & Review

### T-P2.2-01 — Proposal lands as reviewable branch
**Verifies:** FR-CEM-07
**Action:** At L1, run a Mode B synthesis producing new turbine-stage blocks.
**PASS:** results appear as a proposal on a branch (not on `main`); each element is individually accept/reject-able.
**FAIL:** anything written to `main` without review at L1.

### T-P2.2-02 — Autonomy level enforcement (L3 threshold)
**Verifies:** FR-CEM-16, FR-CEM-17
**Setup:** L3 with a mass-deviation threshold of 5%.
**Action:** Generate two turbine variants: one 3% over the Mode B mass target, one 12% over.
**PASS:** the 3% variant auto-merges; the 12% variant drops to individual review.
**FAIL:** the 12% variant auto-merges, or the 3% one is needlessly held.

### T-P2.2-03 — Safety override is absolute (L4)
**Verifies:** FR-CEM-18
**Setup:** L4 (full autonomy); a generated turbine part linked to unmitigated `HAZ-OVERSPEED`.
**Action:** Run the autonomous loop.
**PASS:** the hazard-linked part is **forced to individual human review** despite L4; a lock indicator is shown; it never auto-merges.
**FAIL:** the part auto-merges at L4.

### T-P2.2-04 — Autonomy change is audited
**Verifies:** NFR-CEM-06
**Action:** Change the project autonomy L1→L4 via `PUT /cem/autonomy-level`.
**PASS:** audit log records actor, timestamp, old=L1, new=L4.
**FAIL:** change unlogged or missing any field.

### T-P2.2-05 — Generative-path concurrency (human wins)
**Verifies:** NFR-OPS-04
**Setup:** An in-flight Mode C write targeting the `Fan` casing block while an engineer edits the same block.
**Action:** Commit the human edit before the autonomous write lands.
**PASS:** human edit is applied; the autonomous result is re-queued against the new state, **not** force-merged over the human change.
**FAIL:** autonomous write overwrites the human edit.

---

## Phase P2.3 — CEM Mode C + Connector Framework

### T-P2.3-01 — Geometry synthesis within the Interface Contract
**Verifies:** FR-CEM-09
**Setup:** The Fan & LP Compression casing/mount Interface Contract.
**Action:** `POST /cem/mode-c/synthesize`.
**PASS:** produces manufacturable geometry whose bounding envelope, interfaces/ports, and material fall within the contract; nothing outside the contract's declared fields is required.
**FAIL:** geometry violates the envelope/ports, or synthesis demands data absent from the contract (indicates contract incompleteness).

### T-P2.3-02 — Assembly composition
**Verifies:** FR-CEM-10
**Action:** Synthesize a casing + mount + fastener set and compose them.
**PASS:** parts mate per the contract's port definitions; the assembly is valid (no interpenetration; interfaces aligned).
**FAIL:** mating violates ports, or geometric interference is present.

### T-P2.3-03 — Connector round-trip & result provenance
**Verifies:** FR-CEM-12, FR-CEM-19
**Action:** Submit the generated mount to the configured external FEA solver via `cem-connectors`.
**PASS:** a `SimulationRun` node is created recording solver name+version+settings+input-hash+timestamp, linked to the mount via `validatedBy`; large result files are stored in object storage and referenced by pointer.
**FAIL:** missing/incomplete `SimulationRun`, no `validatedBy` edge, or result bytes inlined into the graph.

### T-P2.3-04 — Solver result states gate autonomy
**Verifies:** FR-CEM-13, NFR-REL-03
**Setup:** L4; force a solver run to **diverge** (bad BC / non-convergent mesh).
**Action:** Run Mode C validation on the mount.
**PASS:** run typed `Diverged`; the item is **not** auto-merged despite L4; it drops to human review. Only a `Converged`-within-bounds run could have satisfied the gate.
**FAIL:** a non-`Converged` result treated as a pass, or auto-merged.

### T-P2.3-05 — Plausibility gate catches a "valid-but-wrong" pass
**Verifies:** FR-CEM-13
**Setup:** A solver returns `Converged` but with a physically impossible result (negative safety factor).
**Action:** Ingest the result.
**PASS:** the plausibility pass flags it `Suspect-Numerical` **before** any graph write; it does not satisfy the gate.
**FAIL:** the implausible "pass" is written as a validated result.

### T-P2.3-06 — Multi-run Campaign & comparative evaluation
**Verifies:** FR-CEM-14, FR-CEM-15
**Setup:** A Campaign of 5 mount geometry variants against the same contract, with a cost budget set.
**Action:** `POST /cem/campaigns` (with `budget`); await results.
**PASS:**
- All 5 dispatched in parallel via the scheduler; results ranked against contract targets (e.g. lowest mass among variants meeting the stress-margin threshold), presented as a comparison/Pareto view.
**FAIL:** incorrect ranking, or serial-only dispatch when parallel capacity exists.

### T-P2.3-07 — Campaign resource governance
**Verifies:** NFR-PERF-05
**Action (a):** Submit a Campaign with **no** `budget`. **Action (b):** Run an L4 loop that would exceed the project cost ceiling.
**PASS:** (a) rejected for missing budget; (b) halted at the ceiling with a clear reason; neither runs unbounded.
**FAIL:** budget-less Campaign accepted, or L4 loop exceeds the ceiling.

### T-P2.3-08 — Bidirectional MDO feedback (Suspect propagation)
**Verifies:** FR-CEM-11
**Setup:** Mode C returns an actual mount mass 12% above the Mode B target.
**Action:** Ingest actuals.
**PASS:** the mount's `mass` property updates; dependents are flagged `Suspect` and shown with the staleness encoding (§6.3); re-optimization is triggered or offered per the active autonomy level.
**FAIL:** actuals not written back, or no `Suspect` propagation on a material deviation.

### T-P2.3-09 — Validation-gate tagging (Validated vs. Unverified)
**Verifies:** FR-CEM-03
**Setup:** Two generated mounts — one with a `Converged`-within-bounds FEA run, one never submitted to a solver.
**Action:** Inspect each element's validation tag.
**PASS:** the solver-backed mount is tagged `Validated` (with its `SimulationRun` link); the unsubmitted one is tagged `Unverified`; only the `Validated` one can satisfy an autonomy gate.
**FAIL:** an unverified element tagged as validated, or the tag absent.

### T-P2.3-10 — Feedback ingestion improves future generation
**Verifies:** FR-CEM-06
**Setup:** A first Fan-casing generation whose FEA showed a stress hotspot at a fillet; the run's outcome is ingested.
**Action:** Request a second generation of the same part type under the same contract.
**PASS:** the second generation reflects the ingested outcome for that element type (e.g. the known-bad fillet geometry is not reproduced); the influence is traceable to the prior `SimulationRun`.
**FAIL:** identical known-bad output with no evidence the prior outcome was used.

---

## Phase P2.4 — Expansion (regression as complexity grows)

### T-P2.4-01 — Second subsystem reuses the same contract/feedback machinery
**Verifies:** FR-CEM-08/09/13 (regression)
**Setup:** Extend Mode C from the Fan casing to a Core-Compressor stator (aero, higher complexity) using a CFD connector.
**Action:** Run synthesis → CFD Campaign → feedback, with **no** contract-schema changes.
**PASS:** the same Interface Contract schema and result-state/feedback pipeline work unchanged for a more complex subsystem; only the solver adapter differs.
**FAIL:** the contract schema or pipeline needs structural change to accommodate the new subsystem.

### T-P2.4-02 — Multi-solver Campaign
**Verifies:** FR-CEM-12, FR-CEM-14
**Action:** A Campaign spanning two configured solvers (one FEA, one CFD) on a mixed part set.
**PASS:** both solvers driven through the common connector interface; results normalized into comparable `SimulationRun` records.
**FAIL:** a solver requires bypassing the common interface, or results are not comparable.

---

## Cross-Cutting Tests (run every phase from the point the capability exists)

### T-X-01 — Convergence vs. validity under concurrent edit
**Verifies:** FR-CORE-05, FR-CORE-06, NFR-REL-01
**Setup:** Two engineers edit the `Turbine` block; one adds a `Power` port, the other concurrently deletes the block.
**Action:** Let CRDT converge.
**PASS:** clients converge to one state; the semantic-validation pass detects the dangling-port illegality and **quarantines** it as a surfaced conflict — the illegal state is never committed to `main`.
**FAIL:** an illegal state silently persists, or clients diverge.

### T-X-02 — Multi-tenant isolation (default deployment)
**Verifies:** NFR-OPS-02
**Setup:** Two projects, `Turbofan-A` and `Turbofan-B`, different tenants, shared deployment.
**Action:** As a `Turbofan-A` user, attempt to read a `Turbofan-B` element by ID.
**PASS:** access denied; tenancy key enforced in the data-access layer; the B element is not returned.
**FAIL:** any cross-tenant read succeeds.

### T-X-03 — Observability: one correlated trace
**Verifies:** NFR-OPS-01
**Action:** A Mode C synthesis request that fans out to `cem-geometry` → `cem-connectors` → external solver.
**PASS:** the request produces a single correlated distributed trace spanning all services with timing per span; metrics exported.
**FAIL:** missing spans or no correlation across services.

### T-X-04 — Rate limiting on autonomy-driven calls
**Verifies:** NFR-OPS-03
**Action:** An L4 loop issues LLM/solver calls above the configured rate.
**PASS:** calls are throttled/queued per the limit; no endpoint is overwhelmed.
**FAIL:** limits not enforced under programmatic load.

### T-X-05 — Backup / restore to RPO/RTO
**Verifies:** NFR-REL-04
**Setup:** `Turbofan-Ref` with history; take a backup; simulate data loss.
**Action:** Restore from backup.
**PASS:** graph + document store + Git model store restored consistently within the stated RTO; data loss within the stated RPO; a post-restore traceability query on `Turbofan-Ref` succeeds.
**FAIL:** inconsistent restore, or RPO/RTO exceeded.

### T-X-06 — Schema migration on a large model
**Verifies:** NFR-REL-05
**Setup:** `Turbofan-Scale` on schema vN; introduce a node-schema change (e.g. add a required field to `Hazard`).
**Action:** Run the versioned migration.
**PASS:** all affected nodes migrated with no data loss; model usable post-migration; migration is reversible or has a tested rollback.
**FAIL:** data loss, partial migration, or an unrecoverable state.

### T-X-07 — Deployment-mode switch (no rewrite)
**Verifies:** NFR-COMP-01, NFR-COMP-02, NFR-COMP-03, NFR-COMP-05
**Action:** Toggle EU data residency, swap the identity provider, then enable single-tenant isolation — all via configuration only.
**PASS:**
- Each takes effect with **no code change** and no data migration required by the switch itself; `Turbofan-Ref` remains fully functional.
- Data-residency toggle (NFR-COMP-02): new `Turbofan-Ref` data is provably pinned to the selected region.
- Auth-abstraction (NFR-COMP-03): swapping the identity provider (e.g. local OIDC → enterprise SAML) is a config change with no business-logic edit.
- Isolation (NFR-COMP-05): the single-tenant variant runs with a dedicated database/compute, no shared multi-tenant infrastructure.
**FAIL:** a switch requires code changes, forces a migration, region pinning not honored, or an IdP swap touches business logic.

---

## Appendix A — Test-to-Requirement Coverage Matrix

| Requirement | Verifying test(s) |
| :--- | :--- |
| FR-CORE-01 | T-P1.1-01, T-P1.1-05, T-P1.1-06 |
| FR-CORE-02 | T-P1.2-01, T-P1.2-03, T-P1.4-05 |
| FR-CORE-03 | T-P1.3-01, T-P1.3-03, T-CORE-03-EXT |
| FR-CORE-04 | T-P1.4-01, T-P1.4-05 |
| FR-CORE-05 | T-P1.1-02, T-X-01 |
| FR-CORE-06 | T-X-01 |
| FR-CORE-07 | T-P1.1-06 |
| FR-CORE-08 | T-P1.2-06 |
| FR-CORE-09 | T-P1.4-02, T-P1.4-03, T-P1.4-04 |
| **FR-CORE-10** | T-CORE-10-01 |
| **FR-CORE-11** | T-CORE-10-01 |
| **FR-CORE-12** | T-CORE-12-01 |
| **FR-CORE-13** | T-CORE-12-01 |
| **FR-CORE-14** | T-DOCIMPORT-01, T-DOCIMPORT-06, T-DOCIMPORT-07 |
| **FR-CORE-15** | T-DOCIMPORT-02 |
| **FR-CORE-16** | T-DOCIMPORT-03 |
| **FR-CORE-17** | T-DOCIMPORT-04 |
| **FR-CORE-18** | T-DOCIMPORT-05 |
| FR-CEM-01 | T-P2.1-05 |
| FR-CEM-02 | T-P2.1-01 |
| FR-CEM-03 | T-P2.3-09 |
| FR-CEM-04 | T-P2.1-06 |
| FR-CEM-05 | T-P2.1-04, T-P2.1-06 |
| FR-CEM-06 | T-P2.3-10 |
| FR-CEM-07 | T-P2.2-01 |
| FR-CEM-08 | T-P2.1-03, T-P2.4-01 |
| FR-CEM-09 | T-P2.3-01, T-P2.4-01 |
| FR-CEM-10 | T-P2.3-02 |
| FR-CEM-11 | T-P2.3-08 |
| FR-CEM-12 | T-P2.3-03, T-P2.4-02 |
| FR-CEM-13 | T-P2.3-04, T-P2.3-05, T-P2.4-01 |
| FR-CEM-14 | T-P2.3-06, T-P2.4-02 |
| FR-CEM-15 | T-P2.3-06 |
| FR-CEM-16 | T-P2.2-02 |
| FR-CEM-17 | T-P2.2-02 |
| FR-CEM-18 | T-P2.2-03 |
| FR-CEM-19 | T-P2.3-03 |
| **FR-PARAM-01…04** | T-PARAM-01, T-PARAM-02 |
| **FR-INFO-01…04** | T-INFO-01, T-INFO-02 |
| **FR-INTX-01…04** | T-INTX-01, T-INTX-02 |
| **FR-EXPORT-01…04** | T-EXPORT-01, T-EXPORT-02, T-EXPORT-03 |
| **FR-COMP-01…06** | *none yet — Phase 6* |
| **FR-ARCH-01…08** | *none yet — Phase 6* |
| FR-SAFE-01 | T-P1.2-04 |
| FR-SAFE-02 | T-P1.2-04 |
| FR-SAFE-03 | T-P1.2-07 |
| FR-SAFE-04 | T-P1.3-03 |
| FR-SAFE-05 | T-P1.3-04 |
| FR-MSN-01 | T-P1.2-05 |
| FR-MSN-02 | T-P1.2-08 |
| FR-MSN-03 | T-P1.2-05 |
| FR-MSN-04 | T-P1.3-05 |
| NFR-PERF-01 | T-P1.2-02 |
| NFR-PERF-02 | T-P1.1-07 |
| NFR-PERF-03 | T-P1.4-06 |
| NFR-PERF-04 | T-P1.3-01, T-P1.3-02 |
| NFR-PERF-05 | T-P2.3-07 |
| NFR-PERF-06 | T-P1.4-06 |
| NFR-REL-01 | T-P1.1-02, T-X-01 |
| NFR-REL-02 | T-P1.1-03 |
| NFR-REL-03 | T-P2.3-04 |
| NFR-REL-04 | T-X-05 |
| NFR-REL-05 | T-X-06 |
| NFR-CEM-02 | T-P1.4-01, T-P2.1-01 |
| NFR-CEM-03 | T-DOCIMPORT-06 |
| NFR-CEM-04 | T-P2.1-04 |
| NFR-CEM-05 | T-P2.1-02 |
| NFR-CEM-06 | T-P2.2-04 |
| NFR-DATA-01/02 | T-P1.1-04 |
| NFR-OPS-01 | T-X-03 |
| NFR-OPS-02 | T-X-02 |
| NFR-OPS-03 | T-X-04 |
| NFR-OPS-04 | T-P2.2-05 |
| NFR-COMP-01/02/03/05 | T-X-07 |
| NFR-COMP-04 | T-P1.1-05 |

**Coverage note:** every FR and NFR in `Axioma_requirements_v5.md` is verified by at least one test above, **except FR-COMP-01…06 and FR-ARCH-01…08**, which have no test-spec rows yet (see this document's header — Phase 6 work, not part of this doc-consolidation pass). Two NFRs are covered *indirectly* and should gain dedicated tests only if they become release-gating: **NFR-CEM-01** (Mode A/Mode B latency — asserted within T-P2.1-02 and T-P1.4-01 rather than as a standalone benchmark) and **NFR-CEM-03** (no-training-on-customer-data posture — a policy/deployment guarantee validated via the isolation tests T-X-02/T-X-07 and, **[REV-D]**, T-DOCIMPORT-06, rather than a runtime assertion).
