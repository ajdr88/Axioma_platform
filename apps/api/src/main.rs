//! `api` — the Axum REST surface (impl §2.1). Wires the polyglot persistence split (ADR-003):
//! Neo4j for topology, Postgres for element bodies, MinIO/S3 for blob pointers. The full REST
//! surface (impl §1) — Projects/Commits, query-budget enforcement, CEM/safety/mission endpoints —
//! is still follow-on work; this covers the AX-101/AX-105/AX-106 onboarding scope plus a first
//! real read/write path per store.

mod import;
mod store;

use std::collections::HashMap;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, patch, post},
    Json, Router,
};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use store::neo4j::ApplyOpsOutcome;
use store::{Neo4jStore, ObjectStore, PostgresStore};
use sysml_core::{Edge, EdgeKind, Element, ElementBody, NodeKind, Origin, ValidationError};
use sysml_textual::GraphOp;

#[derive(Clone)]
struct AppState {
    neo4j: Neo4jStore,
    postgres: PostgresStore,
    objects: ObjectStore,
    prometheus_handle: PrometheusHandle,
}

/// Wraps any error as a 500 by default, logging the full chain server-side. A `ValidationError`
/// anywhere in the chain (containment cycle, kind conflict) is a client mistake, not a server
/// fault, and downgrades to 400 with that message. Handlers needing a different status still
/// (e.g. 404) build their own `Response` instead of using `?` with this type.
#[derive(Debug)]
struct ApiError(anyhow::Error);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        if let Some(validation_err) = self.0.downcast_ref::<ValidationError>() {
            return (StatusCode::BAD_REQUEST, validation_err.to_string()).into_response();
        }
        if let Some(bad_request) = self.0.downcast_ref::<import::BadRequest>() {
            return (StatusCode::BAD_REQUEST, bad_request.0.clone()).into_response();
        }
        tracing::error!(error = ?self.0, "request failed");
        (StatusCode::INTERNAL_SERVER_ERROR, self.0.to_string()).into_response()
    }
}

impl<E: Into<anyhow::Error>> From<E> for ApiError {
    fn from(err: E) -> Self {
        ApiError(err.into())
    }
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load a local `.env` if present (see .env.example) — a no-op if it's absent, which is the
    // expected case outside local dev (real envs supply these vars directly).
    dotenvy::dotenv().ok();

    // Structured logging baseline (NFR-OPS-01). TODO: add an OTLP exporter (tracing-opentelemetry)
    // once there's a collector to point at in the local/dev stack.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let prometheus_handle = PrometheusBuilder::new()
        .install_recorder()
        .expect("failed to install Prometheus recorder");

    let neo4j = Neo4jStore::connect(
        &env_or("NEO4J_URI", "bolt://localhost:7687"),
        &env_or("NEO4J_USER", "neo4j"),
        &env_or("NEO4J_PASSWORD", "axioma-dev"),
    )
    .await?;

    let postgres = PostgresStore::connect(&env_or(
        "DATABASE_URL",
        "postgres://axioma:axioma-dev@localhost:5433/axioma",
    ))
    .await?;

    let objects = ObjectStore::connect(
        &env_or("S3_ENDPOINT", "http://localhost:9000"),
        &env_or("S3_ACCESS_KEY", "axioma"),
        &env_or("S3_SECRET_KEY", "axioma-dev"),
        &env_or("S3_BUCKET", "axioma-geometry"),
    )
    .await?;

    let state = AppState {
        neo4j,
        postgres,
        objects,
        prometheus_handle,
    };

    seed_turbofan_ref(&state).await?;

    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/metrics", get(metrics))
        .route("/api/v0/elements", get(list_elements).post(create_element))
        .route("/api/v0/elements/:id", patch(rename_element))
        .route("/api/v0/elements/:id/active", patch(set_element_active))
        .route("/api/v0/elements/:id/origin", patch(set_element_origin))
        .route(
            "/api/v0/elements/:id/body",
            get(get_element_body).put(update_element_body),
        )
        .route(
            "/api/v0/elements/:id/position",
            patch(update_element_position),
        )
        .route(
            "/api/v0/contains",
            get(list_contains_edges)
                .post(create_contains_edge)
                .delete(delete_contains_edge),
        )
        .route(
            "/api/v0/edges",
            get(list_edges).post(create_edge).delete(delete_edge),
        )
        .route("/api/v0/positions", get(list_positions))
        .route("/api/v0/text-model/apply", post(apply_text_model))
        .route("/import/sysml-v2", post(import::sysml_v2::import_sysml_v2))
        .route("/import/reqif", post(import::reqif::import_reqif))
        .with_state(state)
        .layer(tower_http::trace::TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080")
        .await
        .expect("failed to bind 0.0.0.0:8080");

    tracing::info!("api listening on http://0.0.0.0:8080");
    axum::serve(listener, app).await.expect("server error");
    Ok(())
}

/// Liveness — the process is up. Never depends on downstream services.
async fn healthz() -> impl IntoResponse {
    StatusCode::OK
}

/// Readiness — pings all three stores; 200 only if every one responds.
async fn readyz(State(state): State<AppState>) -> impl IntoResponse {
    let mut failed = Vec::new();
    if state.neo4j.ping().await.is_err() {
        failed.push("neo4j");
    }
    if state.postgres.ping().await.is_err() {
        failed.push("postgres");
    }
    if state.objects.ping().await.is_err() {
        failed.push("objects");
    }

    if failed.is_empty() {
        (
            StatusCode::OK,
            Json(serde_json::json!({ "status": "ready" })),
        )
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "status": "not ready", "failed": failed })),
        )
    }
}

/// Prometheus exposition format (NFR-OPS-01).
async fn metrics(State(state): State<AppState>) -> impl IntoResponse {
    state.prometheus_handle.render()
}

/// Stand-in for the real `GET /projects/{id}/commits/{id}/elements` endpoint (impl §1.1) — lists
/// every element from the topology store (Neo4j).
async fn list_elements(State(state): State<AppState>) -> Result<Json<Vec<Element>>, ApiError> {
    Ok(Json(state.neo4j.list_elements().await?))
}

/// Lists every `Contains` edge — this project's stand-in for a real traceability endpoint (impl
/// §1.1 doesn't name this one specifically; `/api/v0/elements` is the same kind of stand-in).
/// The frontend canvas needs both this and `list_elements` to draw the graph.
async fn list_contains_edges(State(state): State<AppState>) -> Result<Json<Vec<Edge>>, ApiError> {
    Ok(Json(state.neo4j.contains_edges().await?))
}

/// First real use of the document-store side of the split (NFR-DATA-02): the element's body
/// (rationale, large/structured properties) lives in Postgres, never in the graph.
async fn get_element_body(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    match state.postgres.get_body(&id).await? {
        Some(body) => Ok((StatusCode::OK, Json(body)).into_response()),
        None => Ok((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("no body for element {id}") })),
        )
            .into_response()),
    }
}

#[derive(Debug, serde::Deserialize)]
struct CreateElementRequest {
    name: String,
    /// Defaults to `Structure` when omitted — preserves the existing canvas "+ Add Node" button
    /// behavior untouched for callers that don't care about kind.
    #[serde(default)]
    kind: Option<NodeKind>,
}

/// Canvas "Add Node"/"Add Hazard"/"Add Control" (Edit Mode) — `active: true`; the server
/// generates the id, so there's no kind-conflict risk (it's always fresh).
async fn create_element(
    State(state): State<AppState>,
    Json(payload): Json<CreateElementRequest>,
) -> Result<Json<Element>, ApiError> {
    if payload.name.trim().is_empty() {
        return Err(import::BadRequest("name must not be empty".to_string()).into());
    }
    let element = Element {
        id: uuid::Uuid::new_v4().to_string(),
        kind: payload.kind.unwrap_or(NodeKind::Structure),
        name: payload.name,
        active: true,
        origin: Origin::Human,
    };
    state.neo4j.upsert_element(&element).await?;
    Ok(Json(element))
}

#[derive(Debug, serde::Deserialize)]
struct RenameRequest {
    name: String,
}

/// Canvas inline rename (Edit Mode) — preserves the element's existing `kind`/`active`.
async fn rename_element(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<RenameRequest>,
) -> Result<Response, ApiError> {
    if payload.name.trim().is_empty() {
        return Err(import::BadRequest("name must not be empty".to_string()).into());
    }
    let Some(existing) = state.neo4j.get_element(&id).await? else {
        return Ok((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("no element {id}") })),
        )
            .into_response());
    };
    let updated = Element {
        name: payload.name,
        ..existing
    };
    state.neo4j.rename_element(&id, &updated.name).await?;
    Ok(Json(updated).into_response())
}

#[derive(Debug, serde::Deserialize)]
struct SetActiveRequest {
    active: bool,
}

/// Canvas deactivate/reactivate (Edit Mode). Deactivating keeps every bit of the element's data —
/// it only marks it as excluded from *future* system-optimization loops (Mode B, not built yet;
/// see `sysml_core::Element::active`'s doc comment). Nothing filters by this yet.
async fn set_element_active(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<SetActiveRequest>,
) -> Result<StatusCode, ApiError> {
    state.neo4j.set_active(&id, payload.active).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, serde::Deserialize)]
struct SetOriginRequest {
    origin: Origin,
}

/// FR-CORE-08 provenance scaffolding (T-P1.2-06): "mark as ai-suggested via the API." Mirrors
/// `set_element_active` exactly — never touches `name`/`active`.
async fn set_element_origin(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<SetOriginRequest>,
) -> Result<StatusCode, ApiError> {
    state.neo4j.set_origin(&id, payload.origin).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, serde::Deserialize)]
struct CreateContainsRequest {
    parent: String,
    child: String,
}

/// Canvas drag-to-connect (Edit Mode) — goes through the same validated
/// `Neo4jStore::create_edge` the batch importers use (containment-acyclicity, FR-CORE-05).
async fn create_contains_edge(
    State(state): State<AppState>,
    Json(payload): Json<CreateContainsRequest>,
) -> Result<Json<Edge>, ApiError> {
    let edge = Edge {
        source: payload.parent,
        target: payload.child,
        kind: EdgeKind::Contains,
    };
    state.neo4j.create_edge(&edge).await?;
    Ok(Json(edge))
}

/// Canvas disconnect (Edit Mode) — removes a `Contains` edge. No validation gate needed;
/// removing an edge can only heal a cycle/conflict, never create one.
async fn delete_contains_edge(
    State(state): State<AppState>,
    Json(payload): Json<CreateContainsRequest>,
) -> Result<StatusCode, ApiError> {
    let edge = Edge {
        source: payload.parent,
        target: payload.child,
        kind: EdgeKind::Contains,
    };
    state.neo4j.delete_edge(&edge).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, serde::Deserialize)]
struct EdgeKindQuery {
    kind: EdgeKind,
}

#[derive(Debug, serde::Deserialize)]
struct CreateEdgeRequest {
    source: String,
    target: String,
    kind: EdgeKind,
}

/// Generic edge listing for every kind besides `Contains` (which keeps its own
/// `/api/v0/contains` route) — e.g. the Hazard/Risk panel's `Causes`/`MitigatedBy` edges.
async fn list_edges(
    State(state): State<AppState>,
    Query(params): Query<EdgeKindQuery>,
) -> Result<Json<Vec<Edge>>, ApiError> {
    Ok(Json(state.neo4j.edges_of_kind(params.kind).await?))
}

/// Generic edge creation — goes through the same validated `Neo4jStore::create_edge` as
/// `create_contains_edge` (dangling-endpoint rejection, endpoint type-legality, and
/// containment-acyclicity when `kind` is `Contains`).
async fn create_edge(
    State(state): State<AppState>,
    Json(payload): Json<CreateEdgeRequest>,
) -> Result<Json<Edge>, ApiError> {
    let edge = Edge {
        source: payload.source,
        target: payload.target,
        kind: payload.kind,
    };
    state.neo4j.create_edge(&edge).await?;
    Ok(Json(edge))
}

/// Generic edge removal — no validation gate needed, same reasoning as `delete_contains_edge`.
async fn delete_edge(
    State(state): State<AppState>,
    Json(payload): Json<CreateEdgeRequest>,
) -> Result<StatusCode, ApiError> {
    let edge = Edge {
        source: payload.source,
        target: payload.target,
        kind: payload.kind,
    };
    state.neo4j.delete_edge(&edge).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, serde::Deserialize)]
struct PositionRequest {
    x: f64,
    y: f64,
}

/// Canvas drag persistence (Edit Mode) — Postgres only, never touches Neo4j or the element's
/// body/rationale (NFR-DATA-01: position is UI metadata, not topology).
async fn update_element_position(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<PositionRequest>,
) -> Result<StatusCode, ApiError> {
    state
        .postgres
        .upsert_position(&id, payload.x, payload.y)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PositionEntry {
    element_id: String,
    x: f64,
    y: f64,
}

async fn list_positions(
    State(state): State<AppState>,
) -> Result<Json<Vec<PositionEntry>>, ApiError> {
    let positions = state
        .postgres
        .list_positions()
        .await?
        .into_iter()
        .map(|(element_id, x, y)| PositionEntry { element_id, x, y })
        .collect();
    Ok(Json(positions))
}

#[derive(Debug, serde::Deserialize)]
struct UpdateBodyRequest {
    rationale: Option<String>,
    properties: serde_json::Value,
}

/// Canvas properties-inspector save (Edit Mode) — the write side of `get_element_body`.
async fn update_element_body(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<UpdateBodyRequest>,
) -> Result<StatusCode, ApiError> {
    state
        .postgres
        .upsert_body(&ElementBody {
            element_id: id,
            rationale: payload.rationale,
            properties: payload.properties,
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, serde::Deserialize)]
struct ApplyTextModelRequest {
    ops: Vec<GraphOp>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ApplyTextModelResponse {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    id_map: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    errors: Option<Vec<OpErrorResponse>>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct OpErrorResponse {
    op_index: usize,
    message: String,
}

/// FR-CORE-02 / T-P1.2-01: applies a batch of `GraphOp`s (rename/create/reparent, computed by the
/// LSP server's text↔diagram diff, see `sysml-textual`) as one atomic transaction — either every
/// op commits, or none do. `ok: false` with `errors` is a normal, expected response (invalid
/// edits are common while typing), not a server fault — only a genuine I/O/store failure becomes
/// a `500` via `ApiError`.
async fn apply_text_model(
    State(state): State<AppState>,
    Json(payload): Json<ApplyTextModelRequest>,
) -> Result<Json<ApplyTextModelResponse>, ApiError> {
    match state.neo4j.apply_graph_ops(&payload.ops).await? {
        ApplyOpsOutcome::Applied { id_map } => Ok(Json(ApplyTextModelResponse {
            ok: true,
            id_map: Some(id_map),
            errors: None,
        })),
        ApplyOpsOutcome::Rejected { errors } => Ok(Json(ApplyTextModelResponse {
            ok: false,
            id_map: None,
            errors: Some(
                errors
                    .into_iter()
                    .map(|e| OpErrorResponse {
                        op_index: e.op_index,
                        message: e.message,
                    })
                    .collect(),
            ),
        })),
    }
}

/// Seeds `Turbofan-Ref`'s P1.1 structural fixture (test spec §0) across all three stores:
/// `Engine` composed of the five reference subsystems in Neo4j; `REQ-THRUST` with a 20 KB
/// rationale body in Postgres (mirrors T-P1.1-04's setup); a placeholder geometry blob for
/// `TurbineHpLp`, with only its pointer recorded — never the bytes (NFR-DATA-02).
async fn seed_turbofan_ref(state: &AppState) -> anyhow::Result<()> {
    let engine = Element {
        id: "Engine".to_string(),
        kind: NodeKind::Structure,
        name: "Engine".to_string(),
        active: true,
        origin: Origin::Human,
    };
    state.neo4j.upsert_element(&engine).await?;

    let subsystems = [
        ("FanLpCompression", "Fan & LP Compression"),
        ("CoreHpCompressor", "Core (HP) Compressor"),
        ("Combustor", "Combustor"),
        ("TurbineHpLp", "Turbine (HP & LP)"),
        ("ControlFadecEec", "Control (FADEC/EEC)"),
    ];

    for (id, name) in subsystems {
        state
            .neo4j
            .upsert_element(&Element {
                id: id.to_string(),
                kind: NodeKind::Structure,
                name: name.to_string(),
                active: true,
                origin: Origin::Human,
            })
            .await?;
        state
            .neo4j
            .create_edge(&Edge {
                source: "Engine".to_string(),
                target: id.to_string(),
                kind: EdgeKind::Contains,
            })
            .await?;
    }

    let req_thrust = Element {
        id: "REQ-THRUST".to_string(),
        kind: NodeKind::Requirement,
        name: "Engine shall provide >= 30,000 lbf takeoff thrust".to_string(),
        active: true,
        origin: Origin::Human,
    };
    state.neo4j.upsert_element(&req_thrust).await?;
    state
        .postgres
        .upsert_body(&ElementBody {
            element_id: "REQ-THRUST".to_string(),
            // Stand-in for a real rationale document — sized to match test spec T-P1.1-04's
            // 20 KB fixture, proving large text never lands in Neo4j.
            rationale: Some("x".repeat(20_000)),
            properties: serde_json::json!({}),
        })
        .await?;

    let pointer = state
        .objects
        .put_placeholder(
            "turbine/casing-placeholder.txt",
            b"placeholder geometry blob".to_vec(),
        )
        .await?;
    state
        .postgres
        .upsert_body(&ElementBody {
            element_id: "TurbineHpLp".to_string(),
            rationale: None,
            properties: serde_json::json!({ "geometryPointer": pointer }),
        })
        .await?;

    Ok(())
}

/// Integration tests against the real docker-compose stack (`docker compose up -d`) — `#[ignore]`d
/// so `cargo test --workspace` stays green in CI without a live Neo4j/Postgres/MinIO. Run with
/// `cargo test -p api -- --ignored`.
#[cfg(test)]
mod tests {
    use super::*;

    async fn connect_test_stores() -> (Neo4jStore, PostgresStore, ObjectStore) {
        let neo4j = Neo4jStore::connect(
            &env_or("NEO4J_URI", "bolt://localhost:7687"),
            &env_or("NEO4J_USER", "neo4j"),
            &env_or("NEO4J_PASSWORD", "axioma-dev"),
        )
        .await
        .expect("connect to Neo4j — is `docker compose up -d` running?");

        let postgres = PostgresStore::connect(&env_or(
            "DATABASE_URL",
            "postgres://axioma:axioma-dev@localhost:5433/axioma",
        ))
        .await
        .expect("connect to Postgres — is `docker compose up -d` running?");

        let objects = ObjectStore::connect(
            &env_or("S3_ENDPOINT", "http://localhost:9000"),
            &env_or("S3_ACCESS_KEY", "axioma"),
            &env_or("S3_SECRET_KEY", "axioma-dev"),
            &env_or("S3_BUCKET", "axioma-geometry"),
        )
        .await
        .expect("connect to object store — is `docker compose up -d` running?");

        (neo4j, postgres, objects)
    }

    /// The Prometheus recorder is a process-global singleton — installing it more than once
    /// panics, so every test that needs a full `AppState` shares one handle via `OnceLock`
    /// instead of calling `install_recorder()` itself.
    fn shared_prometheus_handle() -> PrometheusHandle {
        static HANDLE: std::sync::OnceLock<PrometheusHandle> = std::sync::OnceLock::new();
        HANDLE
            .get_or_init(|| {
                PrometheusBuilder::new()
                    .install_recorder()
                    .expect("install Prometheus recorder once")
            })
            .clone()
    }

    async fn test_app_state() -> AppState {
        let (neo4j, postgres, objects) = connect_test_stores().await;
        AppState {
            neo4j,
            postgres,
            objects,
            prometheus_handle: shared_prometheus_handle(),
        }
    }

    #[tokio::test]
    #[ignore = "requires `docker compose up -d`"]
    async fn readyz_ok_when_stores_healthy() {
        let (neo4j, postgres, objects) = connect_test_stores().await;

        assert!(neo4j.ping().await.is_ok());
        assert!(postgres.ping().await.is_ok());
        assert!(objects.ping().await.is_ok());
    }

    /// T-P1.1-03(a) against the real store: making an element a containment child of its own
    /// child must be rejected.
    #[tokio::test]
    #[ignore = "requires `docker compose up -d`"]
    async fn containment_cycle_rejected_against_neo4j() {
        let (neo4j, _postgres, _objects) = connect_test_stores().await;

        let engine = Element {
            id: "IntegrationTestEngine".to_string(),
            kind: NodeKind::Structure,
            name: "Integration Test Engine".to_string(),
            active: true,
            origin: Origin::Human,
        };
        let turbine = Element {
            id: "IntegrationTestTurbine".to_string(),
            kind: NodeKind::Structure,
            name: "Integration Test Turbine".to_string(),
            active: true,
            origin: Origin::Human,
        };
        neo4j.upsert_element(&engine).await.unwrap();
        neo4j.upsert_element(&turbine).await.unwrap();

        neo4j
            .create_edge(&Edge {
                source: engine.id.clone(),
                target: turbine.id.clone(),
                kind: EdgeKind::Contains,
            })
            .await
            .expect("Engine contains Turbine should succeed");

        let result = neo4j
            .create_edge(&Edge {
                source: turbine.id.clone(),
                target: engine.id.clone(),
                kind: EdgeKind::Contains,
            })
            .await;

        assert!(result.is_err(), "the containment cycle should be rejected");
    }

    /// T-P1.1-02 against the real store: a `Satisfy` edge must target a Requirement, not a
    /// Block — Combustor -> Turbine (both Structures) is rejected, with no partial write.
    #[tokio::test]
    #[ignore = "requires `docker compose up -d`"]
    async fn satisfy_endpoint_rejected_against_neo4j() {
        let (neo4j, _postgres, _objects) = connect_test_stores().await;

        let combustor = Element {
            id: "IntegrationTestCombustor".to_string(),
            kind: NodeKind::Structure,
            name: "Integration Test Combustor".to_string(),
            active: true,
            origin: Origin::Human,
        };
        let turbine = Element {
            id: "IntegrationTestSatisfyTurbine".to_string(),
            kind: NodeKind::Structure,
            name: "Integration Test Turbine".to_string(),
            active: true,
            origin: Origin::Human,
        };
        neo4j.upsert_element(&combustor).await.unwrap();
        neo4j.upsert_element(&turbine).await.unwrap();

        let result = neo4j
            .create_edge(&Edge {
                source: combustor.id.clone(),
                target: turbine.id.clone(),
                kind: EdgeKind::Satisfy,
            })
            .await;
        assert!(
            result.is_err(),
            "Satisfy targeting a Block, not a Requirement, should be rejected"
        );

        let satisfy_edges = neo4j.edges_of_kind(EdgeKind::Satisfy).await.unwrap();
        assert!(
            !satisfy_edges
                .iter()
                .any(|e| e.source == combustor.id && e.target == turbine.id),
            "no partial write: the rejected Satisfy edge must not appear on read-back"
        );
    }

    /// An edge referencing a nonexistent element id is rejected outright — it must not silently
    /// no-op (the underlying Cypher `MATCH` simply matches zero rows and would otherwise report
    /// success with nothing written).
    #[tokio::test]
    #[ignore = "requires `docker compose up -d`"]
    async fn dangling_edge_rejected_against_neo4j() {
        let (neo4j, _postgres, _objects) = connect_test_stores().await;

        let engine = Element {
            id: "IntegrationTestDanglingEngine".to_string(),
            kind: NodeKind::Structure,
            name: "Integration Test Engine".to_string(),
            active: true,
            origin: Origin::Human,
        };
        neo4j.upsert_element(&engine).await.unwrap();

        let result = neo4j
            .create_edge(&Edge {
                source: engine.id.clone(),
                target: "IntegrationTestDoesNotExist".to_string(),
                kind: EdgeKind::Contains,
            })
            .await;
        assert!(
            result.is_err(),
            "an edge to a nonexistent element must be rejected, not silently no-op"
        );

        let contains_edges = neo4j.contains_edges().await.unwrap();
        assert!(
            !contains_edges
                .iter()
                .any(|e| e.source == engine.id && e.target == "IntegrationTestDoesNotExist"),
            "no partial write: the rejected edge must not appear on read-back"
        );
    }

    /// FR-CORE-08 / T-P1.2-06: an element defaults to `Human` origin, `set_origin` marks it
    /// `AiSuggested`, and that sticks on read-back without touching `name`/`active`.
    #[tokio::test]
    #[ignore = "requires `docker compose up -d`"]
    async fn set_origin_persists_against_neo4j() {
        let (neo4j, _postgres, _objects) = connect_test_stores().await;

        let element = Element {
            id: "IntegrationTestOriginBlock".to_string(),
            kind: NodeKind::Structure,
            name: "Integration Test Origin Block".to_string(),
            active: true,
            origin: Origin::Human,
        };
        neo4j.upsert_element(&element).await.unwrap();
        assert_eq!(
            neo4j
                .get_element(&element.id)
                .await
                .unwrap()
                .unwrap()
                .origin,
            Origin::Human
        );

        neo4j
            .set_origin(&element.id, Origin::AiSuggested)
            .await
            .unwrap();
        let reloaded = neo4j.get_element(&element.id).await.unwrap().unwrap();
        assert_eq!(reloaded.origin, Origin::AiSuggested);
        assert_eq!(reloaded.name, element.name);
        assert!(reloaded.active);
    }

    /// T-P1.1-04: a large body lives in Postgres, a blob is referenced from the object store by
    /// pointer, and neither ever lands in Neo4j.
    #[tokio::test]
    #[ignore = "requires `docker compose up -d`"]
    async fn polyglot_split_body_not_in_graph() {
        let (neo4j, postgres, objects) = connect_test_stores().await;
        let element_id = "IntegrationTestReq";
        let rationale = "x".repeat(20_000);

        postgres
            .upsert_body(&ElementBody {
                element_id: element_id.to_string(),
                rationale: Some(rationale),
                properties: serde_json::json!({}),
            })
            .await
            .unwrap();

        let pointer = objects
            .put_placeholder("integration-test/blob.txt", b"placeholder".to_vec())
            .await
            .unwrap();
        assert!(
            pointer.starts_with("s3://"),
            "pointer should reference the object store, not inline bytes"
        );

        let stored = postgres
            .get_body(element_id)
            .await
            .unwrap()
            .expect("body should exist in Postgres");
        assert_eq!(
            stored["rationale"].as_str().unwrap().len(),
            20_000,
            "the large body should be readable back from Postgres"
        );

        // Neo4j's upsert_element only ever sets `id`/`name`/`active`/`origin` — there is no path
        // for the 20 KB rationale to leak into the graph, but assert it directly rather than by
        // construction.
        neo4j
            .upsert_element(&Element {
                id: element_id.to_string(),
                kind: NodeKind::Requirement,
                name: "Integration test requirement".to_string(),
                active: true,
                origin: Origin::Human,
            })
            .await
            .unwrap();
        let elements = neo4j.list_elements().await.unwrap();
        let node = elements
            .iter()
            .find(|e| e.id == element_id)
            .expect("node should exist in Neo4j");
        assert!(
            node.name.len() < 1_000,
            "Neo4j node should never carry the large body"
        );
    }

    /// FR-CORE-07 / T-P1.1-06: importing the SysML v2 fixture reproduces `Turbofan-Ref`'s
    /// six-element, five-edge structural hierarchy.
    #[tokio::test]
    #[ignore = "requires `docker compose up -d`"]
    async fn import_sysml_v2_reproduces_turbofan_hierarchy() {
        let state = test_app_state().await;
        let fixture = include_str!("../tests/fixtures/sample-sysml-v2.json");
        let payload: import::sysml_v2::SysmlV2ImportRequest =
            serde_json::from_str(fixture).unwrap();

        let response = import::sysml_v2::import_sysml_v2(State(state.clone()), Json(payload))
            .await
            .expect("import should succeed")
            .0;
        assert_eq!(response.elements_imported, 6);
        assert_eq!(response.edges_imported, 5);

        let elements = state.neo4j.list_elements().await.unwrap();
        for id in [
            "Engine",
            "FanLpCompression",
            "CoreHpCompressor",
            "Combustor",
            "TurbineHpLp",
            "ControlFadecEec",
        ] {
            assert!(
                elements.iter().any(|e| e.id == id),
                "expected imported element {id}"
            );
        }

        let contains = state.neo4j.contains_edges().await.unwrap();
        for child in [
            "FanLpCompression",
            "CoreHpCompressor",
            "Combustor",
            "TurbineHpLp",
            "ControlFadecEec",
        ] {
            assert!(
                contains
                    .iter()
                    .any(|e| e.source == "Engine" && e.target == child),
                "expected Engine to contain {child}"
            );
        }
    }

    /// A batch that cycles against itself is rejected wholesale — none of its elements get
    /// written, even the ones that would otherwise be perfectly valid.
    #[tokio::test]
    #[ignore = "requires `docker compose up -d`"]
    async fn import_sysml_v2_rejects_cycle_with_no_partial_write() {
        let state = test_app_state().await;
        let fixture = include_str!("../tests/fixtures/sample-sysml-v2-cycle.json");
        let payload: import::sysml_v2::SysmlV2ImportRequest =
            serde_json::from_str(fixture).unwrap();

        let result = import::sysml_v2::import_sysml_v2(State(state.clone()), Json(payload)).await;
        assert!(result.is_err(), "the self-cyclic batch should be rejected");

        let elements = state.neo4j.list_elements().await.unwrap();
        for id in ["CycleTestA", "CycleTestB", "CycleTestValidLeaf"] {
            assert!(
                !elements.iter().any(|e| e.id == id),
                "{id} should not have been written — the whole batch was rejected"
            );
        }
    }

    /// FR-CORE-07 / T-P1.1-06: importing the ReqIF fixture reproduces each requirement's text
    /// (as the Postgres rationale) and its other attributes (as properties).
    #[tokio::test]
    #[ignore = "requires `docker compose up -d`"]
    async fn import_reqif_reproduces_requirement_text_and_attributes() {
        let state = test_app_state().await;
        let fixture = include_str!("../tests/fixtures/sample.reqif");

        let response = import::reqif::import_reqif(State(state.clone()), fixture.to_string())
            .await
            .expect("import should succeed")
            .0;
        assert_eq!(response.requirements_imported, 3);

        let body = state
            .postgres
            .get_body("REQ-THRUST-IMPORTED")
            .await
            .unwrap()
            .expect("body should exist");
        assert_eq!(
            body["rationale"].as_str().unwrap(),
            "Engine shall provide at least 30,000 lbf takeoff thrust."
        );
        assert_eq!(
            body["properties"]["VerificationMethod"].as_str().unwrap(),
            "Test"
        );

        let elements = state.neo4j.list_elements().await.unwrap();
        assert!(elements
            .iter()
            .any(|e| e.id == "REQ-THRUST-IMPORTED" && e.kind == NodeKind::Requirement));
    }

    /// alf-lite's "precise error, never silent partial" convention, applied to ReqIF: a
    /// `SPEC-OBJECT` missing `IDENTIFIER` is rejected, nothing written.
    #[tokio::test]
    #[ignore = "requires `docker compose up -d`"]
    async fn import_reqif_rejects_missing_identifier() {
        let state = test_app_state().await;
        let fixture = include_str!("../tests/fixtures/sample-malformed.reqif");

        let result = import::reqif::import_reqif(State(state), fixture.to_string()).await;
        assert!(result.is_err(), "missing IDENTIFIER should be rejected");
    }

    /// Re-importing an id already used by a different `NodeKind` is rejected (FR-CORE-05's
    /// type-legal-identity rule, `sysml_core::check_kind_conflict`).
    #[tokio::test]
    #[ignore = "requires `docker compose up -d`"]
    async fn import_rejects_kind_conflict() {
        let state = test_app_state().await;

        state
            .neo4j
            .upsert_element(&Element {
                id: "KindConflictTest".to_string(),
                kind: NodeKind::Structure,
                name: "Originally a Structure".to_string(),
                active: true,
                origin: Origin::Human,
            })
            .await
            .unwrap();

        let payload = import::sysml_v2::SysmlV2ImportRequest {
            elements: vec![Element {
                id: "KindConflictTest".to_string(),
                kind: NodeKind::Requirement,
                name: "Now claimed as a Requirement".to_string(),
                active: true,
                origin: Origin::Human,
            }],
            contains: vec![],
        };

        let result = import::sysml_v2::import_sysml_v2(State(state), Json(payload)).await;
        assert!(result.is_err(), "the kind conflict should be rejected");
    }
}
