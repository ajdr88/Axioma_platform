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
        // docs/IMPLEMENTATION_KICKOFF.md Phase 1 — a real, compiler-caught spot this crate's own
        // exhaustive `match` exists specifically to catch (see this fn's own doc comment,
        // "fixed, closed set"). Not called out in the Phase 1 plan's own file list; found only
        // by actually building the workspace after the `NodeKind` enum changed.
        NodeKind::Constraint => "constraint",
        NodeKind::Parameter => "parameter",
        NodeKind::InformationElement => "informationelement",
        NodeKind::Interaction => "interaction",
        NodeKind::InteractionFragment => "interactionfragment",
        NodeKind::Collection => "collection",
        NodeKind::CandidateStructureSuggestion => "candidatestructuresuggestion",
        NodeKind::Function => "function",
        NodeKind::SelectionChoice => "selectionchoice",
        NodeKind::ConnectionChoice => "connectionchoice",
        NodeKind::Action => "action",
        NodeKind::Model => "model",
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
        "constraint" => Some(NodeKind::Constraint),
        "parameter" => Some(NodeKind::Parameter),
        "informationelement" => Some(NodeKind::InformationElement),
        "interaction" => Some(NodeKind::Interaction),
        "interactionfragment" => Some(NodeKind::InteractionFragment),
        "collection" => Some(NodeKind::Collection),
        "candidatestructuresuggestion" => Some(NodeKind::CandidateStructureSuggestion),
        "function" => Some(NodeKind::Function),
        "selectionchoice" => Some(NodeKind::SelectionChoice),
        "connectionchoice" => Some(NodeKind::ConnectionChoice),
        "action" => Some(NodeKind::Action),
        "model" => Some(NodeKind::Model),
        _ => None,
    }
}
