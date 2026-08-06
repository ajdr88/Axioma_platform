//! `alf-lite` — a minimal, in-house, clean-room subset of OMG Alf, compiled to fUML for
//! execution by the `fuml-runtime` JVM sidecar (roadmap: P1.4, FR-CORE-09, ADR-005). Written
//! solely against the public OMG Alf specification; shares no code with the GPL-v3 Alf
//! Reference Implementation.
//!
//! This crate is a compiler front-end only, and a pure-logic package like `sysml-core`/
//! `sysml-textual` — it has no protobuf/gRPC dependency. `apps/api` compiles Alf source via
//! [`parse`], converts the resulting [`ast::Program`] into the wire-format protobuf messages
//! `fuml-runtime` consumes (`apps/api/src/alf_ir.rs`), and calls the sidecar — this crate never
//! touches the wire format itself.
//!
//! **Supported subset (each construct has a golden test in `parser.rs`)**: local variable
//! declaration (`let`), boolean/integer/real/string literals, one-level property access and
//! assignment (`target.feature`), arithmetic/comparison/boolean binary operators plus unary
//! `!`, `if`/`else`, a bare behavior-invocation statement, and `send SignalName(args...)`.
//!
//! **Deliberately excluded** (§9.6: "grown on demand" — nothing in the pilot fixture or test
//! specs needs these yet): loops, collection/sequence expression operators, the standard model
//! library, generics/templates, and the advanced multiplicity/typing rules of Extended
//! conformance. An out-of-subset construct is a precise compile-time error naming it
//! (T-P1.4-03), never a silent partial compile.

pub mod ast;
pub mod error;
mod parser;

pub use error::CompileError;
pub use parser::parse;
