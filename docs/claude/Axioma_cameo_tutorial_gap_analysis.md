# Cameo Tutorial vs. Axioma Spec — Functionality Gap Analysis

**Source analyzed:** `CAMEO-TUTORIAL-SCRIPT.pdf` (Dr. Mike Borky, Colorado State University, Cameo Systems Modeler Enterprise Edition tutorial, based on the MBSAP methodology; classic SysML v1/UML-profile tool).
**Compared against:** `Axioma_requirements_v4.md`, `Axioma_implementation_v4.md`, `CLAUDE.md`, `Axioma_stage_tracking_amendment.md` (Rev C, current).
**Date:** 2026-08-04
**Status:** The gaps identified here were closed by `Axioma_gap_closure_amendment.md`, which is now merged into `Axioma_requirements_v5.md` / `Axioma_implementation_v5.md` (Phase 0 doc-consolidation pass, 2026-08-27). This analysis is kept for history — it carries no spec text of its own to re-merge.

## Framing caveat (read this first)

Cameo Systems Modeler implements **SysML v1** as a UML profile (Blocks, BDD/IBD, Activity/State Machine/Sequence/Parametric Diagrams, Stereotypes on top of UML metaclasses). Axioma explicitly targets **SysML v2** on a native KerML meta-model (FR-CORE-01, ADR-006) — a different, non-UML-profile language that OMG substantially restructured (e.g., no more classic UML "Sequence Diagram" as a core SysML v2 construct; parametrics are reorganized as constraint usages; Block becomes Part Definition/Usage). So a literal one-for-one feature match isn't the right bar — some items below are genuinely absent from the spec, others are absent *by design* because SysML v2 handles the underlying concept differently, and the current docs don't yet say which is which. That ambiguity is itself flagged below.

Also relevant: Axioma splits capability across **Product 1 (MBSE platform)** and **Product 2 (CEM)**, and Product 1 ships in phases (P1.1–P1.4). Several "SysML fundamentals" gaps below are things a Cameo user would expect on day one but Axioma's roadmap doesn't explicitly commit to for Product 1.

## Confirmed coverage (Cameo functionality with a clear Axioma counterpart)

| Cameo tutorial feature | Axioma coverage |
| :--- | :--- |
| Project/model creation, Containment Tree organization | Implied by `Projects`/`Commits`/`Elements` REST model (impl §1.1); no explicit viewpoint/perspective (MBSAP-style) organization scheme named |
| Block Definition Diagrams, Blocks, structural decomposition | FR-CORE-01 (100% OMG SysML v2 API compliance), `:Structure` node label, canvas/React Flow Block nodes (impl §4.3) |
| Requirements (Requirement Diagram, Requirement Table, Satisfy Matrix) | `:Requirement` node, FR-CORE-03 traceability (Satisfy/Verify/Refine), "budgeted traceability matrix" (impl §4.4) |
| Use Cases / Actors | FR-MSN-01 Mission/UseCase Definition |
| Activity Diagrams, State Machines (behavioral execution) | FR-CORE-04 Behavioral Simulation, `fuml-runtime` + `alf-lite` (impl §4.5, §9) |
| Hazard/Risk modeling | FR-SAFE-01…05 (more rigorous than anything in the Cameo tutorial — Cameo has no native safety/hazard pillar) |
| Requirements traceability, standards export | FR-CORE-03, FR-SAFE-05 (ARP4761/MIL-STD-882/ISO 26262) |
| Import from other tools | FR-CORE-07 (ReqIF, SysML v2 API, AI-assisted documents→draft model) — broader than Cameo's own import story |

## Gaps and open questions

1. **Sequence Diagrams / Interactions — not mentioned anywhere.** The tutorial spends a full section (§12) on SDs: Messages, Lifelines, Combined Fragments (`alt`/`opt`/`par`/`loop`/`region`/`neg`), Interaction Occurrences, Time/Duration Constraints for timing analysis. Axioma's FR-CORE-04 names only "State Machines and Activity Diagrams" as the behavioral scope, and explicitly disclaims full fUML/Alf compliance. Whether interaction/sequence-style modeling is (a) out of scope, (b) assumed subsumed by SysML v2's restructured interaction concepts, or (c) simply not yet written down, is undetermined from the current docs.

2. **Parametric Diagrams / Constraint Blocks — not named as a Product 1 capability.** SysML's tutorial treats Parametrics as one of the "four pillars" (Structure, Behavior, Requirements, Parametrics) — Constraint Blocks, Binding Connectors, and diagram-level equation evaluation, independent of any optimizer. Axioma's only numeric-modeling capability described is CEM Mode B's 0D/1D performance and mass-budget models (`cem-core`, Product 2, deterministic optimizer) — a much heavier-weight, optimization-specific mechanism. There's no Product-1-level, general-purpose "define a constraint, bind it to properties, evaluate it" capability analogous to a Parametric Diagram. If FR-CORE-01's "100% OMG SysML v2 API compliance" is meant to cover this, it should be said explicitly and tested; right now nothing in the FR list, data model, or test spec names it.

3. **Data modeling (CDM/LDM/PDM, Data Types, Enumerations) as a modeled artifact — not present.** MBSAP treats Data as one of five architecture viewpoints, with Blocks-as-information-entities, Conceptual/Logical/Physical Data Models, and user-defined Data Types/Enumerations as first-class modeling content. Axioma's docs mention "data" only as a *persistence* concern (element bodies in Postgres/JSONB, NFR-DATA-02) — never as something the user models (an information/data architecture). This looks like a real scope gap rather than a v1/v2 semantics difference.

4. **Services / SOA modeling (SoaML) — entirely absent.** MBSAP's fifth viewpoint is Services (Participant, ServicePoint/RequestPoint, ServiceContract, ServiceInterface). Axioma's FR list has no equivalent, and given the target system-of-interest (a turbofan engine, not a service-oriented enterprise system), this is plausibly an intentional non-goal rather than an oversight — worth confirming rather than assuming.

5. **General model/diagram export and reporting — narrow.** The tutorial relies heavily on exporting diagrams as images, exporting Requirement Tables to Excel, and general documentation generation from the model. Axioma's only export requirement is FR-SAFE-05 (safety register in a standards format). There's no FR for generic diagram export (image/PDF) or tabular export (CSV/Excel) of any model view, which matters for stakeholder reviews and reports outside the tool.

6. **Smart Packages (dynamic, stereotype-driven catalogs) — no equivalent.** Cameo's Smart Packages let a user build a live, query-defined grouping of elements (e.g., "all Blocks with `<<subsystem>>` applied"). Axioma's graph backend (Neo4j/GDS) could support the underlying capability, but there's no FR describing a user-facing saved/dynamic-query grouping feature.

7. **Requirements dependency taxonomy is narrower than full SysML.** The tutorial uses `Satisfy`, `Copy`, `Refine`, `deriveRqmt`, `Trace`, and `Verify` as distinct, semantically different requirement dependencies. Axioma's data model (reqs §1, impl §2.3) lists only `Satisfy`/`Verify`/`Refine` as first-class edge types. `Copy` and `deriveRqmt` (derived-requirement traceability, a common MBSE pattern) aren't named — worth confirming they're either subsumed under `Refine`/`Satisfy` or intentionally deferred.

8. **Activity Diagram authoring mechanics (Swimlanes/Partitions allocated to structure, Object Flows via Pins, Forks/Joins/Merges) — scope unclear.** This is the tutorial's central technique for linking behavior to structure (allocating Actions to Blocks/Actors via Swimlanes). `alf-lite`'s initial target subset (impl §9.6) is scoped to textual action-language constructs (value specs, arithmetic/boolean ops, if/else, simple loops, behavior invocation, signal send/accept) — it doesn't speak to the *graphical* Activity Diagram authoring experience (partitions/allocation, decision guards, fork/join) at all. It's unclear whether that graphical layer is assumed as "given" by SysML v2 compliance or needs its own FR.

9. **Use Case decomposition details (Actors, `<<include>>`/`<<extend>>`, User Roles Diagram) — implied, not enumerated.** Reasonable to assume under FR-CORE-01's SysML v2 compliance claim, but nothing tests it specifically.

10. **Physical/point-design attachments (Attached Files, Block Instances, physical spec data in an element) — no explicit mention**, though this is a minor convenience feature relative to the items above.

## Net assessment

The two pillars Cameo users would consider load-bearing — Structure and Requirements — are solidly covered, and in the requirements/safety/traceability space Axioma's spec is materially more rigorous than anything in the Cameo tutorial (formal Hazard/Risk model, autonomy levels, provenance, CEM optimization/geometry). The clearest real gaps are Parametrics as a general-purpose Product 1 capability, a Data-modeling viewpoint, and Sequence-Diagram-equivalent interaction modeling — none of these are mentioned even as "deferred" or "out of scope," so it's not clear if they were consciously dropped or simply not yet written down. Recommend a short pass with the SysML v2 spec to state, for each of Structure/Behavior/Requirements/Parametrics/(Interconnection), what Axioma actually claims — and, if Parametrics/Data/Interactions are intentionally out of Product 1's initial scope, say so explicitly in the requirements doc rather than leaving it implicit in a blanket "100% API compliance" claim.
