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
    // docs/IMPLEMENTATION_KICKOFF.md Phase 1 — Parametrics (FR-PARAM, reqs v5 §5.9).
    Constraint,
    Parameter,
    // Phase 1 — Information/Data Architecture (FR-INFO, reqs v5 §5.10).
    InformationElement,
    // Phase 1 — Interaction/Timing Modeling (FR-INTX, reqs v5 §5.11). Underlying SysML v2
    // mapping is still pending ADR-009 — these two kinds exist so the rest of the schema/
    // validation layer has something concrete to reference in the meantime.
    Interaction,
    InteractionFragment,
    // Phase 1 — Dynamic Element Collections (FR-CORE-10/11, reqs v5 §5.13).
    Collection,
    // Phase 1 — document-import pipeline (FR-CORE-17, reqs v5 §5.14). Proposal-scoped only —
    // never promoted automatically to `Structure`; enforcing that stays an application-layer
    // concern (the document-import pipeline itself, not yet built), not this crate's job.
    CandidateStructureSuggestion,
    // Phase 1 — Mode B architecture design-space representation (FR-ARCH, reqs v5 §5.17).
    // Confirmed against adsg-core's own real API during the Phase 2 spike (packages/cem-archspace):
    // adsg-core itself has no FUN/COMP/MULTI/NOF/DE/CON type hierarchy either — these three kinds
    // plus edge-level tags (see `EdgeKind`'s own Phase 1 doc comments) are this crate's own,
    // deliberately topology-driven mirror of that same design, not a gap the spike exposed.
    Function,
    SelectionChoice,
    ConnectionChoice,
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
            NodeKind::Constraint => "Constraint",
            NodeKind::Parameter => "Parameter",
            NodeKind::InformationElement => "InformationElement",
            NodeKind::Interaction => "Interaction",
            NodeKind::InteractionFragment => "InteractionFragment",
            NodeKind::Collection => "Collection",
            NodeKind::CandidateStructureSuggestion => "CandidateStructureSuggestion",
            NodeKind::Function => "Function",
            NodeKind::SelectionChoice => "SelectionChoice",
            NodeKind::ConnectionChoice => "ConnectionChoice",
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
            "Constraint" => Some(NodeKind::Constraint),
            "Parameter" => Some(NodeKind::Parameter),
            "InformationElement" => Some(NodeKind::InformationElement),
            "Interaction" => Some(NodeKind::Interaction),
            "InteractionFragment" => Some(NodeKind::InteractionFragment),
            "Collection" => Some(NodeKind::Collection),
            "CandidateStructureSuggestion" => Some(NodeKind::CandidateStructureSuggestion),
            "Function" => Some(NodeKind::Function),
            "SelectionChoice" => Some(NodeKind::SelectionChoice),
            "ConnectionChoice" => Some(NodeKind::ConnectionChoice),
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
    /// docs/IMPLEMENTATION_KICKOFF.md Phase 1 — a Constraint's `Parameter` bound to a Value
    /// Property of some structural element (FR-PARAM-02, reqs v5 §5.9). Source is always a
    /// `Parameter`; target is unconstrained (any element can carry the bound property).
    Bound,
    /// Phase 1 — a lower-level `Requirement` derived by analysis from a higher-level one
    /// (FR-CORE-03 amended, reqs v5 §5.3). **Naming note**: reqs v5 §5.3 names this edge
    /// `Derive`; a *different*, unrelated concept (architecture-choice derivation, cycles
    /// permitted) is separately named `derives` in impl v5 §2.3's FR-ARCH section — same word,
    /// two different edges in two different subsystems. Resolved here as two distinct variants:
    /// this one keeps the `Derive` name (Requirements-only); the FR-ARCH one is `ArchDerives`
    /// below. Both `Requirement`.
    Derive,
    /// Phase 1 — marks a `Requirement` as a duplicate of another, kept discoverable as the same
    /// underlying requirement content (FR-CORE-03 amended, reqs v5 §5.3). Both `Requirement`.
    Copy,
    /// Phase 1 — a `Collection`'s membership edge (FR-CORE-10/11, reqs v5 §5.13), deliberately
    /// distinct from `Contains` so a Collection can legitimately reference elements from anywhere
    /// in the graph without threatening `Contains`'s acyclicity guarantee (NFR-REL-02). Source is
    /// always the `Collection`.
    Member,
    /// Phase 1 — Mode B's DSG-style derivation edge (FR-ARCH-01/02, reqs v5 §5.17): "if selected,
    /// these elements exist." **Renamed from the spec's own `derives`** to avoid colliding with
    /// `Derive` above (see that variant's doc comment) — same collision-at-implementation-time
    /// shape this session already resolved for ADR-011/reqs v5 §5.14, documented at the point of
    /// collision rather than silently. Cycles are explicitly permitted (NFR-REL-02 already allows
    /// this generally; mutually-dependent architecture-choice derivation is a legitimate, expected
    /// shape here specifically, confirmed directly against adsg-core's own `derives` edges during
    /// the Phase 2 spike). Deliberately kind-unconstrained — any DSG-participating node can be
    /// either endpoint, matching adsg-core's own genericity here.
    ArchDerives,
    /// Phase 1 — mutual exclusion between two architecture-choice options (FR-ARCH-04, reqs v5
    /// §5.17). Undirected in semantics though modeled as a directed edge like everything else in
    /// this graph (NFR-REL-02's own convention). Kind-unconstrained, same reasoning as
    /// `ArchDerives`.
    IncompatibleWith,
    /// Phase 1 — a Linked/Permutations/Unordered [non-]replacing combination rule across ≥2
    /// architecture choices (FR-ARCH-04, reqs v5 §5.17; mirrors `adsg_core.ChoiceConstraintType`,
    /// confirmed during the Phase 2 spike — the real enum, read directly from
    /// `packages/cem-archspace/.venv/.../adsg_core/graph/choice_constraints.py`, is
    /// `{LINKED, PERMUTATION, UNORDERED, UNORDERED_NOREPL}`). Kind-unconstrained, same reasoning
    /// as `ArchDerives`. **The constraint's own type is real, persisted data** — see `Edge::metadata`
    /// — not just a claim in this comment (a real gap this scope-downs pass closed; the seed
    /// content's own `ChoiceConstraint` edges now set
    /// `metadata: {"choiceConstraintType": "Linked"}`).
    ChoiceConstraint,
    /// docs/IMPLEMENTATION_KICKOFF.md Phase 5 (FR-CORE-12, reqs v5 §5.11) — the swimlane-partition
    /// allocation reqs v5 §5.11 itself calls "the existing `Allocate` dependency stereotype
    /// already implied by FR-CORE-03's dependency taxonomy," but which Phase 1 never actually
    /// added to this enum — a real gap, closed here, not a new design call (unlike ADR-009, which
    /// this same phase separately ratifies). Kind-unconstrained on both ends: "Block/Actor/
    /// Interface" (§5.11's own target list) are all modeled as plain `:Structure` today — no
    /// separate Actor/Interface `NodeKind` exists either, so pinning a stricter rule now would be
    /// guessing ahead of the spec, same discipline already applied to `ArchDerives`/
    /// `IncompatibleWith`/`ChoiceConstraint`.
    Allocate,
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
            EdgeKind::Bound => "BOUND",
            EdgeKind::Derive => "DERIVE",
            EdgeKind::Copy => "COPY",
            EdgeKind::Member => "MEMBER",
            EdgeKind::ArchDerives => "ARCH_DERIVES",
            EdgeKind::IncompatibleWith => "INCOMPATIBLE_WITH",
            EdgeKind::ChoiceConstraint => "CHOICE_CONSTRAINT",
            EdgeKind::Allocate => "ALLOCATE",
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
            "BOUND" => Some(EdgeKind::Bound),
            "DERIVE" => Some(EdgeKind::Derive),
            "COPY" => Some(EdgeKind::Copy),
            "MEMBER" => Some(EdgeKind::Member),
            "ARCH_DERIVES" => Some(EdgeKind::ArchDerives),
            "INCOMPATIBLE_WITH" => Some(EdgeKind::IncompatibleWith),
            "CHOICE_CONSTRAINT" => Some(EdgeKind::ChoiceConstraint),
            "ALLOCATE" => Some(EdgeKind::Allocate),
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
    /// A generic JSON tag for edge-kind-specific data too small to warrant a dedicated field per
    /// kind — matches `ElementBody.properties`'s own generic-bag precedent. Only `ChoiceConstraint`
    /// populates it today (`{"choiceConstraintType": "Linked"|"Permutation"|"Unordered"|
    /// "UnorderedNorepl"}`, mirroring `adsg_core.ChoiceConstraintType` exactly) — no other kind has
    /// a documented, real need yet, so none are touched. `#[serde(default)]` so edges serialized
    /// before this field existed still deserialize; omitted from output entirely when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// A single semantic-validation failure. The real layer (impl §4.2) also checks parametric
/// consistency — no parametric model exists yet, so that rule has nothing to validate against
/// and isn't represented here yet.
// `Eq` dropped from this derive in docs/IMPLEMENTATION_KICKOFF.md Phase 3: `f64` (needed for
// `CompressorLoadingOutOfBounds`'s real-valued diffusion-factor/Mach fields) has no `Eq` impl
// (NaN breaks reflexivity) — confirmed nothing in this codebase actually relied on `ValidationError:
// Eq` specifically (only `PartialEq`, via `assert_eq!` in tests) before making this change.
#[derive(Debug, Clone, PartialEq)]
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
    /// docs/IMPLEMENTATION_KICKOFF.md Phase 3 (FR-COMP-03) — a compressor-subsystem
    /// configuration's diffusion factor or relative Mach number falls outside the stated bound
    /// without an explicit, human-acknowledged override. See
    /// [`check_compressor_blade_loading`]. `metric` is `"diffusionFactor"`/`"relativeMach"`.
    CompressorLoadingOutOfBounds {
        element_id: ElementId,
        metric: &'static str,
        value: f64,
        bound: f64,
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
            ValidationError::CompressorLoadingOutOfBounds {
                element_id,
                metric,
                value,
                bound,
            } => write!(
                f,
                "{element_id}: {metric} {value} exceeds the {bound} bound without an \
                 acknowledged override"
            ),
        }
    }
}

impl std::error::Error for ValidationError {}

/// The Postgres/JSONB side of the polyglot split (NFR-DATA-02): element bodies, long text, and
/// large metadata. Never held in the graph (topology store) — see `GeometryPointer` for the
/// object-store equivalent.
///
/// `properties` is a deliberately untyped bag (no per-`NodeKind` Rust struct exists, and none is
/// planned — every existing body property in this codebase, e.g. Hazard's `severity`/
/// `likelihood`, already works this way). docs/IMPLEMENTATION_KICKOFF.md Phase 1 adds these
/// conventions on top, documented here rather than as a schema change since none is needed:
/// - `citation` on a `Requirement`'s body (FR-CORE-15, reqs v5 §5.14): `{ documentId, page,
///   offset? }`, required when that Requirement's `origin` is `AiSuggested` and it came from the
///   document-import pipeline (not yet built).
/// - `confidence` on a `Requirement`'s body (FR-CORE-18): proposal-scoped/transient — describes a
///   draft, discarded on accept, never persisted to a `Requirement` once it's on `main`.
/// - An optimization-role tag (`objective`/`constraint`/`generic`, plus a permanence flag) on a
///   `Constraint`'s body, and a fulfillment-mechanism tag (`DE`/`MULTI`/`NOF`/`CON`/direct `COMP`)
///   on a `Function`'s body (FR-ARCH, reqs v5 §5.17) — metric/fulfillment roles adsg-core itself
///   models as edge/node *properties*, not distinct types either (confirmed during the Phase 2
///   spike), so mirroring that as a body-property convention here rather than new `NodeKind`
///   variants is deliberate, not a shortcut.
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
        // docs/IMPLEMENTATION_KICKOFF.md Phase 1 — same "only encode what the docs concretely
        // pin down" discipline as Satisfy/Concerns above. `ArchDerives`/`IncompatibleWith`/
        // `ChoiceConstraint` are deliberately left in the `_ => true` catch-all below: the FR-ARCH
        // spec (reqs v5 §5.17) keeps them kind-unconstrained on purpose (any DSG-participating
        // node can be either endpoint), so adding a constraint here would be guessing ahead of it.
        // `Allocate` (Phase 5, FR-CORE-12) joins the same catch-all — see its own doc comment.
        EdgeKind::Bound => source_kind == NodeKind::Parameter,
        EdgeKind::Derive => {
            source_kind == NodeKind::Requirement && target_kind == NodeKind::Requirement
        }
        EdgeKind::Copy => {
            source_kind == NodeKind::Requirement && target_kind == NodeKind::Requirement
        }
        EdgeKind::Member => source_kind == NodeKind::Collection,
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

/// FR-COMP-03 (docs/IMPLEMENTATION_KICKOFF.md Phase 3) — rejects a compressor-subsystem
/// configuration whose diffusion factor or relative Mach number falls outside the stated bound.
/// Thresholds are the exact numbers reqs v5 §5.15/FR-COMP-03 cite (sourced from NASA SP-36 via
/// the literature-extraction working doc), not invented here:
/// - Diffusion factor > 0.4 is rejected unless `override_acknowledged`.
/// - Relative Mach > 1.35 ("demonstrated-extended," the doc's own outer ceiling) is rejected
///   **unconditionally** — no override accepts beyond it.
/// - Relative Mach in `(1.2, 1.35]` ("routine" vs. "demonstrated-extended") is rejected unless
///   `override_acknowledged`.
/// - Either metric can be `None` (not yet specified) — a `None` metric is never checked, matching
///   how every other optional body property in this codebase is treated (absence isn't a
///   violation of a bound that doesn't apply yet).
///
/// **Pure and tested, not yet wired into any HTTP endpoint** — this codebase's one generic
/// body-property endpoint (`PATCH .../elements/:id/body`) validates nothing kind-specific today
/// (Hazard severity, Stage Tracking status, etc. are all unvalidated JSONB-bag conventions), and
/// bolting a check onto only this one property would make that endpoint inconsistently stricter
/// for compressors than for everything else already flowing through it. Wiring this into a real
/// calling convention (with `traceability.rs`'s existing `?acknowledge=true`/`409` pattern as the
/// natural REST shape) is Phase 5's "API surface" work, not this phase's.
pub fn check_compressor_blade_loading(
    element_id: &str,
    diffusion_factor: Option<f64>,
    relative_mach: Option<f64>,
    override_acknowledged: bool,
) -> Result<(), ValidationError> {
    const DIFFUSION_FACTOR_BOUND: f64 = 0.4;
    const RELATIVE_MACH_ROUTINE_BOUND: f64 = 1.2;
    const RELATIVE_MACH_DEMONSTRATED_BOUND: f64 = 1.35;

    if let Some(value) = diffusion_factor {
        if value > DIFFUSION_FACTOR_BOUND && !override_acknowledged {
            return Err(ValidationError::CompressorLoadingOutOfBounds {
                element_id: element_id.to_string(),
                metric: "diffusionFactor",
                value,
                bound: DIFFUSION_FACTOR_BOUND,
            });
        }
    }

    if let Some(value) = relative_mach {
        if value > RELATIVE_MACH_DEMONSTRATED_BOUND {
            return Err(ValidationError::CompressorLoadingOutOfBounds {
                element_id: element_id.to_string(),
                metric: "relativeMach",
                value,
                bound: RELATIVE_MACH_DEMONSTRATED_BOUND,
            });
        }
        if value > RELATIVE_MACH_ROUTINE_BOUND && !override_acknowledged {
            return Err(ValidationError::CompressorLoadingOutOfBounds {
                element_id: element_id.to_string(),
                metric: "relativeMach",
                value,
                bound: RELATIVE_MACH_ROUTINE_BOUND,
            });
        }
    }

    Ok(())
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
            metadata: None,
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
                metadata: None,
            })
            .unwrap();

        let result = graph.add_edge(Edge {
            source: "REQ-THRUST".to_string(),
            target: "Turbine".to_string(),
            kind: EdgeKind::Refine,
            metadata: None,
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
            metadata: None,
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
            metadata: None,
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
            NodeKind::Constraint,
            NodeKind::Parameter,
            NodeKind::InformationElement,
            NodeKind::Interaction,
            NodeKind::InteractionFragment,
            NodeKind::Collection,
            NodeKind::CandidateStructureSuggestion,
            NodeKind::Function,
            NodeKind::SelectionChoice,
            NodeKind::ConnectionChoice,
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
            EdgeKind::Bound,
            EdgeKind::Derive,
            EdgeKind::Copy,
            EdgeKind::Member,
            EdgeKind::ArchDerives,
            EdgeKind::IncompatibleWith,
            EdgeKind::ChoiceConstraint,
            EdgeKind::Allocate,
        ] {
            assert_eq!(EdgeKind::from_rel_type(kind.as_rel_type()), Some(kind));
        }
        assert_eq!(EdgeKind::from_rel_type("NOT_A_KIND"), None);
    }

    /// docs/IMPLEMENTATION_KICKOFF.md Phase 1 (FR-PARAM-02): a Constraint's Parameter binds to a
    /// Value Property of any structural element — source must be a `Parameter`, target is
    /// unconstrained.
    #[test]
    fn accepts_legal_bound_endpoint() {
        assert!(check_relationship_endpoints(
            EdgeKind::Bound,
            "BypassRatioParam",
            NodeKind::Parameter,
            "FanLpCompression",
            NodeKind::Structure,
        )
        .is_ok());
    }

    #[test]
    fn rejects_illegal_bound_endpoint() {
        assert_eq!(
            check_relationship_endpoints(
                EdgeKind::Bound,
                "FanLpCompression",
                NodeKind::Structure,
                "BypassRatioParam",
                NodeKind::Parameter,
            ),
            Err(ValidationError::IllegalEndpoint {
                kind: EdgeKind::Bound,
                source: "FanLpCompression".to_string(),
                source_kind: NodeKind::Structure,
                target: "BypassRatioParam".to_string(),
                target_kind: NodeKind::Parameter,
            })
        );
    }

    /// Phase 1 (FR-CORE-03 amended): `Derive`/`Copy` connect two Requirements — a Requirement
    /// deriving from/copying a Structure is rejected either way.
    #[test]
    fn accepts_legal_derive_and_copy_endpoints() {
        for kind in [EdgeKind::Derive, EdgeKind::Copy] {
            assert!(check_relationship_endpoints(
                kind,
                "REQ-LOW",
                NodeKind::Requirement,
                "REQ-THRUST",
                NodeKind::Requirement,
            )
            .is_ok());
        }
    }

    #[test]
    fn rejects_illegal_derive_and_copy_endpoints() {
        for kind in [EdgeKind::Derive, EdgeKind::Copy] {
            assert!(check_relationship_endpoints(
                kind,
                "REQ-LOW",
                NodeKind::Requirement,
                "Turbine",
                NodeKind::Structure,
            )
            .is_err());
        }
    }

    /// Phase 1 (FR-CORE-10/11): a `Member` edge's source must be the `Collection` — a plain
    /// element "member-ing" another plain element (not a Collection) is rejected.
    #[test]
    fn accepts_legal_member_endpoint() {
        assert!(check_relationship_endpoints(
            EdgeKind::Member,
            "SubsystemBlocks",
            NodeKind::Collection,
            "Turbine",
            NodeKind::Structure,
        )
        .is_ok());
    }

    #[test]
    fn rejects_illegal_member_endpoint() {
        assert!(check_relationship_endpoints(
            EdgeKind::Member,
            "Turbine",
            NodeKind::Structure,
            "SubsystemBlocks",
            NodeKind::Collection,
        )
        .is_err());
    }

    /// Phase 1 (FR-ARCH): `ArchDerives`/`IncompatibleWith`/`ChoiceConstraint` are deliberately
    /// kind-unconstrained — the spec keeps them generic across any DSG-participating node.
    #[test]
    fn accepts_arch_edges_between_any_kinds() {
        for kind in [
            EdgeKind::ArchDerives,
            EdgeKind::IncompatibleWith,
            EdgeKind::ChoiceConstraint,
        ] {
            assert!(check_relationship_endpoints(
                kind,
                "GenerateThrust",
                NodeKind::Function,
                "IncludeGearbox",
                NodeKind::SelectionChoice,
            )
            .is_ok());
        }
    }

    /// docs/IMPLEMENTATION_KICKOFF.md Phase 5 (FR-CORE-12): `Allocate` is kind-unconstrained on
    /// both ends — no separate Actor/Interface `NodeKind` exists to pin a stricter rule against.
    #[test]
    fn accepts_allocate_between_any_kinds() {
        assert!(check_relationship_endpoints(
            EdgeKind::Allocate,
            "SomeElement",
            NodeKind::Requirement,
            "ControlFadecEec",
            NodeKind::Structure,
        )
        .is_ok());
    }

    /// FR-COMP-03 (Phase 3): within the routine bound on both metrics — always accepted.
    #[test]
    fn compressor_loading_within_routine_bounds_is_accepted() {
        assert!(
            check_compressor_blade_loading("FanLpCompression", Some(0.35), Some(1.1), false)
                .is_ok()
        );
        // Missing metrics are never checked.
        assert!(check_compressor_blade_loading("FanLpCompression", None, None, false).is_ok());
    }

    #[test]
    fn compressor_loading_over_diffusion_factor_bound_needs_override() {
        let result = check_compressor_blade_loading("FanLpCompression", Some(0.45), None, false);
        assert_eq!(
            result,
            Err(ValidationError::CompressorLoadingOutOfBounds {
                element_id: "FanLpCompression".to_string(),
                metric: "diffusionFactor",
                value: 0.45,
                bound: 0.4,
            })
        );
        assert!(check_compressor_blade_loading("FanLpCompression", Some(0.45), None, true).is_ok());
    }

    #[test]
    fn compressor_loading_demonstrated_extended_mach_needs_override() {
        // 1.2 < 1.3 <= 1.35: rejected without override, accepted with one.
        assert!(
            check_compressor_blade_loading("CoreHpCompressor", None, Some(1.3), false).is_err()
        );
        assert!(check_compressor_blade_loading("CoreHpCompressor", None, Some(1.3), true).is_ok());
    }

    #[test]
    fn compressor_loading_beyond_demonstrated_ceiling_is_never_accepted() {
        // Above 1.35 is rejected even with an override -- it's the doc's own outer ceiling, not
        // just the routine/demonstrated split.
        let result = check_compressor_blade_loading("CoreHpCompressor", None, Some(1.4), true);
        assert_eq!(
            result,
            Err(ValidationError::CompressorLoadingOutOfBounds {
                element_id: "CoreHpCompressor".to_string(),
                metric: "relativeMach",
                value: 1.4,
                bound: 1.35,
            })
        );
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
