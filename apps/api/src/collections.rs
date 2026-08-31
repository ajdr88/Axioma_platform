//! docs/IMPLEMENTATION_KICKOFF.md Phase 5 (FR-CORE-10/11) — Dynamic Element Collections.
//! `/collections/dynamic` stores a query **definition** only — on-demand evaluation, no
//! scheduled/on-write-triggered re-evaluation policy (that needs a job scheduler, Product-2-scoped
//! via `scheduler`, not built here) — a deliberate scope-down of FR-CORE-10's "re-evaluation
//! policy," not silently dropped. `/collections/{id}/freeze` actually **runs** the stored
//! definition — reusing `traceability::run_traversal`, the same budgeted traversal engine P1.3
//! already built (not a second traversal implementation) — and materializes the result as a real
//! `:Collection` + `Member` edges (FR-CORE-11), never the acyclic `Contains` edge (NFR-REL-02):
//! a Collection legitimately references elements from anywhere in the graph, including elements
//! that already have a different container.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use sysml_core::{Edge, EdgeKind, Element, NodeKind, Origin};

use crate::{import, record_commit, traceability, ApiError, AppState, DiffEntry};

#[derive(Debug, Deserialize)]
pub(crate) struct SaveDynamicCollectionRequest {
    pub(crate) name: String,
    #[serde(rename = "rootId")]
    pub(crate) root_id: String,
    pub(crate) depth: u32,
    #[serde(rename = "maxFanout")]
    pub(crate) max_fanout: u32,
    pub(crate) direction: traceability::Direction,
}

#[derive(Debug, Serialize)]
pub(crate) struct SaveDynamicCollectionResponse {
    pub(crate) id: String,
}

/// `POST /api/v0/projects/:projectId/collections/dynamic` (FR-CORE-10). Rejected at save time if
/// `depth`/`maxFanout` exceed this server's caps (NFR-PERF-04's own wording — "an unbounded
/// Dynamic Query is rejected at save time, not just at run time," impl v5 §1.4).
pub(crate) async fn save_dynamic_collection(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    Json(payload): Json<SaveDynamicCollectionRequest>,
) -> Result<Json<SaveDynamicCollectionResponse>, ApiError> {
    if payload.name.trim().is_empty() {
        return Err(import::BadRequest("name must not be empty".to_string()).into());
    }
    traceability::validate_budget(payload.depth, payload.max_fanout)?;
    if state
        .neo4j
        .get_element(&project_id, &payload.root_id)
        .await?
        .is_none()
    {
        return Err(import::BadRequest(format!("no such root element {}", payload.root_id)).into());
    }
    let id = uuid::Uuid::new_v4().to_string();
    let definition = serde_json::json!({
        "rootId": payload.root_id,
        "depth": payload.depth,
        "maxFanout": payload.max_fanout,
        "direction": payload.direction,
    });
    state
        .postgres
        .save_dynamic_collection(&project_id, &id, &payload.name, &definition)
        .await?;
    Ok(Json(SaveDynamicCollectionResponse { id }))
}

#[derive(Debug, Deserialize)]
struct StoredDefinition {
    #[serde(rename = "rootId")]
    root_id: String,
    depth: u32,
    #[serde(rename = "maxFanout")]
    max_fanout: u32,
    direction: traceability::Direction,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct FreezeCollectionResponse {
    #[serde(rename = "collectionId")]
    pub(crate) collection_id: String,
    #[serde(rename = "memberIds")]
    pub(crate) member_ids: Vec<String>,
}

/// `POST /api/v0/projects/:projectId/collections/:id/freeze` (FR-CORE-11) — converts a saved
/// Dynamic Query into a frozen Static Snapshot Collection by actually re-running it.
pub(crate) async fn freeze_collection(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project_id, id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let Some((name, definition)) = state
        .postgres
        .get_dynamic_collection(&project_id, &id)
        .await?
    else {
        return Ok((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("no dynamic collection {id}") })),
        )
            .into_response());
    };
    let stored: StoredDefinition = serde_json::from_value(definition)
        .context_bad_request("stored Dynamic Query definition is malformed")?;

    let traversal = traceability::run_traversal(
        &state,
        &project_id,
        &stored.root_id,
        stored.depth,
        stored.max_fanout,
        stored.direction,
    )
    .await?;

    let collection_id = uuid::Uuid::new_v4().to_string();
    let collection = Element {
        id: collection_id.clone(),
        kind: NodeKind::Collection,
        name,
        active: true,
        origin: Origin::Human,
    };
    state.neo4j.upsert_element(&project_id, &collection).await?;
    let mut diff_entries = vec![DiffEntry::ElementCreated {
        element_id: collection.id.clone(),
        kind: collection.kind,
        name: collection.name.clone(),
    }];

    let member_ids: Vec<String> = traversal.visited.into_keys().collect();
    for member_id in &member_ids {
        // `Member`'s real endpoint rule (packages/sysml-core/src/lib.rs): source must be a
        // `Collection`. Deliberately `Member`, not `Contains` — a Collection legitimately
        // references elements that already have a different container (NFR-REL-02).
        state
            .neo4j
            .create_edge(
                &project_id,
                &Edge {
                    source: collection_id.clone(),
                    target: member_id.clone(),
                    kind: EdgeKind::Member,
                    metadata: None,
                },
            )
            .await?;
        diff_entries.push(DiffEntry::EdgeCreated {
            source: collection_id.clone(),
            target: member_id.clone(),
            kind: EdgeKind::Member,
        });
    }

    record_commit(
        &state,
        &project_id,
        &state.auth.resolve_actor(&headers)?,
        "Freeze dynamic collection",
        diff_entries,
    )
    .await?;

    Ok(Json(FreezeCollectionResponse {
        collection_id,
        member_ids,
    })
    .into_response())
}

/// Maps a `serde_json` parse failure to a 400, not a 500 — malformed stored data (should never
/// happen given `save_dynamic_collection` is the only writer) is still a client-visible condition
/// worth a precise message, not an opaque server error.
trait ContextBadRequest<T> {
    fn context_bad_request(self, message: &str) -> Result<T, ApiError>;
}

impl<T> ContextBadRequest<T> for Result<T, serde_json::Error> {
    fn context_bad_request(self, message: &str) -> Result<T, ApiError> {
        self.map_err(|e| import::BadRequest(format!("{message}: {e}")).into())
    }
}
