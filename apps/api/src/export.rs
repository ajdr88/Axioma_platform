//! docs/IMPLEMENTATION_KICKOFF.md Phase 5 (FR-EXPORT-01..04). Reqs v5 §5.12 frames all four as
//! reusing existing mechanisms — none of the three referenced ones (FR-SAFE-05's export as a
//! generalizable template engine, a "Generic Table view" to share scope/column parameters with,
//! a working geometry-pointer read path) actually existed before this pass. Scoped down honestly:
//!
//! - **FR-EXPORT-04 (attachments)**: a real read+write pointer mechanism, built for the first
//!   time — `ObjectStore::get_object` (the missing read half) plus a new `attachments` metadata
//!   table.
//! - **FR-EXPORT-02 (tabular)**: CSV only (no XLSX crate is justified for this pass), scoped over
//!   either a `NodeKind` filter or a frozen `/collections/dynamic` result's membership — the
//!   latter reuses Phase 5's own Collections feature as the "saved scope" concept, rather than
//!   inventing a second one to stand in for the nonexistent Generic Table view.
//! - **FR-EXPORT-03 (report)**: a real, minimal template mechanism — `build_risk_register`
//!   (`traceability.rs`) is now shared by both the existing JSON risk-register endpoint and this
//!   module's HTML report path, per reqs v5's own "no new parallel pipeline" instruction. Exactly
//!   one template is registered (`"risk-register"`); anything else is a precise 400, not a silent
//!   fallback.
//! - **FR-EXPORT-01 (diagram image)** is entirely client-side (`apps/web/src/app/page.tsx`) — no
//!   server-side code in this module. The "headless-render path for full-diagram export at any
//!   size" reqs also names is real, separate new capability, not attempted this pass.

use std::collections::HashSet;

use axum::{
    extract::{Multipart, Path, Query, State},
    http::{header, HeaderValue, StatusCode},
    response::{Html, IntoResponse, Response},
    Json,
};
use sysml_core::{EdgeKind, NodeKind};

use crate::{import, store::postgres::AttachmentMeta, traceability, ApiError, AppState};

/// `POST /api/v0/projects/:projectId/elements/:elementId/attachments` (FR-EXPORT-04) — a single
/// file per request, matching the one-attachment-at-a-time upload UX every comparable tool uses.
/// No Neo4j write, no `record_commit` — an attachment references an existing element by id; it
/// doesn't create or modify any graph node/edge (reqs v5 §5.12's own "none of them write to the
/// graph" framing, read as "the graph" meaning the topology store specifically).
pub(crate) async fn create_attachment(
    State(state): State<AppState>,
    Path((project_id, element_id)): Path<(String, String)>,
    mut multipart: Multipart,
) -> Result<Json<AttachmentMeta>, ApiError> {
    if state
        .neo4j
        .get_element(&project_id, &element_id)
        .await?
        .is_none()
    {
        return Err(import::BadRequest(format!("no such element {element_id}")).into());
    }
    let Some(field) = multipart.next_field().await.map_err(anyhow::Error::from)? else {
        return Err(import::BadRequest(
            "multipart request must contain one file field".to_string(),
        )
        .into());
    };
    let file_name = field.file_name().unwrap_or("attachment").to_string();
    let content_type = field
        .content_type()
        .unwrap_or("application/octet-stream")
        .to_string();
    let bytes = field.bytes().await.map_err(anyhow::Error::from)?;
    let id = uuid::Uuid::new_v4().to_string();
    // Namespaced by project+attachment id, matching `seed_turbofan_ref`'s own
    // `"turbine/casing-placeholder.txt"`-style key convention -- collision-proof without needing
    // to sanitize `file_name` for path-safety, since it's never itself part of the key.
    let object_key = format!("attachments/{project_id}/{id}");
    state
        .objects
        .put_object(&object_key, bytes.to_vec())
        .await?;
    let size_bytes = bytes.len() as i64;
    state
        .postgres
        .save_attachment(
            &project_id,
            &id,
            &element_id,
            &file_name,
            &content_type,
            &object_key,
            size_bytes,
        )
        .await?;
    Ok(Json(AttachmentMeta {
        id,
        file_name,
        content_type,
        size_bytes,
    }))
}

/// `GET /api/v0/projects/:projectId/elements/:elementId/attachments` (FR-EXPORT-04) — metadata
/// only; never `object_key` (see `AttachmentMeta`'s own doc comment).
pub(crate) async fn list_attachments(
    State(state): State<AppState>,
    Path((project_id, element_id)): Path<(String, String)>,
) -> Result<Json<Vec<AttachmentMeta>>, ApiError> {
    Ok(Json(
        state
            .postgres
            .list_attachments(&project_id, &element_id)
            .await?,
    ))
}

/// `GET /api/v0/projects/:projectId/attachments/:id` (FR-EXPORT-04) — streams the real bytes back,
/// the read half `ObjectStore` never had before this pass.
pub(crate) async fn download_attachment(
    State(state): State<AppState>,
    Path((project_id, id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let Some(record) = state.postgres.get_attachment(&project_id, &id).await? else {
        return Ok((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("no attachment {id}") })),
        )
            .into_response());
    };
    let bytes = state.objects.get_object(&record.object_key).await?;
    let mut response = bytes.into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&record.content_type).map_err(anyhow::Error::from)?,
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{}\"", record.file_name))
            .map_err(anyhow::Error::from)?,
    );
    Ok(response)
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct ExportTableQuery {
    pub(crate) kind: Option<NodeKind>,
    #[serde(rename = "collectionId")]
    pub(crate) collection_id: Option<String>,
}

/// `GET /api/v0/projects/:projectId/export/table` (FR-EXPORT-02) — CSV, scoped by `?kind=X` (every
/// element of one `NodeKind`) or `?collectionId=Y` (a frozen `/collections/{id}/freeze` result's
/// real membership, reusing Phase 5's own Collections feature rather than inventing a second
/// "scope" concept to stand in for the Generic Table view that doesn't exist). Fixed baseline
/// columns (id/name/kind/origin/active) — no column-selection parameter, since there's no Generic
/// Table view's own column set to mirror.
pub(crate) async fn export_table(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    Query(params): Query<ExportTableQuery>,
) -> Result<Response, ApiError> {
    let all_elements = state.neo4j.list_elements(&project_id).await?;
    let elements: Vec<&sysml_core::Element> = if let Some(collection_id) = &params.collection_id {
        let member_edges = state
            .neo4j
            .edges_of_kind(&project_id, EdgeKind::Member)
            .await?;
        let member_ids: HashSet<&str> = member_edges
            .iter()
            .filter(|e| &e.source == collection_id)
            .map(|e| e.target.as_str())
            .collect();
        all_elements
            .iter()
            .filter(|e| member_ids.contains(e.id.as_str()))
            .collect()
    } else if let Some(kind) = params.kind {
        all_elements.iter().filter(|e| e.kind == kind).collect()
    } else {
        return Err(
            import::BadRequest("must specify either ?kind= or ?collectionId=".to_string()).into(),
        );
    };

    let mut csv = String::from("id,name,kind,origin,active\n");
    for e in elements {
        csv.push_str(&csv_row(&[
            &e.id,
            &e.name,
            e.kind.as_label(),
            e.origin.as_str(),
            if e.active { "true" } else { "false" },
        ]));
    }
    let mut response = csv.into_response();
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static("text/csv"));
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"export-{project_id}.csv\""))
            .map_err(anyhow::Error::from)?,
    );
    Ok(response)
}

/// RFC4180 field escaping — quotes a field and doubles embedded quotes only when the field
/// actually contains a comma/quote/newline. Hand-rolled rather than a new crate dependency: the
/// algorithm is small and well-known, and no other export format in this pass needs a heavier
/// writer (XLSX is deliberately not built this pass — see this module's own doc comment).
fn csv_field(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn csv_row(fields: &[&str]) -> String {
    let mut row = fields
        .iter()
        .map(|f| csv_field(f))
        .collect::<Vec<_>>()
        .join(",");
    row.push('\n');
    row
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct ExportReportRequest {
    #[serde(rename = "templateId")]
    pub(crate) template_id: String,
    /// Accepted but unused by the one template registered so far (`"risk-register"` is
    /// project-scoped, not element-scoped) — kept in the request shape reqs v5 §1.4 already
    /// specifies, ready for a future template that actually needs it.
    #[serde(rename = "scopeElementId", default)]
    #[allow(dead_code)]
    pub(crate) scope_element_id: Option<String>,
}

/// `POST /api/v0/projects/:projectId/export/report` (FR-EXPORT-03) — the real, minimal template
/// mechanism this module's doc comment describes. `"risk-register"` is the only registered
/// template; anything else is a precise 400 naming what's missing, never a silent fallback (same
/// "reject, don't guess" discipline `sysml-core`'s own validation layer already follows).
pub(crate) async fn export_report(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    Json(payload): Json<ExportReportRequest>,
) -> Result<Response, ApiError> {
    match payload.template_id.as_str() {
        "risk-register" => {
            let register = traceability::build_risk_register(&state, &project_id).await?;
            let html = render_risk_register_html(&register);
            let mut response = Html(html).into_response();
            response.headers_mut().insert(
                header::CONTENT_DISPOSITION,
                HeaderValue::from_str(&format!(
                    "attachment; filename=\"risk-register-{project_id}.html\""
                ))
                .map_err(anyhow::Error::from)?,
            );
            Ok(response)
        }
        other => Err(import::BadRequest(format!(
            "unknown report template {other:?} -- only \"risk-register\" is registered so far"
        ))
        .into()),
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn render_risk_register_html(register: &traceability::RiskRegister) -> String {
    let mut html = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Risk Register — {}</title></head><body>\
         <h1>Risk Register — {} ({})</h1>\
         <table border=\"1\" cellpadding=\"4\" cellspacing=\"0\">\
         <tr><th>Hazard</th><th>Description</th><th>Causing Structure</th><th>Severity</th>\
         <th>Likelihood</th><th>Risk Index</th><th>Residual Risk</th><th>Status</th></tr>",
        html_escape(&register.project_id),
        html_escape(&register.project_id),
        register.format,
    );
    for entry in &register.entries {
        html.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            html_escape(&entry.hazard_id),
            html_escape(&entry.description),
            entry
                .causing_structure
                .as_deref()
                .map(html_escape)
                .unwrap_or_default(),
            html_escape(&entry.severity_classification),
            html_escape(&entry.likelihood),
            entry.risk_index,
            entry.residual_risk,
            entry.status,
        ));
    }
    html.push_str("</table></body></html>");
    html
}
