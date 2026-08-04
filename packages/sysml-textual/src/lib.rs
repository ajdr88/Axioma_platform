//! `sysml-textual` — a deliberately minimal, in-house SysML v2-textual-flavored notation
//! (FR-CORE-02, T-P1.2-01), in the same clean-room "only what's needed" spirit as `alf-lite`
//! (impl §9.6). No grammar/version of the real OMG SysML v2 textual notation is targeted or
//! claimed — this is a small tree-shaped syntax over the existing `Element`/`Contains` model,
//! just enough to represent the reference model and support round-trip editing.
//!
//! Scope, deliberately: `kind`/`name`/`Contains`-nesting only. Not represented in text (v1):
//! `active`/`origin`/rationale/properties, non-`Contains` edges, or element deletion — an
//! element missing from edited text is a validation error, not a delete, matching the rest of
//! the app (no delete path exists anywhere yet).

pub mod diff;
pub mod parser;
pub mod printer;

pub use diff::{diff, GraphOp, TextualError};
pub use parser::{parse, ParseError, ParsedElement, Span};
pub use printer::print_tree;

use sysml_core::NodeKind;

/// The textual keyword for a `NodeKind` — lowercased variant name. Fixed, closed set; shared by
/// the parser (keyword → `NodeKind`) and the printer (`NodeKind` → keyword).
pub(crate) fn kind_keyword(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Element => "element",
        NodeKind::Structure => "structure",
        NodeKind::Requirement => "requirement",
        NodeKind::Port => "port",
        NodeKind::Hazard => "hazard",
        NodeKind::Control => "control",
        NodeKind::Mission => "mission",
        NodeKind::Stakeholder => "stakeholder",
        NodeKind::SimulationRun => "simulationrun",
    }
}

/// Reverse of [`kind_keyword`].
pub(crate) fn kind_from_keyword(word: &str) -> Option<NodeKind> {
    match word {
        "element" => Some(NodeKind::Element),
        "structure" => Some(NodeKind::Structure),
        "requirement" => Some(NodeKind::Requirement),
        "port" => Some(NodeKind::Port),
        "hazard" => Some(NodeKind::Hazard),
        "control" => Some(NodeKind::Control),
        "mission" => Some(NodeKind::Mission),
        "stakeholder" => Some(NodeKind::Stakeholder),
        "simulationrun" => Some(NodeKind::SimulationRun),
        _ => None,
    }
}
