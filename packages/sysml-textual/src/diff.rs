//! Diffs a parsed document against the current graph snapshot into a batch of [`GraphOp`]s.
//!
//! Cycle-checking is deliberately *not* done here: this grammar is strictly nested, so a single
//! valid parse can never describe a containment cycle among the elements it lists (an element
//! can't simultaneously contain, and be contained by, another within one tree). The backend's
//! atomic apply endpoint still re-validates with `sysml_core::would_create_containment_cycle`
//! before writing, as defense-in-depth against a non-parser caller sending an op batch directly.

use crate::parser::{ParsedElement, Span};
use std::collections::{HashMap, HashSet};
use sysml_core::{Edge, EdgeKind, Element, ElementId, NodeKind};

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GraphOp {
    Rename {
        id: ElementId,
        name: String,
    },
    /// `parent_id` may be a real element id, or another op's `temp_id` earlier in the same
    /// batch — the applying endpoint resolves temp ids to real ones as it processes the batch
    /// in order.
    Create {
        temp_id: String,
        kind: NodeKind,
        name: String,
        parent_id: Option<ElementId>,
    },
    Reparent {
        id: ElementId,
        new_parent_id: Option<ElementId>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextualError {
    pub message: String,
    pub span: Option<Span>,
}

/// Diffs `parsed` against the current `current_elements`/`current_contains` snapshot. Returns
/// every validation error found (not just the first), so a caller (e.g. the LSP server) can
/// publish all diagnostics for a document at once.
pub fn diff(
    current_elements: &[Element],
    current_contains: &[Edge],
    parsed: &[ParsedElement],
) -> Result<Vec<GraphOp>, Vec<TextualError>> {
    let by_id: HashMap<&str, &Element> = current_elements
        .iter()
        .map(|e| (e.id.as_str(), e))
        .collect();
    let mut parent_of: HashMap<&str, &str> = HashMap::new();
    for edge in current_contains {
        if edge.kind == EdgeKind::Contains {
            parent_of.insert(edge.target.as_str(), edge.source.as_str());
        }
    }

    let mut ops = Vec::new();
    let mut errors = Vec::new();
    let mut seen_ids: HashSet<String> = HashSet::new();
    let mut temp_counter = 0usize;

    for root in parsed {
        walk(
            root,
            None,
            &by_id,
            &parent_of,
            &mut ops,
            &mut errors,
            &mut seen_ids,
            &mut temp_counter,
        );
    }

    for element in current_elements {
        if !seen_ids.contains(element.id.as_str()) {
            errors.push(TextualError {
                message: format!(
                    "element #{} ('{}') is missing from the edited text — deleting elements via \
                     text isn't supported yet; restore it or use the canvas",
                    element.id, element.name
                ),
                span: None,
            });
        }
    }

    if errors.is_empty() {
        Ok(ops)
    } else {
        Err(errors)
    }
}

#[allow(clippy::too_many_arguments)]
fn walk(
    node: &ParsedElement,
    parent_ref: Option<String>,
    by_id: &HashMap<&str, &Element>,
    parent_of: &HashMap<&str, &str>,
    ops: &mut Vec<GraphOp>,
    errors: &mut Vec<TextualError>,
    seen_ids: &mut HashSet<String>,
    temp_counter: &mut usize,
) {
    let this_ref: Option<String> = match &node.anchor_id {
        Some(id) => {
            if !seen_ids.insert(id.clone()) {
                errors.push(TextualError {
                    message: format!(
                        "identity anchor #{id} appears more than once in the document"
                    ),
                    span: Some(node.span),
                });
                return;
            }
            match by_id.get(id.as_str()) {
                Some(existing) => {
                    if existing.kind != node.kind {
                        errors.push(TextualError {
                            message: format!(
                                "element #{id} is a {:?} in the model — changing its kind to \
                                 '{}' via text isn't supported",
                                existing.kind,
                                crate::kind_keyword(node.kind)
                            ),
                            span: Some(node.span),
                        });
                    } else {
                        if existing.name != node.name {
                            ops.push(GraphOp::Rename {
                                id: id.clone(),
                                name: node.name.clone(),
                            });
                        }
                        let current_parent = parent_of.get(id.as_str()).map(|s| s.to_string());
                        if current_parent != parent_ref {
                            ops.push(GraphOp::Reparent {
                                id: id.clone(),
                                new_parent_id: parent_ref.clone(),
                            });
                        }
                    }
                    Some(id.clone())
                }
                None => {
                    errors.push(TextualError {
                        message: format!(
                            "element #{id} doesn't exist in the model (was it removed elsewhere?)"
                        ),
                        span: Some(node.span),
                    });
                    None
                }
            }
        }
        None => {
            let temp_id = format!("__new_{}", *temp_counter);
            *temp_counter += 1;
            ops.push(GraphOp::Create {
                temp_id: temp_id.clone(),
                kind: node.kind,
                name: node.name.clone(),
                parent_id: parent_ref.clone(),
            });
            Some(temp_id)
        }
    };

    for child in &node.children {
        walk(
            child,
            this_ref.clone(),
            by_id,
            parent_of,
            ops,
            errors,
            seen_ids,
            temp_counter,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use sysml_core::Origin;

    fn structure(id: &str, name: &str) -> Element {
        Element {
            id: id.to_string(),
            kind: NodeKind::Structure,
            name: name.to_string(),
            active: true,
            origin: Origin::Human,
        }
    }

    fn contains(parent: &str, child: &str) -> Edge {
        Edge {
            source: parent.to_string(),
            target: child.to_string(),
            kind: EdgeKind::Contains,
            metadata: None,
        }
    }

    #[test]
    fn unchanged_document_diffs_to_no_ops() {
        let elements = vec![structure("Combustor", "Combustor")];
        let parsed = parse("structure Combustor /* #Combustor */ {}").unwrap();
        let ops = diff(&elements, &[], &parsed).unwrap();
        assert!(ops.is_empty());
    }

    #[test]
    fn rename_diffs_to_exactly_one_rename_op() {
        let elements = vec![structure("Combustor", "Combustor")];
        let parsed = parse("structure AnnularCombustor /* #Combustor */ {}").unwrap();
        let ops = diff(&elements, &[], &parsed).unwrap();
        assert_eq!(
            ops,
            vec![GraphOp::Rename {
                id: "Combustor".to_string(),
                name: "AnnularCombustor".to_string(),
            }]
        );
    }

    #[test]
    fn unanchored_element_diffs_to_create() {
        let ops = diff(&[], &[], &parse("structure NewPart {}").unwrap()).unwrap();
        match &ops[0] {
            GraphOp::Create {
                kind,
                name,
                parent_id,
                ..
            } => {
                assert_eq!(*kind, NodeKind::Structure);
                assert_eq!(name, "NewPart");
                assert_eq!(*parent_id, None);
            }
            other => panic!("expected Create, got {other:?}"),
        }
    }

    #[test]
    fn nested_unanchored_element_gets_temp_parent_ref() {
        let src = "structure Engine /* #Engine */ { structure NewPart {} }";
        let elements = vec![structure("Engine", "Engine")];
        let ops = diff(&elements, &[], &parse(src).unwrap()).unwrap();
        match &ops[0] {
            GraphOp::Create { parent_id, .. } => {
                assert_eq!(parent_id.as_deref(), Some("Engine"));
            }
            other => panic!("expected Create, got {other:?}"),
        }
    }

    #[test]
    fn moving_a_node_to_a_new_parent_diffs_to_reparent() {
        let src = "structure Root /* #Root */ { structure Fan /* #Fan */ { structure Blade /* #Blade */ {} } }";
        let elements = vec![
            structure("Fan", "Fan"),
            structure("Blade", "Blade"),
            structure("Root", "Root"),
        ];
        // Blade currently under Root, not Fan.
        let existing_contains = vec![contains("Root", "Blade"), contains("Root", "Fan")];
        let ops = diff(&elements, &existing_contains, &parse(src).unwrap()).unwrap();
        assert!(ops.iter().any(|op| matches!(
            op,
            GraphOp::Reparent { id, new_parent_id } if id == "Blade" && new_parent_id.as_deref() == Some("Fan")
        )));
    }

    #[test]
    fn vanished_anchored_element_is_a_validation_error_not_a_delete() {
        let elements = vec![structure("Combustor", "Combustor")];
        let errors = diff(&elements, &[], &[]).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("missing from the edited text"));
    }

    #[test]
    fn changing_kind_via_text_is_rejected() {
        let elements = vec![structure("Combustor", "Combustor")];
        let parsed = parse("requirement Combustor /* #Combustor */ {}").unwrap();
        let errors = diff(&elements, &[], &parsed).unwrap_err();
        assert!(errors[0].message.contains("changing its kind"));
    }

    #[test]
    fn duplicate_anchor_in_one_document_is_rejected() {
        let elements = vec![structure("Combustor", "Combustor")];
        let src = "structure A /* #Combustor */ {}\nstructure B /* #Combustor */ {}";
        let errors = diff(&elements, &[], &parse(src).unwrap()).unwrap_err();
        assert!(errors
            .iter()
            .any(|e| e.message.contains("appears more than once")));
    }

    #[test]
    fn unknown_anchor_id_is_rejected() {
        let src = "structure Ghost /* #does-not-exist */ {}";
        let errors = diff(&[], &[], &parse(src).unwrap()).unwrap_err();
        assert!(errors[0].message.contains("doesn't exist in the model"));
    }
}
