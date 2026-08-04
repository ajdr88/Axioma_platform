//! `POST /import/sysml-v2` — imports a structural hierarchy.
//!
//! **Wire format is our own minimal JSON subset**, standing in for the real OMG SysML v2 API
//! interchange format (FR-CORE-01's "100% compliance" is separately-scoped, large work):
//!
//! ```json
//! { "elements": [{"id": "Engine", "kind": "Structure", "name": "Engine"}],
//!   "contains": [{"parent": "Engine", "child": "FanLpCompression"}] }
//! ```
//!
//! Validation (kind-conflict, containment-acyclicity) and the atomic write both happen inside
//! [`crate::store::Neo4jStore::import_elements_and_edges`] — nothing here re-implements that.

use axum::{
    extract::{Path, State},
    Json,
};
use serde::{Deserialize, Serialize};
use sysml_core::Element;

use crate::{ApiError, AppState};

#[derive(Debug, Deserialize)]
pub struct ContainsPair {
    pub parent: String,
    pub child: String,
}

#[derive(Debug, Deserialize)]
pub struct SysmlV2ImportRequest {
    pub elements: Vec<Element>,
    #[serde(default)]
    pub contains: Vec<ContainsPair>,
}

#[derive(Debug, Serialize)]
pub struct ImportSummary {
    pub elements_imported: usize,
    pub edges_imported: usize,
}

pub async fn import_sysml_v2(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    Json(payload): Json<SysmlV2ImportRequest>,
) -> Result<Json<ImportSummary>, ApiError> {
    let contains: Vec<(String, String)> = payload
        .contains
        .into_iter()
        .map(|pair| (pair.parent, pair.child))
        .collect();

    state
        .neo4j
        .import_elements_and_edges(&project_id, &payload.elements, &contains)
        .await?;

    Ok(Json(ImportSummary {
        elements_imported: payload.elements.len(),
        edges_imported: contains.len(),
    }))
}
