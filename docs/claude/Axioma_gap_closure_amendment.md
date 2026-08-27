# Axioma: Gap-Closure Amendment (Rev D candidate)

**Status:** Merged into `Axioma_requirements_v5.md` / `Axioma_implementation_v5.md` (Phase 0 doc-consolidation pass, 2026-08-27) — kept here for history, not superseded content.
**Basis:** `Axioma_cameo_tutorial_gap_analysis.md` (Cameo tutorial vs. Axioma spec comparison, 2026-08-04).
**Scope of this amendment:** Closes the gaps that analysis classified as genuine scope gaps — not the ones classified as likely-intentional non-goals or SysML v1→v2 semantic differences. Explicitly **excluded** from this amendment (flagged for a decision, not silently dropped):

- **Services/SOA modeling (SoaML)** — plausible intentional non-goal given the turbofan pilot's system-of-interest is not a service-oriented enterprise system. Needs a one-line confirmation in `Axioma_requirements_v4.md` §1 (Strategic Objectives or an explicit "Non-Goals" subsection) rather than a new FR group. Not designed here.
- **Use Case decomposition mechanics** (Actors, `<<include>>`/`<<extend>>`, User Roles Diagram) — treated as covered by FR-CORE-01's SysML v2 compliance claim; only needs a named test scenario, added at the end of §5 below.
- **Physical/point-design attachments** (file attachments, Block Instances) — minor; folded into FR-EXPORT-04 below rather than given its own group.

Everything else from the gap analysis gets a full FR group, design section, implementation guidance, and test coverage below, in the same structure as the existing requirements/implementation docs so this can be merged in directly.

---

## 1. New/Amended Functional Requirements

### 1.1 Parametrics (Product 1) — new group **FR-PARAM**

| ID | Requirement Name | Description | Design | Test |
| :--- | :--- | :--- | :--- | :--- |
| **FR-PARAM-01** | Constraint Definition | A user can define a reusable Constraint (an equation or inequality over named parameters) independent of any specific element, stored in a Model Library-equivalent location in the graph. | §5.9 | impl §5 |
| **FR-PARAM-02** | Parametric Binding | A user can bind a Constraint's parameters to Value Properties of one or more structural elements via typed Binding relationships; the binding enforces type/unit consistency at bind time, not just at evaluation time. | §5.9 | impl §5 |
| **FR-PARAM-03** | Parametric Evaluation | Given concrete input values, the platform evaluates all bound Constraints for an element or subgraph and returns computed values for derived parameters, without invoking `cem-core` or any solver — this is direct algebraic/numeric evaluation, distinct from Mode B optimization (FR-CEM-02) and from external FEA/CFD validation (FR-CEM-12). | §5.9 | impl §5 |
| **FR-PARAM-04** | Parametric Traceability | Every Constraint and Binding is a first-class graph element and participates in traceability (FR-CORE-03) — e.g., "which Requirements are supported by this Constraint" is answerable the same way Satisfy/Verify queries are. | §5.9, §5.3 | impl §5 |

**Why this is a Product 1 requirement, not a CEM (Product 2) one:** Parametrics is one of the four SysML pillars (Structure, Behavior, Requirements, Parametrics) and is used for lightweight engineering calculations (mass budgets, margins, unit conversions) *before* and *independent of* whether Mode B's deterministic optimizer or Mode C's external solvers are ever invoked. A user of Product 1 alone (no CEM) still needs to check that `Cook Energy = Power × Time` or that `Sum(subsystem mass) <= mass budget`. Making this a CEM-only capability would force every Product-1-only customer into the CEM track for basic engineering math, which contradicts "Product 1 usable as a standalone MBSE tool with no CEM present" (reqs §1, Success Metrics).

### 1.2 Information/Data Architecture (Product 1) — new group **FR-INFO**

*(Named `FR-INFO` rather than reusing `NFR-DATA` to keep this clearly distinct: `NFR-DATA-01/02` govern how the platform **stores** element bodies; `FR-INFO` governs the user-facing capability to **model** a system's information architecture as SysML content, same as Cameo's Data Package / CDM-LDM-PDM.)*

| ID | Requirement Name | Description | Design | Test |
| :--- | :--- | :--- | :--- | :--- |
| **FR-INFO-01** | Information Element Modeling | A user can define information/data entities as typed structural elements (SysML v2's data-definition equivalent), organized into a dedicated information-architecture view distinct from the physical/behavioral structure view. | §5.10 | impl §5 |
| **FR-INFO-02** | Data Type & Enumeration Definition | A user can define custom Data Types and Enumerations (name + literal set), in addition to a built-in primitive set (boolean, integer, real, string, and unit-bearing numeric types). | §5.10 | impl §5 |
| **FR-INFO-03** | Conceptual → Logical Refinement | Information elements support an explicit abstraction-level tag (Conceptual / Logical / Physical, or equivalent) and a `Refine`/`Specialize` relationship between levels, so a Conceptual entity's Logical realizations are traceable — mirroring CDM→LDM→PDM without mandating the exact three-tier vocabulary. | §5.10 | impl §5 |
| **FR-INFO-04** | Information Flow Typing | Object/Item Flows between behavioral elements (Activities, Ports) are typed by an Information Element or Data Type defined per FR-INFO-01/02, so flow content is traceable back to the information model rather than left as an untyped label. | §5.10, §5.11 | impl §5 |

### 1.3 Interaction / Cross-Element Timing Behavior (Product 1) — new group **FR-INTX**

| ID | Requirement Name | Description | Design | Test |
| :--- | :--- | :--- | :--- | :--- |
| **FR-INTX-01** | Message-Based Interaction Modeling | A user can model a time-ordered exchange of messages/invocations among a defined set of structural elements (the SysML v2-native equivalent of a UML Sequence Diagram/Interaction) — showing who calls what, in what order, without requiring a full State Machine or Activity to express it. | §5.11 | impl §5 |
| **FR-INTX-02** | Timing Constraints on Interactions | An Interaction can carry minimum/maximum absolute-time and duration constraints between two points in the exchange, usable for early, pre-simulation timing/latency analysis. | §5.11 | impl §5 |
| **FR-INTX-03** | Conditional/Repeated Sub-Interactions | An Interaction supports at minimum: an alternative-branch construct (mutually exclusive guarded sub-sequences), an optional sub-sequence, a parallel sub-sequence, and a loop sub-sequence — covering the common cases from SysML v1's Combined Fragment operators (`alt`/`opt`/`par`/`loop`) even if the underlying SysML v2 construct differs. | §5.11 | impl §5 |
| **FR-INTX-04** | Reusable Sub-Interactions | A named Interaction can be referenced from within another Interaction as a reusable sub-sequence (the SysML v2-native equivalent of an Interaction Occurrence/`ref` fragment), so common exchange patterns are defined once. | §5.11 | impl §5 |

**Open ADR needed:** §5.11 below records that the exact SysML v2 construct(s) satisfying FR-INTX-01…04 are not yet chosen — SysML v2 does not have a direct "Sequence Diagram" element, and OMG's approach to interaction/occurrence modeling in v2 is still maturing. This amendment specifies the *user-facing capability*, not the underlying metaclass; resolving that mapping is a new ADR candidate (ADR-009, see §3).

### 1.4 Export & Reporting (Product 1) — new group **FR-EXPORT**

| ID | Requirement Name | Description | Design | Test |
| :--- | :--- | :--- | :--- | :--- |
| **FR-EXPORT-01** | Diagram Image Export | Any canvas view (BDD-equivalent, behavioral diagram, or Interaction) can be exported as a static image (PNG/SVG at minimum) at the current viewport or full-diagram extent, for use outside the tool (reports, decks). | §5.12 | impl §5 |
| **FR-EXPORT-02** | Tabular Export | Any generated table view (Requirements Table, Generic Element Table, traceability matrix) can be exported to a spreadsheet-compatible format (CSV/XLSX), preserving column selection and applied filters. | §5.12 | impl §5 |
| **FR-EXPORT-03** | Model Report Generation | A user can generate a structured document (at minimum: PDF or HTML) summarizing a selected scope of the model — e.g., a Use Case with its Specification and scenario, or a Block with its Requirements and Hazards — extending the existing FR-SAFE-05 standards-export mechanism to non-safety content. | §5.12 | impl §5 |
| **FR-EXPORT-04** | Element File Attachments | Any model element can have one or more external files (spec sheets, drawings, point-design data) attached and hyperlinked, retrievable via the element's API representation; attachments are stored in the object store per NFR-DATA-02, never inlined in the graph or document store. | §5.12 | impl §5 |

### 1.5 Dynamic Element Collections ("Smart Packages" equivalent) — extends **FR-CORE**

| ID | Requirement Name | Description | Design | Test |
| :--- | :--- | :--- | :--- | :--- |
| **FR-CORE-10** | Saved Dynamic Queries | A user can define a named, saved graph query (e.g., "all Blocks with stereotype `subsystem` under Package X") whose result set is live — re-evaluated on demand or on a defined refresh trigger — and can be pinned into the Browser/navigation tree alongside statically-organized content. | §5.13 | impl §5 |
| **FR-CORE-11** | Static Snapshot Collections | A Dynamic Query result can be frozen into a static, manually-curated collection (add/remove members by hand, no further live re-evaluation) — the equivalent of Cameo's "Freeze Contents." | §5.13 | impl §5 |

### 1.6 Behavior-to-Structure Allocation Authoring — extends **FR-CORE**

| ID | Requirement Name | Description | Design | Test |
| :--- | :--- | :--- | :--- | :--- |
| **FR-CORE-12** | Swimlane/Partition Allocation | An Activity-equivalent behavioral diagram supports partitioning its canvas by structural element (Block/Actor/Interface), with each partition allocated to exactly one structural element, so an Action's owner is visually and semantically explicit — not just inferable from a hyperlink. | §5.11 (amended), impl §4.5 | impl §5 |
| **FR-CORE-13** | Control/Object Flow Validity | The semantic-validation layer (FR-CORE-05) rejects an Action with no incoming or outgoing flow path ("orphan Action") and rejects a Decision node with more than one outgoing flow whose guard can evaluate `True` simultaneously, consistent with SysML's well-formedness rules for Activities. | §5.1 (amended) | impl §5 |

### 1.7 Requirements Dependency Taxonomy — amends **FR-CORE-03** and the data model

The current data model (reqs §2.1 note, impl §2.3) lists only `Satisfy`/`Verify`/`Refine` as first-class traceability edges. This amendment adds:

- **`Derive`** — connects a lower-level Requirement to the higher-level Requirement(s) it was derived from by analysis (distinct from a Containment/decomposition relationship, which is structural subsetting, not derivation).
- **`Copy`** — marks a Requirement as a duplicate of another, for reuse across model scopes, keeping both discoverable as the same underlying requirement content.

**Amended edge list (impl §2.3):** `contains` (acyclic), `Satisfy`/`Verify`/`Refine`/`Derive`/`Copy`, `causes`/`mitigatedBy`, `validatedBy`, `Suspect`.

No new FR ID is needed — this is a data-model completeness fix to FR-CORE-03's existing scope ("n-degree relationship maps... via graph-query languages"). Test coverage is added in §4 below.

---

## 2. Design Reference (new §5.9–§5.13 for `Axioma_requirements_v4.md`)

### §5.9 Parametrics Architecture

Constraints and Bindings are graph elements like any other (`:Constraint`, `:Parameter`, edge type `Bound`), stored via the same polyglot split as everything else — topology in the graph store, the constraint expression body in the document store (NFR-DATA-01/02 apply unchanged). Evaluation (FR-PARAM-03) is a **pure, synchronous, server-side computation** — no LLM, no external solver, no job scheduler/Campaign involved; it is architecturally closer to a spreadsheet formula evaluation than to a CEM run, and should not touch `cem-core`, `cem-connectors`, or `scheduler`. This distinction matters for the "Product 1 stands alone" success metric (reqs §1): a Product-1-only deployment must be able to evaluate Constraints with zero Product-2 services running.

A Constraint's parameters are typed by Data Types/Value Types (FR-INFO-02); binding-time validation (FR-PARAM-02) reuses the same type-checking machinery `sysml-core` already runs for other relationship legality checks (FR-CORE-05).

### §5.10 Information/Data Architecture

Information Elements (FR-INFO-01) get a new node label, `:InformationElement`, alongside the existing `:Structure`/`:Requirement`/etc. set (reqs §"Data model" list, amended). They participate in the same containment/traceability rules as other elements (NFR-REL-02) — no special-casing. The Conceptual/Logical/Physical tiering (FR-INFO-03) is a **property on the element** (an enumerated `abstractionLevel` field), not three separate node labels, keeping the schema simple and avoiding a proliferation of near-duplicate types for what is fundamentally the same concept at different refinement stages.

### §5.11 Interaction / Timing Modeling

**Open decision, not resolved here:** the concrete SysML v2 metaclass(es) backing FR-INTX-01…04 need an ADR (ADR-009 candidate, §3). Two shapes are plausible and should be evaluated in that ADR:

1. **Native SysML v2 Occurrences** — model the interaction as a graph of Occurrence usages connected by succession/message links, which is closer to how SysML v2 actually represents behavior, at the cost of not looking like a classic Sequence Diagram to a Cameo-trained user.
2. **A dedicated Interaction diagram type in `diagram-engine`** that renders as a Lifeline/Message diagram (visually familiar to MBSE practitioners) backed by whatever the chosen SysML v2 metaclass turns out to be — i.e., treat "looks like a Sequence Diagram" as a **view concern** in `packages/diagram-engine`, independent of the underlying graph representation.

Recommendation for the ADR: option 2. It decouples "what SysML v2 actually stores" from "what a systems engineer expects to draw," which is consistent with how Axioma already treats Mode B as "a live, optimizing implementation of the Parametric Diagram concept, not a new artifact type" (reqs §5.5) — the diagram is a view, the semantics live in the graph.

FR-CORE-12 (Swimlane allocation) belongs here rather than under Parametrics/Data because it's the same "behavior-to-structure linkage" problem as Activity Partitions, just expressed as a canvas capability rather than a data-model one. It requires a new React Flow layout mode in `diagram-engine` (vertical/horizontal partitions with drag-to-allocate headers) — no backend data-model change beyond the existing `Allocate` dependency stereotype already implied by FR-CORE-03's dependency taxonomy.

### §5.12 Export & Reporting Pipeline

All four FR-EXPORT items are **read paths only** — none of them write to the graph, so they carry no semantic-validation or provenance concerns. Recommended shape:

- **FR-EXPORT-01 (image)**: client-side canvas export (React Flow's own export utilities) for the current viewport; a server-side headless-render path for "export full diagram regardless of size" so a 500-block diagram doesn't require the client to instantiate it all (reuses the virtualization/off-screen-clustering machinery from NFR-PERF-01 in reverse — cluster for viewing, un-cluster for render).
- **FR-EXPORT-02 (tabular)**: a generic `/export/table` endpoint taking the same scope/column-selection parameters as a Generic Table view; streams CSV directly, or XLSX via a lightweight server-side writer — no new persistence, this is a transform over an existing query result.
- **FR-EXPORT-03 (report)**: templated document generation, following the same pattern already used for FR-SAFE-05's ARP4761/MIL-STD-882 export — generalize that template mechanism to accept a report template + a model scope, rather than building a second, safety-specific-only pipeline. This directly parallels FR-PM-05's "one mechanism, two origins" pattern already used for the review gate.
- **FR-EXPORT-04 (attachments)**: identical mechanism to how Mode C geometry/mesh files are referenced from the graph by pointer into the S3-compatible object store (NFR-DATA-02) — reuse that pointer-reference pattern for arbitrary user-attached files rather than inventing a second attachment mechanism.

### §5.13 Dynamic Element Collections

A Dynamic Query (FR-CORE-10) is a stored graph query (Cypher-equivalent, scoped by the same query-budget rules as any other traversal — NFR-PERF-04 applies; an unbounded Dynamic Query is rejected at save time, not just at run time) plus a re-evaluation policy (on-demand / on-write-to-scope / scheduled). A Static Snapshot Collection (FR-CORE-11) is the frozen result set: a `:Collection` node with explicit `contains`-like membership edges (not the acyclic containment edge itself, to avoid conflating "organizational grouping" with "structural decomposition" — NFR-REL-02's acyclicity guarantee must not be threatened by a collection that legitimately references elements from anywhere in the graph, including elements that already have a different container).

---

## 3. Implementation Guidance (additions to `Axioma_implementation_v4.md`)

### 3.1 New REST Endpoints

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

All new write endpoints pass through the existing semantic-validation layer (§4.2) before commit — no new bypass path is introduced. All new traversal-shaped endpoints (`/parametrics/evaluate` over a subgraph, `/collections/dynamic` query definition) are subject to the existing query-budget enforcement (NFR-PERF-04) — a Dynamic Query without depth/fan-out bounds is rejected at save time.

### 3.2 Data Model Additions

| Addition | Type | Notes |
| :--- | :--- | :--- |
| `:Constraint`, `:Parameter` | Node labels | Body (equation text) in document store; graph holds topology only (NFR-DATA-01) |
| `:InformationElement` | Node label | Participates in existing containment/traceability rules unchanged |
| `:Interaction`, `:InteractionFragment` | Node labels | Pending ADR-009 (§3.3) on underlying SysML v2 mapping |
| `:Collection` | Node label | For FR-CORE-10/11 |
| `Bound` | Edge type | Constraint parameter ↔ Value Property |
| `Derive`, `Copy` | Edge types | Extends the FR-CORE-03 traceability edge set (§1.7 above) |
| `member` | Edge type | Collection membership; explicitly distinct from `contains` |

### 3.3 New ADR Candidates

| ADR | Decision needed | Status |
| :--- | :--- | :--- |
| **ADR-009** | SysML v2 metaclass mapping for Interaction/timing modeling (FR-INTX group) — native Occurrence graph vs. dedicated diagram-engine construct over a chosen backing representation. Recommendation in §5.11: treat the Lifeline/Message *view* as a `diagram-engine` concern, decoupled from the underlying SysML v2 storage representation. | Proposed |
| **ADR-010** | Report-template mechanism for FR-EXPORT-03 — generalize FR-SAFE-05's existing safety-register template pipeline rather than building a second one, mirroring the FR-PM-05 "one mechanism, multiple origins" precedent. | Proposed |

### 3.4 Roadmap Placement

None of these belong in Product 2 or block it. Recommended insertion into the existing Product 1 phase plan (impl §4.1):

- **P1.1 Core Graph:** add `:InformationElement`, `:Constraint`, `:Parameter`, `Derive`/`Copy` edges to the initial KerML meta-model work — these are pure data-model additions, cheapest to land before the meta-model is load-bearing elsewhere.
- **P1.2 IDE Experience:** add FR-CORE-10/11 (Dynamic Collections) and FR-CORE-12 (Swimlane allocation UI) alongside the canvas/virtualization work already scheduled here — both are canvas/Browser features, natural fits.
- **P1.3 Digital Thread:** add FR-EXPORT-01/02 (image/tabular export) alongside the traceability-matrix work already scheduled here — export is a natural extension of "here's a table/matrix, now get it out of the tool."
- **P1.4 Behavioral Simulation + Pilot:** add FR-INTX-01…04 (Interaction modeling) here, since it's a behavioral-modeling capability and the phase already stands up the fUML/Alf execution path it may eventually need to interoperate with; land FR-PARAM-01…04 (Parametrics) here too, since parametric evaluation is architecturally closest to "compute a value from model state," which this phase's Constraint/Value Property machinery already touches.
- **Mode A fast-follow / P2.1:** FR-EXPORT-03 (report generation) and FR-EXPORT-04 (attachments) can land opportunistically anywhere after P1.3 — they have no hard phase dependency, listed last only because they're lowest-risk/lowest-urgency relative to the modeling-capability gaps.

None of this placement is load-bearing for Product 2 — it's a sequencing suggestion, not a new dependency into the CEM track.

---

## 4. Test Coverage (additions to `Axioma_test_specification_v3.md`)

| Test ID | Verifies | Setup / Action | PASS | FAIL |
| :--- | :--- | :--- | :--- | :--- |
| **T-PARAM-01** | FR-PARAM-01/02 | Define Constraint `CookEnergy = PowerLevel * TimeValue`; bind to two Value Properties on the Processor Block. | Binding succeeds with type-checked parameters; Constraint appears in the Block's Relations. | Binding accepted with mismatched types, or Constraint not visible in traceability queries. |
| **T-PARAM-02** | FR-PARAM-03 | Set PowerLevel=750, TimeValue=120; evaluate. | Returns CookEnergy=90000 without invoking `cem-core`/`cem-connectors`/`scheduler` (verified via trace — no spans from those services). | Wrong value, or evaluation dispatches to a Product-2 service. |
| **T-INFO-01** | FR-INFO-01/02 | Create an Information Element `ControlData` at Conceptual level; specialize to a Logical element `PowerLevelParam` with a custom Data Type `Watts`. | Specialization traceable; `Watts` Data Type usable to type a Value Property elsewhere in the model. | Specialization edge missing, or Data Type not reusable outside its defining scope. |
| **T-INFO-02** | FR-INFO-04 | Type an Object Flow between two Actions using an Information Element. | Flow's typed content is queryable back to the Information Element (not an untyped string label). | Flow content is a free-text label with no graph link. |
| **T-INTX-01** | FR-INTX-01/02 | Model a 3-step message exchange among two Blocks with a Duration Constraint between steps 1 and 3. | Exchange renders in time order; Duration Constraint evaluable/checkable against a supplied timing value. | Steps unordered, or constraint not attached to the correct pair of points. |
| **T-INTX-02** | FR-INTX-03 | Add a `loop` sub-sequence with a guard condition. | Guard editable and evaluated at simulation/evaluation time; loop bounded (no infinite-loop acceptance without an explicit bound or budget). | Unbounded loop accepted with no safeguard. |
| **T-EXPORT-01** | FR-EXPORT-01 | Export a 500-element diagram as PNG at full extent, not just viewport. | Full diagram rendered server-side; client does not need to instantiate all 500 elements to trigger it. | Export limited to visible viewport, or client-side memory spike. |
| **T-EXPORT-02** | FR-EXPORT-02 | Export a filtered Requirements Table (10 of 50 requirements, 4 of 8 columns) to XLSX. | Exported file matches exactly the filtered/column-selected view. | Export includes unfiltered rows/columns. |
| **T-EXPORT-03** | FR-EXPORT-03 | Generate a report for a Use Case scope. | Includes UC Specification, linked Requirements, and scenario diagram reference; uses the same template mechanism as FR-SAFE-05 (verified: no second, divergent template engine in the codebase). | Missing content, or a parallel/divergent report pipeline exists. |
| **T-CORE-10-01** | FR-CORE-10 | Save a Dynamic Query for "all Blocks with stereotype `subsystem`"; add a new matching Block after save. | New Block appears in the collection on next re-evaluation without manual action. | New Block absent, or query required unbounded traversal (should be rejected per NFR-PERF-04, not silently allowed). |
| **T-CORE-12-01** | FR-CORE-12 | Create an Activity-equivalent diagram with 3 Swimlanes allocated to 3 Blocks; add an Action with no flow. | Orphan Action rejected per FR-CORE-13; allocated Actions show correct owning-Block stereotype. | Orphan Action silently accepted, or allocation not visible/queryable. |
| **T-CORE-03-EXT** | FR-CORE-03 (amended) | Create a `Derive` edge from `REQ-LOW` to `REQ-HIGH`, and a `Copy` edge duplicating `REQ-LOW` into another scope. | Both edges queryable via the same traceability endpoint as Satisfy/Verify/Refine; `Derive` distinguishable from a Containment edge in query results. | Edge type not persisted, or indistinguishable from Containment. |

---

## 5. Summary Table — Gap → Resolution

| Gap (from tutorial comparison) | Resolution |
| :--- | :--- |
| Parametric Diagrams / Constraint Blocks | New FR-PARAM group (§1.1), Product 1, P1.4 |
| Data modeling (CDM/LDM/PDM, Data Types) | New FR-INFO group (§1.2), Product 1, P1.1 |
| Sequence Diagrams / Interactions | New FR-INTX group (§1.3), Product 1, P1.4, pending ADR-009 |
| Export/reporting | New FR-EXPORT group (§1.4), Product 1, P1.3–P1.4+ |
| Smart Packages | New FR-CORE-10/11 (§1.5), Product 1, P1.2 |
| Activity Diagram Swimlane allocation | New FR-CORE-12/13 (§1.6), Product 1, P1.2 |
| Narrow requirements dependency taxonomy | Amended FR-CORE-03 + data model (§1.7), Product 1, P1.1 |
| Services/SOA (SoaML) | **Not resolved here** — recommend an explicit non-goal statement instead of a new FR group |
| Use Case mechanics (Actors, include/extend) | **Not resolved here** — add a named test scenario only, no new FR |
| Physical attachments | Folded into FR-EXPORT-04, not a separate group |
