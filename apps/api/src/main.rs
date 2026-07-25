//! `api` — the Axum REST surface (impl §2.1). Wires the polyglot persistence split (ADR-003):
//! Neo4j for topology, Postgres for element bodies, MinIO/S3 for blob pointers. The full REST
//! surface (impl §1) — Projects/Commits, query-budget enforcement, CEM/safety/mission endpoints —
//! is still follow-on work; this covers the AX-101/AX-105/AX-106 onboarding scope plus a first
//! real read/write path per store.

mod import;
mod store;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use store::{Neo4jStore, ObjectStore, PostgresStore};
use sysml_core::{Edge, EdgeKind, Element, ElementBody, NodeKind, ValidationError};

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
        .route("/api/v0/elements", get(list_elements))
        .route("/api/v0/elements/:id/body", get(get_element_body))
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

/// Seeds `Turbofan-Ref`'s P1.1 structural fixture (test spec §0) across all three stores:
/// `Engine` composed of the five reference subsystems in Neo4j; `REQ-THRUST` with a 20 KB
/// rationale body in Postgres (mirrors T-P1.1-04's setup); a placeholder geometry blob for
/// `TurbineHpLp`, with only its pointer recorded — never the bytes (NFR-DATA-02).
async fn seed_turbofan_ref(state: &AppState) -> anyhow::Result<()> {
    let engine = Element {
        id: "Engine".to_string(),
        kind: NodeKind::Structure,
        name: "Engine".to_string(),
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
        };
        let turbine = Element {
            id: "IntegrationTestTurbine".to_string(),
            kind: NodeKind::Structure,
            name: "Integration Test Turbine".to_string(),
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

        // Neo4j's upsert_element only ever sets `id`/`name` — there is no path for the 20 KB
        // rationale to leak into the graph, but assert it directly rather than by construction.
        neo4j
            .upsert_element(&Element {
                id: element_id.to_string(),
                kind: NodeKind::Requirement,
                name: "Integration test requirement".to_string(),
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
            })
            .await
            .unwrap();

        let payload = import::sysml_v2::SysmlV2ImportRequest {
            elements: vec![Element {
                id: "KindConflictTest".to_string(),
                kind: NodeKind::Requirement,
                name: "Now claimed as a Requirement".to_string(),
            }],
            contains: vec![],
        };

        let result = import::sysml_v2::import_sysml_v2(State(state), Json(payload)).await;
        assert!(result.is_err(), "the kind conflict should be rejected");
    }
}
