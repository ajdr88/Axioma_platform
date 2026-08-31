//! Pretty-prints current graph state into the textual grammar. Deterministic (sorted by id at
//! every level) so re-printing an unchanged graph is byte-identical — needed for the diagram→text
//! direction to be a stable no-op when nothing changed, and for tests to assert exact output.

use std::collections::{HashMap, HashSet};
use sysml_core::{Edge, EdgeKind, Element};

pub fn print_tree(elements: &[Element], contains: &[Edge]) -> String {
    let mut children_of: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut has_parent: HashSet<&str> = HashSet::new();
    for edge in contains {
        if edge.kind != EdgeKind::Contains {
            continue;
        }
        children_of
            .entry(edge.source.as_str())
            .or_default()
            .push(edge.target.as_str());
        has_parent.insert(edge.target.as_str());
    }
    for children in children_of.values_mut() {
        children.sort_unstable();
    }

    let by_id: HashMap<&str, &Element> = elements.iter().map(|e| (e.id.as_str(), e)).collect();

    let mut roots: Vec<&str> = elements
        .iter()
        .map(|e| e.id.as_str())
        .filter(|id| !has_parent.contains(id))
        .collect();
    roots.sort_unstable();

    let mut out = String::new();
    for root_id in roots {
        print_element(root_id, &by_id, &children_of, 0, &mut out);
    }
    out
}

fn print_element(
    id: &str,
    by_id: &HashMap<&str, &Element>,
    children_of: &HashMap<&str, Vec<&str>>,
    depth: usize,
    out: &mut String,
) {
    let Some(element) = by_id.get(id) else {
        return;
    };
    let indent = "  ".repeat(depth);
    out.push_str(&indent);
    out.push_str(crate::kind_keyword(element.kind));
    out.push(' ');
    out.push_str(&format_identifier(&element.name));
    out.push_str(" /* #");
    out.push_str(id);
    out.push_str(" */");

    match children_of.get(id) {
        Some(children) if !children.is_empty() => {
            out.push_str(" {\n");
            for child_id in children {
                print_element(child_id, by_id, children_of, depth + 1, out);
            }
            out.push_str(&indent);
            out.push_str("}\n");
        }
        _ => out.push_str(" {}\n"),
    }
}

/// Bare identifier if `name` is composed entirely of characters the parser's bare-identifier
/// path accepts; a quoted, escaped string otherwise (spaces, punctuation, etc.).
fn format_identifier(name: &str) -> String {
    let is_bare = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-');
    if is_bare {
        name.to_string()
    } else {
        let escaped = name.replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"{escaped}\"")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sysml_core::{NodeKind, Origin};

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
    fn prints_flat_element() {
        let elements = vec![structure("Combustor", "Combustor")];
        let text = print_tree(&elements, &[]);
        assert_eq!(text, "structure Combustor /* #Combustor */ {}\n");
    }

    #[test]
    fn prints_nested_children_sorted_and_indented() {
        let elements = vec![
            structure("Engine", "Engine"),
            structure("Fan", "Fan"),
            structure("Core", "Core"),
        ];
        let edges = vec![contains("Engine", "Fan"), contains("Engine", "Core")];
        let text = print_tree(&elements, &edges);
        assert_eq!(
            text,
            "structure Engine /* #Engine */ {\n  structure Core /* #Core */ {}\n  structure Fan /* #Fan */ {}\n}\n"
        );
    }

    #[test]
    fn quotes_a_name_with_spaces() {
        let elements = vec![structure("F1", "Fan & LP Compression")];
        let text = print_tree(&elements, &[]);
        assert_eq!(text, "structure \"Fan & LP Compression\" /* #F1 */ {}\n");
    }

    #[test]
    fn print_then_parse_round_trips() {
        let elements = vec![structure("Engine", "Engine"), structure("Fan", "Fan")];
        let edges = vec![contains("Engine", "Fan")];
        let text = print_tree(&elements, &edges);
        let parsed = crate::parser::parse(&text).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].anchor_id, Some("Engine".to_string()));
        assert_eq!(parsed[0].children[0].anchor_id, Some("Fan".to_string()));
    }
}
