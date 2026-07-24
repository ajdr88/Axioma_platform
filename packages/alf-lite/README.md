# alf-lite

**Not started.** Planned for P1.4 (impl §4.1/§9.6). A minimal, clean-room, in-house Rust compiler
for a deliberately restricted subset of OMG Alf, compiling to the fUML executed by
`fuml-runtime`. Written solely against the public Alf spec — shares no code with the GPL-v3 Alf
Reference Implementation (ADR-005). Scope grows only when a pilot model actually needs a
construct; see impl §9.6 for the initial subset and the required "precise compile error on
unsupported construct" behavior.
