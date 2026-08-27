# Axioma Literature Extraction — Reference Corpus (Turbofan Pilot)

**Status:** Working extraction, first pass. Companion to `CLAUDE.md`, `Axioma_requirements_v4.md`, `Axioma_implementation_v4.md`. Its findings were used directly by `Axioma_turbofan_system_model_amendment.md`, now merged into `Axioma_requirements_v5.md` §5.15/§5.16/§5.17 and `Axioma_implementation_v5.md` §2.6 (Phase 0 doc-consolidation pass, 2026-08-27). Kept here for history and re-verifiable citations — it carries no FR/ADR spec text of its own to re-merge.
**Purpose:** Ground the pilot turbofan spec/system model and the CEM Mode B (`cem-core`) architecture-optimization methodology in established literature rather than inventing either from scratch. Source files live in the user's local `literature/` folder (not in this project's file store — see Source Corpus below for how to re-fetch them).
**How to use this doc:** every claim below is page-cited against the source PDF so it can be re-verified. Two spot-checks were done against source text (§6); results and caveats are noted there — treat NASA SP-36 page citations as accurate ±1 page, Bussemaker citations as exact.

---

## Source corpus

| Document | Type | Length | Direct relevance |
|---|---|---|---|
| Tan, Cimtalay, Mavris (2024), *"An MBSE Approach to Hydrogen Combustion Turbofan Propulsion System Design"*, AIAA 2024-4378, Georgia Tech ASDL | Conference paper | 19 pp | Full MBSE methodology + SysML decomposition example for a turbofan — closest template for the pilot system model |
| Bussemaker, J.H. (2025), *"System Architecture Optimization: Function-Based Modeling, Optimization Algorithms, and Multidisciplinary Evaluation"*, PhD dissertation, TU Delft / ISAE-SUPAERO | Dissertation | 262 pp | Methodology (DSG/ADSG/ADORE) + optimization algorithms (hierarchical BO) directly targeted at architecture optimization — the closest published analogue to Mode B (`cem-core`); includes an explicit jet-engine architecture case study |
| NASA SP-36, *"Aerodynamic Design of Axial-Flow Compressors"* (revised ed., 1965), NASA Lewis Research Center | Reference handbook | 526 pp (scanned, OCR'd via Acrobat Capture 3.0) | Classic, still-authoritative compressor design-requirements structure and gas-generator/engine matching methodology — seeds the Fan & LP Compression / Core (HP) Compressor subsystem requirements and 0D/1D engine-matching logic |

All three are text-extractable (Bussemaker and Tan cleanly; NASA SP-36's prose extracts cleanly but its **displayed equations and all figures/charts do not** — see §5, Extractability, before trying to mine numeric/equation content from it programmatically).

---

## 1. Tan et al. (2024) — MBSE turbofan decomposition template

Full paper read (19 pp). Directly usable as a comparison template for Axioma's pilot decomposition, with one structural difference to reconcile (see below).

**Methodology sequence used by the paper:** Stakeholder Needs Analysis → Requirement Analysis (requirement statements → engineering parameters, grouped by subsystem) → Prioritization Matrix (Safety > Operation > Environment) → Quality Function Deployment (House of Quality: requirements × engineering parameters × interrelations × weights/risks) → System Decomposition (SysML Block Definition Diagrams) → cross-cutting impact analysis (here: hydrogen-fuel impact via a `HydrogenCharacteristicsConstraint` block + a dependence matrix mapping component interdependencies).

**Engineering-parameter taxonomy by subsystem** (p.4-5 of the paper) — a ready-made checklist to cross-reference against Axioma's own FR set:
- Fuel Supply: tank size, pressure, temperature, pipe size
- Combustion Chamber: chamber size, flame temperature, pressure loss, NOx emission
- Compressor: pressure ratio, mass flow rate, inlet velocities
- Turbine: blade geometry, rotational speed, inlet temperature, number of stages
- Whole-engine: TSFC, propulsion efficiency, thermal efficiency
- Thermal management: heat exchanger power, cooling
- Material/structure: fatigue, elasticity, melting point, thermal conductivity
- Exhaust: temperature, thrust, velocity, emission
- Control: throttle position, ignition timing, fuel injection timing

**Decomposition used (important difference from Axioma's pilot):** the paper segments the propulsion system into **3 top-level subsystems** — Fuel Supply System, Engine Control System, Thrust Generation System — with Compressor/Burner/Turbine/Bleed/Nozzle/Afterburner/Inlet/Heat-Exchanger all nested *inside* Thrust Generation System (p.8-9). This is coarser than Axioma's pilot, which per `CLAUDE.md` already commits to **5 subsystems**: Fan & LP Compression, Core (HP) Compressor, Combustor, Turbine (HP & LP), Control (FADEC/EEC) — i.e. Axioma's decomposition promotes Compressor/Combustor/Turbine to top-level subsystems and merges the paper's Fuel Supply System into Control/other subsystems (or leaves it implicit). **This is worth an explicit decision, not a silent divergence** — Axioma's finer-grained split is arguably better suited to independent subsystem ownership/optimization (each becomes its own Mode B trade-study unit), but the paper's Engine Control System component list (FCU, sensors/monitors, ignition control, throttle control, FADEC) and dependence-matrix technique are directly reusable for Axioma's Control subsystem regardless of the top-level split chosen.

**Component-level detail directly reusable per Axioma subsystem:**
- *Control (FADEC/EEC)*: Fuel Control Unit, Sensors and Monitors, Ignition Control, Throttle Control, FADEC (p.10) — hydrogen has near-zero impact on this subsystem's *core functionality* per the paper (fuel-type-agnostic control algorithms), only fuel-management calibration changes (p.13) — useful negative-result data point if a hydrogen variant is ever considered for the Axioma pilot.
- *Compressor*: engineering parameters = number of stages, compression ratio, inlet velocity, mass flow rate (p.15); explicitly cross-linked (dependence matrix) to turbine, shaft, inlet, fan, burner, bleed — i.e., changes propagate to all of these, which is a concrete traceability-edge template for Axioma's `Satisfy`/`Verify`/`Refine`/dependency graph.
- *Turbine, Burner, Heat Exchanger, Fan, Inlet*: each gets a short design-impact narrative (p.16-18) — useful as a starting requirements-rationale text if Axioma later needs a hydrogen-combustion variant, lower priority for the initial (presumably conventional-fuel) pilot.

**Cited prior work worth knowing about (not read, referenced by Tan et al.):** pyCycle (open-source gas-turbine cycle analysis tool by E. Hendricks, used as the thermodynamic backend in both this paper and — independently — in Bussemaker's jet-engine benchmark, see §2 below) and De Smedt's TU Delft thesis "Aircraft Jet Engine Architecture Modeling" (2021), which used pyCycle as an architecture-optimization benchmark and is a likely direct ancestor/relative of Bussemaker's jet-engine case study. Both are candidates for a follow-up literature pull if Mode B needs an actual thermodynamic-cycle solver rather than just matching methodology.

---

## 2. Bussemaker (2025) — SAO methodology for Mode B (`cem-core`)

Deep-read of Chapters 1-5 (skipped implementation-level Appendices A-C, E-F). All citations are **PDF page numbers**, verified exact against source text at one spot-check (p.195 SysML v2 mapping quote, reproduced verbatim — see §6).

### 2.1 Core concept
System Architecture Optimization (SAO) = an **architecture generator** (design-space model + encode/decode + optimization algorithm) coupled to an **architecture evaluator** (multidisciplinary simulation, via MDAO). Three deliverables map cleanly onto Axioma's stack:

| Bussemaker's contribution | Axioma equivalent |
|---|---|
| DSG / ADSG / ADORE (design-space modeling) | Mode B's architecture/topology generator over the SysML v2 graph |
| SBArchOpt (hierarchical BO + NSGA-II library) | Mode B's deterministic optimizer core |
| Collaborative MDAO extensions (dynamic workflows, NFE, ask-tell) | `cem-connectors` / FEA-CFD connector framework / `scheduler` |

Headline result: a hierarchy-aware Bayesian Optimization algorithm matched NSGA-II's result on a jet-engine architecture problem using **92% fewer function evaluations** (300 vs. 3250) — pp.13-14, 96.

### 2.2 Design Space Graph / Architecture Design Space Graph (pp.100-129) — architecture modeling primitives
- The DSG models a **design space**, not one instance: the same graph represents *all possible architectures simultaneously*; resolving choices narrows it to one "final state" (an architecture instance) (p.105). This is structurally different from a typical SysML v2 model graph, which represents realized configurations — see the author's own bridging proposal below.
- Primitives: **derivation edges** (AND/OR requires-relations, cycles explicitly allowed), **selection choice nodes** (pick-one, 4-step graph-rewrite to resolve), **incompatibility constraints** (undirected, cross-branch exclusion), **choice constraints** (Linked / Permutations / Unordered [non-]replacing combinations — e.g. "# compressors = # turbines") (pp.104-108).
- **Connection domain**: separate mechanism for assignment/allocation problems (source↔target connectors with cardinality rules), resolved after selection since existence of connectable elements depends on selection outcomes. Table showing this reproduces all of Selva et al.'s standard architecture-decision patterns (Combining/Assigning/Partitioning/Downselecting/Connecting/Permuting) as cardinality special-cases (p.118) — a strong genericity signal.
- **ADSG semantic layer** (p.122-129): node-type catalog organized around three architecting tasks — function-to-form allocation (`FUN`, `COMP`, `MULTI` multi-fulfillment, `NOF` non-fulfillment, `DE` decomposition, `CON` concept), characterization (`DV` design variable, `INP` static input, `MET`/`OBJ`/`CON` metric/objective/constraint), and connection (`OUT`/`IN` ports). Includes recursive, independently-instantiable **subsystems (SYS)** — directly relevant to modeling Axioma's 5 subsystems as independently-optimizable units within one architecture graph.

**Author's own SysML v2 bridging proposal (p.195, verbatim, spot-check confirmed):**
> "connections should be established between ADORE and existing standardized MBSE languages, such as SysML, OPM, and Arcadia/Capella. An especially promising candidate is SysML v2, as it defines standardized APIs for data integration and includes variability modeling as an integral part of the language. For example, ADORE functions can be mapped to SysMLv2 actions, components to parts, selection choices to variation definitions, design problems to trade studies, and generated architectures to trade study alternatives."

This is a direct, author-endorsed mapping Axioma can adopt or adapt for how Mode B's choice/hierarchy model sits on top of the Product 1 graph, rather than needing to invent the SysML v2 bridge from first principles. The same page also flags that ADORE is *not* a full Architecture Description Language (no stakeholders/needs layer) — implying Axioma's Product 1 graph (which *does* model Stakeholder/Requirement nodes) should stay the outer layer, with an ADSG-like choice model as an inner layer for the architecture-generation problem specifically.

### 2.3 Bottom-up, function-based modeling process (pp.136-145) — candidate Mode B authoring methodology
An 11-step guideline for building a design-space model from functional requirements (full step list in the raw extraction on file — condensed here): identify boundary functions (verb+noun, solution-neutral) → for each unfulfilled function choose a fulfillment mechanism (concept-narrowing / decomposition / component / multi-fulfillment / non-fulfillment, each with explicit applicability tests) → add incompatibility constraints only where truly needed → group into subsystems → characterize (sizing variables, static inputs, constraints) → add choice constraints across instances → add ports for cross-function connections → iterate → define system-level metrics on a permanent element → verify every choice affects ≥1 metric → manually sanity-check in the GUI.

**Key finding directly useful for how Axioma should structure the pilot's function/component graph:** a worked side-by-side comparison (aircraft propulsion, p.145) shows a **top-down decomposition** model needs many ad-hoc cross-tree incompatibility constraints and actually violates its own stated applicability conditions (there's a real execution order — supply energy → convert to work → convert to thrust — so it isn't truly parallel), whereas the **bottom-up function-fulfillment model needs zero incompatibility constraints** because compatibility is encoded implicitly via derivation/induction edges. The thesis's stated conclusion: bottom-up is "a more natural approach" (p.145, restated p.161). **Recommendation for Axioma:** model the turbofan pilot's 5-subsystem architecture bottom-up from boundary functions (e.g. "generate thrust," "provide bleed air," "provide shaft power") rather than top-down from a fixed subsystem tree, even though the 5-subsystem grouping is already decided — the subsystems can be the `SYS` grouping nodes, populated by a bottom-up function/component graph underneath.

### 2.4 Optimization algorithm findings (Ch.2) — directly reusable for `cem-core`
- Four metrics for **characterizing hierarchical design-space difficulty**, computable for any DSG-modeled problem: **Imputation Ratio** (declared/valid space size — measures wasted sampling), **Correction Ratio** (declared/value-constrained-but-not-imputed — isolates value-constraint-driven hierarchy), **Correction Fraction** (splits IR's cause between correction-need vs. imputation-need), **Max Rate Diversity** (worst-case option-occurrence imbalance across valid vectors — flags rare-but-important architecture families, e.g. only 20% of the jet-engine benchmark's valid space is pure-turbojet vs. 80% turbofan) (pp.64-68). **These four numbers are a ready-made health-check Mode B could compute on any subsystem's declared design space before running an expensive optimization.**
- **Hierarchical sampling** (sample from the enumerated valid set, grouped by which variables are active) beats naive random sampling by up to 455% when the true optimum sits in a rare architecture family — grouping by "which variables are active" is the recommended default (pp.69-73).
- **Design-vector correction:** plain finding, stated bluntly — "problem-specific greedy correction is sufficient... problem-agnostic correction does not necessarily improve performance" (p.97). I.e., don't over-invest in generic constraint-repair machinery if simple per-subsystem clamping logic is available.
- **BO vs NSGA-II:** NSGA-II is largely insensitive to how much hierarchy information it's given; BO's performance depends heavily on it (89-149% penalty if hierarchy is ignored) (pp.78-83).
- **Hidden constraints** (simulation non-convergence, i.e. a subsystem's evaluation function can simply fail): best strategy found is a **Probability-of-Viability classifier** (mixed-discrete GP or Random Forest performed best) used either as an infill constraint or a penalty multiplier, plus inflating DoE size by `n_doe = k·n_x/(1-FR_expected)` to guarantee enough viable training points survive (pp.83-91). **Directly relevant**: Axioma's own solver-result typing (`Converged`/`Diverged`/`Failed`/`Timeout`/`Suspect-Numerical`) is exactly the kind of viability signal this classifier would consume.
- Packaged as open-source: **SBArchOpt** (Python, built on `pymoo`) — worth evaluating directly as a `cem-core` dependency or reference implementation rather than reimplementing hierarchical BO from scratch (p.92).

### 2.5 Jet Engine Architecture case study (pp.93-99, 158-161) — closest published analogue to the pilot
Two treatments of essentially the same problem (a raw optimization demo, and a re-formulation inside ADORE for methodology comparison). Architecture choices modeled (`SIMPLE_TURBOFAN_ARCH`, p.94) — **read this as a checklist against Axioma's own pilot decomposition**:
- `IncludeFan` (bool, turbojet vs turbofan) → conditionally activates `BPR` [2.0-12.5], `FPR` [1.1-1.8], `MixedNozzle` (bool), `IncludeGearbox` (bool) → conditionally `GearRatio` [1.0-5.0]
- `OPR` [1.1-60.0], continuous, always active
- `n_shafts ∈ {1,2,3}` with per-shaft `PR_factor,i` [0.1-0.9], `RPM_i` [1000-20000]
- `PowerOfftake`, `BleedOfftake` ∈ {1..n_shafts} — location choices, value-constrained by shaft count (this is the direct analogue of an Axioma Control/FADEC offtake-routing choice)
- Constraints: jet Mach ≤1.0, sum of shaft PR factors ≤0.9, per-stage max PR ≤15.0

Evaluation backend: **OpenMDAO + pyCycle** thermodynamic cycle analysis (1-5 min/eval, ~50% non-convergence rate in random sampling — the source of the hidden-constraint problem above). Resulting problem: 15 design variables (9 continuous, 6 discrete), IR=3.89, 70 valid discrete vectors out of 216 declared. In the ADORE re-formulation (p.158-161), compressor/shaft/turbine stage counts are kept consistent via a **choice constraint linking their instantiation counts** — directly analogous to keeping Axioma's Fan/LP-Compressor, Core/HP-Compressor, and HP/LP-Turbine stage counts mutually consistent.

### 2.6 Collaborative MDAO findings (Ch.4, pp.164-188) — relevant to `cem-connectors` / scheduler
- Four ways architecture affects a fixed MDAO workflow: conditional variables, changed data routing, discipline repetition (e.g. a sizing tool run once per instantiated component), and discipline activation/deactivation.
- Four implementation strategies, with an explicit feasibility ruling: single-static and multi-static workflows are ruled out for real SAO problems (either requires no architectural variation ever, or doesn't scale); **single-dynamic** (one workflow, activation/repetition/routing logic evaluated live from the architecture instance's data) is what the thesis actually implements for a distributed/collaborative setting; **on-demand** (auto-generate a fresh workflow per instance) is what was actually used for the local jet-engine and hybrid-electric case studies (hand-written Python/OpenMDAO, no formal schema) — judged infeasible only for *cross-organizational* collaborative settings, due to current tooling immaturity around auto-deployment, not for a fundamental reason (p.199).
- **Explicit scoping caveat directly relevant to Axioma's near-term architecture decision** (p.188, paraphrased faithfully): if cross-org/IP/distributed-tool constraints don't apply and one team can manage all analysis tools, a locally-integrated workflow (no formal Central Data Schema, no dynamic-workflow wrapper machinery) is more proportionate — the collaborative-MDAO machinery (CDS schemas, Node Factory Evaluator rules, dynamic activation scripts) is real engineering overhead that should be reserved for genuine cross-org cases. **For the pilot specifically, this argues for building `cem-connectors` initially as a straightforward per-subsystem solver interface (on-demand, locally integrated) rather than a full dynamic-workflow/CDS system — deferring that complexity until/unless Axioma actually needs distributed, cross-organization solver integration.**

### 2.7 Author's own gap list (Ch.5 recommendations, pp.193-200) — worth reading before committing to replicate this methodology
- Complete selection-choice encoder (exhaustive enumeration) doesn't scale past a certain design-space size; recommends CSP/SMT solvers instead of brute-force enumeration for large spaces (p.194) — relevant if Mode B's combined 5-subsystem design space gets large.
- Connection choices currently can't influence downstream selection choices (only the reverse) — a known real limitation surfaced by an external application case (p.193-194).
- Recommends composability (import/export design-space sub-models, e.g. per-subsystem) with synchronized function definitions at boundaries — directly relevant to modeling Axioma's 5 subsystems as independently maintainable sub-models (p.195-196, verified verbatim at spot-check, §6).
- On AI: explicitly floats generative-AI assistance for design-space *authoring*, exploration steering, and results interpretation — but never for the search/decision step itself, which stays a classical optimizer (p.195-196). This independently arrives at the same boundary as Axioma's own non-negotiable rule ("`cem-core` never uses an LLM to decide").
- BO scaling limits: GP training struggles past dozens-hundreds of dimensions, validated only on ≤2-objective problems (p.197) — a real ceiling to watch as the pilot's combined design vector grows across 5 subsystems.

---

## 3. NASA SP-36 (1965) — compressor requirements & gas-generator matching

**Caveat up front (see §5 for detail): this book's prose and process descriptions extract cleanly; its equations and all charts/figures do not** — the OCR (1965 scan, Acrobat Capture 3.0) drops or garbles nearly every displayed equation body while preserving the surrounding explanatory prose and equation numbers. Treat this source as strong for *requirements taxonomy and process/matching logic*, weak for *exact formulas and numeric map data* — those need a modern source or manual page-image reading.

**Page-offset note:** book-internal page = PDF page − 18 (verified at three points). Citations below are PDF page numbers.

### 3.1 Compressor design requirements (Ch. I-II, PDF pp.19-70)
Requirements-derivation chain: airplane mission → engine-cycle requirements (via a modified Breguet range equation) → compressor requirements, with explicit worked cycle analysis showing how pressure ratio and efficiency trade off against specific thrust and TSFC across flight regimes (subsonic through Mach 3) (pp.29-46).

**The single most reusable artifact from this source — a 9-item "over-all specifications" checklist an axial-flow compressor design should be handed, stated explicitly at PDF p.108** (spot-check confirmed verbatim, see §6; note: previously mis-cited as p.109 in an earlier pass of this extraction, corrected here):
1. Design weight flow
2. Design over-all pressure ratio
3. Design equivalent speed
4. Desired level of efficiency
5. Range of operation for which a high level of efficiency must be obtained
6. Inlet and outlet diameters
7. Maximum velocity of air at compressor outlet
8. Desired length and weight
9. Some idea of velocity distortions likely to be encountered at inlet

The text immediately notes: "In the process of design, adjustments in some of these initial specifications are necessary when they are not completely compatible" (p.108) — i.e., this list is explicitly a *starting negotiable spec*, not a rigid contract, which is a useful framing for how Axioma should treat auto-generated subsystem requirements from Mode B (candidate for tagging as `source: ai-generated`, subject to the same negotiation/review pattern already required by FR-CEM-04/07).

Also from Ch. II: an explicit **compressor performance map** (pressure ratio vs. equivalent weight flow `w√θ/δ`, parametrized by equivalent speed `N/√θ`, with a stall/surge limit line) is presented as *the* standard way to represent off-design requirements across the full operating envelope (starting, acceleration, stall margin, flight-Mach-number excursions, Reynolds-number effects at altitude, inlet distortion) (pp.61-68) — this is a strong candidate for the canonical parametric representation of Fan/Core-Compressor subsystem performance in Axioma's own model, both as a requirements artifact and as Mode B's optimization-metric surface.

### 3.2 Compressor design system / process (Ch. III, PDF pp.71-95)
States the design problem as a **quasi-3D decomposition**: a blade-to-blade (circumferential) plane analysis (cascade/blade-element theory: empirically-correlated turning and loss vs. geometry, Mach number, diffusion factor, radial/axial position) combined with a hub-to-casing (meridional) plane analysis (continuity, energy, radial-equilibrium equations under an axisymmetry assumption) (pp.88-89). Three explicit design phases: (1) design-point meridional-plane solution (velocity diagrams satisfying continuity/energy/equilibrium), (2) blade selection (from empirical 2D-cascade/annular-cascade data — incidence angle, deviation angle, total-pressure-loss coefficient as the adopted parameter set), (3) off-design performance prediction (explicitly noted as only rigorously valid near the design point; simpler stage-stacking used in practice, detailed in Ch. X) (pp.90-95).

**Explicit process-philosophy statement, directly relevant to how Axioma should frame Mode B rather than a scripted pipeline:** "this chapter does not attempt to outline a complete systematic step-by-step design procedure... the actual sequence... is left to the individual designer" (p.90) — the source models compressor design as a **system of interrelated constraints to be satisfied together**, not a fixed sequential process. This is a 60-year-old, independently-arrived-at argument for exactly the kind of optimizer-driven (not scripted-pipeline) approach Mode B already commits to.

### 3.3 Gas-generator / engine-matching methodology (Ch. XVII, PDF pp.487-513) — most directly reusable for a 0D/1D Mode B model
Covers exactly the architecture space relevant to a turbofan-with-turbojet-heritage pilot: **simple turbojet, afterburning turbojet, turboprop with coupled power turbine, turboprop with free-wheeling power turbine**, all built around a common one- or two-spool **gas generator** (compressor+combustor+turbine core) — this maps directly onto Axioma's Core-Compressor+Combustor+HP-Turbine as the "gas generator," matched against Fan/LP-Turbine/nozzle as the "other components," exactly the pattern Ch. XVII formalizes.

**Station-numbering convention** (two-spool: 0=ambient → 1=outer-compressor inlet → 2=outer-compressor exit/inner-compressor inlet → 3=inner-compressor exit/combustor inlet → 4=combustor exit/inner-turbine inlet → 5=inner-turbine exit/outer-turbine inlet → 6=outer-turbine exit → 7=nozzle inlet → 8=nozzle exit) is a ready-made interface-numbering scheme Axioma could adopt for 0D model boundaries between its 5 subsystems (p.489).

**Core matching method — superposition of component performance maps:** compressor map and turbine map (both plotted vs. equivalent weight flow, parametrized by equivalent speed) are overlaid/offset against each other to simultaneously satisfy **continuity** (mass-flow consistency, accounting for bleed fraction and fuel-air ratio), **power balance** (turbine work = compressor work + accessory power, per spool), and **rotational-speed compatibility** (shared shaft speed) (pp.489-494). Both a **direct method** and an explicit numbered **iterative method** (8 steps) are given for two-spool matching (pp.494-498), and a full numbered matching procedure is given for each of the four engine architectures against their respective "other components" (inlet, tailpipe/nozzle, afterburner, or free power turbine) (pp.499-503). Once matched, output equations give jet thrust as a function of nozzle pressure ratio (complete-expansion and choked-nozzle cases), fuel flow = airflow × fuel-air ratio, and TSFC = fuel flow / thrust (p.502-503).

**Most directly portable content for a fast 0D/1D surrogate — "Simplified Methods for Equilibrium Operation" (pp.503-505):** explicitly built as a reduced-order alternative to full map-superposition, using two stated simplifying assumptions with explicit domain-of-validity caveats: (a) turbine-inlet equivalent weight flow is constant across operating points, valid because turbine nozzles choke above pressure ratio ≈2.0-2.5; (b) turbine efficiency follows a simple algebraic function of `N/√ΔH` — stated as "quite good over much of the turbine map... near limiting loading, actual efficiencies are much lower" (p.503, explicit caveat). Yields closed-form-ish sequential calculation procedures (not full map iteration) for both one- and two-spool gas generators. **This is the single best candidate in the whole corpus for a first-cut Mode B 0D gas-generator surrogate**, precisely because its assumptions and validity limits are stated explicitly rather than buried in a chart.

A transient/acceleration matching extension (excess torque, moment of inertia per spool, time-marching procedure, p.512-513) is available if Mode B ever needs dynamic (not just steady-state) engine matching.

### 3.4 Nomenclature (consolidated, multiple chapters)
Key nondimensional parameters used throughout and worth adopting into Axioma's own subsystem parameter schema: `η` (efficiency), `δ = P/P_std` and `θ = T/T_std` (standard-day normalization), diffusion factor `D` (stage/blade loading limit, ≲0.4 typical), solidity `σ`, incidence `i` / deviation `δ°` / turning `Δβ = φ+i−δ°`, relative Mach number (design limit ≈1.2 routine, up to 1.35 demonstrated), equivalent weight flow `w√θ/δ` and equivalent speed `N/√θ` (the standard compressor-map axes), bleed fraction `B`, fuel-air ratio `f`. **Caveat:** Greek-letter glyphs are frequently dropped by the OCR in symbol-definition tables (blank where the glyph should be) — cross-check any symbol table pulled from this PDF's text layer against the source images before treating it as authoritative.

---

## 4. Cross-cutting synthesis — how this maps onto Axioma's actual requirements

| Axioma requirement / component | Literature grounding |
|---|---|
| FR-CEM-02 (Architecture Synthesis, Mode B) — "generate/optimize allocation of Blocks and interfaces... via 0D/1D performance and mass-budget models" | Bussemaker §2.1-2.4 (DSG/ADSG modeling + hierarchical BO) is a directly transferable methodology; his jet-engine case study (§2.5) is close to a worked example of exactly this requirement for a turbofan |
| Pilot's 5-subsystem decomposition | Tan et al. gives a 3-subsystem alternative decomposition (§1) worth reconciling explicitly rather than silently diverging from; NASA SP-36 Ch. XVII's station-numbering (§3.3) gives ready-made subsystem-interface conventions |
| Fan & LP Compression / Core (HP) Compressor subsystem requirements | NASA SP-36's 9-item spec checklist (§3.1) and compressor-map representation are direct templates |
| `cem-core`'s "never an LLM decides" constraint | Independently arrived at by Bussemaker (§2.7) — AI scoped to authoring assistance only, never the search/decision step |
| `cem-connectors` / scheduler architecture | Bussemaker Ch.4's collaborative-MDAO findings, especially the explicit p.188 caveat that a locally-integrated, on-demand approach (no formal CDS) is the proportionate choice absent real cross-org constraints (§2.6) — argues for a simpler first cut than full dynamic-workflow machinery |
| Mode B 0D/1D gas-generator model | NASA SP-36 Ch. XVII's "Simplified Methods for Equilibrium Operation" (§3.3) is the best candidate starting point, though its equations need to be sourced from a non-OCR'd reference (see §5) before implementation |
| FR-CEM-04/07 (provenance, review gates) applied to AI-generated requirements | NASA SP-36's own framing of its 9-item spec list as negotiable/adjustable (§3.1) is a good precedent for how Mode B-generated subsystem requirements should be presented for review, not as fixed contracts |

---

## 5. Extractability assessment (mining this corpus programmatically)

- **Bussemaker thesis**: fully text-extractable, no OCR issues. Figures/diagrams render as scattered node/edge-label fragments in text extraction (not structured), but the accompanying body text is self-contained enough that figure content was recoverable from prose in every case checked.
- **Tan et al. paper**: fully text-extractable, no issues (modern PDF).
- **NASA SP-36**: prose, process descriptions, and numbered procedures extract cleanly. **Displayed equations are largely unusable from the text layer** — the OCR (Acrobat Capture 3.0, ~2007-era) typically preserves the equation number and surrounding referential text but drops or garbles the equation body itself; observed consistently across Ch. II, III, and essentially all of Ch. XVII's numbered matching equations (422-479). **Figures/charts (compressor performance maps, matching-procedure charts, diffusion-factor correlations) are pure raster/vector content and are not text-extractable at all** — recovering numeric data from them would require manual/image-level digitization, not attempted in this pass. **Practical implication:** this source is safe to cite for methodology, requirements structure, and process logic (as done throughout §3 above); do not encode its equations or chart data into an actual Mode B parametric model without independently sourcing/deriving the formulas (e.g., from a modern textbook or from careful manual reading of the page images) and cross-checking against this text's qualitative description.

---

## 6. Verification notes

Two spot-checks were run against the source PDFs before this doc was written:
1. **Bussemaker p.195** (SysML v2 mapping quote) — confirmed **exact, verbatim match** against source text.
2. **NASA SP-36 "9-item over-all specifications" list** — confirmed content **accurate and near-verbatim**, but the original extraction pass mis-cited it as PDF p.109; verified actual location is **PDF p.108**. Corrected throughout this document. Given this one confirmed off-by-one, treat all other NASA SP-36 page citations in this document as accurate to within ±1 page unless independently re-verified.

No claims from either source were found to be fabricated or unsupported in the spot-checks performed; the one error found was a citation-location slip, not a content error.

---

## Suggested next steps (not yet done)

1. **Reconcile the subsystem-decomposition difference** between Axioma's 5-subsystem pilot split and Tan et al.'s 3-subsystem split (§1) — explicit decision, not silent divergence.
2. **Draft Fan & LP Compression / Core (HP) Compressor subsystem requirements**, seeded from NASA SP-36's 9-item checklist + compressor-map representation (§3.1).
3. **Draft a Mode B methodology note** (candidate new amendment doc) proposing how the DSG/ADSG concepts (§2.2) map onto Axioma's Neo4j-backed SysML v2 graph, using Bussemaker's own p.195 bridging proposal as the starting point, scoped down per his p.188 caveat (on-demand/local integration first, not full collaborative-MDAO machinery).
4. **Source real equations** for a first-cut 0D gas-generator surrogate (NASA SP-36 Ch. XVII gives the right *structure* — station numbering, matching logic, simplified equilibrium-operation assumptions — but not usable equation bodies; pyCycle, cited by both Tan et al. and Bussemaker as an open-source gas-turbine cycle tool, is a candidate to investigate directly rather than re-deriving from a 1965 scan).
5. Consider pulling **pyCycle** and **De Smedt (2021)** (both cited in Tan et al., §1) as a follow-up literature/software addition if Mode B needs an actual thermodynamic-cycle solver.
