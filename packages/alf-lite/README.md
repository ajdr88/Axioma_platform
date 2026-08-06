# alf-lite

A minimal, clean-room, in-house Rust compiler for a deliberately restricted subset of OMG Alf,
compiling to the fUML executed by `fuml-runtime` (P1.4, impl §4.1/§9.6, FR-CORE-09). Written
solely against the public Alf spec — shares no code with the GPL-v3 Alf Reference Implementation
(ADR-005).

Pure logic, no protobuf/gRPC dependency — same convention as `sysml-core`/`sysml-textual`.
`apps/api` converts this crate's AST into the wire-format protobuf messages `fuml-runtime`
consumes (`apps/api/src/alf_ir.rs`); this crate never touches the wire format.

## Supported subset

Every construct below has a golden test in `src/parser.rs`, per §9.6's own testing requirement:

- Local variable declaration: `let x = expr;`
- Literals: boolean, integer, real, string
- Property access (read) and assignment (write): `Target.feature`, `Target.feature = expr;`
- Binary operators: arithmetic (`+ - * /`), comparison (`< <= > >= == !=`), boolean (`&& ||`),
  unary `!`
- `if (cond) { ... } else { ... }`
- A bare behavior-invocation statement: `Name(args...);`
- `send SignalName(args...);`

## Explicitly out of scope (grown on demand, per §9.6)

Nothing in the pilot's Control-subsystem fixture or the P1.4 test specs needs any of these yet:

- Loops (`while`/`for`)
- Collection/sequence expression operators (`Sequence{...}`, `Set{...}`, etc.)
- The full standard model library
- Generics/templates
- The advanced multiplicity/typing rules of Extended-conformance Alf

The parser recognizes (rather than merely fails to parse) the loop keywords and the
collection-literal keywords, so an out-of-subset construct is a precise compile-time error naming
it (`CompileError.construct`), never a generic syntax error or a silent partial compile
(T-P1.4-03).
