# fuml-runtime

P1.4 (impl §4.1/§4.5). A JVM sidecar wrapping the fUML Reference Implementation (CPL-1.0 +
Apache-2.0, `org.modeldriven:fuml` on Maven Central), driven from the Rust backend (`apps/api`)
over gRPC per ADR-005/ADR-008. Execution only — `alf-lite` compiles to the fUML this service
runs; there is exactly one execution path.

**`Execute`/`ExecuteStateMachine` are both server-streaming RPCs** — each fUML log/fire/output
event is forwarded to the client the instant it's produced (via a `TraceStreamingAppender` hooked
into the RI's own log4j `fuml.Debug` logger, plus a `System.out.print` override for the RI's
direct stdout writes), not collected and sent as one terminal blob. This is what makes
T-P1.4-01's "streams incrementally, deterministically" literally true rather than just claimed.

**Determinism note**: 100 identical `Execute` calls produce an identical *sequence* of
`(kind, activityName, actionName)` trace events — verified directly. The `detail` field on a small
number of `"log"`-kind events embeds a JVM object-identity hash (e.g. `[destroy] object =
1d837fa3#1420a84d`) that the RI's own debug logging includes and that differs run-to-run by
design (it's a memory-address-derived `Object` identity token, not model state) — this is the
underlying library's behavior, not a bug in this sidecar, and the integration test compares on the
structural tuple rather than raw `detail` text for exactly this reason.

## alf-lite / Control state machine (`ExecuteStateMachine`)

Runs the pilot's Control state machine (Idle → Armed → Running → Shutdown, signals
`arm`/`ignite`/`cutoff`) compiled from `packages/alf-lite`. **fUML has no native
`StateMachine`/`Transition`/`Vertex`/`Region` execution semantics at all** — confirmed directly by
listing the vendored RI jar's contents; fUML per the OMG spec covers Activities/Actions only. So
the state machine is compiled as **one linear, self-driven fUML Activity chain**:
`AcceptEventAction(arm)` → `{compiled action}` → `AcceptEventAction(ignite)` → `{compiled
action}` → `AcceptEventAction(cutoff)` → `{compiled action}`. This is one-shot and forward-only —
it goes through the 4 states once, in the fixed order the pilot fixture describes; nothing in the
test specs needs branching between different next-states. A separate, non-active "driver"
Activity creates + starts the state machine as an active object and sends all 3 signals in
sequence from the same request (mirroring the RI's own proven `createSender` pattern) — this
genuinely exercises the RI's real accept/send machinery, not true external/async multi-actor
signaling.

`AlfCompiledActivityBuilder.java` interprets `alf-lite`'s compiled `CompiledStatement`/
`CompiledExpression` protobuf messages (new messages in `proto/fuml_runtime.proto`, mirroring the
compiled subset directly — no XMI marshalling on either side) into real fUML action
nodes/edges — literals, property access/assignment, arithmetic/comparison/boolean binary
operators, `if`/`else`, a bare invocation, `send`. `if`/`else` compiles to a flat
`DecisionNode`/`MergeNode` pair (the RI's own proven `createSimpleDecision` pattern), **not** a
nested `ConditionalNode`/`Clause` — see that class's doc comment for the real bug a nested
structured node hit.

**Two real, non-obvious bugs found and fixed while getting this to actually execute** (both
confirmed via minimal isolated reproductions, independent of the rest of this codebase):
1. **A `ConditionalNode`'s `Clause` consuming a `ReadStructuralFeatureAction`'s output as a
   `CallBehaviorAction` argument silently never fires** — the clause's decider/test resolution
   just never completes, with no exception anywhere. Reproduced with a *minimal*, non-active,
   non-isActive, no-signal top-level activity — nesting inside a `ConditionalNode`/`Clause` is
   the actual trigger, independent of everything else (isActive, `AcceptEventAction`, multiple
   chained accepts — all individually confirmed fine on their own). Fixed by compiling `if`/`else`
   as a flat `DecisionNode`/`MergeNode` pair instead (see `AlfCompiledActivityBuilder.appendIf`).
2. **Reading a freshly-created object's never-written structural feature silently yields zero
   tokens, not a default value, with no exception** — this starves any downstream consumer of
   that read forever. `Turbine.rpm` must be explicitly initialized via
   `AddStructuralFeatureValueAction` right after `CreateObjectAction`, before anything ever reads
   it (see `StateMachineActivityBuilder.initializeRpm`).

Each real construct's own read (`Turbine.rpm`) is a fresh `ReadExtentAction` every time it's
needed, not a shared/forked token — a pure, idempotent read of a true singleton, so skipping
`ForkNode` plumbing costs nothing but a little efficiency (see
`AlfCompiledActivityBuilder`'s class doc for this and the other deliberate pilot-fixture
simplifications: single fixed `Turbine` object, all numeric literals compile to `Real`, `==`/`!=`
composed from `<`/`>`).

**T-P1.4-04's comparison path** (`use_hand_authored_reference` on `StateMachineRequest`): the
identical golden Armed→Running action, built directly via raw fUML calls in
`StateMachineActivityBuilder.buildHandAuthoredArmedToRunning` rather than through
`AlfCompiledActivityBuilder`'s interpreter. Compared on the final `Turbine.rpm` output value (both
paths end with a shared `ReadStructuralFeatureAction` → `RealFunctions::ToString` →
`StandardOutputChannel::writeLine` step, giving both an identical, directly comparable "output"
trace event) rather than raw internal action names, which legitimately differ between two
independently-built graphs (the compiled path's helpers use a counter-suffixed naming scheme; the
hand-authored build does not).

## Explicitly deferred to later passes (not started)

- Graph/`sysml-core` wiring for Alf source, State Machines, Signals, or Transitions as first-class
  model concepts — `sysml-core` has zero behavioral-modeling concepts today; Alf source is
  supplied directly in the request body this pass, not read from any `Element`/property.
- A general reentrant/branching state-machine dispatcher — only the one linear forward path above
  is built.
- Loops, collection/sequence operators, generics/templates, extended multiplicity/typing in
  `alf-lite`'s compiled subset (see `packages/alf-lite/README.md`).
- T-P1.4-05's trade-study workflow, T-P1.4-06's 1M-element fixture.
- True incremental HTTP-to-browser streaming (`apps/api`'s routes currently collect the gRPC
  stream server-side and return one JSON array — the gRPC leg is genuinely streaming; the HTTP
  leg to a browser caller is a separate, later concern).
- A registered gRPC health-check service (`docker-compose.yml`'s `fuml-runtime` entry has no
  healthcheck yet for this reason).
- True external/asynchronous multi-actor signaling (signals are self-fired by the same request).

## Local build

No Maven/Gradle installed or assumed — `fetch-deps.sh` vendors every jar dependency as a direct
Maven Central download (see its own comments for the exact JDK-11-compatibility and
protoc/protobuf version-matching gotchas this uncovered), and `build.sh` fetches `protoc` +
`protoc-gen-grpc-java` the same way, then `javac`s everything.

```sh
./build.sh   # fetch-deps.sh + codegen + compile
./run.sh     # FUML_RUNTIME_PORT (default 50051)
```

Both `apps/api` ignored integration test suites (`fuml_execute_*`, `alf_state_machine_*`) need
`--test-threads=1` — the sidecar's `TraceStreamingAppender` attaches to a single process-global
log4j logger per call (see that class's own doc comment), so two calls running concurrently
against the same sidecar process cross-contaminate each other's trace (confirmed directly).
