//! docs/IMPLEMENTATION_KICKOFF.md Phase 5 (FR-INFO-01) — creates a real `:InformationElement`,
//! setting its `abstractionLevel` body property (FR-INFO-03, reqs v5 §5.10 — a property on the
//! element, "not three separate node labels") in the same call/commit, rather than the generic
//! `POST /elements` path's two round trips (create, then a separate `PUT .../body`). A genuine,
//! small convenience/atomicity win, not a redundant wrapper around an existing endpoint.
//!
//! `/information/data-types` (impl v5 §1.4) is deliberately not built — no `:DataType` `NodeKind`
//! exists (reqs v5 §5.10 never names one; a Data Type/Enumeration is itself just an
//! `:InformationElement`), so this same endpoint already covers it.

use axum::{
    extract::{Path, State},
    http::HeaderMap,
    Json,
};
use serde::Deserialize;
use sysml_core::{Element, ElementBody, NodeKind, Origin};

use crate::{import, record_commit, ApiError, AppState, DiffEntry};

#[derive(Debug, Clone, Copy, Deserialize)]
pub(crate) enum AbstractionLevel {
    Conceptual,
    Logical,
    Physical,
}

impl AbstractionLevel {
    fn as_str(&self) -> &'static str {
        match self {
            AbstractionLevel::Conceptual => "Conceptual",
            AbstractionLevel::Logical => "Logical",
            AbstractionLevel::Physical => "Physical",
        }
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct CreateInformationElementRequest {
    pub(crate) name: String,
    #[serde(rename = "abstractionLevel")]
    pub(crate) abstraction_level: AbstractionLevel,
}

/// `POST /api/v0/projects/:projectId/information/elements` (FR-INFO-01/03).
pub(crate) async fn create_information_element(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project_id): Path<String>,
    Json(payload): Json<CreateInformationElementRequest>,
) -> Result<Json<Element>, ApiError> {
    if payload.name.trim().is_empty() {
        return Err(import::BadRequest("name must not be empty".to_string()).into());
    }
    let element = Element {
        id: uuid::Uuid::new_v4().to_string(),
        kind: NodeKind::InformationElement,
        name: payload.name,
        active: true,
        origin: Origin::Human,
    };
    state.neo4j.upsert_element(&project_id, &element).await?;
    state
        .postgres
        .upsert_body(
            &project_id,
            &ElementBody {
                element_id: element.id.clone(),
                rationale: None,
                properties: serde_json::json!({
                    "abstractionLevel": payload.abstraction_level.as_str(),
                }),
            },
        )
        .await?;
    record_commit(
        &state,
        &project_id,
        &state.auth.resolve_actor(&headers)?,
        "Create information element",
        vec![
            DiffEntry::ElementCreated {
                element_id: element.id.clone(),
                kind: element.kind,
                name: element.name.clone(),
            },
            DiffEntry::PropertyChanged {
                element_id: element.id.clone(),
                property: "abstractionLevel".to_string(),
                old: serde_json::Value::Null,
                new: serde_json::json!(payload.abstraction_level.as_str()),
            },
        ],
    )
    .await?;
    Ok(Json(element))
}
