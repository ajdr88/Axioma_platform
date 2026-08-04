# Axioma: Stage Tracking Amendment (Rev C)

**Applies to:** Axioma_requirements_v4.md §2.5, §5.8 · Axioma_implementation_v4.md §1.1, §7
**Status:** Draft for Architecture Review — Rev C
**Supersedes:** No prior stage-tracking spec existed; this is new in v4.
**Product track:** Product 1 (MBSE Platform) — no CEM dependency.

---

## 1. Purpose

Axioma currently tracks program-level lifecycle phase (FR-MSN-03: Concept → Development → Production → Operations → Disposal) as a single timeline overlay, and tracks per-element trust/provenance state (§6.2/§6.3: Origin / Validation / Staleness). Neither gives visibility into **where each subsystem actually is** in its engineering lifecycle — requirements written, design reviewed, prototype built, tested — or how much of that work is done.

This amendment adds that layer: a **per-subsystem stage and status**, a **computed progress percentage**, and an explicit, derived link back to the program-level phase so the two don't drift into contradicting each other.

---

## 2. New Functional Requirements

| ID | Requirement Name | Description | Design | Test |
| :--- | :--- | :--- | :--- | :--- |
| **FR-PM-01** | Subsystem Stage Tracking | Each `Block` (subsystem) carries a current stage from the ordered set: **Requirements Definition → Preliminary Design → Detailed Design → Prototype Fabrication → Testing**, each with its own controlled status vocabulary (§4 below). | §5.8 | impl §5 |
| **FR-PM-02** | Program Phase Rollup | Program Phase (FR-MSN-03) is **computed, not manually set**, as the minimum phase implied by all subsystems' current stages (§5 mapping table). The program cannot advance past a phase until every subsystem has cleared the stages mapped to it. | §5.8 | impl §5 |
| **FR-PM-03** | Computed Stage Progress | Each subsystem's per-stage and overall progress percentage is computed from underlying model state — never manually entered. See §6 for the computation rule and its limits. | §5.8 | impl §5 |
| **FR-PM-04** | Testing Stage Status Derivation | Testing-stage status per subsystem is derived from, and stays consistent with, the `SimulationRun`/solver-result provenance already recorded per FR-CEM-19 and the element Validation state (§6.2) — not an independently-editable field. | §5.8, §6.2 | impl §5 |
| **FR-PM-05** | Unified Review Gate | Requirements Definition and design-stage approvals ("In Review" → "Approved") use the **same** proposal/branch review-gate mechanism as CEM-generated changes (FR-CEM-07, `/cem/proposals/*`), generalized to accept a `human-authored` proposal origin alongside the existing CEM-generated one — not a second, parallel approval mechanism. | §5.6 (amended) | impl §5 |

---

## 3. Status Vocabularies

| Stage | Statuses (ordered) |
| :--- | :--- |
| Requirements Definition | Draft → In Review → Edits Requested → Approved → Baselined |
| Preliminary Design | In Progress → Internal Review → Peer Review → PDR Prep → Approved w/ Actions → PDR Complete |
| Detailed Design | In Progress → Internal Review → Peer Review → CDR Prep → Approved w/ Actions → Released for Fabrication |
| Prototype Fabrication | Not Started → Procurement → In Fabrication → In Assembly → QA/Inspection → Nonconformance/Rework → Complete |
| Testing | Test Planning → TRR (Test Readiness Review) → In Test → Anomaly/Failed → Passed → Verified/Closed *(set by FR-PM-04, not manually editable)* |

"In Review" / "Edits Requested" / "Approved" for Requirements Definition, and the review sub-statuses in Preliminary/Detailed Design, are all instances of the single review-gate mechanism in FR-PM-05 — they are not separately implemented approval flows.

---

## 4. Subsystem Stage → Program Phase Mapping

| Subsystem stage | Program phase | Notes |
| :--- | :--- | :--- |
| Requirements Definition | Concept | |
| Preliminary Design | Concept | Program leaves Concept only once **every** subsystem has closed this stage |
| Detailed Design | Development | |
| Prototype Fabrication | Development | Explicitly scoped to a prototype/qualification article, not a production-run unit — this is what removes the phase ambiguity a "Manufacturing" stage would otherwise have (see §7.1) |
| Testing | Development | Qualification/verification testing of the prototype article |
| — | Production, Operations, Disposal | Program-level only in this revision; no subsystem sub-stage is tracked below Production |

**Rollup rule (FR-PM-02):** the program's computed phase is the *minimum* phase implied across all subsystems — i.e., the program is only as advanced as its least-mature subsystem. It does not enter Development until all five reference subsystems (Fan & LP Compression, Core/HP Compressor, Combustor, Turbine HP&LP, Control/FADEC) have closed Preliminary Design.

---

## 5. Computed Progress (FR-PM-03)

- **Stage %** = position of the subsystem's current status within that stage's ordered vocabulary (§3), e.g. "Peer Review" out of 6 Preliminary Design statuses ≈ 3/6.
- **Subsystem overall %** = weighted average across its five stages. Default weight is equal (20% each); weights are overridable per program.
- **Program overall %** = weighted average of subsystem overall %, gated by FR-PM-02 — the program percentage cannot imply a phase more advanced than the rollup rule allows.
- **Requirements Definition %** is computed from the proportion of Requirements under that Block that are Baselined.
- **Preliminary/Detailed Design %** is computed from the proportion of Blocks/Interface Contracts under that subsystem that have closed review (Approved / Released for Fabrication).
- **Testing %** is computed directly from `SimulationRun` outcomes and Validation state per FR-PM-04 — not a separate count.
- **Prototype Fabrication %** has **no model-graph proxy** for physical build progress — see the limitation in §7.2. Until a shop-floor data source exists, this is computed as a proxy (e.g., % of Parts released for manufacture / Interface Contracts accepted), and is explicitly documented as measuring "ready to build," not "percent physically built."

---

## 6. Amendment to §5.6 (Autonomy Levels)

The proposal/branch review-gate (`/cem/proposals/*`) is generalized to accept a second proposal origin, `human-authored`, alongside the existing CEM-generated one. Human-authored proposals (e.g. a Requirement or Block edit awaiting approval) carry `autonomyLevel: n/a` — the L0–L4 scale governs *CEM-generated* change only and does not apply to a human editing their own model. One review-gate mechanism, two proposal origins; no second approval pipeline is built.

---

## 7. Open Items Flagged, Not Silently Resolved

### 7.1 "Fabrication" renamed to "Prototype Fabrication"
The stage is explicitly scoped to a prototype/qualification article. This resolves the earlier ambiguity where a bare "Manufacturing" or "Fabrication" stage could refer to either a prototype build (Development-phase work) or a production-run build (Production-phase work) — the same stage name, two different program phases, depending on which article was meant. With the rename, Prototype Fabrication maps unambiguously to Development. Production-run build tracking is out of scope for this revision and remains program-level only.

### 7.2 No model-graph proxy for physical fabrication progress
Axioma's technology stack (reqs §4) has no MES/ERP/shop-floor connector. "Computed progress" for Prototype Fabrication (FR-PM-03) can only be a model-side proxy (parts released, contracts accepted) until such a connector exists. This is documented as a known limitation rather than presented as true build-completion percentage.

### 7.3 Naming collision with FR-CEM-09
FR-CEM-09 ("Geometry Synthesis") already uses "manufacturable geometry" to mean *design validated as buildable*, not *physically being built*. "Prototype Fabrication" as a stage name keeps clear separation from that usage; no further action needed, but worth keeping in mind if either term is renamed again.

---

## 8. Cross-References

- Axioma_requirements_v4.md §2.5 (FR-PM-01…05 summary table), §5.8 (stage/phase mapping, computed-progress summary), §5.6 (review-gate amendment)
- Axioma_implementation_v4.md §1.1 (`GET /blocks/{id}/lifecycle-status`, `GET /projects/{id}/program-phase`), §7 (traceability matrix row for FR-PM-01…05)
- FR-CEM-07, FR-CEM-19, §6.2 (dependencies for FR-PM-04/05)
- FR-MSN-03 (program phase, now computed per FR-PM-02 rather than manually set)
