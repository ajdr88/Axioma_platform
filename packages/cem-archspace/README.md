# cem-archspace

**`docs/IMPLEMENTATION_KICKOFF.md` Phase 2 spike — built and verified real, end-to-end.** A Python
gRPC sidecar wrapping [`adsg-core`](https://github.com/jbussemaker/adsg-core) (MIT) and
[`SBArchOpt`](https://github.com/jbussemaker/SBArchOpt) (MIT) for Mode B's architecture
design-space representation (`Axioma_requirements_v5.md` §5.17, FR-ARCH), driven from Rust
(`apps/api/src/archspace_client.rs`) over gRPC — the same sidecar pattern `fuml-runtime` already
established (ADR-005/008), applied to a second, unrelated external-tool boundary rather than
inventing a new integration shape.

Same "driven only via `docker compose`, no pnpm/Cargo workspace membership" convention as
`fuml-runtime`: no `package.json`, not in root `Cargo.toml`'s `members`. Reached purely over gRPC
via `ARCHSPACE_ADDR` (Rust side) / `ARCHSPACE_PORT` (this side, default `50052` — one past
`fuml-runtime`'s `50051`).

## What this spike actually proved (real numbers, not invented)

Every claim below was exercised against the real installed libraries (`adsg-core==1.4.1`,
`sb-arch-opt==1.6.3`, pinned in `requirements.txt`), first directly in a Python REPL, then over
the real gRPC wire against this exact server:

- **All four primitives `docs/IMPLEMENTATION_KICKOFF.md` Phase 2 names round-trip correctly**:
  a selection choice (`BleedOfftakeStage` ∈ 4 stage options), a connection choice (bleed-air
  routing between two connector ports), an incompatibility constraint (mutually excluding one
  nozzle-config option with one gearbox option), and a `LINKED` choice constraint (Core (HP)
  Compressor's `n_HP_stages` tied to Turbine's `n_HP_turbine_stages` — FR-COMP-04, using the exact
  test problem `Axioma_sysml_tool_landscape_evaluation.md`'s own suggested next step named).
- **The `LINKED` constraint does exactly what FR-COMP-04 needs**: `GraphProcessor.des_vars`
  reports **4** design variables for a definition with 2 linked design-variable nodes + 3 selection
  choices — adsg-core collapses the two linked stage-count variables into one shared search axis,
  confirmed directly (`n_des_vars: 4`, not 5) before any proto/gRPC layer was even written.
- **A real Imputation Ratio comes back**: `1.3333...` for this spike's declared vs. valid design
  space (32 declared, 24 valid) — `GetDesignSpaceStats` (FR-ARCH-06's IR half; Correction Ratio,
  Correction Fraction, and Max Rate Diversity are real adsg-core concepts but live behind a
  separate pandas-`DataFrame`-shaped `get_statistics()` call this spike deliberately didn't wire
  into the proto surface — see "Not built" below).
- **`DecodeInstance` returns a real, internally-consistent architecture instance** — a design
  vector, an activeness mask, and the actual present-node names (e.g.
  `['CoreHpCompressor', ..., 'Stage2', 'DirectDrive', 'SeparateNozzle']` for one sampled vector),
  not just "did not error."
- **SBArchOpt genuinely drives adsg-core's evaluation loop, not just a bare `adsg-core` demo**:
  wrapping the spike's `DSGEvaluator` in `adsg_core.optimization.problem.DSGArchOptProblem` and
  running `sb_arch_opt.algo.pymoo_interface.get_nsga2()` for 3 generations (pop size 10) over gRPC
  converged to a best objective of `1.0688` against a placeholder "minimize stage count" objective
  whose true minimum (at the declared `[1, 4]` bounds) is `1.0` — this is ADR-011's *other* half
  (SBArchOpt actually consumes an adsg-core-built problem), not assumed from the two libraries
  merely being compatible on paper.
- **A bogus handle is rejected loudly** (`grpc.StatusCode.NOT_FOUND`), and `adsg-core` rejecting a
  malformed design space (e.g. a node referenced by name that was never declared) surfaces as
  `INVALID_ARGUMENT`, not a silent 500 or a hang — same "reject, don't silently accept" discipline
  `sysml-core`'s own semantic-validation layer already follows.

## A real, non-obvious API finding from building this

`adsg-core`'s actual node vocabulary is `NamedNode`/`ConnectorNode`/`DesignVariableNode`/
`MetricNode` — **there is no literal `FUN`/`COMP`/`MULTI`/`NOF`/`DE`/`CON` class hierarchy** in the
library; that vocabulary is Bussemaker's thesis terminology for graph *topology* (derivation edges,
choices), not distinct Python types. Confirmed by reading `adsg_core/graph/adsg_nodes.py` directly.
This validates — rather than contradicts — `Axioma_requirements_v5.md` §5.17's own design (a
fulfillment-mechanism *tag* on `:Function`↔`:Structure` edges, not a type hierarchy) as the right
call, not a gap this spike needed to fix.

Also non-obvious, confirmed by directly hitting the error while building `dsg_builder.py`: a
selection choice's option nodes (and a connection choice's connectors) must be derived **only**
through the choice/connection call itself. Pre-wiring them with a plain derivation edge *and* also
passing them as choice options makes `GraphProcessor` reject the whole graph as infeasible
("The provided graph is not feasible to begin with!") — not documented anywhere in the public
guide/API-reference pages, found only by actually building something and reading the real error.

## What's explicitly not built here (deferred, not silently dropped)

- **Not wired into Mode B's real `optimize`/`propose` flow** (`apps/api/src/mode_b.rs`) — that's
  P2.1 proper, after ADR-011 is ratified on the strength of this spike. `cem-core` itself is
  untouched by this pass, on purpose: it stays "pure computation, no I/O" (its own README's
  existing claim); the gRPC client lives in `apps/api`, exactly like `fuml_client.rs`.
  `/cem/mode-b/design-space/*` (`Axioma_implementation_v5.md` §1.2a) is not built as a public REST
  surface yet — this spike is proven via a direct Rust integration test, not an HTTP endpoint.
- **No persistence** — every `DesignSpaceHandle` lives only in this process's memory, for its
  lifetime. Matches the kickoff doc's own "in-memory ADSG instance" phrasing for this phase; a real
  P2.1 build needs a decision on whether Axioma stores the ADSG natively in its own graph or keeps
  treating this sidecar's in-memory instance as the working representation with a sync/export
  step — still an open question, not resolved by this spike (see the ADR-011 appendix in
  `Axioma_implementation_v5.md` §10 for the concrete recommendation this spike's findings support).
- **No turbofan instance seeding** (`Axioma_requirements_v5.md` §5.16's full 5-subsystem model) —
  this spike's test problem is deliberately a small, synthetic slice of it (one compressor
  subsystem's stage-count/bleed-offtake choices), per the kickoff doc's own instruction not to
  block this phase on full instance seeding (Phase 4).
- **Correction Ratio / Correction Fraction / Max Rate Diversity** (the other three of FR-ARCH-06's
  four health metrics) — real `adsg-core` capability, not exposed by this pass's `DesignSpaceStats`
  message; needs `GraphProcessor.get_statistics()`'s richer (DataFrame-shaped) output mapped into
  proto fields, deferred rather than rushed into this pass's minimal contract.
- **Multi-objective/constraint metrics** — the proto's `Objective` message and this spike's
  evaluator are single-objective only; `adsg-core`/`SBArchOpt` support more, just not exercised
  here.
- **No auth, no TLS, no health-check RPC** — same gaps `fuml-runtime`'s own `docker-compose.yml`
  entry already has and defers for the same reason (no gRPC health-check service registered yet
  anywhere in this repo).

## Local dev

```bash
uv venv --python 3.12
source .venv/Scripts/activate   # or .venv/bin/activate on Linux/Mac
uv pip install -r requirements.txt
python -m grpc_tools.protoc -I proto --python_out=src --grpc_python_out=src proto/cem_archspace.proto
./run.sh
```

Or via Docker Compose: `docker compose up -d cem-archspace` (port `50052`).
