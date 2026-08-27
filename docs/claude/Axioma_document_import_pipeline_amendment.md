# Axioma: Documents → Draft Model Pipeline Amendment (Rev D candidate)

**Status:** Draft for review — not yet folded into `Axioma_requirements_v4.md` / `Axioma_implementation_v4.md`.
**Basis:** FR-CORE-07 currently asserts an "AI-assisted 'documents → draft model' path" and a `POST /import/documents` endpoint exist, but defines no pipeline, no provenance/citation mechanism, no review-gate assignment, and no test coverage beyond the ReqIF/SysML-v2 round-trip test (which doesn't exercise free-text documents at all). This amendment specifies the mechanism.
**Scope:** Requirements PDF (or other document) → structured, human-reviewable `Requirement` elements in the graph. Explicitly does **not** cover requirements → subsystem decomposition/allocation — that remains Mode B's job (FR-CEM-02, Product 2), unchanged and out of scope here. See §0 for why that boundary is drawn where it is.

**Sync note (2026-08-27, resolved):** This amendment originally proposed ADR-011, which collided with the ADR-011 also proposed by `Axioma_turbofan_system_model_amendment.md` / `Axioma_sysml_tool_landscape_evaluation.md` (a different, unrelated decision about adopting `adsg-core`/`SBArchOpt`). Per `IMPLEMENTATION_KICKOFF.md` Phase 0's own recommendation, the ADSG/SBArchOpt decision keeps **ADR-011**; the `llm-gateway`-as-shared-dependency decision below is renumbered to **ADR-012**. Resolved as part of the Phase 0 doc-consolidation pass — see `Axioma_requirements_v5.md`/`Axioma_implementation_v5.md` for the merged, authoritative text; this document is kept for history.

---

## 0. Scope Boundary (why this stops at Requirements, not Subsystems)

FR-CORE-07 lives in the Core Platform (Product 1) group, and reqs §1's success metric requires "Product 1 usable as a standalone MBSE tool with no CEM present." If "documents → draft model" silently depended on Mode B's optimizer to produce subsystems, FR-CORE-07 would secretly depend on Product 2 — contradicting that metric. So this pipeline is scoped to produce:

- Structured `Requirement` elements (text, ID, best-effort category) with full citation provenance, and
- **Optionally**, low-confidence *candidate* structural nouns extracted from the document (e.g., "the Fan & LP Compression subsystem" mentioned in prose) surfaced as suggestions in the review UI — never auto-created as `Structure`/Block elements. A human decides whether and how to turn a candidate noun into a real Block.

Turning a confirmed set of Requirements into an optimized, interface-complete subsystem allocation is FR-CEM-02 (Mode B), a separate, later, Product-2-gated step, run explicitly by the user once Requirements are reviewed and accepted. This keeps the dependency direction one-way: Mode B *consumes* Requirements that FR-CORE-07 produced; FR-CORE-07 never reaches into Mode B to do its job.

---

## 1. New/Amended Functional Requirements (extends FR-CORE)

| ID | Requirement Name | Description | Design | Test |
| :--- | :--- | :--- | :--- | :--- |
| **FR-CORE-14** | Document Requirement Extraction | Given an uploaded document (PDF at minimum; OCR applied if the PDF is scanned/image-based), the platform extracts candidate requirement statements and drafts each as a `Requirement` element with name, ID, "shall"-text, and a best-effort category (Functional/Non-Functional, pending FR-INFO or a future taxonomy amendment). Extraction runs as an asynchronous job, not a blocking request. | §5.14 | impl §5 |
| **FR-CORE-15** | Document Citation Provenance | Every `Requirement` element created by FR-CORE-14 carries a citation back to its source location in the originating document (at minimum: page number; paragraph/bounding-box offset where extractable), in addition to the standard AI-generation provenance (model, prompt-template hash, context snapshot) required by FR-CORE-08. A Requirement with no citation is a defect, not an acceptable degraded case. | §5.14 | impl §5 |
| **FR-CORE-16** | Document Import Review Gate | A completed document-extraction job produces a reviewable proposal — never a direct write to Main — using the same proposal/branch review-gate mechanism as CEM-generated changes (FR-CEM-07) and human-authored edits (FR-PM-05), generalized to accept a third proposal origin, `document-import`. One review-gate mechanism, three origins; no new parallel approval pipeline. | §5.6 (amended), §5.14 | impl §5 |
| **FR-CORE-17** | Candidate Structure Suggestions (Non-Binding) | The extraction job may additionally surface candidate structural nouns (subsystem/component names mentioned in the document) as unlinked suggestions in the review UI. These are display-only hints, never persisted as `Structure`/Block elements automatically — a human must explicitly create or link a Block for a suggestion to become real model content. | §5.14 | impl §5 |
| **FR-CORE-18** | Extraction Failure & Low-Confidence Handling | The extraction job reports, per candidate requirement, a confidence signal and surfaces (rather than silently drops) statements it could not confidently structure — e.g., ambiguous "shall" statements, tables, or requirements split across a page break. A document that yields zero extractable requirements is a reported failure state, not an empty successful import. | §5.14 | impl §5 |

---

## 2. Design Reference (new §5.14 for `Axioma_requirements_v4.md`)

### §5.14 Documents → Draft Model Pipeline

The pipeline runs as an asynchronous job with five stages. Each stage's output is inspectable independently, so a failure at stage 3 doesn't discard stage 1–2 work, and a reviewer can see *why* a given Requirement was drafted the way it was — this mirrors how `SimulationRun` provenance already makes solver reasoning inspectable rather than a black box (FR-CEM-19), applied here to LLM drafting instead of solver execution.

**Stage 1 — Extraction.** The uploaded PDF is parsed for text; a scanned/image-based PDF runs through OCR first (FR-CORE-14). Output: a page-addressable text stream, not yet structured.

**Stage 2 — Candidate Segmentation.** The text stream is segmented into candidate requirement statements (classically, "shall" sentences) plus surrounding context. This is a lightweight, deterministic/heuristic pass — not an LLM call — so obviously-mechanical segmentation doesn't consume LLM budget or introduce non-determinism where none is needed. Ambiguous or malformed candidates are flagged low-confidence here (FR-CORE-18), not silently dropped.

**Stage 3 — Structuring (Mode A / `llm-gateway`).** Each candidate is sent through the same pluggable LLM interface Mode A uses (`llm-gateway`, ADR-004) to produce a structured `Requirement` draft: name, ID, cleaned "shall" text, and a best-effort category. **This is drafting, not deciding** — the same discipline already applied to `cem-core` (LLM never decides, only drafts) applies here: the LLM proposes structure, nothing auto-merges without passing Stage 5.

**Stage 4 — Grounding & Provenance.** Each drafted `Requirement` is stamped with: (a) FR-CORE-08's standard origin/validation/staleness provenance, tagged `origin: ai-suggested`; (b) the LLM generation provenance already required elsewhere (model, version, prompt-template hash, temperature/seed, context snapshot — the same fields FR-CEM-05 requires for CEM-generated content, applied here even though this is a Product 1 capability); and (c) the document citation required by FR-CORE-15. A Requirement missing any of these three is rejected before Stage 5, not surfaced for review with gaps.

**Stage 5 — Validation & Proposal.** The drafted set passes the standard `sysml-core` semantic-validation layer (FR-CORE-05) — the same gate every write goes through, human or AI. The validated set lands as a Git branch/commit (never Main) and becomes a reviewable proposal under the generalized review-gate mechanism (FR-CORE-16). The reviewer sees each Requirement with its citation, confidence, and any candidate-structure suggestions (FR-CORE-17) alongside it, and accepts/rejects individually or in one consolidated batch — mirroring the existing L1 (individual) / L2 (batch) review patterns from §5.6, even though Autonomy Levels themselves govern CEM-generated change specifically; document-import proposals reuse the same *review UX*, not the L0–L4 *scale* (which is CEM-specific per FR-CEM-16's own scope statement).

**Why `llm-gateway` in a Product 1 flow doesn't break "Product 1 stands alone":** `llm-gateway` is listed in the general technology stack (reqs §4), not under the Product 2/CEM-specific column — it's shared infrastructure. A Product-1-only deployment still needs *an* LLM behind that interface (local Ollama satisfies NFR-CEM-03's data-isolation default) to get FR-CORE-07's documents→draft-model path at all; what it does *not* need is `cem-core`, `cem-geometry`, `cem-connectors`, or the scheduler. This should be stated explicitly in reqs §4 so it's not read as an accidental Product-2 dependency.

### §5.6 Amendment — Review-Gate Origins

§5.6's existing amendment (FR-PM-05) already generalizes the proposal/branch mechanism to two origins: `cem-generated` and `human-authored`. This amendment adds a third: **`document-import`** (FR-CORE-16), carrying `autonomyLevel: n/a` exactly as `human-authored` proposals do — the L0–L4 scale governs CEM-generated change only. Three origins, one mechanism; still no second approval pipeline.

---

## 3. Implementation Guidance (additions to `Axioma_implementation_v4.md`)

### 3.1 REST Endpoints (revises the existing `POST /import/documents` entry)

```
POST /import/documents                      — { fileRef } → { jobId }  (async; was previously a bare, unspecified endpoint)
GET  /import/documents/{jobId}               — status: Extracting | Segmenting | Drafting | Validating | AwaitingReview | Failed
GET  /import/documents/{jobId}/candidates    — per-candidate: text, confidence, citation, category, flags (FR-CORE-18)
GET  /import/documents/{jobId}/suggestions   — candidate structural nouns (FR-CORE-17), display-only
POST /import/documents/{jobId}/proposal      — materializes the validated set as a `document-import` proposal (FR-CORE-16)
```

`GET /cem/proposals/{branchId}` and `POST /cem/proposals/{id}/accept|reject` (already specified, impl §1.2) are reused unchanged for the resulting proposal — no new accept/reject endpoint is introduced, consistent with "one mechanism, three origins."

### 3.2 Data Model Additions

| Addition | Type | Notes |
| :--- | :--- | :--- |
| `citation` | Property (on `:Requirement`) | `{ documentId, page, offset? }` — required when `origin = ai-suggested` and `source = document-import` |
| `confidence` | Property (on `:Requirement`, transient/proposal-scoped) | Not persisted to Main; lives on the proposal/branch only, discarded on accept (it describes the draft, not the accepted model content) |
| `document-import` | Enum value (on the existing `proposalOrigin` field introduced by FR-PM-05) | Extends `{human-authored, cem-generated}` → `{human-authored, cem-generated, document-import}` |
| `:CandidateStructureSuggestion` | Node label, proposal-scoped only | Never promoted automatically to `:Structure`; a human action converts a suggestion into a real Block, at which point the suggestion node is discarded |

### 3.3 Service Ownership

Extraction/OCR (Stage 1) and deterministic segmentation (Stage 2) are new responsibilities of the `api` import path — no new service needed, this is CPU-bound text processing, not model-graph logic. Stage 3 (structuring) calls `llm-gateway` exactly as Mode A does. Stages 4–5 reuse `sysml-core`'s existing validation and the existing Git-backed MVS branch/commit flow unchanged.

### 3.4 New ADR Candidate

| ADR | Decision needed | Status |
| :--- | :--- | :--- |
| **ADR-012** | Confirm `llm-gateway` as a Product-1-tier shared dependency (not CEM-exclusive), and confirm local-Ollama-by-default satisfies a CEM-absent, Product-1-only deployment's need for FR-CORE-07's documents→draft-model path. | Proposed |

### 3.5 Roadmap Placement

FR-CORE-14…18 belong in **P1.3 Digital Thread** (impl §4.1), alongside the already-scheduled "Import: ReqIF, SysML v2 API, and AI-assisted docs→draft-model (FR-CORE-07)" line — this amendment is filling in a line item that phase already claims, not adding a new phase dependency. No change to Product 2's independence: this pipeline never calls `cem-core`, `cem-connectors`, or `scheduler`.

---

## 4. Test Coverage (additions to `Axioma_test_specification_v3.md`)

| Test ID | Verifies | Setup / Action | PASS | FAIL |
| :--- | :--- | :--- | :--- | :--- |
| **T-DOCIMPORT-01** | FR-CORE-14 | Upload a 10-page turbofan requirements PDF with 20 clear "shall" statements. | Job completes; 20 `Requirement` candidates produced, each with name/ID/text/category. | Any statement dropped without being surfaced as low-confidence (FR-CORE-18), or job blocks synchronously instead of running as a job. |
| **T-DOCIMPORT-02** | FR-CORE-15 | Inspect a drafted Requirement's provenance. | Citation present (page + offset where extractable); LLM generation provenance fields all present; missing any one of these fails validation before the proposal is created. | Requirement reaches the review UI with a missing citation or missing generation provenance. |
| **T-DOCIMPORT-03** | FR-CORE-16 | Complete an extraction job; open the resulting proposal. | Proposal appears via the existing `/cem/proposals/{branchId}` endpoint with `origin: document-import`, `autonomyLevel: n/a`; accept/reject works identically to a `human-authored` or `cem-generated` proposal. | A second, divergent review UI/endpoint exists for document-import proposals. |
| **T-DOCIMPORT-04** | FR-CORE-17 | Upload a document that mentions "the Combustor assembly" in prose with no prior Combustor Block in the model. | "Combustor" appears as a candidate structure suggestion, display-only; no `:Structure` element is created automatically. | A Block is auto-created without human action. |
| **T-DOCIMPORT-05** | FR-CORE-18 | Upload a document with one requirement split across a page break and one requirement embedded in a table. | Both are surfaced with a lower confidence score and a flag explaining why, not silently merged incorrectly or dropped. | Either statement is missing from the candidate list with no failure/flag recorded. |
| **T-DOCIMPORT-06** | FR-CORE-14, NFR-CEM-03 | Run the pipeline against a local-Ollama-only deployment with no CEM services running (`cem-core`/`cem-connectors`/`scheduler` absent). | Pipeline completes successfully — confirms FR-CORE-07 has no hidden Product-2 dependency. | Pipeline fails or silently calls a CEM-tier service. |
| **T-DOCIMPORT-07** | FR-CORE-14 | Upload a scanned (image-only) PDF. | OCR runs automatically; extraction proceeds without a separate user action. | Job fails or requires a manual OCR step outside the platform. |

---

## 5. Summary

| Question | Answer per this amendment |
| :--- | :--- |
| What does FR-CORE-07's documents→draft-model path actually produce? | Structured, cited `Requirement` elements + non-binding candidate structure suggestions. Not subsystems — that's Mode B (FR-CEM-02), separate and later. |
| Is it synchronous? | No — asynchronous job with 5 inspectable stages (Extraction → Segmentation → Structuring → Grounding/Provenance → Validation/Proposal). |
| Does it need the CEM running? | No — `llm-gateway` is shared Product-1-tier infrastructure; `cem-core`/`cem-connectors`/`scheduler` are never invoked (T-DOCIMPORT-06 verifies this explicitly). |
| How does a human review the result? | Same proposal/branch review-gate as CEM-generated and human-authored changes (FR-CEM-07, FR-PM-05), extended with a third origin, `document-import` (FR-CORE-16) — one mechanism, three origins. |
| What happens to low-confidence or malformed extractions? | Surfaced with a confidence signal and flag, never silently dropped or silently merged (FR-CORE-18). |
| What provenance does a generated Requirement carry? | Standard origin/validation/staleness (FR-CORE-08), LLM generation provenance (model/version/prompt-hash/seed/context), and a document citation (page/offset) — all three required, none optional. |
