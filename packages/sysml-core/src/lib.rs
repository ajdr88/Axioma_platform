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

/// Provenance origin (FR-CORE-08, impl §6.3) — who/what created an element. Scaffolding only:
/// `validation`/`suspect` (the other two of §6.3's three orthogonal signals) stay hardcoded
/// placeholders on the frontend until something real produces them (solver/test runs for
/// validation, staleness propagation for suspect) — inventing settable-but-untriggered fields for
/// those now would be building ahead of the spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Origin {
    Human,
    AiSuggested,
    AiAutoMerged,
}

impl Origin {
    /// String form stored as a Neo4j node property. Mirrors [`NodeKind::as_label`]'s pattern —
    /// a fixed, closed set of variants, safe to bind as a plain Cypher parameter.
    pub fn as_str(&self) -> &'static str {
        match self {
            Origin::Human => "Human",
            Origin::AiSuggested => "AiSuggested",
            Origin::AiAutoMerged => "AiAutoMerged",
        }
    }

    /// Reverse of [`Origin::as_str`]. Defaults to `Human` for anything unrecognized (including
    /// data written before this property existed) rather than failing the read — same
    /// backward-compat stance `default_origin` takes on deserialize.
    pub fn from_str_or_default(s: &str) -> Origin {
        match s {
            "AiSuggested" => Origin::AiSuggested,
            "AiAutoMerged" => Origin::AiAutoMerged,
            _ => Origin::Human,
        }
    }
}

impl NodeKind {
    /// The Neo4j node label for this kind. Safe to interpolate directly into a Cypher query
    /// string — this is a fixed, closed set of variants, never user-supplied text.
    pub fn as_label(&self) -> &'static str {
        match self {
            NodeKind::Element => "Element",
            NodeKind::Structure => "Structure",
            NodeKind::Requirement => "Requirement",
            NodeKind::Port => "Port",
            NodeKind::Hazard => "Hazard",
            NodeKind::Control => "Control",
            NodeKind::Mission => "Mission",
            NodeKind::Stakeholder => "Stakeholder",
            NodeKind::SimulationRun => "SimulationRun",
        }
    }

    /// Reverse of [`NodeKind::as_label`] — reconstructs a `NodeKind` from a Neo4j label string.
    pub fn from_label(label: &str) -> Option<NodeKind> {
        match label {
            "Element" => Some(NodeKind::Element),
            "Structure" => Some(NodeKind::Structure),
            "Requirement" => Some(NodeKind::Requirement),
            "Port" => Some(NodeKind::Port),
            "Hazard" => Some(NodeKind::Hazard),
            "Control" => Some(NodeKind::Control),
            "Mission" => Some(NodeKind::Mission),
            "Stakeholder" => Some(NodeKind::Stakeholder),
            "SimulationRun" => Some(NodeKind::SimulationRun),
            _ => None,
        }
    }
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
    /// A `Stakeholder`'s link to a `Mission` or `Requirement` it owns (FR-MSN-02).
    Concerns,
}

impl EdgeKind {
    /// Only `Contains` is subject to the acyclicity rule (NFR-REL-02).
    pub fn is_acyclicity_scoped(&self) -> bool {
        matches!(self, EdgeKind::Contains)
    }

    /// The Cypher relationship type for this edge kind. Safe to interpolate directly into a
    /// query string — this is a fixed, closed set of variants, never user-supplied text.
    pub fn as_rel_type(&self) -> &'static str {
        match self {
            EdgeKind::Contains => "CONTAINS",
            EdgeKind::Satisfy => "SATISFY",
            EdgeKind::Verify => "VERIFY",
            EdgeKind::Refine => "REFINE",
            EdgeKind::Causes => "CAUSES",
            EdgeKind::MitigatedBy => "MITIGATED_BY",
            EdgeKind::ValidatedBy => "VALIDATED_BY",
            EdgeKind::Suspect => "SUSPECT",
            EdgeKind::Concerns => "CONCERNS",
        }
    }

    /// Reverse of [`EdgeKind::as_rel_type`] — reconstructs an `EdgeKind` from a Neo4j relationship
    /// type string (e.g. `type(r)` in a Cypher `RETURN`).
    pub fn from_rel_type(rel_type: &str) -> Option<EdgeKind> {
        match rel_type {
            "CONTAINS" => Some(EdgeKind::Contains),
            "SATISFY" => Some(EdgeKind::Satisfy),
            "VERIFY" => Some(EdgeKind::Verify),
            "REFINE" => Some(EdgeKind::Refine),
            "CAUSES" => Some(EdgeKind::Causes),
            "MITIGATED_BY" => Some(EdgeKind::MitigatedBy),
            "VALIDATED_BY" => Some(EdgeKind::ValidatedBy),
            "SUSPECT" => Some(EdgeKind::Suspect),
            "CONCERNS" => Some(EdgeKind::Concerns),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Element {
    pub id: ElementId,
    pub kind: NodeKind,
    pub name: String,
    /// Whether this element is considered in system-optimization loops (Mode B / `cem-core` —
    /// Product 2, not built yet). Deactivating an element keeps all its data; it's a visual/
    /// modeling marker today, with nothing yet to actually filter by. Defaults to `true` on
    /// deserialize so data written before this field existed still parses.
    #[serde(default = "default_active")]
    pub active: bool,
    /// FR-CORE-08 provenance origin. Defaults to `Human` on deserialize so data written before
    /// this field existed still parses — same backward-compat pattern as `active`.
    #[serde(default = "default_origin")]
    pub origin: Origin,
}

fn default_active() -> bool {
    true
}

fn default_origin() -> Origin {
    Origin::Human
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Edge {
    pub source: ElementId,
    pub target: ElementId,
    pub kind: EdgeKind,
}

/// A single semantic-validation failure. The real layer (impl §4.2) also checks parametric
/// consistency — no parametric model exists yet, so that rule has nothing to validate against
/// and isn't represented here yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    ContainmentCycle {
        parent: ElementId,
        child: ElementId,
    },
    /// An incoming element reuses the id of an existing element under a *different* `NodeKind` —
    /// a type-legal-identity violation (FR-CORE-05), same rule family as endpoint type-legality.
    KindConflict {
        id: ElementId,
        existing: NodeKind,
        incoming: NodeKind,
    },
    /// An edge's source/target kinds violate the relationship's type-legality rule (FR-CORE-05),
    /// e.g. a `Satisfy` edge whose target isn't a `Requirement`. See
    /// [`check_relationship_endpoints`].
    IllegalEndpoint {
        kind: EdgeKind,
        source: ElementId,
        source_kind: NodeKind,
        target: ElementId,
        target_kind: NodeKind,
    },
    /// An edge references an id that doesn't exist as an element (NFR-REL-01: "no edge
    /// references a concurrently-deleted node").
    DanglingEdge {
        edge_kind: EdgeKind,
        missing_id: ElementId,
    },
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationError::ContainmentCycle { parent, child } => write!(
                f,
                "adding containment edge {parent} -> {child} would cycle the containment hierarchy"
            ),
            ValidationError::KindConflict {
                id,
                existing,
                incoming,
            } => write!(
                f,
                "element {id} already exists as {existing:?}, cannot import as {incoming:?}"
            ),
            ValidationError::IllegalEndpoint {
                kind,
                source,
                source_kind,
                target,
                target_kind,
            } => write!(
                f,
                "{kind:?} edge {source} ({source_kind:?}) -> {target} ({target_kind:?}) violates the relationship's type-legality rule"
            ),
            ValidationError::DanglingEdge {
                edge_kind,
                missing_id,
            } => write!(
                f,
                "cannot create {edge_kind:?} edge: element {missing_id} does not exist"
            ),
        }
    }
}

impl std::error::Error for ValidationError {}

/// The Postgres/JSONB side of the polyglot split (NFR-DATA-02): element bodies, long text, and
/// large metadata. Never held in the graph (topology store) — see `GeometryPointer` for the
/// object-store equivalent.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ElementBody {
    pub element_id: ElementId,
    pub rationale: Option<String>,
    pub properties: serde_json::Value,
}

/// A pointer to a blob (geometry, mesh, solver result file) in S3-compatible object storage.
/// Only the pointer ever crosses into the graph or a body — raw bytes never do (NFR-DATA-02).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GeometryPointer {
    pub element_id: ElementId,
    pub object_key: String,
}

/// True if adding a `Contains` edge `parent -> child` would cycle the containment hierarchy,
/// i.e. `child` is already an ancestor of `parent` in `existing` (or `parent == child`). Pure
/// function over an edge slice so callers backed by a real store (e.g. Neo4j) can validate
/// without hydrating a full in-memory `Graph` — they only need to fetch the existing `Contains`
/// edges.
pub fn would_create_containment_cycle(existing: &[Edge], parent: &str, child: &str) -> bool {
    if parent == child {
        return true;
    }

    // BFS forward from `child` along existing Contains edges; if we reach `parent`, `child`
    // already (transitively) contains `parent`, so the new edge would close a loop.
    let mut visited: HashSet<&str> = HashSet::new();
    let mut queue: Vec<&str> = vec![child];
    visited.insert(child);

    while let Some(current) = queue.pop() {
        for edge in existing {
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

/// Rejects re-importing an element id under a different `NodeKind` than it already has.
/// `existing_kind` is the id's current kind, if it exists at all — pass `None` for a new id.
pub fn check_kind_conflict(
    existing_kind: Option<NodeKind>,
    element: &Element,
) -> Result<(), ValidationError> {
    match existing_kind {
        Some(existing) if existing != element.kind => Err(ValidationError::KindConflict {
            id: element.id.clone(),
            existing,
            incoming: element.kind,
        }),
        _ => Ok(()),
    }
}

/// Rejects an edge whose source/target `NodeKind`s violate the relationship's type-legality rule
/// (FR-CORE-05). Deliberately partial: only rules the docs concretely pin down are encoded —
/// `Satisfy` ("a Satisfy targets a Requirement, not a Block" — requirements.md §5.1, T-P1.1-02)
/// and `Concerns` (a Stakeholder's link to a Mission/Requirement it owns — FR-MSN-02). Every other
/// `EdgeKind` is accepted between any two `NodeKind`s for now; extend the match arm as more rules
/// are specified rather than guessing ahead of the spec.
pub fn check_relationship_endpoints(
    kind: EdgeKind,
    source: &str,
    source_kind: NodeKind,
    target: &str,
    target_kind: NodeKind,
) -> Result<(), ValidationError> {
    let legal = match kind {
        EdgeKind::Satisfy => target_kind == NodeKind::Requirement,
        EdgeKind::Concerns => {
            source_kind == NodeKind::Stakeholder
                && matches!(target_kind, NodeKind::Mission | NodeKind::Requirement)
        }
        _ => true,
    };
    if legal {
        Ok(())
    } else {
        Err(ValidationError::IllegalEndpoint {
            kind,
            source: source.to_string(),
            source_kind,
            target: target.to_string(),
            target_kind,
        })
    }
}

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

    /// Adds an edge, enforcing containment acyclicity (FR-CORE-05, NFR-REL-02) and relationship
    /// endpoint type-legality (see [`check_relationship_endpoints`]). Non-containment edges are
    /// accepted unconditionally with respect to acyclicity — cycles across them are legal by
    /// design.
    pub fn add_edge(&mut self, edge: Edge) -> Result<(), ValidationError> {
        if edge.kind.is_acyclicity_scoped()
            && would_create_containment_cycle(&self.edges, &edge.source, &edge.target)
        {
            return Err(ValidationError::ContainmentCycle {
                parent: edge.source,
                child: edge.target,
            });
        }
        if let (Some(source_el), Some(target_el)) = (
            self.elements.get(&edge.source),
            self.elements.get(&edge.target),
        ) {
            check_relationship_endpoints(
                edge.kind,
                &edge.source,
                source_el.kind,
                &edge.target,
                target_el.kind,
            )?;
        }
        self.edges.push(edge);
        Ok(())
    }

    pub fn edges(&self) -> &[Edge] {
        &self.edges
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
            active: true,
            origin: Origin::Human,
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
    /// legal — traceability is expected to be cyclic (NFR-REL-02). Satisfy direction is
    /// Structure->Requirement ("a Satisfy targets a Requirement, not a Block" — requirements.md
    /// §5.1, T-P1.1-02); Refine stays kind-unconstrained for now.
    #[test]
    fn allows_non_containment_cycles() {
        let mut graph = Graph::new();
        graph.add_element(Element {
            id: "REQ-THRUST".to_string(),
            kind: NodeKind::Requirement,
            name: "Thrust requirement".to_string(),
            active: true,
            origin: Origin::Human,
        });
        graph.add_element(structure("Turbine"));

        graph
            .add_edge(Edge {
                source: "Turbine".to_string(),
                target: "REQ-THRUST".to_string(),
                kind: EdgeKind::Satisfy,
            })
            .unwrap();

        let result = graph.add_edge(Edge {
            source: "REQ-THRUST".to_string(),
            target: "Turbine".to_string(),
            kind: EdgeKind::Refine,
        });

        assert!(result.is_ok());
    }

    /// T-P1.1-02: Satisfy must target a Requirement, not a Block — a Block satisfying another
    /// Block (e.g. Combustor -> Turbine) is rejected.
    #[test]
    fn rejects_illegal_satisfy_endpoint() {
        let mut graph = Graph::new();
        graph.add_element(structure("Combustor"));
        graph.add_element(structure("Turbine"));

        let result = graph.add_edge(Edge {
            source: "Combustor".to_string(),
            target: "Turbine".to_string(),
            kind: EdgeKind::Satisfy,
        });

        assert_eq!(
            result,
            Err(ValidationError::IllegalEndpoint {
                kind: EdgeKind::Satisfy,
                source: "Combustor".to_string(),
                source_kind: NodeKind::Structure,
                target: "Turbine".to_string(),
                target_kind: NodeKind::Structure,
            })
        );
    }

    #[test]
    fn accepts_legal_satisfy_endpoint() {
        let mut graph = Graph::new();
        graph.add_element(structure("Turbine"));
        graph.add_element(Element {
            id: "REQ-THRUST".to_string(),
            kind: NodeKind::Requirement,
            name: "Thrust requirement".to_string(),
            active: true,
            origin: Origin::Human,
        });

        let result = graph.add_edge(Edge {
            source: "Turbine".to_string(),
            target: "REQ-THRUST".to_string(),
            kind: EdgeKind::Satisfy,
        });

        assert!(result.is_ok());
    }

    /// Causes/MitigatedBy (and every other kind besides Satisfy) are kind-unconstrained for
    /// now — the docs don't pin down an endpoint rule for them yet.
    #[test]
    fn accepts_causes_and_mitigated_by_between_any_kinds() {
        assert!(check_relationship_endpoints(
            EdgeKind::Causes,
            "Turbine",
            NodeKind::Structure,
            "HAZ-OVERSPEED",
            NodeKind::Hazard,
        )
        .is_ok());
        assert!(check_relationship_endpoints(
            EdgeKind::MitigatedBy,
            "HAZ-OVERSPEED",
            NodeKind::Hazard,
            "CTRL-CUTOFF",
            NodeKind::Control,
        )
        .is_ok());
    }

    /// FR-MSN-02: a Stakeholder's `Concerns` link may target a Mission or a Requirement.
    #[test]
    fn accepts_legal_concerns_endpoints() {
        assert!(check_relationship_endpoints(
            EdgeKind::Concerns,
            "Chief-Engineer",
            NodeKind::Stakeholder,
            "MSN-CLIMB",
            NodeKind::Mission,
        )
        .is_ok());
        assert!(check_relationship_endpoints(
            EdgeKind::Concerns,
            "Chief-Engineer",
            NodeKind::Stakeholder,
            "REQ-THRUST",
            NodeKind::Requirement,
        )
        .is_ok());
    }

    #[test]
    fn rejects_illegal_concerns_endpoints() {
        // Wrong source kind.
        assert_eq!(
            check_relationship_endpoints(
                EdgeKind::Concerns,
                "Turbine",
                NodeKind::Structure,
                "MSN-CLIMB",
                NodeKind::Mission,
            ),
            Err(ValidationError::IllegalEndpoint {
                kind: EdgeKind::Concerns,
                source: "Turbine".to_string(),
                source_kind: NodeKind::Structure,
                target: "MSN-CLIMB".to_string(),
                target_kind: NodeKind::Mission,
            })
        );
        // Wrong target kind.
        assert!(check_relationship_endpoints(
            EdgeKind::Concerns,
            "Chief-Engineer",
            NodeKind::Stakeholder,
            "Turbine",
            NodeKind::Structure,
        )
        .is_err());
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

    /// The free function backing `Graph::add_edge` works directly off an edge slice, so a
    /// caller backed by a real store (Neo4j) can validate without hydrating a full `Graph`.
    #[test]
    fn would_create_containment_cycle_works_without_a_graph() {
        let existing = vec![contains("Engine", "Turbine")];

        assert!(would_create_containment_cycle(
            &existing, "Turbine", "Engine"
        ));
        assert!(!would_create_containment_cycle(
            &existing,
            "Engine",
            "Combustor"
        ));
    }

    #[test]
    fn check_kind_conflict_allows_new_id_and_matching_kind() {
        let el = structure("Turbine");
        assert!(check_kind_conflict(None, &el).is_ok());
        assert!(check_kind_conflict(Some(NodeKind::Structure), &el).is_ok());
    }

    #[test]
    fn check_kind_conflict_rejects_mismatched_kind() {
        let el = Element {
            id: "Turbine".to_string(),
            kind: NodeKind::Requirement,
            name: "Turbine".to_string(),
            active: true,
            origin: Origin::Human,
        };
        let result = check_kind_conflict(Some(NodeKind::Structure), &el);
        assert_eq!(
            result,
            Err(ValidationError::KindConflict {
                id: "Turbine".to_string(),
                existing: NodeKind::Structure,
                incoming: NodeKind::Requirement,
            })
        );
    }

    #[test]
    fn node_kind_label_roundtrips() {
        for kind in [
            NodeKind::Element,
            NodeKind::Structure,
            NodeKind::Requirement,
            NodeKind::Port,
            NodeKind::Hazard,
            NodeKind::Control,
            NodeKind::Mission,
            NodeKind::Stakeholder,
            NodeKind::SimulationRun,
        ] {
            assert_eq!(NodeKind::from_label(kind.as_label()), Some(kind));
        }
        assert_eq!(NodeKind::from_label("NotAKind"), None);
    }

    #[test]
    fn edge_kind_rel_type_roundtrips() {
        for kind in [
            EdgeKind::Contains,
            EdgeKind::Satisfy,
            EdgeKind::Verify,
            EdgeKind::Refine,
            EdgeKind::Causes,
            EdgeKind::MitigatedBy,
            EdgeKind::ValidatedBy,
            EdgeKind::Suspect,
            EdgeKind::Concerns,
        ] {
            assert_eq!(EdgeKind::from_rel_type(kind.as_rel_type()), Some(kind));
        }
        assert_eq!(EdgeKind::from_rel_type("NOT_A_KIND"), None);
    }

    #[test]
    fn element_body_and_geometry_pointer_roundtrip() {
        let body = ElementBody {
            element_id: "REQ-THRUST".to_string(),
            rationale: Some("x".repeat(20_000)),
            properties: serde_json::json!({ "unit": "lbf" }),
        };
        let json = serde_json::to_string(&body).unwrap();
        let parsed: ElementBody = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.element_id, "REQ-THRUST");
        assert_eq!(parsed.rationale.unwrap().len(), 20_000);

        let pointer = GeometryPointer {
            element_id: "TurbineHpLp".to_string(),
            object_key: "turbine/casing.stl".to_string(),
        };
        let json = serde_json::to_string(&pointer).unwrap();
        let parsed: GeometryPointer = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.object_key, "turbine/casing.stl");
    }

    /// FR-CORE-08: an element's origin round-trips through JSON, and data written before the
    /// field existed still parses (defaults to `Human`) — same backward-compat pattern as
    /// `active`.
    #[test]
    fn element_origin_roundtrips_and_defaults() {
        let el = structure("Turbine");
        let json = serde_json::to_string(&el).unwrap();
        let parsed: Element = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.origin, Origin::Human);

        let pre_origin_json =
            r#"{"id":"Turbine","kind":"Structure","name":"Turbine","active":true}"#;
        let parsed: Element = serde_json::from_str(pre_origin_json).unwrap();
        assert_eq!(parsed.origin, Origin::Human);

        let ai_suggested = Element {
            origin: Origin::AiSuggested,
            ..structure("Turbine")
        };
        let json = serde_json::to_string(&ai_suggested).unwrap();
        let parsed: Element = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.origin, Origin::AiSuggested);
    }
}
