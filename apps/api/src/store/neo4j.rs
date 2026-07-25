//! Topology store (ADR-003 / NFR-DATA-01): Neo4j holds elements and relationships only — no
//! bodies, no blobs. See `super::postgres` and `super::objects` for those.

use std::collections::HashMap;

use anyhow::{Context, Result};
use neo4rs::{query, Graph as Neo4jConn};
use sysml_core::{Edge, EdgeKind, Element, ElementId, NodeKind, ValidationError};

#[derive(Clone)]
pub struct Neo4jStore {
    conn: Neo4jConn,
}

impl Neo4jStore {
    pub async fn connect(uri: &str, user: &str, password: &str) -> Result<Self> {
        let conn = Neo4jConn::new(uri, user, password)
            .await
            .with_context(|| format!("connecting to Neo4j at {uri}"))?;
        Ok(Self { conn })
    }

    pub async fn ping(&self) -> Result<()> {
        self.conn
            .run(query("RETURN 1"))
            .await
            .context("Neo4j ping failed")
    }

    /// `MERGE`s an element node by id. Only `id`/`kind`(label)/`name` are stored here — bodies
    /// and blobs live elsewhere (NFR-DATA-01).
    pub async fn upsert_element(&self, element: &Element) -> Result<()> {
        let label = element.kind.as_label();
        let cypher = format!("MERGE (n:{label} {{id: $id}}) SET n.name = $name");
        self.conn
            .run(
                query(&cypher)
                    .param("id", element.id.clone())
                    .param("name", element.name.clone()),
            )
            .await
            .with_context(|| format!("upserting element {}", element.id))
    }

    pub async fn list_elements(&self) -> Result<Vec<Element>> {
        let mut result = self
            .conn
            .execute(query(
                "MATCH (n) RETURN n.id AS id, n.name AS name, labels(n) AS labels",
            ))
            .await
            .context("listing elements")?;

        let mut elements = Vec::new();
        while let Some(row) = result.next().await.context("reading element row")? {
            let id: String = row.get("id").context("missing id")?;
            let name: String = row.get("name").context("missing name")?;
            let labels: Vec<String> = row.get("labels").context("missing labels")?;
            let kind = labels
                .iter()
                .find_map(|l| NodeKind::from_label(l))
                .unwrap_or(NodeKind::Element);
            elements.push(Element { id, kind, name });
        }
        Ok(elements)
    }

    /// Every existing element id mapped to its current `NodeKind` — used to detect a
    /// kind-conflict (re-importing an id under a different kind) before writing anything.
    pub async fn element_kinds(&self) -> Result<HashMap<ElementId, NodeKind>> {
        Ok(self
            .list_elements()
            .await?
            .into_iter()
            .map(|el| (el.id, el.kind))
            .collect())
    }

    /// All existing `Contains` edges — enough to run
    /// [`sysml_core::would_create_containment_cycle`] without hydrating a full graph.
    pub async fn contains_edges(&self) -> Result<Vec<Edge>> {
        let mut result = self
            .conn
            .execute(query(
                "MATCH (a)-[:CONTAINS]->(b) RETURN a.id AS source, b.id AS target",
            ))
            .await
            .context("listing containment edges")?;

        let mut edges = Vec::new();
        while let Some(row) = result
            .next()
            .await
            .context("reading containment edge row")?
        {
            edges.push(Edge {
                source: row.get("source").context("missing source")?,
                target: row.get("target").context("missing target")?,
                kind: EdgeKind::Contains,
            });
        }
        Ok(edges)
    }

    /// Creates a relationship between two already-`upsert_element`ed nodes, matched by id.
    /// `Contains` edges are validated against the current containment hierarchy first
    /// (FR-CORE-05, NFR-REL-02) — every other edge kind is legal to cycle (§5.7/NFR-REL-02).
    pub async fn create_edge(&self, edge: &Edge) -> Result<()> {
        if edge.kind.is_acyclicity_scoped() {
            let existing = self.contains_edges().await?;
            if sysml_core::would_create_containment_cycle(&existing, &edge.source, &edge.target) {
                return Err(ValidationError::ContainmentCycle {
                    parent: edge.source.clone(),
                    child: edge.target.clone(),
                }
                .into());
            }
        }

        let rel_type = edge.kind.as_rel_type();
        let cypher =
            format!("MATCH (a {{id: $source}}), (b {{id: $target}}) MERGE (a)-[:{rel_type}]->(b)");
        self.conn
            .run(
                query(&cypher)
                    .param("source", edge.source.clone())
                    .param("target", edge.target.clone()),
            )
            .await
            .with_context(|| {
                format!(
                    "creating {rel_type} edge {} -> {}",
                    edge.source, edge.target
                )
            })
    }

    /// Validates and writes a batch of elements + `Contains` edges atomically (import handlers,
    /// FR-CORE-07): kind-conflict and containment-cycle checks run first, against existing state
    /// *and* against the batch itself (so a batch that only cycles internally is also caught) —
    /// nothing is written unless every check passes. The writes then run in a single Neo4j
    /// transaction, explicitly rolled back on any failure, so a mid-batch error can't leave a
    /// partial import (T-P1.1-02's "no partial write" standard, extended to a whole batch).
    pub async fn import_elements_and_edges(
        &self,
        elements: &[Element],
        contains: &[(ElementId, ElementId)],
    ) -> Result<()> {
        let existing_kinds = self.element_kinds().await?;
        for element in elements {
            sysml_core::check_kind_conflict(existing_kinds.get(&element.id).copied(), element)?;
        }

        let mut working_edges = self.contains_edges().await?;
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
            let cypher = format!("MERGE (n:{label} {{id: $id}}) SET n.name = $name");
            if let Err(err) = txn
                .run(
                    query(&cypher)
                        .param("id", element.id.clone())
                        .param("name", element.name.clone()),
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
                "MATCH (a {{id: $source}}), (b {{id: $target}}) MERGE (a)-[:{rel_type}]->(b)"
            );
            if let Err(err) = txn
                .run(
                    query(&cypher)
                        .param("source", parent.clone())
                        .param("target", child.clone()),
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
}
