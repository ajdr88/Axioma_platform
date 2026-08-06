//! Topology store (ADR-003 / NFR-DATA-01): Neo4j holds elements and relationships only — no
//! bodies, no blobs. See `super::postgres` and `super::objects` for those.
//!
//! Every element carries a `project_id` property (multi-project support, roadmap versioning
//! work) — the same tier as `active`/`origin`, not a separate label. `MERGE`/`MATCH` always key
//! on `{id, project_id}` together, so the same human-readable id string (e.g. a seeded fixture's
//! `"Combustor"`) in two different projects addresses two distinct nodes, never one shared node.

use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result};
use neo4rs::{query, BoltType, Graph as Neo4jConn, Row};
use sysml_core::{Edge, EdgeKind, Element, ElementId, NodeKind, Origin, ValidationError};
use sysml_textual::GraphOp;

/// Shared by `list_elements` and `get_element` — both queries return the same five columns.
fn row_to_element(row: &Row) -> Result<Element> {
    let id: String = row.get("id").context("missing id")?;
    let name: String = row.get("name").context("missing name")?;
    let labels: Vec<String> = row.get("labels").context("missing labels")?;
    let active: bool = row.get("active").context("missing active")?;
    let origin: String = row.get("origin").context("missing origin")?;
    let kind = labels
        .iter()
        .find_map(|l| NodeKind::from_label(l))
        .unwrap_or(NodeKind::Element);
    Ok(Element {
        id,
        kind,
        name,
        active,
        origin: Origin::from_str_or_default(&origin),
    })
}

#[derive(Clone)]
pub struct Neo4jStore {
    conn: Neo4jConn,
}

/// Every `NodeKind` label — used to create the `(id, project_id)` index on each one (roadmap:
/// T-P1.4-06). Cypher can't parameterize a label, so this is a fixed list of statements, not a
/// loop over a query template with a bound label.
const ALL_LABELS: [&str; 9] = [
    "Element",
    "Structure",
    "Requirement",
    "Port",
    "Hazard",
    "Control",
    "Mission",
    "Stakeholder",
    "SimulationRun",
];

impl Neo4jStore {
    pub async fn connect(uri: &str, user: &str, password: &str) -> Result<Self> {
        let conn = Neo4jConn::new(uri, user, password)
            .await
            .with_context(|| format!("connecting to Neo4j at {uri}"))?;
        let store = Self { conn };
        store.ensure_indexes().await?;
        Ok(store)
    }

    /// Every existing query matches on `{id, project_id}` with no index at all — confirmed
    /// directly (grepped the whole store for `CREATE INDEX`/`CREATE CONSTRAINT`: none exist),
    /// meaning every read/write today is an unindexed property scan. This is the single
    /// highest-leverage fix for T-P1.4-06's 1M-element scale — every existing feature benefits,
    /// not just the new fixture.
    ///
    /// This creates one index per `ALL_LABELS` entry, which serves two different query shapes
    /// found in this file:
    /// - The handful of queries that already state a *specific* label (`upsert_element`'s
    ///   `MERGE`, `bulk_upsert_elements`, `bulk_create_edges`) use their own specific label's
    ///   index directly.
    /// - Every OTHER query in this file matches by `{id, project_id}` with *no* label at all
    ///   (`get_element`, `trace_neighbors`, `create_edge`'s final `MERGE`, `delete_element`,
    ///   etc.) — a label-less `MATCH` can't use a label-scoped index no matter how many exist.
    ///   These are rewritten to match `:Element` instead — the one label [`Self::upsert_element`]
    ///   (and `bulk_upsert_elements`) now applies to *every* node in addition to its specific
    ///   kind label, so the plain `Element` entry already in `ALL_LABELS` covers them too; no
    ///   separate index is needed. Confirmed directly this second half was necessary: with only
    ///   the specific-label indexes in place, `trace_neighbors` (via the traceability endpoint)
    ///   was just as slow as before at real 1M-element scale, since none of its own patterns
    ///   named a label.
    ///
    /// All idempotent via `IF NOT EXISTS`, safe to run on every connect.
    async fn ensure_indexes(&self) -> Result<()> {
        for label in ALL_LABELS {
            let cypher = format!(
                "CREATE INDEX element_id_project_{label} IF NOT EXISTS \
                 FOR (n:{label}) ON (n.id, n.project_id)"
            );
            self.conn
                .run(query(&cypher))
                .await
                .with_context(|| format!("creating index for label {label}"))?;
        }
        Ok(())
    }

    pub async fn ping(&self) -> Result<()> {
        self.conn
            .run(query("RETURN 1"))
            .await
            .context("Neo4j ping failed")
    }

    /// `MERGE`s an element node by `(id, project_id)`. Only `id`/`kind`(label)/`name`/`active`/
    /// `origin`/`project_id` are stored here — bodies and blobs live elsewhere (NFR-DATA-01).
    ///
    /// Every node gets a second, shared `:Element` label alongside its specific kind label
    /// (multi-label nodes are a first-class Neo4j feature) — see `ensure_indexes`'s doc comment
    /// for why: it's what lets every OTHER method's label-less `MATCH (n {id: ..., project_id:
    /// ...})` (which has no way to know a specific kind up front) use an index at all.
    pub async fn upsert_element(&self, project_id: &str, element: &Element) -> Result<()> {
        let label = element.kind.as_label();
        let cypher = format!(
            "MERGE (n:Element:{label} {{id: $id, project_id: $project_id}}) \
             SET n.name = $name, n.active = $active, n.origin = $origin"
        );
        self.conn
            .run(
                query(&cypher)
                    .param("id", element.id.clone())
                    .param("project_id", project_id.to_string())
                    .param("name", element.name.clone())
                    .param("active", element.active)
                    .param("origin", element.origin.as_str()),
            )
            .await
            .with_context(|| format!("upserting element {}", element.id))
    }

    /// Renames an element by id — wrapped in an explicit Neo4j transaction rather than
    /// `upsert_element`'s implicit single-statement write. T-P1.2-01 makes an explicit "backend
    /// records exactly one transaction" claim about this exact write path, so it uses a real
    /// transaction primitive rather than relying on a single Cypher statement being *incidentally*
    /// atomic.
    pub async fn rename_element(&self, project_id: &str, id: &str, name: &str) -> Result<()> {
        let mut txn = self
            .conn
            .start_txn()
            .await
            .context("starting rename transaction")?;
        let result = txn
            .run(
                query("MATCH (n:Element {id: $id, project_id: $project_id}) SET n.name = $name")
                    .param("id", id.to_string())
                    .param("project_id", project_id.to_string())
                    .param("name", name.to_string()),
            )
            .await;
        if let Err(err) = result {
            let _ = txn.rollback().await;
            return Err(err).with_context(|| format!("renaming element {id}"));
        }
        txn.commit().await.context("committing rename transaction")
    }

    /// Sets just the `active` flag (canvas deactivate/reactivate) — never touches `name`.
    pub async fn set_active(&self, project_id: &str, id: &str, active: bool) -> Result<()> {
        self.conn
            .run(
                query(
                    "MATCH (n:Element {id: $id, project_id: $project_id}) SET n.active = $active",
                )
                .param("id", id.to_string())
                .param("project_id", project_id.to_string())
                .param("active", active),
            )
            .await
            .with_context(|| format!("setting active={active} on element {id}"))
    }

    /// Sets just the `origin` flag (FR-CORE-08 provenance scaffolding, T-P1.2-06's "mark as
    /// ai-suggested via the API") — never touches `name`/`active`.
    pub async fn set_origin(&self, project_id: &str, id: &str, origin: Origin) -> Result<()> {
        self.conn
            .run(
                query(
                    "MATCH (n:Element {id: $id, project_id: $project_id}) SET n.origin = $origin",
                )
                .param("id", id.to_string())
                .param("project_id", project_id.to_string())
                .param("origin", origin.as_str()),
            )
            .await
            .with_context(|| format!("setting origin={origin:?} on element {id}"))
    }

    /// Single-element lookup — used by rename to preserve `kind`/`active`/`origin` when only
    /// `name` changes.
    pub async fn get_element(&self, project_id: &str, id: &str) -> Result<Option<Element>> {
        let mut result = self
            .conn
            .execute(
                query(
                    "MATCH (n:Element {id: $id, project_id: $project_id}) RETURN n.id AS id, \
                     n.name AS name, labels(n) AS labels, coalesce(n.active, true) AS active, \
                     coalesce(n.origin, 'Human') AS origin",
                )
                .param("id", id.to_string())
                .param("project_id", project_id.to_string()),
            )
            .await
            .with_context(|| format!("looking up element {id}"))?;

        match result.next().await.context("reading element row")? {
            Some(row) => Ok(Some(row_to_element(&row)?)),
            None => Ok(None),
        }
    }

    pub async fn list_elements(&self, project_id: &str) -> Result<Vec<Element>> {
        let mut result = self
            .conn
            .execute(
                query(
                    "MATCH (n:Element {project_id: $project_id}) RETURN n.id AS id, n.name AS name, \
                     labels(n) AS labels, coalesce(n.active, true) AS active, \
                     coalesce(n.origin, 'Human') AS origin",
                )
                .param("project_id", project_id.to_string()),
            )
            .await
            .context("listing elements")?;

        let mut elements = Vec::new();
        while let Some(row) = result.next().await.context("reading element row")? {
            elements.push(row_to_element(&row)?);
        }
        Ok(elements)
    }

    /// Every existing element id mapped to its current `NodeKind` — used to detect a
    /// kind-conflict (re-importing an id under a different kind) before writing anything.
    pub async fn element_kinds(&self, project_id: &str) -> Result<HashMap<ElementId, NodeKind>> {
        Ok(self
            .list_elements(project_id)
            .await?
            .into_iter()
            .map(|el| (el.id, el.kind))
            .collect())
    }

    /// All existing edges of one kind (within one project) — enough to run
    /// [`sysml_core::would_create_containment_cycle`] (for `Contains`) without hydrating a full
    /// graph, and to list a relationship kind (e.g. `Causes`/`MitigatedBy`) for the API.
    pub async fn edges_of_kind(&self, project_id: &str, kind: EdgeKind) -> Result<Vec<Edge>> {
        let rel_type = kind.as_rel_type();
        let cypher = format!(
            "MATCH (a:Element {{project_id: $project_id}})-[:{rel_type}]->\
             (b:Element {{project_id: $project_id}}) RETURN a.id AS source, b.id AS target"
        );
        let mut result = self
            .conn
            .execute(query(&cypher).param("project_id", project_id.to_string()))
            .await
            .with_context(|| format!("listing {rel_type} edges"))?;

        let mut edges = Vec::new();
        while let Some(row) = result.next().await.context("reading edge row")? {
            edges.push(Edge {
                source: row.get("source").context("missing source")?,
                target: row.get("target").context("missing target")?,
                kind,
            });
        }
        Ok(edges)
    }

    /// All existing `Contains` edges — enough to run
    /// [`sysml_core::would_create_containment_cycle`] without hydrating a full graph.
    pub async fn contains_edges(&self, project_id: &str) -> Result<Vec<Edge>> {
        self.edges_of_kind(project_id, EdgeKind::Contains).await
    }

    /// Creates a relationship between two already-`upsert_element`ed nodes, matched by
    /// `(id, project_id)`. Rejects a dangling edge (either endpoint not an existing element,
    /// NFR-REL-01) and an endpoint-type violation ([`sysml_core::check_relationship_endpoints`],
    /// FR-CORE-05) before checking containment acyclicity (FR-CORE-05, NFR-REL-02,
    /// `Contains`-only) — every other edge kind is legal to cycle (§5.7/NFR-REL-02).
    pub async fn create_edge(&self, project_id: &str, edge: &Edge) -> Result<()> {
        let source_el = self
            .get_element(project_id, &edge.source)
            .await?
            .ok_or_else(|| ValidationError::DanglingEdge {
                edge_kind: edge.kind,
                missing_id: edge.source.clone(),
            })?;
        let target_el = self
            .get_element(project_id, &edge.target)
            .await?
            .ok_or_else(|| ValidationError::DanglingEdge {
                edge_kind: edge.kind,
                missing_id: edge.target.clone(),
            })?;
        sysml_core::check_relationship_endpoints(
            edge.kind,
            &edge.source,
            source_el.kind,
            &edge.target,
            target_el.kind,
        )?;

        if edge.kind.is_acyclicity_scoped() {
            let existing = self.contains_edges(project_id).await?;
            if sysml_core::would_create_containment_cycle(&existing, &edge.source, &edge.target) {
                return Err(ValidationError::ContainmentCycle {
                    parent: edge.source.clone(),
                    child: edge.target.clone(),
                }
                .into());
            }
        }

        let rel_type = edge.kind.as_rel_type();
        let cypher = format!(
            "MATCH (a:Element {{id: $source, project_id: $project_id}}), \
             (b:Element {{id: $target, project_id: $project_id}}) MERGE (a)-[:{rel_type}]->(b)"
        );
        self.conn
            .run(
                query(&cypher)
                    .param("source", edge.source.clone())
                    .param("target", edge.target.clone())
                    .param("project_id", project_id.to_string()),
            )
            .await
            .with_context(|| {
                format!(
                    "creating {rel_type} edge {} -> {}",
                    edge.source, edge.target
                )
            })
    }

    /// Removes a relationship between two nodes, matched by id and kind. No validation needed —
    /// removing an edge can't create a cycle or a kind conflict, only heal one.
    pub async fn delete_edge(&self, project_id: &str, edge: &Edge) -> Result<()> {
        let rel_type = edge.kind.as_rel_type();
        let cypher = format!(
            "MATCH (a:Element {{id: $source, project_id: $project_id}})-[r:{rel_type}]->\
             (b:Element {{id: $target, project_id: $project_id}}) DELETE r"
        );
        self.conn
            .run(
                query(&cypher)
                    .param("source", edge.source.clone())
                    .param("target", edge.target.clone())
                    .param("project_id", project_id.to_string()),
            )
            .await
            .with_context(|| {
                format!(
                    "deleting {rel_type} edge {} -> {}",
                    edge.source, edge.target
                )
            })
    }

    /// Removes an element and every relationship touching it, any kind, either direction
    /// (`DETACH DELETE` — Neo4j's atomic node-plus-incident-edges delete). Callers (P1.3,
    /// `apps/api/src/traceability.rs`) are responsible for the Traceability Breach gate *before*
    /// calling this — it performs no dependent check itself, matching `delete_edge`'s "no
    /// validation needed" stance for the graph-level operation itself.
    pub async fn delete_element(&self, project_id: &str, id: &str) -> Result<()> {
        self.conn
            .run(
                query("MATCH (n:Element {id: $id, project_id: $project_id}) DETACH DELETE n")
                    .param("id", id.to_string())
                    .param("project_id", project_id.to_string()),
            )
            .await
            .with_context(|| format!("deleting element {id}"))
    }

    /// One hop of {`Satisfy`, `Verify`, `Refine`} out of this element (this element is the edge's
    /// source) — the traceability BFS engine's (`apps/api/src/traceability.rs`) "outgoing"
    /// direction (FR-CORE-03).
    pub async fn trace_outgoing_neighbors(
        &self,
        project_id: &str,
        id: &str,
    ) -> Result<Vec<(ElementId, EdgeKind)>> {
        self.trace_neighbors(project_id, id, "->").await
    }

    /// One hop of {`Satisfy`, `Verify`, `Refine`} into this element (this element is the edge's
    /// target) — "what depends on / satisfies / verifies / refines me," the change-impact
    /// direction (T-P1.3-01/03).
    pub async fn trace_incoming_neighbors(
        &self,
        project_id: &str,
        id: &str,
    ) -> Result<Vec<(ElementId, EdgeKind)>> {
        self.trace_neighbors(project_id, id, "<-").await
    }

    async fn trace_neighbors(
        &self,
        project_id: &str,
        id: &str,
        arrow: &str,
    ) -> Result<Vec<(ElementId, EdgeKind)>> {
        // `arrow` is always one of the two literal strings above, never caller/user input —
        // same "fixed, closed set, safe to interpolate" reasoning as `EdgeKind::as_rel_type`.
        let cypher = if arrow == "->" {
            "MATCH (a:Element {id: $id, project_id: $project_id})-[r:SATISFY|VERIFY|REFINE]->\
             (b:Element {project_id: $project_id}) RETURN b.id AS neighbor_id, type(r) AS rel_type"
        } else {
            "MATCH (a:Element {id: $id, project_id: $project_id})<-[r:SATISFY|VERIFY|REFINE]-\
             (b:Element {project_id: $project_id}) RETURN b.id AS neighbor_id, type(r) AS rel_type"
        };
        let mut result = self
            .conn
            .execute(
                query(cypher)
                    .param("id", id.to_string())
                    .param("project_id", project_id.to_string()),
            )
            .await
            .with_context(|| format!("listing trace neighbors of {id}"))?;

        let mut neighbors = Vec::new();
        while let Some(row) = result.next().await.context("reading trace neighbor row")? {
            let neighbor_id: String = row.get("neighbor_id").context("missing neighbor_id")?;
            let rel_type: String = row.get("rel_type").context("missing rel_type")?;
            let kind = EdgeKind::from_rel_type(&rel_type)
                .with_context(|| format!("unknown relationship type {rel_type}"))?;
            neighbors.push((neighbor_id, kind));
        }
        Ok(neighbors)
    }

    /// Bulk-writes elements in `UNWIND`-batched chunks — the one-`MERGE`-per-element pattern
    /// (`upsert_element`) is architecturally the wrong shape at 1M-element scale (roadmap:
    /// T-P1.4-06's synthetic `Turbofan-Scale` fixture). Groups by `NodeKind` first since Cypher
    /// can't parameterize a label, so each kind gets its own `UNWIND` statement per chunk. No
    /// validation of any kind (no kind-conflict check) — a trusted bulk path for a fixture
    /// generator that owns every id it writes, never exposed via any HTTP endpoint.
    ///
    /// Only `apps/api/src/bin/seed_turbofan_scale.rs` calls this, not `main.rs`'s HTTP surface —
    /// `#[allow(dead_code)]` because each `[[bin]]` target's dead-code analysis is independent,
    /// so `main.rs`'s own compilation sees this as unused even though the other binary needs it.
    #[allow(dead_code)]
    pub async fn bulk_upsert_elements(&self, project_id: &str, elements: &[Element]) -> Result<()> {
        const CHUNK_SIZE: usize = 5_000;
        let mut by_kind: HashMap<NodeKind, Vec<&Element>> = HashMap::new();
        for element in elements {
            by_kind.entry(element.kind).or_default().push(element);
        }
        for (kind, group) in by_kind {
            let label = kind.as_label();
            let cypher = format!(
                "UNWIND $rows AS row \
                 MERGE (n:Element:{label} {{id: row.id, project_id: $project_id}}) \
                 SET n.name = row.name, n.active = row.active, n.origin = row.origin"
            );
            for chunk in group.chunks(CHUNK_SIZE) {
                let rows: Vec<BoltType> = chunk
                    .iter()
                    .map(|element| {
                        let mut row: HashMap<String, BoltType> = HashMap::new();
                        row.insert("id".to_string(), element.id.clone().into());
                        row.insert("name".to_string(), element.name.clone().into());
                        row.insert("active".to_string(), element.active.into());
                        row.insert(
                            "origin".to_string(),
                            element.origin.as_str().to_string().into(),
                        );
                        BoltType::from(row)
                    })
                    .collect();
                self.conn
                    .run(
                        query(&cypher)
                            .param("rows", rows)
                            .param("project_id", project_id.to_string()),
                    )
                    .await
                    .with_context(|| format!("bulk-upserting a chunk of {label} elements"))?;
            }
        }
        Ok(())
    }

    /// Bulk-writes edges of one kind in `UNWIND`-batched chunks. Deliberately skips the
    /// dangling-edge/cycle validation [`Self::create_edge`] normally does — safe only because the
    /// caller (the `Turbofan-Scale` seeder) constructs an already-acyclic, fully-formed graph by
    /// construction; never exposed via any HTTP endpoint. Same `#[allow(dead_code)]` reasoning as
    /// [`Self::bulk_upsert_elements`] just above.
    ///
    /// `source_label`/`target_label` name every endpoint's `NodeKind` label — confirmed
    /// necessary, not a style preference: a label-less `MATCH (a {id: ..., project_id: ...})`
    /// (the same shape [`Self::get_element`]/[`Self::create_edge`] already use) can't use the
    /// per-label index `ensure_indexes` creates, since the planner has no way to know which
    /// label's index to consult, and falls back to an unindexed scan. Confirmed directly at real
    /// ~200k-in-flight scale: `bulk_upsert_elements`'s labeled `MERGE` finished a 200,000-element
    /// chunk set in well under a minute; the *first* unlabeled edge-`MATCH` attempt was still on
    /// its first few 5,000-edge chunks ten minutes in. This is also why looping
    /// [`Self::create_edge`] for a modest edge count (e.g. a few hundred `Satisfy` edges) is only
    /// safe *before* the graph reaches real scale — at 1M+ elements the exact same unlabeled-scan
    /// cost applies per call, just amortized over fewer calls, not eliminated.
    #[allow(dead_code)]
    pub async fn bulk_create_edges(
        &self,
        project_id: &str,
        kind: EdgeKind,
        source_label: &str,
        target_label: &str,
        pairs: &[(ElementId, ElementId)],
    ) -> Result<()> {
        const CHUNK_SIZE: usize = 5_000;
        let rel_type = kind.as_rel_type();
        let cypher = format!(
            "UNWIND $rows AS row \
             MATCH (a:{source_label} {{id: row.source, project_id: $project_id}}), \
                   (b:{target_label} {{id: row.target, project_id: $project_id}}) \
             MERGE (a)-[:{rel_type}]->(b)"
        );
        for chunk in pairs.chunks(CHUNK_SIZE) {
            let rows: Vec<BoltType> = chunk
                .iter()
                .map(|(source, target)| {
                    let mut row: HashMap<String, BoltType> = HashMap::new();
                    row.insert("source".to_string(), source.clone().into());
                    row.insert("target".to_string(), target.clone().into());
                    BoltType::from(row)
                })
                .collect();
            self.conn
                .run(
                    query(&cypher)
                        .param("rows", rows)
                        .param("project_id", project_id.to_string()),
                )
                .await
                .with_context(|| format!("bulk-creating a chunk of {rel_type} edges"))?;
        }
        Ok(())
    }

    /// Validates and writes a batch of elements + `Contains` edges atomically (import handlers,
    /// FR-CORE-07): kind-conflict and containment-cycle checks run first, against existing state
    /// *and* against the batch itself (so a batch that only cycles internally is also caught) —
    /// nothing is written unless every check passes. The writes then run in a single Neo4j
    /// transaction, explicitly rolled back on any failure, so a mid-batch error can't leave a
    /// partial import (T-P1.1-02's "no partial write" standard, extended to a whole batch).
    pub async fn import_elements_and_edges(
        &self,
        project_id: &str,
        elements: &[Element],
        contains: &[(ElementId, ElementId)],
    ) -> Result<()> {
        let existing_kinds = self.element_kinds(project_id).await?;
        for element in elements {
            sysml_core::check_kind_conflict(existing_kinds.get(&element.id).copied(), element)?;
        }

        let mut working_edges = self.contains_edges(project_id).await?;
        for (parent, child) in contains {
            if sysml_core::would_create_containment_cycle(&working_edges, parent, child) {
                return Err(ValidationError::ContainmentCycle {
                    parent: parent.clone(),
                    child: child.clone(),
                }
                .into());
            }
            working_edges.push(Edge {
                source: parent.clone(),
                target: child.clone(),
                kind: EdgeKind::Contains,
            });
        }

        let mut txn = self
            .conn
            .start_txn()
            .await
            .context("starting import transaction")?;

        for element in elements {
            let label = element.kind.as_label();
            let cypher = format!(
                "MERGE (n:Element:{label} {{id: $id, project_id: $project_id}}) \
                 SET n.name = $name, n.active = $active, n.origin = $origin"
            );
            if let Err(err) = txn
                .run(
                    query(&cypher)
                        .param("id", element.id.clone())
                        .param("project_id", project_id.to_string())
                        .param("name", element.name.clone())
                        .param("active", element.active)
                        .param("origin", element.origin.as_str()),
                )
                .await
            {
                let _ = txn.rollback().await;
                return Err(err).with_context(|| format!("importing element {}", element.id));
            }
        }

        for (parent, child) in contains {
            let rel_type = EdgeKind::Contains.as_rel_type();
            let cypher = format!(
                "MATCH (a:Element {{id: $source, project_id: $project_id}}), \
                 (b:Element {{id: $target, project_id: $project_id}}) MERGE (a)-[:{rel_type}]->(b)"
            );
            if let Err(err) = txn
                .run(
                    query(&cypher)
                        .param("source", parent.clone())
                        .param("target", child.clone())
                        .param("project_id", project_id.to_string()),
                )
                .await
            {
                let _ = txn.rollback().await;
                return Err(err)
                    .with_context(|| format!("importing containment edge {parent} -> {child}"));
            }
        }

        txn.commit().await.context("committing import transaction")
    }

    /// Applies a batch of [`GraphOp`]s (FR-CORE-02 / T-P1.2-01's text↔diagram sync) as one
    /// atomic transaction — every op is validated against current state (and against the
    /// batch's own effect on that state, processed in order) before anything is written; if any
    /// op fails, nothing commits. Unlike [`Self::import_elements_and_edges`]'s per-pair
    /// re-scan-a-growing-`Vec` cycle check, this builds one children/parent adjacency index up
    /// front and updates it incrementally — an ancestry check walks only the affected subtree,
    /// not the whole edge set, and there's exactly one Neo4j round trip for state, not one per
    /// op.
    pub async fn apply_graph_ops(
        &self,
        project_id: &str,
        ops: &[GraphOp],
    ) -> Result<ApplyOpsOutcome> {
        let existing_elements = self.list_elements(project_id).await?;
        let existing_ids: HashSet<ElementId> =
            existing_elements.iter().map(|e| e.id.clone()).collect();
        let contains = self.contains_edges(project_id).await?;

        let mut children_of: HashMap<ElementId, Vec<ElementId>> = HashMap::new();
        let mut parent_of: HashMap<ElementId, ElementId> = HashMap::new();
        for edge in &contains {
            children_of
                .entry(edge.source.clone())
                .or_default()
                .push(edge.target.clone());
            parent_of.insert(edge.target.clone(), edge.source.clone());
        }

        let mut id_map: HashMap<String, ElementId> = HashMap::new();
        let mut op_errors: Vec<OpError> = Vec::new();

        // Resolves a referenced id that may be a real element id, or an earlier `Create` op's
        // `temp_id` in this same batch.
        let resolve = |id_map: &HashMap<String, ElementId>, raw: &str| -> String {
            id_map.get(raw).cloned().unwrap_or_else(|| raw.to_string())
        };

        for (index, op) in ops.iter().enumerate() {
            match op {
                GraphOp::Rename { id, name } => {
                    if !existing_ids.contains(id) {
                        op_errors.push(OpError {
                            op_index: index,
                            message: format!("element #{id} does not exist"),
                        });
                        continue;
                    }
                    if name.trim().is_empty() {
                        op_errors.push(OpError {
                            op_index: index,
                            message: "name must not be empty".to_string(),
                        });
                    }
                }
                GraphOp::Create {
                    temp_id, parent_id, ..
                } => {
                    let resolved_parent = parent_id.as_ref().map(|p| resolve(&id_map, p));
                    if let Some(parent) = &resolved_parent {
                        if !existing_ids.contains(parent) && !id_map.values().any(|v| v == parent) {
                            op_errors.push(OpError {
                                op_index: index,
                                message: format!("parent #{parent} does not exist"),
                            });
                            continue;
                        }
                    }
                    let real_id = uuid::Uuid::new_v4().to_string();
                    id_map.insert(temp_id.clone(), real_id.clone());
                    if let Some(parent) = resolved_parent {
                        children_of
                            .entry(parent.clone())
                            .or_default()
                            .push(real_id.clone());
                        parent_of.insert(real_id, parent);
                    }
                }
                GraphOp::Reparent { id, new_parent_id } => {
                    if !existing_ids.contains(id) {
                        op_errors.push(OpError {
                            op_index: index,
                            message: format!("element #{id} does not exist"),
                        });
                        continue;
                    }
                    let resolved_parent = new_parent_id.as_ref().map(|p| resolve(&id_map, p));
                    if let Some(parent) = &resolved_parent {
                        if !existing_ids.contains(parent) && !id_map.values().any(|v| v == parent) {
                            op_errors.push(OpError {
                                op_index: index,
                                message: format!("new parent #{parent} does not exist"),
                            });
                            continue;
                        }
                        if is_descendant(&children_of, id, parent) {
                            op_errors.push(OpError {
                                op_index: index,
                                message: format!(
                                    "reparenting #{id} under #{parent} would create a \
                                     containment cycle"
                                ),
                            });
                            continue;
                        }
                    }
                    if let Some(old_parent) = parent_of.remove(id) {
                        if let Some(siblings) = children_of.get_mut(&old_parent) {
                            siblings.retain(|c| c != id);
                        }
                    }
                    if let Some(parent) = resolved_parent {
                        children_of
                            .entry(parent.clone())
                            .or_default()
                            .push(id.clone());
                        parent_of.insert(id.clone(), parent);
                    }
                }
            }
        }

        if !op_errors.is_empty() {
            return Ok(ApplyOpsOutcome::Rejected { errors: op_errors });
        }

        let mut txn = self
            .conn
            .start_txn()
            .await
            .context("starting text-model apply transaction")?;

        for op in ops {
            let result = match op {
                GraphOp::Rename { id, name } => {
                    txn.run(
                        query(
                            "MATCH (n:Element {id: $id, project_id: $project_id}) \
                             SET n.name = $name",
                        )
                        .param("id", id.clone())
                        .param("project_id", project_id.to_string())
                        .param("name", name.clone()),
                    )
                    .await
                }
                GraphOp::Create {
                    temp_id,
                    kind,
                    name,
                    parent_id,
                } => {
                    let real_id = id_map.get(temp_id).expect("resolved during validation");
                    let label = kind.as_label();
                    let create_cypher = format!(
                        "MERGE (n:Element:{label} {{id: $id, project_id: $project_id}}) \
                         SET n.name = $name, n.active = true, n.origin = 'Human'"
                    );
                    let mut res = txn
                        .run(
                            query(&create_cypher)
                                .param("id", real_id.clone())
                                .param("project_id", project_id.to_string())
                                .param("name", name.clone()),
                        )
                        .await;
                    if res.is_ok() {
                        if let Some(parent) = parent_id.as_ref().map(|p| resolve(&id_map, p)) {
                            res = txn
                                .run(
                                    query(
                                        "MATCH (a:Element {id: $parent, project_id: $project_id}), \
                                         (b:Element {id: $id, project_id: $project_id}) \
                                         MERGE (a)-[:CONTAINS]->(b)",
                                    )
                                    .param("parent", parent)
                                    .param("id", real_id.clone())
                                    .param("project_id", project_id.to_string()),
                                )
                                .await;
                        }
                    }
                    res
                }
                GraphOp::Reparent { id, new_parent_id } => {
                    let mut res = txn
                        .run(
                            query(
                                "MATCH (:Element {id: $id, project_id: $project_id})\
                                 <-[r:CONTAINS]-() DELETE r",
                            )
                            .param("id", id.clone())
                            .param("project_id", project_id.to_string()),
                        )
                        .await;
                    if res.is_ok() {
                        if let Some(parent) = new_parent_id.as_ref().map(|p| resolve(&id_map, p)) {
                            res = txn
                                .run(
                                    query(
                                        "MATCH (a:Element {id: $parent, project_id: $project_id}), \
                                         (b:Element {id: $id, project_id: $project_id}) \
                                         MERGE (a)-[:CONTAINS]->(b)",
                                    )
                                    .param("parent", parent)
                                    .param("id", id.clone())
                                    .param("project_id", project_id.to_string()),
                                )
                                .await;
                        }
                    }
                    res
                }
            };
            if let Err(err) = result {
                let _ = txn.rollback().await;
                return Err(err).context("applying text-model op batch");
            }
        }

        txn.commit()
            .await
            .context("committing text-model apply transaction")?;
        Ok(ApplyOpsOutcome::Applied { id_map })
    }
}

/// True if `candidate` is `node` itself or a descendant of it in `children_of` — used to reject
/// a `Reparent` that would make an element its own ancestor, walking only `node`'s current
/// subtree rather than the whole edge set.
fn is_descendant(
    children_of: &HashMap<ElementId, Vec<ElementId>>,
    node: &str,
    candidate: &str,
) -> bool {
    if node == candidate {
        return true;
    }
    let mut stack: Vec<&str> = children_of
        .get(node)
        .map(|c| c.iter().map(String::as_str).collect())
        .unwrap_or_default();
    let mut visited: HashSet<&str> = HashSet::new();
    while let Some(current) = stack.pop() {
        if current == candidate {
            return true;
        }
        if !visited.insert(current) {
            continue;
        }
        if let Some(children) = children_of.get(current) {
            stack.extend(children.iter().map(String::as_str));
        }
    }
    false
}

#[derive(Debug, Clone)]
pub struct OpError {
    pub op_index: usize,
    pub message: String,
}

#[derive(Debug, Clone)]
pub enum ApplyOpsOutcome {
    Applied { id_map: HashMap<String, ElementId> },
    Rejected { errors: Vec<OpError> },
}
