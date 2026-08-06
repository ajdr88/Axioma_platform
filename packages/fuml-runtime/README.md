# fuml-runtime

P1.4 (impl §4.1/§4.5). A JVM sidecar wrapping the fUML Reference Implementation (CPL-1.0 +
Apache-2.0, `org.modeldriven:fuml` on Maven Central), driven from the Rust backend (`apps/api`)
over gRPC per ADR-005/ADR-008. Execution only — `alf-lite` compiles to the fUML this service
runs; there is exactly one execution path.

**Current scope (this pass)**: the gRPC sidecar plumbing itself, verified against the fixed
`HelloWorld2` activity the ADR-005 spike already proved the RI executes correctly. `Execute` is a
server-streaming RPC — each fUML log/fire/output event is forwarded to the client the instant it's
produced (via a `TraceStreamingAppender` hooked into the RI's own log4j `fuml.Debug` logger, plus
a `System.out.print` override for the RI's direct stdout writes), not collected and sent as one
terminal blob. This is what makes T-P1.4-01's "streams incrementally, deterministically" literally
true rather than just claimed.

**Determinism note**: 100 identical `Execute` calls produce an identical *sequence* of
`(kind, activityName, actionName)` trace events — verified directly. The `detail` field on a small
number of `"log"`-kind events embeds a JVM object-identity hash (e.g. `[destroy] object =
1d837fa3#1420a84d`) that the RI's own debug logging includes and that differs run-to-run by
design (it's a memory-address-derived `Object` identity token, not model state) — this is the
underlying library's behavior, not a bug in this sidecar, and the integration test compares on the
structural tuple rather than raw `detail` text for exactly this reason.

**Explicitly deferred to later passes** (not started):
- `alf-lite` (in-house Alf-subset compiler → fUML) — there is no model/XMI transfer yet;
  `activity_name` selects among a small fixed set the sidecar constructs in-process via the RI's
  own Java model-builder API.
- The pilot's real Control-subsystem (FADEC/EEC) State Machine.
- T-P1.4-05's trade-study workflow, T-P1.4-06's 1M-element fixture.
- True incremental HTTP-to-browser streaming (`apps/api`'s route currently collects the gRPC
  stream server-side and returns one JSON array — the gRPC leg is genuinely streaming; the HTTP
  leg to a browser caller is a separate, later concern).
- A registered gRPC health-check service (`docker-compose.yml`'s `fuml-runtime` entry has no
  healthcheck yet for this reason).

## Local build

No Maven/Gradle installed or assumed — `fetch-deps.sh` vendors every jar dependency as a direct
Maven Central download (see its own comments for the exact JDK-11-compatibility and
protoc/protobuf version-matching gotchas this uncovered), and `build.sh` fetches `protoc` +
`protoc-gen-grpc-java` the same way, then `javac`s everything.

```sh
./build.sh   # fetch-deps.sh + codegen + compile
./run.sh     # FUML_RUNTIME_PORT (default 50051)
```
