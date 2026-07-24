# fuml-runtime

**Not started.** Planned for P1.4 (impl §4.1/§4.5). A JVM sidecar wrapping the fUML Reference
Implementation (CPL-1.0 + Apache-2.0, `org.modeldriven:fuml` on Maven Central), driven from the
Rust backend over gRPC per ADR-005/ADR-008. Execution only — `alf-lite` compiles to the fUML this
service runs; there is exactly one execution path.
