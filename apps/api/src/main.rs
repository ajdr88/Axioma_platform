//! `api` — the Axum REST surface (impl §2.1). This is the AX-101/AX-106 onboarding seed: liveness/
//! readiness/metrics endpoints, structured tracing, and a first proof that `sysml-core` is wired
//! into the workspace. The full REST surface (impl §1) — Projects/Commits/Elements, query-budget
//! enforcement, CEM/safety/mission endpoints — is follow-on work.

use std::sync::{Arc, RwLock};

use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::get, Json, Router};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use sysml_core::{Edge, EdgeKind, Element, Graph, NodeKind};

#[derive(Clone)]
struct AppState {
    graph: Arc<RwLock<Graph>>,
    prometheus_handle: PrometheusHandle,
}

#[tokio::main]
async fn main() {
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

    let state = AppState {
        graph: Arc::new(RwLock::new(seed_turbofan_ref())),
        prometheus_handle,
    };

    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/metrics", get(metrics))
        .route("/api/v0/elements", get(list_elements))
        .with_state(state)
        .layer(tower_http::trace::TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080")
        .await
        .expect("failed to bind 0.0.0.0:8080");

    tracing::info!("api listening on http://0.0.0.0:8080");
    axum::serve(listener, app).await.expect("server error");
}

/// Liveness — the process is up. Never depends on downstream services.
async fn healthz() -> impl IntoResponse {
    StatusCode::OK
}

/// Readiness — the process can serve traffic. Once real persistence (Neo4j/Postgres/MinIO) is
/// wired, this should check those connections; for now it checks the in-memory graph is seeded.
async fn readyz(State(state): State<AppState>) -> impl IntoResponse {
    let graph = state.graph.read().expect("graph lock poisoned");
    if graph.elements().next().is_some() {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}

/// Prometheus exposition format (NFR-OPS-01).
async fn metrics(State(state): State<AppState>) -> impl IntoResponse {
    state.prometheus_handle.render()
}

/// Temporary stand-in for the real `GET /projects/{id}/commits/{id}/elements` endpoint (impl
/// §1.1) — lists the in-memory seed graph so the frontend has something to point at during early
/// development.
async fn list_elements(State(state): State<AppState>) -> impl IntoResponse {
    let graph = state.graph.read().expect("graph lock poisoned");
    let elements: Vec<Element> = graph.elements().cloned().collect();
    Json(elements)
}

/// Seeds the in-memory graph with `Turbofan-Ref`'s P1.1 structural fixture (test spec §0):
/// `Engine` composed of the five reference subsystems.
fn seed_turbofan_ref() -> Graph {
    let mut graph = Graph::new();

    let engine = Element {
        id: "Engine".to_string(),
        kind: NodeKind::Structure,
        name: "Engine".to_string(),
    };
    graph.add_element(engine);

    let subsystems = [
        ("FanLpCompression", "Fan & LP Compression"),
        ("CoreHpCompressor", "Core (HP) Compressor"),
        ("Combustor", "Combustor"),
        ("TurbineHpLp", "Turbine (HP & LP)"),
        ("ControlFadecEec", "Control (FADEC/EEC)"),
    ];

    for (id, name) in subsystems {
        graph.add_element(Element {
            id: id.to_string(),
            kind: NodeKind::Structure,
            name: name.to_string(),
        });
        graph
            .add_edge(Edge {
                source: "Engine".to_string(),
                target: id.to_string(),
                kind: EdgeKind::Contains,
            })
            .expect("seed containment edges are acyclic by construction");
    }

    graph
}
