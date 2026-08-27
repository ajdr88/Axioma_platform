# Axioma: SysML/MBSE Tool Landscape Evaluation & Build-vs-Adopt Update

**Status:** Merged into `Axioma_requirements_v5.md` / `Axioma_implementation_v5.md` (Phase 0 doc-consolidation pass, 2026-08-27) — kept here for history, not superseded content. This update's ADR-011 recommendation is the one that won the numbering collision and is what v5 records.
**Sources evaluated:** sysml.org/sysml-tools/ (tool directory), adore.mbse-env.com (DLR's ADORE — Jasper Bussemaker's own tool, same author as the thesis behind Part 2 of the system-model amendment), sinelabore.de (SysML v2 code-generation product), plus the GitHub repos and license pages those pages linked to.
**Bottom line up front:** none of these tools is adoptable as Axioma's platform — none combines native SysML v2, safety/hazard modeling, CRDT collaboration, and a CEM-equivalent optimization/geometry layer, and the one tool that's architecturally closest to Mode B (ADORE) is explicitly non-commercial-use-only. But **two MIT-licensed Python libraries underneath ADORE — `adsg-core` and `SBArchOpt` — are directly adoptable** as the encoder/optimizer foundation for `cem-core`, which meaningfully de-risks the FR-ARCH gap identified in the previous amendment. This is a genuine, actionable finding, not just a survey.

---

## 1. sysml.org/sysml-tools/ — the general tool landscape

### 1.1 Inventory (as listed on the page)

**SysML v1 (mature ecosystem)** — Cameo/MagicDraw/CATIA Magic, Enterprise Architect, IBM Rhapsody, Visual Paradigm (all commercial); Modelio SysML Architect, Eclipse Papyrus (v1) (free/open-source).

**SysML v2 (emerging ecosystem)** — Cameo/CATIA Magic SysML v2, Syside Modeler, Ansys System Architecture Modeler, Siemens Systems Modeler, IBM Rhapsody SE, Tom Sawyer SysML v2 Viewer (all commercial); **Syside Editor** (free VS Code extension, textual notation only); **Eclipse SysON**, **Eclipse Papyrus (SysML v2)**, **SysML v2 Pilot Implementation** (all open-source).

Axioma's own ADR-006/FR-CORE-01 already commit to native SysML v2 on KerML, so the v1 tools (Cameo included, despite being the subject of the existing `Axioma_cameo_tutorial_gap_analysis.md`) are a different generation of the same problem — the v2 column is the relevant comparison set. The three open-source v2 entries are examined below; free/open-source SysML v2 tooling is a genuinely small set (three projects), which itself is informative — this is an early, thin ecosystem, not a crowded one Axioma would be late to.

### 1.2 Capability comparison — open-source SysML v2 tools vs. Axioma

| Capability | Eclipse SysON | SysML v2 Pilot Implementation | Syside Editor (free) | **Axioma (as specified)** |
| :--- | :--- | :--- | :--- | :--- |
| Native SysML v2 / KerML | Yes | Yes (reference impl. — the thing conformance is measured against) | Yes (textual only) | Yes (FR-CORE-01, ADR-006) |
| Graphical + textual dual-notation sync | Yes (web-based, Sirius Web) | Textual (Xtext) + PlantUML-rendered diagrams (generated, not live-synced) | Textual only | Yes, real-time bi-directional (FR-CORE-02) — a real differentiator; nothing in the open-source set does *live* graphical↔textual sync the way FR-CORE-02 specifies |
| Deployment model | Web app (Sirius Web / Spring stack) | Eclipse desktop IDE, Jupyter kernel | VS Code extension | Web app (Next.js/React) |
| Model scale / performance engineering | Not documented — no published 1M-element target | Not documented; explicitly "pilot," not perf-engineered | N/A (editor only) | 1M-element target, WebGPU + virtualization, CI-enforced (NFR-PERF-01…06) |
| Collaboration (CRDT/multi-user) | Not documented as a current feature | No | No (single-file editing) | Yes (FR-CORE-06, CRDT) |
| Safety/Hazard modeling | No | No | No | Yes (FR-SAFE-01…05) — **no tool in this set has anything comparable** |
| Behavioral simulation/execution | Not a stated SysON feature | Yes — this pilot implementation is the closest existing thing to `fuml-runtime`'s job (it's an actual fUML-adjacent execution capability tied to the reference language work) | No | Planned (FR-CORE-04, fUML RI sidecar + `alf-lite`) |
| Architecture optimization (Mode B equivalent) | No | No | No | Planned, currently under-specified (see FR-ARCH gap) |
| Geometry synthesis (Mode C equivalent) | No | No | No | Planned, research-risk (Mode C) |
| Grounded AI copilot (Mode A equivalent) | No | No | No | Planned (FR-CEM-01) |
| Provenance/autonomy model | No | No | No | Yes (FR-CORE-08, FR-CEM-16…18) — unique to Axioma in this comparison set |
| License for commercial use | Open-source (Eclipse Foundation) | EPL-2.0 / GPL option | Free | N/A (proprietary product) |
| Maturity | First release Dec 2023; industrial deployment explicitly targeted for **2026**, not yet claimed as production-ready | Explicitly labeled "pilot," ongoing since the SysML v2 standardization effort itself, still experimental | Editor-only, stable for its narrow scope | Pre-implementation (spec stage) |

### 1.3 What's genuinely missing from Axioma's plan, surfaced by this comparison

- **Live text↔diagram sync is a real, not-yet-solved problem even in the open-source SysML v2 world.** SysON does graphical *and* textual editing but the page doesn't claim they're kept in continuous sync the way FR-CORE-02 requires; the Pilot Implementation's diagrams are PlantUML-*generated* (a render step, not a live view). This means FR-CORE-02 is a harder, more novel requirement than the requirements doc currently implies — it's not "catch up to what open tools already do," it's ahead of what they do. Worth flagging as a delivery-risk item, not a routine one.
- **A conformance/interop testing gap.** Axioma's FR-CORE-07 (import/export, ReqIF + "SysML v2 standard API") has no named target to validate against. The **SysML v2 Pilot Implementation is literally the reference implementation the OMG standard itself is validated against** — it should be an explicit interop-conformance test target for `sysml-core`, not just "the SysML v2 API" in the abstract. Recommend adding a named test: round-trip a Pilot-Implementation-authored model through Axioma's import/export path without semantic loss, the same way the gap-closure amendment already names a Cameo/ReqIF round-trip test.
- **No open-source tool attempts anything like Mode A/B/C.** This confirms (rather than closes) the earlier finding: Axioma's CEM is not chasing an existing capability anywhere in the free/open SysML tooling space — it would be novel even against the full landscape, not just against Cameo.

---

## 2. ADORE (adore.mbse-env.com) — deep dive, and the licensing finding that matters

ADORE is DLR's own implementation of the exact DSG/ADSG methodology already used as the primary source for Part 2 of `claude/Axioma_turbofan_system_model_amendment.md` — same author (Jasper Bussemaker), same institute. This is the tool version of the thesis, actively developed since 2019, with 12 documented application cases (academic — Apollo mission, supersonic business jet — and industrial aerospace — hybrid-electric propulsion, flight control, landing gear, space mission planning) under EU-funded projects (COLOSSUS, AGILE 4.0). Demonstrated handling design spaces from 70 to 79 million candidate architectures.

### 2.1 What it actually is, architecturally

Three pillars, confirmed from the AIAA 2024 paper describing it:
1. **Web-based GUI** — a system view (functions/derivation), a component view (subsystem detail), and a port view (connections); architectural choices are drawn as blue-dashed arrows (a simplified visual shorthand for the formal choice-node graph theory from the thesis).
2. **Evaluation connectivity** — Python-direct (with the Class Factory Evaluator pattern already covered in the literature extraction), file-based (XML/JSON/RDF, the Node Factory Evaluator pattern), and dynamic MDAO-workflow coupling (pyCycle/OpenMDAO).
3. **Optimization algorithm interfaces** — links out to external solvers, i.e. SBArchOpt.

A fourth item not in the thesis but confirmed by the paper: a **Supplementary Design Space Graph (SupDSG)**, extending graph variability modeling beyond pure architecture semantics — used in the jet-engine case to model airflow connection *sequences*, not just which components connect. Worth a follow-up read if Axioma ever needs to model flow-path ordering (e.g., bypass-duct-then-mixer-then-nozzle) as a first-class graph concept rather than an implicit port-connection consequence.

**No current SysML integration.** The paper states integration with existing MBSE methods, including SysML v2, is explicitly **future work**, not a current capability — confirming what the previous amendment already inferred from the thesis alone. This is a genuine open opportunity: nobody, including ADORE's own authors, has yet built the SysML v2 ↔ ADSG bridge that `Axioma_turbofan_system_model_amendment.md` §2.9 sketches. If Axioma builds it, it would be ahead of ADORE itself on this specific point, not behind it.

### 2.2 The licensing finding — this changes the ADR-011 recommendation

**ADORE itself is free-for-research-and-teaching only, and explicitly prohibits commercial use — including use in third-party-funded research projects.** It is proprietary to DLR, not open-source, has no public repository, and the license page states it plainly: no cost for research/teaching, but commercial use is disallowed outright, redistribution is disallowed, and it's offered as "prototypical" research code with no support or error-free guarantee. **This rules out adopting ADORE itself for Axioma, full stop** — not a licensing detail to negotiate around, a hard exclusion for a commercial GmbH product.

**But the two libraries underneath it are a different story:**

| Library | License | What it is | Adoptable for Axioma? |
| :--- | :--- | :--- | :--- |
| **`adsg-core`** (github.com/jbussemaker/adsg-core) | **MIT** | Standalone Python implementation of the Design Space Graph / Architecture Design Space Graph — selection choices, connection choices, additional design variables, hierarchical modeling, design-vector generation | **Yes** — this is the actual encoder/decoder engine that Part 3 §3.1 (FR-ARCH-05) and §3.3 of the system-model amendment identified as missing from `cem-core` |
| **`SBArchOpt`** (github.com/jbussemaker/SBArchOpt) | **MIT** | Surrogate-based (hierarchical Bayesian) + evolutionary optimization toolbox for exactly this class of mixed-discrete, hierarchical, hidden-constraint architecture problems; built on `pymoo` | **Yes** — this is the optimizer half of the same gap (FR-ARCH-06/08) |

Both are pip-installable Python packages (`pip install adsg-core`, `pip install sb-arch-opt`), MIT-licensed with no commercial-use restriction, actively maintained by the same author whose methodology already grounds Axioma's system model. **This is materially better evidence than what ADR-005's fUML/Alf survey had to work with** — there, the maintained engine (fUML RI) was permissively licensed but the authoring layer (Alf RI) was GPL-v3 and had to be excluded, forcing an in-house build (`alf-lite`). Here, *both* halves of the equivalent stack (design-space modeling + optimization algorithms) are MIT-licensed and immediately usable — no exclusion, no in-house rebuild forced by licensing.

### 2.3 The real remaining question is architectural, not legal

Axioma's backend is Rust (Axum), and `adsg-core`/`SBArchOpt` are Python. This is the same "language mismatch" shape ADR-005 already worked through for fUML (Java) — and the same resolution likely applies: **wrap them as a sidecar service**, analogous to `fuml-runtime`, rather than porting the DSG/ADSG encoder logic to Rust. Concretely, this argues for a new backing service — call it `cem-archspace` or fold it into `cem-core`'s existing scope as a Python sidecar — that `cem-core`'s Rust code drives over gRPC (reusing ADR-008's transport standardization, exactly as recommended), rather than a ground-up Rust reimplementation of hierarchical BO and DSG graph-rewrite logic. This is a much smaller, much lower-risk build than what Part 3 of the system-model amendment scoped assuming no reusable library existed.

**Updated ADR-011 recommendation:** Adopt `adsg-core` (MIT) and `SBArchOpt` (MIT) as a Python sidecar service for the design-space/optimization core of Mode B, driven over gRPC from `cem-core`'s Rust code, mirroring the `fuml-runtime` sidecar pattern (ADR-005/008). Do not adopt ADORE itself (license excludes commercial use); Axioma still needs to build its own web GUI (the diagram-engine additions already scoped in the system-model amendment §3.5), and still needs to build the SysML v2 ↔ ADSG bridge that neither ADORE nor anyone else has built yet. This roughly halves the previously-scoped FR-ARCH build risk (§3.3's encoder/optimizer gap) while leaving the UI and SysML-integration gaps (§3.2, §3.4, §3.5) unchanged — those were never going to be solved by an external library regardless.

---

## 3. Sinelabore — deep dive

Sinelabore's SysML v2 offering is **narrower and differently-shaped than it might first appear, and it is commercial, not free/open-source** (the linked wiki page is documentation for a paid product, not itself an open tool). What it actually is: a **code generator**, not a modeling or simulation platform — it reads SysML v2 textual models (parts, ports, attributes, state machines) and emits executable C++ (state-machine handlers plus structural glue code), positioned explicitly for early-design mockups and executable simulation of behavior, not as a system-of-record model store. It requires a JRE (17+) to run and is one small piece of a broader toolchain (the author's own framing: complementary to, not a replacement for, a full MBSE environment).

**Relevance to Axioma: low, and not because it's a bad tool — because it solves a problem Axioma has already solved differently.** Sinelabore's whole value proposition (state machine → executable code, from a textual SysML v2 source) is the same territory as `alf-lite` → `fuml-runtime` (FR-CORE-04/09, ADR-005): compiling authored behavior into something executable. Axioma's approach targets the OMG-standard fUML execution semantics via a licensed-clean sidecar; Sinelabore targets direct C++ codegen for embedded deployment, which is a different and arguably more production-deployment-oriented goal (Axioma's `fuml-runtime`/`alf-lite` path is about *simulating and verifying* behavior inside the model environment, not about generating deployable flight code — that's out of Axioma's stated scope entirely, per CLAUDE.md). **No adoption case here** — it's solving an adjacent problem for an adjacent audience (embedded-code generation from SysML v2, not MBSE-platform behavioral simulation), and it's commercial software Axioma would be competing with in spirit, not borrowing from.

One transferable idea, not a tool to adopt: Sinelabore's own framing — "textual modeling lowers the barrier to MBSE... executable simulations early in design, without a heavyweight GUI toolchain" — is a useful external validation of a choice Axioma has already made (FR-CORE-02's textual/LSP path, Monaco-based, impl §4.3) rather than a new idea to import.

---

## 4. Synthesis — what's missing, and does adoption make sense

### 4.1 What's missing from Axioma's current plan (surfaced by this landscape scan)

1. A named **conformance/interop test target** for FR-CORE-07 — recommend the SysML v2 Pilot Implementation, since it's the standard's own reference implementation (§1.3).
2. **FR-CORE-02's live text↔diagram sync is harder than the current docs imply** — no open-source SysML v2 tool has clearly solved this yet either; worth an explicit delivery-risk flag in the requirements doc rather than treating it as routine (§1.3).
3. The **SysML v2 ↔ ADSG bridge is unbuilt anywhere, including by ADORE's own authors** — confirmed, not just inferred, as a genuine opportunity rather than a known solved problem Axioma is behind on (§2.1).
4. A concrete **library-adoption path for the FR-ARCH encoder/optimizer gap** (`adsg-core` + `SBArchOpt`, both MIT) that didn't exist as a documented option before this research pass (§2.2) — this is new information, not something the previous amendment's ADR-011 placeholder could have specified.

### 4.2 Does it make sense to adopt one of these tools?

**Not as a platform.** None of SysON, the Pilot Implementation, ADORE, or Sinelabore is a candidate to replace or absorb any part of Axioma's own build — each is either too narrow (Sinelabore: codegen only; Syside Editor: text editing only), too early (SysON: pre-industrial, targeting 2026), too experimental-by-design (Pilot Implementation: explicitly a pilot), or legally excluded (ADORE: non-commercial license). None combines SysML v2 + safety/hazard modeling + CRDT collaboration + a CEM-equivalent layer — that combination doesn't exist yet anywhere in this landscape, open-source or commercial, which is itself the clearest evidence for why Axioma's product thesis holds.

**Yes, as libraries, for one specific gap.** `adsg-core` and `SBArchOpt` (both MIT, both from the same author whose methodology already grounds Axioma's Mode B design) are a direct, low-risk answer to the encoder/optimizer half of the FR-ARCH gap identified in the previous amendment — adopt them behind a Python sidecar (mirroring the `fuml-runtime` pattern) rather than building a Rust hierarchical-BO/DSG-graph-rewrite engine from scratch. This is the update ADR-011 needed and didn't have when it was first proposed.

---

## Suggested next steps

1. Fold the updated ADR-011 recommendation (§2.2, §2.3) into `claude/Axioma_turbofan_system_model_amendment.md` before that amendment is merged.
2. Add a named SysML v2 Pilot Implementation round-trip test to FR-CORE-07's test coverage (§1.3).
3. Add an explicit delivery-risk note to FR-CORE-02 flagging that live text↔diagram sync is not solved anywhere in the current open-source SysML v2 ecosystem (§1.3) — this affects P1.2 estimation, not just documentation.
4. Spike `adsg-core` + `SBArchOpt` behind a gRPC sidecar against one small test problem (e.g., a single compressor subsystem's stage-count/bleed-offtake choices from Part 2 of the system-model amendment) before committing P2.1's estimate.
