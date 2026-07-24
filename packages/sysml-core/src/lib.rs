//! `sysml-core` — SysML v2 / KerML element and relationship types, plus the
//! server-authoritative semantic-validation layer (FR-CORE-05).
//!
//! This is an early seed of the meta-model described in
//! `docs/Axioma_implementation_v3.md` §2.3 and `docs/Axioma_requirements_v3.md` §2.1 — the node
//! and edge kinds are modeled, and one real rule (containment acyclicity, NFR-REL-02) is
//! implemented end-to-end as a pattern for the rest of the validation rule set.

use std::collections::{HashMap, HashSet};

pub type ElementId = String;

/// Node labels from the data model (impl §2.3 / reqs §2.1). `Element` is the common base;
/// every other kind carries all of `Element`'s fields plus its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum NodeKind {
    Element,
    Structure,
    Requirement,
    Port,
    Hazard,
    Control,
    Mission,
    Stakeholder,
    SimulationRun,
}

/// Edge kinds from the data model. The graph is a directed property graph, not a DAG
/// (NFR-REL-02) — only `Contains` is required to stay acyclic; the rest legitimately form
/// cycles (traceability, feedback, `Suspect` propagation) and must never be treated as acyclic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum EdgeKind {
    Contains,
    Satisfy,
    Verify,
    Refine,
    Causes,
    MitigatedBy,
    ValidatedBy,
    Suspect,
}

impl EdgeKind {
    /// Only `Contains` is subject to the acyclicity rule (NFR-REL-02).
    pub fn is_acyclicity_scoped(&self) -> bool {
        matches!(self, EdgeKind::Contains)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Element {
    pub id: ElementId,
    pub kind: NodeKind,
    pub name: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Edge {
    pub source: ElementId,
    pub target: ElementId,
    pub kind: EdgeKind,
}

/// A single semantic-validation failure. The real layer (impl §4.2) also checks type-legal
/// relationship endpoints, parametric consistency, and dangling-edge rejection — this seed
/// covers containment acyclicity only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    ContainmentCycle { parent: ElementId, child: ElementId },
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationError::ContainmentCycle { parent, child } => write!(
                f,
                "adding containment edge {parent} -> {child} would cycle the containment hierarchy"
            ),
        }
    }
}

impl std::error::Error for ValidationError {}

/// An in-memory model graph. Real persistence is polyglot (Neo4j for topology, Postgres/JSONB
/// for bodies, S3 for blobs, per ADR-003) — this type stands in for the topology store during
/// early development.
#[derive(Debug, Default)]
pub struct Graph {
    elements: HashMap<ElementId, Element>,
    edges: Vec<Edge>,
}

impl Graph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_element(&mut self, element: Element) {
        self.elements.insert(element.id.clone(), element);
    }

    pub fn element(&self, id: &str) -> Option<&Element> {
        self.elements.get(id)
    }

    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.elements.values()
    }

    /// Adds an edge, enforcing containment acyclicity (FR-CORE-05, NFR-REL-02). Non-containment
    /// edges are accepted unconditionally here — cycles across them are legal by design.
    pub fn add_edge(&mut self, edge: Edge) -> Result<(), ValidationError> {
        if edge.kind.is_acyclicity_scoped()
            && self.creates_containment_cycle(&edge.source, &edge.target)
        {
            return Err(ValidationError::ContainmentCycle {
                parent: edge.source,
                child: edge.target,
            });
        }
        self.edges.push(edge);
        Ok(())
    }

    pub fn edges(&self) -> &[Edge] {
        &self.edges
    }

    /// True if adding a `Contains` edge `parent -> child` would cycle the containment hierarchy,
    /// i.e. `child` is already an ancestor of `parent` in the existing containment graph (or
    /// `parent == child`).
    fn creates_containment_cycle(&self, parent: &str, child: &str) -> bool {
        if parent == child {
            return true;
        }

        // BFS forward from `child` along existing Contains edges; if we reach `parent`, `child`
        // already (transitively) contains `parent`, so the new edge would close a loop.
        let mut visited: HashSet<&str> = HashSet::new();
        let mut queue: Vec<&str> = vec![child];
        visited.insert(child);

        while let Some(current) = queue.pop() {
            for edge in &self.edges {
                if edge.kind != EdgeKind::Contains || edge.source != current {
                    continue;
                }
                if edge.target == parent {
                    return true;
                }
                if visited.insert(edge.target.as_str()) {
                    queue.push(edge.target.as_str());
                }
            }
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn structure(id: &str) -> Element {
        Element {
            id: id.to_string(),
            kind: NodeKind::Structure,
            name: id.to_string(),
        }
    }

    fn contains(parent: &str, child: &str) -> Edge {
        Edge {
            source: parent.to_string(),
            target: child.to_string(),
            kind: EdgeKind::Contains,
        }
    }

    /// T-P1.1-03(a): making `Engine` a containment child of its own child `Turbine` must be
    /// rejected.
    #[test]
    fn rejects_containment_cycle() {
        let mut graph = Graph::new();
        graph.add_element(structure("Engine"));
        graph.add_element(structure("Turbine"));

        graph.add_edge(contains("Engine", "Turbine")).unwrap();

        let result = graph.add_edge(contains("Turbine", "Engine"));

        assert_eq!(
            result,
            Err(ValidationError::ContainmentCycle {
                parent: "Turbine".to_string(),
                child: "Engine".to_string(),
            })
        );
    }

    #[test]
    fn rejects_transitive_containment_cycle() {
        let mut graph = Graph::new();
        graph.add_element(structure("Engine"));
        graph.add_element(structure("Turbine"));
        graph.add_element(structure("HpTurbineBlade"));

        graph.add_edge(contains("Engine", "Turbine")).unwrap();
        graph
            .add_edge(contains("Turbine", "HpTurbineBlade"))
            .unwrap();

        let result = graph.add_edge(contains("HpTurbineBlade", "Engine"));

        assert!(matches!(
            result,
            Err(ValidationError::ContainmentCycle { .. })
        ));
    }

    #[test]
    fn rejects_self_containment() {
        let mut graph = Graph::new();
        graph.add_element(structure("Turbine"));

        let result = graph.add_edge(contains("Turbine", "Turbine"));

        assert!(result.is_err());
    }

    /// T-P1.1-03(b): non-containment cycles (e.g. Satisfy/Refine across the same pair) are
    /// legal — traceability is expected to be cyclic (NFR-REL-02).
    #[test]
    fn allows_non_containment_cycles() {
        let mut graph = Graph::new();
        graph.add_element(Element {
            id: "REQ-THRUST".to_string(),
            kind: NodeKind::Requirement,
            name: "Thrust requirement".to_string(),
        });
        graph.add_element(structure("Turbine"));

        graph
            .add_edge(Edge {
                source: "REQ-THRUST".to_string(),
                target: "Turbine".to_string(),
                kind: EdgeKind::Satisfy,
            })
            .unwrap();

        let result = graph.add_edge(Edge {
            source: "Turbine".to_string(),
            target: "REQ-THRUST".to_string(),
            kind: EdgeKind::Refine,
        });

        assert!(result.is_ok());
    }

    #[test]
    fn accepts_valid_containment() {
        let mut graph = Graph::new();
        graph.add_element(structure("Engine"));
        graph.add_element(structure("Turbine"));

        let result = graph.add_edge(contains("Engine", "Turbine"));

        assert!(result.is_ok());
        assert_eq!(graph.edges().len(), 1);
    }
}
