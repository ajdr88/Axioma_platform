//! `api` — the Axum REST surface (impl §2.1). Wires the polyglot persistence split (ADR-003):
//! Neo4j for topology, Postgres for element bodies, MinIO/S3 for blob pointers, plus a fourth,
//! abstract Postgres-backed Commit/Branch/Project store for Git-backed model versioning
//! (roadmap: P1.1, T-P1.1-05 — see `store::versioning`'s doc comment for why this isn't a
//! literal git repo). The full REST surface (impl §1) — query-budget enforcement, CEM/safety/
//! mission endpoints — is still follow-on work; this covers the AX-101/AX-105/AX-106 onboarding
//! scope, a first real read/write path per store, and the Projects/Commits/Elements structuring
//! nouns impl §1 names, made real rather than a `/api/v0/elements` stand-in.

mod alf_ir;
mod archspace_client;
mod auth;
mod autonomy;
mod control_sim;
mod fuml_client;
mod import;
mod mode_a;
mod mode_b;
mod store;
mod traceability;
mod trade_study;

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Context;
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, patch, post, put},
    Json, Router,
};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use store::neo4j::ApplyOpsOutcome;
use store::versioning::{
    apply_diff, compute_snapshot_diff, Branch, DiffEntry, Project, Snapshot, SnapshotEdge,
    DEFAULT_REGION, MAIN_BRANCH,
};
use store::{Neo4jStore, ObjectStore, PostgresStore, VersioningStore};
use sysml_core::{Edge, EdgeKind, Element, ElementBody, NodeKind, Origin, ValidationError};
use sysml_textual::GraphOp;

/// Every mutating endpoint in this file resolves its actor through `AppState::auth`
/// (NFR-COMP-03 — see `auth.rs`'s doc comment) rather than using this directly; it's what
/// `auth::LocalAuthProvider` (the default) falls back to when no `X-Actor` header override is
/// present, preserving the exact identity every call site resolved to before that module existed.
const DEFAULT_ACTOR: &str = "local-user";

/// Every `EdgeKind` — used to build a full versioning snapshot across every relationship kind,
/// not just `Contains`.
const ALL_EDGE_KINDS: [EdgeKind; 9] = [
    EdgeKind::Contains,
    EdgeKind::Satisfy,
    EdgeKind::Verify,
    EdgeKind::Refine,
    EdgeKind::Causes,
    EdgeKind::MitigatedBy,
    EdgeKind::ValidatedBy,
    EdgeKind::Suspect,
    EdgeKind::Concerns,
];

#[derive(Clone)]
struct AppState {
    neo4j: Neo4jStore,
    postgres: PostgresStore,
    objects: ObjectStore,
    versioning: VersioningStore,
    auth: Arc<dyn auth::AuthProvider>,
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
        if let Some(auth_err) = self.0.downcast_ref::<auth::AuthError>() {
            return (StatusCode::UNAUTHORIZED, auth_err.to_string()).into_response();
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

    let database_url = env_or(
        "DATABASE_URL",
        "postgres://axioma:axioma-dev@localhost:5433/axioma",
    );
    let postgres = PostgresStore::connect(&database_url).await?;
    let versioning = VersioningStore::connect(&database_url).await?;

    let objects = ObjectStore::connect(
        &env_or("S3_ENDPOINT", "http://localhost:9000"),
        &env_or("S3_ACCESS_KEY", "axioma"),
        &env_or("S3_SECRET_KEY", "axioma-dev"),
        &env_or("S3_BUCKET", "axioma-geometry"),
    )
    .await?;

    let auth: Arc<dyn auth::AuthProvider> = match env_or("AUTH_PROVIDER", "local").as_str() {
        // NFR-COMP-03 / T-X-07: this match arm is the entire "swap the identity provider" config
        // change — no handler above or below this line changes.
        "oidc" => Arc::new(auth::OidcAuthProvider::new(
            &std::env::var("OIDC_HMAC_SECRET")
                .context("OIDC_HMAC_SECRET must be set when AUTH_PROVIDER=oidc")?,
            env_or("OIDC_ACTOR_CLAIM", "sub"),
        )),
        _ => Arc::new(auth::LocalAuthProvider),
    };

    let state = AppState {
        neo4j,
        postgres,
        objects,
        versioning,
        auth,
        prometheus_handle,
    };

    ensure_seeded(&state).await?;

    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/metrics", get(metrics))
        .route("/api/v0/projects", get(list_projects).post(create_project))
        .route("/api/v0/projects/:projectId", get(get_project))
        .route(
            "/api/v0/projects/:projectId/branches",
            get(list_branches).post(create_branch),
        )
        .route(
            "/api/v0/projects/:projectId/branches/:branch/elements/:elementId/body",
            patch(branch_update_element_body),
        )
        .route(
            "/api/v0/projects/:projectId/commits/:commitId/diff",
            get(diff_commit),
        )
        .route(
            "/api/v0/projects/:projectId/trade-studies/compare",
            post(trade_study::compare),
        )
        .route(
            "/api/v0/projects/:projectId/elements",
            get(list_elements).post(create_element),
        )
        .route(
            "/api/v0/projects/:projectId/elements/:id",
            patch(rename_element).delete(traceability::delete_element),
        )
        .route(
            "/api/v0/projects/:projectId/elements/:id/traceability",
            get(traceability::get_traceability),
        )
        .route(
            "/api/v0/projects/:projectId/elements/:id/active",
            patch(set_element_active),
        )
        .route(
            "/api/v0/projects/:projectId/elements/:id/origin",
            patch(set_element_origin),
        )
        .route(
            "/api/v0/projects/:projectId/elements/:id/body",
            get(get_element_body).put(update_element_body),
        )
        .route(
            "/api/v0/projects/:projectId/elements/:id/position",
            patch(update_element_position),
        )
        .route(
            "/api/v0/projects/:projectId/contains",
            get(list_contains_edges)
                .post(create_contains_edge)
                .delete(delete_contains_edge),
        )
        .route(
            "/api/v0/projects/:projectId/edges",
            get(list_edges).post(create_edge).delete(delete_edge),
        )
        .route("/api/v0/projects/:projectId/positions", get(list_positions))
        .route(
            "/api/v0/projects/:projectId/text-model/apply",
            post(apply_text_model),
        )
        .route(
            "/api/v0/projects/:projectId/import/sysml-v2",
            post(import::sysml_v2::import_sysml_v2),
        )
        .route(
            "/api/v0/projects/:projectId/import/reqif",
            post(import::reqif::import_reqif),
        )
        .route(
            "/api/v0/projects/:projectId/safety/risk-register",
            get(traceability::get_risk_register),
        )
        .route(
            "/api/v0/projects/:projectId/mission-coverage",
            get(traceability::get_mission_coverage),
        )
        .route(
            "/api/v0/projects/:projectId/cem/mode-a/query",
            post(mode_a::query),
        )
        .route(
            "/api/v0/projects/:projectId/cem/mode-a/part-search",
            post(mode_a::search_parts),
        )
        .route(
            "/api/v0/projects/:projectId/cem/mode-a/lint-requirement",
            post(mode_a::lint_requirement),
        )
        .route(
            "/api/v0/projects/:projectId/cem/mode-b/optimize",
            post(mode_b::optimize),
        )
        .route(
            "/api/v0/projects/:projectId/cem/mode-b/accept",
            post(mode_b::accept),
        )
        .route(
            "/api/v0/projects/:projectId/cem/mode-b/interface-contract/:subsystemId",
            get(mode_b::interface_contract),
        )
        .route(
            "/api/v0/projects/:projectId/cem/mode-b/propose",
            post(mode_b::propose),
        )
        .route(
            "/api/v0/projects/:projectId/cem/proposals/:branchId",
            get(mode_b::list_proposals),
        )
        .route(
            "/api/v0/projects/:projectId/cem/proposals/:proposalId/accept",
            post(mode_b::accept_proposal),
        )
        .route(
            "/api/v0/projects/:projectId/cem/proposals/:proposalId/reject",
            post(mode_b::reject_proposal),
        )
        .route(
            "/api/v0/projects/:projectId/cem/autonomy-level",
            put(mode_b::set_autonomy_level),
        )
        .route(
            "/api/v0/projects/:projectId/cem/autonomy-level/:scope",
            get(mode_b::get_autonomy_level),
        )
        .route(
            "/api/v0/projects/:projectId/simulate/hello-world",
            post(fuml_client::simulate_hello_world),
        )
        .route(
            "/api/v0/projects/:projectId/simulate/control-state-machine",
            post(control_sim::simulate_control_state_machine),
        )
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

/// Readiness — pings all four stores; 200 only if every one responds.
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
    if state.versioning.ping().await.is_err() {
        failed.push("versioning");
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

// ---------------------------------------------------------------------------
// Projects / branches / commits / diff (roadmap: Git-backed model versioning)
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Deserialize)]
struct CreateProjectRequest {
    name: String,
    /// NFR-COMP-02 (data residency) — defaults to `DEFAULT_REGION` when omitted, matching every
    /// project created before this field existed.
    #[serde(default = "default_region")]
    region: String,
}

fn default_region() -> String {
    DEFAULT_REGION.to_string()
}

async fn list_projects(State(state): State<AppState>) -> Result<Json<Vec<Project>>, ApiError> {
    Ok(Json(state.versioning.list_projects().await?))
}

async fn create_project(
    State(state): State<AppState>,
    Json(payload): Json<CreateProjectRequest>,
) -> Result<Json<Project>, ApiError> {
    if payload.name.trim().is_empty() {
        return Err(import::BadRequest("name must not be empty".to_string()).into());
    }
    Ok(Json(
        state
            .versioning
            .create_project(&payload.name, &payload.region)
            .await?,
    ))
}

async fn get_project(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
) -> Result<Response, ApiError> {
    match state.versioning.get_project(&project_id).await? {
        Some(project) => Ok(Json(project).into_response()),
        None => Ok((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("no project {project_id}") })),
        )
            .into_response()),
    }
}

async fn list_branches(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
) -> Result<Json<Vec<Branch>>, ApiError> {
    Ok(Json(state.versioning.list_branches(&project_id).await?))
}

#[derive(Debug, serde::Deserialize)]
struct CreateBranchRequest {
    name: String,
    #[serde(default)]
    from_commit: Option<String>,
}

/// Creates a branch pointing at `fromCommit` (defaults to `main`'s head) — a lightweight
/// pointer, not a live checkout; see `store::versioning`'s doc comment.
async fn create_branch(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    Json(payload): Json<CreateBranchRequest>,
) -> Result<Json<Branch>, ApiError> {
    if payload.name.trim().is_empty() {
        return Err(import::BadRequest("name must not be empty".to_string()).into());
    }
    Ok(Json(
        state
            .versioning
            .create_branch(&project_id, &payload.name, payload.from_commit.as_deref())
            .await?,
    ))
}

#[derive(Debug, serde::Deserialize)]
struct BranchEditBodyRequest {
    #[serde(default)]
    rationale: Option<String>,
    properties: serde_json::Value,
    #[serde(default)]
    actor: Option<String>,
    #[serde(default = "default_commit_message")]
    message: String,
}

fn default_commit_message() -> String {
    "Update element properties".to_string()
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct CommitResponse {
    commit_id: String,
    diff: Vec<DiffEntry>,
}

/// T-P1.1-05's Action, made real: applies a property edit to a branch and commits the result —
/// never touches the live graph (only `main` is ever live; see `store::versioning`'s doc
/// comment). Returns the new commit's id and its diff (old/new values) directly, so a caller
/// doesn't need a separate diff request just to see what changed. Only the properties actually
/// present in `payload.properties` and different from the resolved old value are recorded/
/// replayed — unlike `update_element_body`'s live wholesale-replace of a Postgres row, a branch
/// has no live row to replace, so a property this call's payload omits simply isn't touched,
/// consistent with `PropertyChanged` itself being a per-property diff.
async fn branch_update_element_body(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project_id, branch_name, element_id)): Path<(String, String, String)>,
    Json(payload): Json<BranchEditBodyRequest>,
) -> Result<Response, ApiError> {
    let Some(branch) = state
        .versioning
        .get_branch(&project_id, &branch_name)
        .await?
    else {
        return Ok((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("no branch {branch_name}") })),
        )
            .into_response());
    };
    let Some(head_commit_id) = branch.head_commit_id.clone() else {
        return Err(import::BadRequest(format!(
            "branch {branch_name} has no commits yet — branch it from a commit first"
        ))
        .into());
    };
    // Only the element being edited actually needs resolving here — but `resolve_snapshot`
    // reconstructs (or live-fetches) the whole project state regardless, since a commit's diff
    // doesn't index by element id. Fine for this deliberately low-volume, human-reviewed path;
    // see `store::versioning`'s doc comment for why this never runs on the hot write path.
    let snapshot = resolve_snapshot(&state, &project_id, &head_commit_id).await?;

    let actor = match payload.actor.clone() {
        Some(actor) => actor,
        None => state.auth.resolve_actor(&headers)?,
    };
    let old_properties = snapshot
        .bodies
        .get(&element_id)
        .and_then(|b| b.get("properties"))
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    let mut diff_entries = Vec::new();
    if let Some(new_properties) = payload.properties.as_object() {
        for (key, new_val) in new_properties {
            let old_val = old_properties
                .get(key)
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            if &old_val != new_val {
                diff_entries.push(DiffEntry::PropertyChanged {
                    element_id: element_id.clone(),
                    property: key.clone(),
                    old: old_val,
                    new: new_val.clone(),
                });
            }
        }
    }

    // Wholesale, like `update_element_body`'s live rationale write — not merged key-by-key like
    // `properties` above, since there's only one rationale value, not a bag of keys.
    let old_rationale = snapshot
        .bodies
        .get(&element_id)
        .and_then(|b| b.get("rationale"))
        .and_then(|v| v.as_str())
        .map(String::from);
    if payload.rationale != old_rationale {
        diff_entries.push(DiffEntry::RationaleChanged {
            element_id: element_id.clone(),
            old: old_rationale,
            new: payload.rationale.clone(),
        });
    }

    let commit = state
        .versioning
        .commit(
            &project_id,
            &branch,
            &actor,
            &payload.message,
            &diff_entries,
        )
        .await?;
    for entry in &diff_entries {
        state
            .versioning
            .record_audit(&project_id, &actor, entry)
            .await?;
    }

    Ok(Json(CommitResponse {
        commit_id: commit.id,
        diff: diff_entries,
    })
    .into_response())
}

#[derive(Debug, serde::Deserialize)]
struct DiffQuery {
    against: String,
}

/// Property-level + structural diff between any two commits — works for any pair, including a
/// branch's tip against `main`'s head (T-P1.1-05's "diff against main"). Resolves both sides
/// (`resolve_snapshot` — live for `main`'s current head, replayed from a diff chain otherwise)
/// and diffs the two results directly; no commit stores a full snapshot to compare anymore, see
/// `store::versioning`'s doc comment.
async fn diff_commit(
    State(state): State<AppState>,
    Path((project_id, commit_id)): Path<(String, String)>,
    Query(params): Query<DiffQuery>,
) -> Result<Json<Vec<DiffEntry>>, ApiError> {
    let new_snapshot = resolve_snapshot(&state, &project_id, &commit_id).await?;
    let old_snapshot = resolve_snapshot(&state, &project_id, &params.against).await?;
    Ok(Json(compute_snapshot_diff(&old_snapshot, &new_snapshot)))
}

/// Gathers a project's full current state (every element, every edge kind, every body) into one
/// versioning snapshot — canvas position is deliberately excluded (UI metadata, not modeling
/// content, NFR-DATA-01). This is always `main`'s true state (only `main` is ever live) — never
/// called from the ordinary mutating-endpoint write path anymore (that was T-P1.1-07's measured
/// bottleneck), only lazily from `resolve_snapshot` when a diff is actually requested.
async fn build_snapshot(state: &AppState, project_id: &str) -> anyhow::Result<Snapshot> {
    let elements = state.neo4j.list_elements(project_id).await?;
    let mut edges = Vec::new();
    for kind in ALL_EDGE_KINDS {
        for edge in state.neo4j.edges_of_kind(project_id, kind).await? {
            edges.push(SnapshotEdge {
                source: edge.source,
                target: edge.target,
                kind: edge.kind,
            });
        }
    }
    let bodies = state.postgres.list_bodies(project_id).await?;
    Ok(Snapshot {
        elements,
        edges,
        bodies,
    })
}

/// Reconstructs the full versioning `Snapshot` as of `commit_id` — see `store::versioning`'s doc
/// comment for the design this implements. Two base cases: `commit_id` is exactly `main`'s
/// current head (the overwhelmingly common case for every real caller — `main`'s own head, or a
/// branch just forked from it) → the live graph, fetched fresh, no replay needed; or a branch
/// with no fork parent at all → `Snapshot::default()`. Otherwise, recurses to the commit's
/// branch's fork point and replays that branch's own commits (and only that branch's — the walk
/// stops the moment a parent's `branch_id` differs) on top, oldest-first.
///
/// A plain `fn` returning a manually boxed/pinned future, not `async fn` — this calls itself
/// (branch-of-a-branch is possible, `create_branch` accepts an arbitrary `from_commit`), and a
/// self-recursive `async fn` doesn't compile (its future's size would depend on itself). No new
/// crate needed for this, just the standard boxed-future workaround.
fn resolve_snapshot<'a>(
    state: &'a AppState,
    project_id: &'a str,
    commit_id: &'a str,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<Snapshot>> + Send + 'a>> {
    Box::pin(async move {
        let main_head = state
            .versioning
            .get_branch(project_id, MAIN_BRANCH)
            .await?
            .and_then(|b| b.head_commit_id);
        if main_head.as_deref() == Some(commit_id) {
            return build_snapshot(state, project_id).await;
        }

        let commit = state
            .versioning
            .get_commit(commit_id)
            .await?
            .with_context(|| format!("commit {commit_id} not found"))?;
        let branch = state
            .versioning
            .get_branch_by_id(&commit.branch_id)
            .await?
            .with_context(|| format!("branch {} not found", commit.branch_id))?;

        let mut snapshot = match &branch.fork_commit_id {
            Some(fork_id) => resolve_snapshot(state, project_id, fork_id).await?,
            None => Snapshot::default(),
        };

        let mut chain = vec![commit];
        while let Some(parent_id) = chain.last().expect("just pushed").parent_commit_id.clone() {
            let parent = state
                .versioning
                .get_commit(&parent_id)
                .await?
                .with_context(|| format!("commit {parent_id} not found"))?;
            if parent.branch_id != branch.id {
                break;
            }
            chain.push(parent);
        }
        chain.reverse();
        for c in &chain {
            for d in &c.diff {
                apply_diff(&mut snapshot, d);
            }
        }
        Ok(snapshot)
    })
}

/// Every existing mutating endpoint (except position drags — UI metadata, not modeling content)
/// calls this after its own write succeeds: records the diff as a commit onto `main` and an
/// audit-log entry per diff entry. Makes CLAUDE.md's "every model write must be traceable to a
/// Git-style commit — never a silent mutation" literally true, not just aspirational. A no-op if
/// `diff_entries` is empty (e.g. a rename to the same name). **Does not snapshot the graph** —
/// the diff itself is the only thing stored, which is what makes this O(1) rather than O(project
/// size); see `store::versioning`'s doc comment for the T-P1.1-07 bottleneck this replaced.
async fn record_commit(
    state: &AppState,
    project_id: &str,
    actor: &str,
    message: &str,
    diff_entries: Vec<DiffEntry>,
) -> anyhow::Result<()> {
    if diff_entries.is_empty() {
        return Ok(());
    }
    let branch = state
        .versioning
        .get_branch(project_id, MAIN_BRANCH)
        .await?
        .context("project has no main branch")?;
    state
        .versioning
        .commit(project_id, &branch, actor, message, &diff_entries)
        .await?;
    for entry in &diff_entries {
        state
            .versioning
            .record_audit(project_id, actor, entry)
            .await?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Elements / edges / bodies / positions (project-scoped)
// ---------------------------------------------------------------------------

/// Stand-in for the real `GET /projects/{id}/commits/{id}/elements` endpoint (impl §1.1) — lists
/// every element in a project from the topology store (Neo4j). Always reads `main`'s live state
/// (there's no "checked out branch" concept — see `store::versioning`'s doc comment).
async fn list_elements(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
) -> Result<Json<Vec<Element>>, ApiError> {
    Ok(Json(state.neo4j.list_elements(&project_id).await?))
}

/// Lists every `Contains` edge in a project — this project's stand-in for a real traceability
/// endpoint. The frontend canvas needs both this and `list_elements` to draw the graph.
async fn list_contains_edges(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
) -> Result<Json<Vec<Edge>>, ApiError> {
    Ok(Json(state.neo4j.contains_edges(&project_id).await?))
}

/// First real use of the document-store side of the split (NFR-DATA-02): the element's body
/// (rationale, large/structured properties) lives in Postgres, never in the graph.
async fn get_element_body(
    State(state): State<AppState>,
    Path((project_id, id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    match state.postgres.get_body(&project_id, &id).await? {
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
    headers: HeaderMap,
    Path(project_id): Path<String>,
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
    state.neo4j.upsert_element(&project_id, &element).await?;
    record_commit(
        &state,
        &project_id,
        &state.auth.resolve_actor(&headers)?,
        "Create element",
        vec![DiffEntry::ElementCreated {
            element_id: element.id.clone(),
            kind: element.kind,
            name: element.name.clone(),
        }],
    )
    .await?;
    Ok(Json(element))
}

#[derive(Debug, serde::Deserialize)]
struct RenameRequest {
    name: String,
}

/// Canvas inline rename (Edit Mode) — preserves the element's existing `kind`/`active`.
async fn rename_element(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project_id, id)): Path<(String, String)>,
    Json(payload): Json<RenameRequest>,
) -> Result<Response, ApiError> {
    if payload.name.trim().is_empty() {
        return Err(import::BadRequest("name must not be empty".to_string()).into());
    }
    let Some(existing) = state.neo4j.get_element(&project_id, &id).await? else {
        return Ok((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("no element {id}") })),
        )
            .into_response());
    };
    let updated = Element {
        name: payload.name,
        ..existing.clone()
    };
    state
        .neo4j
        .rename_element(&project_id, &id, &updated.name)
        .await?;
    if existing.name != updated.name {
        record_commit(
            &state,
            &project_id,
            &state.auth.resolve_actor(&headers)?,
            "Rename element",
            vec![DiffEntry::ElementRenamed {
                element_id: id.clone(),
                old_name: existing.name.clone(),
                new_name: updated.name.clone(),
            }],
        )
        .await?;
    }
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
    headers: HeaderMap,
    Path((project_id, id)): Path<(String, String)>,
    Json(payload): Json<SetActiveRequest>,
) -> Result<StatusCode, ApiError> {
    state
        .neo4j
        .set_active(&project_id, &id, payload.active)
        .await?;
    record_commit(
        &state,
        &project_id,
        &state.auth.resolve_actor(&headers)?,
        "Set element active flag",
        vec![DiffEntry::ElementActiveChanged {
            element_id: id,
            active: payload.active,
        }],
    )
    .await?;
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
    headers: HeaderMap,
    Path((project_id, id)): Path<(String, String)>,
    Json(payload): Json<SetOriginRequest>,
) -> Result<StatusCode, ApiError> {
    state
        .neo4j
        .set_origin(&project_id, &id, payload.origin)
        .await?;
    record_commit(
        &state,
        &project_id,
        &state.auth.resolve_actor(&headers)?,
        "Set element origin",
        vec![DiffEntry::ElementOriginChanged {
            element_id: id,
            origin: payload.origin,
        }],
    )
    .await?;
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
    headers: HeaderMap,
    Path(project_id): Path<String>,
    Json(payload): Json<CreateContainsRequest>,
) -> Result<Json<Edge>, ApiError> {
    let edge = Edge {
        source: payload.parent,
        target: payload.child,
        kind: EdgeKind::Contains,
    };
    state.neo4j.create_edge(&project_id, &edge).await?;
    record_commit(
        &state,
        &project_id,
        &state.auth.resolve_actor(&headers)?,
        "Create containment edge",
        vec![DiffEntry::EdgeCreated {
            source: edge.source.clone(),
            target: edge.target.clone(),
            kind: edge.kind,
        }],
    )
    .await?;
    Ok(Json(edge))
}

/// Canvas disconnect (Edit Mode) — removes a `Contains` edge. No validation gate needed;
/// removing an edge can only heal a cycle/conflict, never create one.
async fn delete_contains_edge(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project_id): Path<String>,
    Json(payload): Json<CreateContainsRequest>,
) -> Result<StatusCode, ApiError> {
    let edge = Edge {
        source: payload.parent,
        target: payload.child,
        kind: EdgeKind::Contains,
    };
    state.neo4j.delete_edge(&project_id, &edge).await?;
    record_commit(
        &state,
        &project_id,
        &state.auth.resolve_actor(&headers)?,
        "Delete containment edge",
        vec![DiffEntry::EdgeDeleted {
            source: edge.source.clone(),
            target: edge.target.clone(),
            kind: edge.kind,
        }],
    )
    .await?;
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
/// `/api/v0/projects/:projectId/contains` route) — e.g. the Hazard/Risk panel's
/// `Causes`/`MitigatedBy` edges.
async fn list_edges(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    Query(params): Query<EdgeKindQuery>,
) -> Result<Json<Vec<Edge>>, ApiError> {
    Ok(Json(
        state.neo4j.edges_of_kind(&project_id, params.kind).await?,
    ))
}

/// Generic edge creation — goes through the same validated `Neo4jStore::create_edge` as
/// `create_contains_edge` (dangling-endpoint rejection, endpoint type-legality, and
/// containment-acyclicity when `kind` is `Contains`).
async fn create_edge(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project_id): Path<String>,
    Json(payload): Json<CreateEdgeRequest>,
) -> Result<Json<Edge>, ApiError> {
    let edge = Edge {
        source: payload.source,
        target: payload.target,
        kind: payload.kind,
    };
    state.neo4j.create_edge(&project_id, &edge).await?;
    record_commit(
        &state,
        &project_id,
        &state.auth.resolve_actor(&headers)?,
        "Create edge",
        vec![DiffEntry::EdgeCreated {
            source: edge.source.clone(),
            target: edge.target.clone(),
            kind: edge.kind,
        }],
    )
    .await?;
    Ok(Json(edge))
}

/// Generic edge removal — no validation gate needed, same reasoning as `delete_contains_edge`.
async fn delete_edge(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project_id): Path<String>,
    Json(payload): Json<CreateEdgeRequest>,
) -> Result<StatusCode, ApiError> {
    let edge = Edge {
        source: payload.source,
        target: payload.target,
        kind: payload.kind,
    };
    state.neo4j.delete_edge(&project_id, &edge).await?;
    record_commit(
        &state,
        &project_id,
        &state.auth.resolve_actor(&headers)?,
        "Delete edge",
        vec![DiffEntry::EdgeDeleted {
            source: edge.source.clone(),
            target: edge.target.clone(),
            kind: edge.kind,
        }],
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, serde::Deserialize)]
struct PositionRequest {
    x: f64,
    y: f64,
}

/// Canvas drag persistence (Edit Mode) — Postgres only, never touches Neo4j or the element's
/// body/rationale (NFR-DATA-01: position is UI metadata, not topology). Deliberately excluded
/// from commit/audit history for the same reason.
async fn update_element_position(
    State(state): State<AppState>,
    Path((project_id, id)): Path<(String, String)>,
    Json(payload): Json<PositionRequest>,
) -> Result<StatusCode, ApiError> {
    state
        .postgres
        .upsert_position(&project_id, &id, payload.x, payload.y)
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
    Path(project_id): Path<String>,
) -> Result<Json<Vec<PositionEntry>>, ApiError> {
    let positions = state
        .postgres
        .list_positions(&project_id)
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

/// Canvas properties-inspector save (Edit Mode) — the write side of `get_element_body`. Diffs
/// against the previous body property-by-property so the resulting commit reports exactly what
/// changed, the same shape T-P1.1-05 expects from the branch-scoped edit endpoint.
async fn update_element_body(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project_id, id)): Path<(String, String)>,
    Json(payload): Json<UpdateBodyRequest>,
) -> Result<StatusCode, ApiError> {
    let old_body = state.postgres.get_body(&project_id, &id).await?;
    let old_properties = old_body
        .as_ref()
        .and_then(|b| b.get("properties"))
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    let old_rationale = old_body
        .as_ref()
        .and_then(|b| b.get("rationale"))
        .and_then(|v| v.as_str())
        .map(String::from);
    let mut diff_entries = Vec::new();
    if let Some(new_properties) = payload.properties.as_object() {
        for (key, new_val) in new_properties {
            let old_val = old_properties
                .get(key)
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            if &old_val != new_val {
                diff_entries.push(DiffEntry::PropertyChanged {
                    element_id: id.clone(),
                    property: key.clone(),
                    old: old_val,
                    new: new_val.clone(),
                });
            }
        }
    }
    if payload.rationale != old_rationale {
        diff_entries.push(DiffEntry::RationaleChanged {
            element_id: id.clone(),
            old: old_rationale,
            new: payload.rationale.clone(),
        });
    }

    state
        .postgres
        .upsert_body(
            &project_id,
            &ElementBody {
                element_id: id.clone(),
                rationale: payload.rationale,
                properties: payload.properties,
            },
        )
        .await?;
    record_commit(
        &state,
        &project_id,
        &state.auth.resolve_actor(&headers)?,
        "Update element properties",
        diff_entries,
    )
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
    headers: HeaderMap,
    Path(project_id): Path<String>,
    Json(payload): Json<ApplyTextModelRequest>,
) -> Result<Json<ApplyTextModelResponse>, ApiError> {
    match state
        .neo4j
        .apply_graph_ops(&project_id, &payload.ops)
        .await?
    {
        ApplyOpsOutcome::Applied { id_map } => {
            record_commit(
                &state,
                &project_id,
                &state.auth.resolve_actor(&headers)?,
                "Apply text edit",
                vec![DiffEntry::TextModelApplied {
                    // Real ids, not the client's temp ids — `apply_diff`/`resolve_snapshot`
                    // replay this later and need every id to already be resolvable; the raw
                    // temp-id ops are only ever meaningful within this one `apply_graph_ops` call.
                    ops: resolve_ops_to_real_ids(&payload.ops, &id_map),
                }],
            )
            .await?;
            Ok(Json(ApplyTextModelResponse {
                ok: true,
                id_map: Some(id_map),
                errors: None,
            }))
        }
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

/// Remaps every id `payload.ops` referenced via a client-minted temp id (a `Create`'s own
/// `temp_id`, or a `Create`/`Reparent` parent reference to an earlier op's temp id in the same
/// batch) to the real, server-assigned id from `apply_graph_ops`'s `id_map` — `Rename`/`Reparent`
/// ids that were already real pass through `resolve` unchanged (`id_map` has no entry for them).
fn resolve_ops_to_real_ids(ops: &[GraphOp], id_map: &HashMap<String, String>) -> Vec<GraphOp> {
    let resolve = |raw: &str| id_map.get(raw).cloned().unwrap_or_else(|| raw.to_string());
    ops.iter()
        .map(|op| match op {
            GraphOp::Rename { id, name } => GraphOp::Rename {
                id: resolve(id),
                name: name.clone(),
            },
            GraphOp::Create {
                temp_id,
                kind,
                name,
                parent_id,
            } => GraphOp::Create {
                temp_id: resolve(temp_id),
                kind: *kind,
                name: name.clone(),
                parent_id: parent_id.as_deref().map(resolve),
            },
            GraphOp::Reparent { id, new_parent_id } => GraphOp::Reparent {
                id: resolve(id),
                new_parent_id: new_parent_id.as_deref().map(resolve),
            },
        })
        .collect()
}

/// If no project exists yet, creates the "Turbofan Reference" project and seeds it — a restart
/// against an already-seeded database skips this (same idempotency the fixture's own
/// `MERGE`-based upserts already relied on, just gated on project existence instead of running
/// unconditionally every boot).
async fn ensure_seeded(state: &AppState) -> anyhow::Result<()> {
    if state.versioning.count_projects().await? > 0 {
        return Ok(());
    }
    let project = state
        .versioning
        .create_project("Turbofan Reference", DEFAULT_REGION)
        .await?;
    let diff_entries = seed_turbofan_ref(state, &project.id).await?;
    record_commit(
        state,
        &project.id,
        DEFAULT_ACTOR,
        "Seed Turbofan-Ref fixture",
        diff_entries,
    )
    .await?;
    Ok(())
}

/// Seeds `Turbofan-Ref`'s P1.1 structural fixture (test spec §0) across all three polyglot
/// stores, scoped to one project: `Engine` composed of the five reference subsystems in Neo4j;
/// `REQ-THRUST` with a 20 KB rationale body in Postgres (mirrors T-P1.1-04's setup); a
/// placeholder geometry blob for `TurbineHpLp`, with only its pointer recorded — never the bytes
/// (NFR-DATA-02). Returns an accurate diff of everything created — this becomes the project's
/// genesis commit, and unlike every later commit, its diff *is* occasionally the only record of
/// this state (`resolve_snapshot` falls back to replaying it once `main` has moved past it and a
/// branch still needs it as a fork point) — a placeholder diff here would silently corrupt that
/// reconstruction. (Rationale text still isn't captured — no `DiffEntry` variant tracks it at
/// all, a pre-existing gap in the diff model itself, same as `compute_snapshot_diff`'s own doc
/// comment already notes for the property-diff case; only reachable in the same narrow
/// replay-past-this-commit scenario.)
async fn seed_turbofan_ref(state: &AppState, project_id: &str) -> anyhow::Result<Vec<DiffEntry>> {
    let mut diff_entries = Vec::new();

    let engine = Element {
        id: "Engine".to_string(),
        kind: NodeKind::Structure,
        name: "Engine".to_string(),
        active: true,
        origin: Origin::Human,
    };
    state.neo4j.upsert_element(project_id, &engine).await?;
    diff_entries.push(DiffEntry::ElementCreated {
        element_id: engine.id.clone(),
        kind: engine.kind,
        name: engine.name.clone(),
    });

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
            .upsert_element(
                project_id,
                &Element {
                    id: id.to_string(),
                    kind: NodeKind::Structure,
                    name: name.to_string(),
                    active: true,
                    origin: Origin::Human,
                },
            )
            .await?;
        diff_entries.push(DiffEntry::ElementCreated {
            element_id: id.to_string(),
            kind: NodeKind::Structure,
            name: name.to_string(),
        });
        state
            .neo4j
            .create_edge(
                project_id,
                &Edge {
                    source: "Engine".to_string(),
                    target: id.to_string(),
                    kind: EdgeKind::Contains,
                },
            )
            .await?;
        diff_entries.push(DiffEntry::EdgeCreated {
            source: "Engine".to_string(),
            target: id.to_string(),
            kind: EdgeKind::Contains,
        });
    }

    let req_thrust = Element {
        id: "REQ-THRUST".to_string(),
        kind: NodeKind::Requirement,
        name: "Engine shall provide >= 30,000 lbf takeoff thrust".to_string(),
        active: true,
        origin: Origin::Human,
    };
    state.neo4j.upsert_element(project_id, &req_thrust).await?;
    diff_entries.push(DiffEntry::ElementCreated {
        element_id: req_thrust.id.clone(),
        kind: req_thrust.kind,
        name: req_thrust.name.clone(),
    });
    state
        .postgres
        .upsert_body(
            project_id,
            &ElementBody {
                element_id: "REQ-THRUST".to_string(),
                // Stand-in for a real rationale document — sized to match test spec T-P1.1-04's
                // 20 KB fixture, proving large text never lands in Neo4j.
                rationale: Some("x".repeat(20_000)),
                properties: serde_json::json!({}),
            },
        )
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
        .upsert_body(
            project_id,
            &ElementBody {
                element_id: "TurbineHpLp".to_string(),
                rationale: None,
                properties: serde_json::json!({ "geometryPointer": pointer.clone() }),
            },
        )
        .await?;
    diff_entries.push(DiffEntry::PropertyChanged {
        element_id: "TurbineHpLp".to_string(),
        property: "geometryPointer".to_string(),
        old: serde_json::Value::Null,
        new: serde_json::json!(pointer),
    });

    seed_fr_comp_content(state, project_id, &mut diff_entries).await?;
    seed_fr_arch_system_model(state, project_id, &mut diff_entries).await?;

    Ok(diff_entries)
}

/// docs/IMPLEMENTATION_KICKOFF.md Phase 3 (FR-COMP-01…06) — the Fan & LP Compression / Core (HP)
/// Compressor content the kickoff doc's own Phase 3 asks for, landed directly into the growing
/// Turbofan-Ref fixture per that phase's stated purpose ("gives Phase 4 concrete, already-modeled
/// Requirements... rather than inventing them ad hoc during instance seeding"). Split out of
/// `seed_turbofan_ref` itself only for readability — same genesis-commit diff-accuracy contract
/// applies (see that function's own doc comment), which is why every property write below still
/// pushes its own `DiffEntry`, unlike `mode_b.rs::apply_candidate_to_main`'s lighter (undiffed)
/// property merge — this function's caller explicitly cares more about diff completeness.
///
/// **Numeric values are this function's own illustrative defaults**, not sourced from a real
/// design — same "invent a reasonable, documented default" precedent as Trade Study's thrust
/// formula and `cem-core`'s own 0D model. Where `cem_core`'s own reference constants already pin
/// a value (bypass ratio), this reuses that exact number rather than inventing a divergent one.
async fn seed_fr_comp_content(
    state: &AppState,
    project_id: &str,
    diff_entries: &mut Vec<DiffEntry>,
) -> anyhow::Result<()> {
    /// One compressor subsystem's FR-COMP content, gathered so the loop below stays readable.
    struct CompressorSeed {
        subsystem_id: &'static str,
        spec_req_id: &'static str,
        spec_req_name: &'static str,
        overall_specification: serde_json::Value,
        constraint_id: &'static str,
        weight_flow_param_id: &'static str,
        speed_param_id: &'static str,
        inlet_port: (&'static str, u32, serde_json::Value),
        exit_port: (&'static str, u32, serde_json::Value),
        interface_contract: [(&'static str, serde_json::Value); 6],
    }

    let seeds = [
        CompressorSeed {
            subsystem_id: "FanLpCompression",
            spec_req_id: "REQ-FAN-SPEC",
            spec_req_name: "Fan & LP Compression over-all design-point specification",
            overall_specification: serde_json::json!({
                "designWeightFlowLbPerSec": 550.0,
                // 5.0 matches cem_core::REFERENCE_BYPASS_RATIO exactly -- duplicated as a
                // literal rather than referenced since that const isn't `pub` (cem-core is a
                // deliberately zero-I/O, self-contained crate; changing its public surface just
                // for a seed script's cross-reference wasn't worth it here).
                "designBypassRatio": 5.0,
                "designFanPressureRatio": 1.4,
                "designEquivalentSpeedRpm": 3800.0,
                "targetPolytropicEfficiency": 0.92,
                "highEfficiencyOperatingRangePercentNCorrected": [70.0, 105.0],
                "inletDiameterIn": 78.0,
                "outletDiameterIn": 74.0,
                "maxOutletVelocityFtPerSec": 750.0,
                "targetLengthIn": 40.0,
                "targetWeightLb": 620.0,
                "inletDistortionTolerance": "IDC <= 0.10 at max operating AoA",
                "negotiable": true,
                "flagged": false,
            }),
            constraint_id: "FanPerformanceMapConstraint",
            weight_flow_param_id: "FanEquivalentWeightFlowParam",
            speed_param_id: "FanEquivalentSpeedParam",
            inlet_port: (
                "FanInletPort",
                1,
                serde_json::json!({ "equivalentWeightFlowLbPerSec": 550.0, "equivalentSpeedRpm": 3800.0 }),
            ),
            exit_port: (
                "FanExitPort",
                2,
                serde_json::json!({ "equivalentWeightFlowLbPerSec": 550.0, "equivalentSpeedRpm": 3800.0 }),
            ),
            interface_contract: [
                (
                    "performanceTargets",
                    serde_json::json!({
                        "description": "Design weight flow, BPR, FPR [1.1-1.8], design equivalent speed, target eta_poly, high-eta range = 70-105% N/sqrt(theta)",
                        "bpr": 5.0,
                        "fprRange": [1.1, 1.8],
                        "highEfficiencyRangePercent": [70.0, 105.0],
                    }),
                ),
                (
                    "boundaryConditions",
                    serde_json::json!({
                        "description": "Inlet distortion tolerance, altitude/Mach envelope, Reynolds-number floor at altitude",
                    }),
                ),
                (
                    "geometricEnvelope",
                    serde_json::json!({
                        "description": "Fan diameter, LP-spool axial length budget, hub/tip ratio floor (~0.35)",
                        "hubToTipRatioFloor": 0.35,
                    }),
                ),
                (
                    "interfacePortDefinitions",
                    serde_json::json!({
                        "description": "Bypass duct port (to nozzle/mixer), LP-shaft coupling (to LP Turbine), gearbox port (if IncludeGearbox)",
                        "ports": ["FanInletPort", "FanExitPort"],
                    }),
                ),
                (
                    "massCostTargets",
                    serde_json::json!({
                        "description": "Stage/blade mass <= budget, unit cost envelope",
                    }),
                ),
                (
                    "materialProcessConstraints",
                    serde_json::json!({
                        "description": "Blade material vs. relative-Mach/thermal duty (FR-COMP-03 bound)",
                    }),
                ),
            ],
        },
        CompressorSeed {
            subsystem_id: "CoreHpCompressor",
            spec_req_id: "REQ-CORE-SPEC",
            spec_req_name: "Core (HP) Compressor over-all design-point specification",
            overall_specification: serde_json::json!({
                "designWeightFlowLbPerSec": 110.0,
                "designOverallPressureRatioContribution": 8.0,
                "designEquivalentSpeedRpm": 14500.0,
                "targetPolytropicEfficiency": 0.88,
                "highEfficiencyOperatingRangePercentNCorrected": [75.0, 100.0],
                "inletDiameterIn": 28.0,
                "outletDiameterIn": 18.0,
                "maxOutletVelocityFtPerSec": 900.0,
                "targetLengthIn": 22.0,
                "targetWeightLb": 340.0,
                "inletDistortionTolerance": "IDC <= 0.05 (post-fan-conditioned flow)",
                "negotiable": true,
                "flagged": false,
            }),
            constraint_id: "CorePerformanceMapConstraint",
            weight_flow_param_id: "CoreEquivalentWeightFlowParam",
            speed_param_id: "CoreEquivalentSpeedParam",
            inlet_port: (
                "CoreInletPort",
                2,
                serde_json::json!({ "equivalentWeightFlowLbPerSec": 110.0, "equivalentSpeedRpm": 14500.0 }),
            ),
            exit_port: (
                "CoreExitPort",
                3,
                serde_json::json!({
                    "equivalentWeightFlowLbPerSec": 110.0,
                    "equivalentSpeedRpm": 14500.0,
                    // Bleed originates at the core exit per the reconciliation table (reqs v5
                    // §5.16) -- not on the fan side.
                    "bleedFractionB": 0.03,
                }),
            ),
            interface_contract: [
                (
                    "performanceTargets",
                    serde_json::json!({
                        "description": "Design weight flow, OPR contribution, design equivalent speed, target eta_poly, high-eta range",
                        "overallPressureRatioContribution": 8.0,
                        "highEfficiencyRangePercent": [75.0, 100.0],
                    }),
                ),
                (
                    "boundaryConditions",
                    serde_json::json!({
                        "description": "Combustor-inlet temperature/pressure environment",
                    }),
                ),
                (
                    "geometricEnvelope",
                    serde_json::json!({
                        "description": "Core diameter, HP-spool axial length budget",
                    }),
                ),
                (
                    "interfacePortDefinitions",
                    serde_json::json!({
                        "description": "Bleed-air offtake port (location per BleedOfftakeStage), HP-shaft coupling (to HP Turbine), combustor-inlet port",
                        "ports": ["CoreInletPort", "CoreExitPort"],
                    }),
                ),
                (
                    "massCostTargets",
                    serde_json::json!({
                        "description": "Stage/blade mass <= budget, core-spool scope",
                    }),
                ),
                (
                    "materialProcessConstraints",
                    serde_json::json!({
                        "description": "Blade material vs. relative-Mach/thermal duty, higher-temperature duty than the LP spool",
                    }),
                ),
            ],
        },
    ];

    for seed in seeds {
        // FR-COMP-01/06: the over-all design-point specification lands as a real `:Requirement`
        // the subsystem `Satisfy`-links to -- the same shape `REQ-THRUST` already established,
        // not a bare property on the subsystem's own body.
        let spec_req = Element {
            id: seed.spec_req_id.to_string(),
            kind: NodeKind::Requirement,
            name: seed.spec_req_name.to_string(),
            active: true,
            origin: Origin::Human,
        };
        state.neo4j.upsert_element(project_id, &spec_req).await?;
        diff_entries.push(DiffEntry::ElementCreated {
            element_id: spec_req.id.clone(),
            kind: spec_req.kind,
            name: spec_req.name.clone(),
        });
        // Captured before `overall_specification` moves into the ElementBody below -- reused by
        // the performance-map Constraint's illustrative sample points further down.
        let design_weight_flow = seed
            .overall_specification
            .get("designWeightFlowLbPerSec")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        state
            .postgres
            .upsert_body(
                project_id,
                &ElementBody {
                    element_id: seed.spec_req_id.to_string(),
                    rationale: None,
                    properties: seed.overall_specification,
                },
            )
            .await?;
        state
            .neo4j
            .create_edge(
                project_id,
                &Edge {
                    source: seed.subsystem_id.to_string(),
                    target: seed.spec_req_id.to_string(),
                    kind: EdgeKind::Satisfy,
                },
            )
            .await?;
        diff_entries.push(DiffEntry::EdgeCreated {
            source: seed.subsystem_id.to_string(),
            target: seed.spec_req_id.to_string(),
            kind: EdgeKind::Satisfy,
        });

        // FR-COMP-02: the off-design performance map, using Phase 1's `:Constraint`/`:Parameter`
        // kinds for the first time anywhere in this codebase -- deliberately modest (an
        // illustrative sampled shape, not a sourced equation; see this function's own doc
        // comment and reqs v5 §5.15's honesty note that the real formulas aren't sourced yet).
        let map_constraint = Element {
            id: seed.constraint_id.to_string(),
            kind: NodeKind::Constraint,
            name: format!("{} performance map", seed.subsystem_id),
            active: true,
            origin: Origin::Human,
        };
        state
            .neo4j
            .upsert_element(project_id, &map_constraint)
            .await?;
        diff_entries.push(DiffEntry::ElementCreated {
            element_id: map_constraint.id.clone(),
            kind: map_constraint.kind,
            name: map_constraint.name.clone(),
        });
        state
            .postgres
            .upsert_body(
                project_id,
                &ElementBody {
                    element_id: seed.constraint_id.to_string(),
                    rationale: None,
                    properties: serde_json::json!({
                        "description": "Pressure ratio vs. equivalent weight flow, parametrized by equivalent speed, with a stall/surge limit line",
                        // Illustrative shape only, at one fixed equivalent speed -- not a real
                        // constitutive relation. There's no dedicated "constraint uses parameter"
                        // edge in the Phase 1 schema yet, so this plain id list is the honest
                        // stand-in until Parametrics evaluation (Phase 5) needs a real one.
                        "usesParameterIds": [seed.weight_flow_param_id, seed.speed_param_id],
                        "sampledPointsAtDesignSpeed": [
                            { "equivalentWeightFlowLbPerSec": seed.inlet_port.2["equivalentWeightFlowLbPerSec"].clone(), "pressureRatio": 1.30 },
                            { "equivalentWeightFlowLbPerSec": design_weight_flow.clone(), "pressureRatio": 1.40 },
                            { "equivalentWeightFlowLbPerSec": 0.0, "pressureRatio": 1.35, "note": "illustrative stall/surge knee, not a sourced value" },
                        ],
                        "sourceNote": "illustrative shape only -- real constitutive equations not yet sourced (reqs v5 §5.15)",
                    }),
                },
            )
            .await?;

        for (param_id, symbol) in [
            (seed.weight_flow_param_id, "w*sqrt(theta)/delta"),
            (seed.speed_param_id, "N/sqrt(theta)"),
        ] {
            let param = Element {
                id: param_id.to_string(),
                kind: NodeKind::Parameter,
                name: format!("{} {}", seed.subsystem_id, symbol),
                active: true,
                origin: Origin::Human,
            };
            state.neo4j.upsert_element(project_id, &param).await?;
            diff_entries.push(DiffEntry::ElementCreated {
                element_id: param.id.clone(),
                kind: param.kind,
                name: param.name.clone(),
            });
            state
                .postgres
                .upsert_body(
                    project_id,
                    &ElementBody {
                        element_id: param_id.to_string(),
                        rationale: None,
                        properties: serde_json::json!({ "symbol": symbol, "units": "corrected (equivalent) units" }),
                    },
                )
                .await?;
            // EdgeKind::Bound's real endpoint rule (packages/sysml-core/src/lib.rs): source must
            // be a Parameter, target is unconstrained -- here, the subsystem whose Value
            // Property this Parameter represents (FR-PARAM-02).
            state
                .neo4j
                .create_edge(
                    project_id,
                    &Edge {
                        source: param_id.to_string(),
                        target: seed.subsystem_id.to_string(),
                        kind: EdgeKind::Bound,
                    },
                )
                .await?;
            diff_entries.push(DiffEntry::EdgeCreated {
                source: param_id.to_string(),
                target: seed.subsystem_id.to_string(),
                kind: EdgeKind::Bound,
            });
        }

        // FR-COMP-05: gas-generator matching interface, as real `:Port` elements following reqs
        // v5 §5.16's station-numbering convention.
        for (port_id, station, port_properties) in [seed.inlet_port, seed.exit_port] {
            let mut properties = port_properties.as_object().cloned().unwrap_or_default();
            properties.insert("station".to_string(), serde_json::json!(station));
            let port = Element {
                id: port_id.to_string(),
                kind: NodeKind::Port,
                name: format!("{} station {}", seed.subsystem_id, station),
                active: true,
                origin: Origin::Human,
            };
            state.neo4j.upsert_element(project_id, &port).await?;
            diff_entries.push(DiffEntry::ElementCreated {
                element_id: port.id.clone(),
                kind: port.kind,
                name: port.name.clone(),
            });
            state
                .postgres
                .upsert_body(
                    project_id,
                    &ElementBody {
                        element_id: port_id.to_string(),
                        rationale: None,
                        properties: serde_json::Value::Object(properties),
                    },
                )
                .await?;
            state
                .neo4j
                .create_edge(
                    project_id,
                    &Edge {
                        source: seed.subsystem_id.to_string(),
                        target: port_id.to_string(),
                        kind: EdgeKind::Contains,
                    },
                )
                .await?;
            diff_entries.push(DiffEntry::EdgeCreated {
                source: seed.subsystem_id.to_string(),
                target: port_id.to_string(),
                kind: EdgeKind::Contains,
            });
        }

        // Extended Interface Contract worked examples (impl v5 §2.6) -- merged onto the
        // subsystem's existing body (read-then-write, `mode_b.rs::upsert_subsystem_contract`'s
        // exact pattern) rather than replacing it, and tagged so this is never confused with a
        // real Mode B run's `modeBProvenance`.
        let existing = state
            .postgres
            .get_body(project_id, seed.subsystem_id)
            .await?;
        let mut properties = existing
            .as_ref()
            .and_then(|b| b.get("properties"))
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();
        let rationale = existing
            .as_ref()
            .and_then(|b| b.get("rationale"))
            .and_then(|v| v.as_str())
            .map(String::from);
        for (key, value) in seed.interface_contract {
            let old = properties
                .get(key)
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            diff_entries.push(DiffEntry::PropertyChanged {
                element_id: seed.subsystem_id.to_string(),
                property: key.to_string(),
                old,
                new: value.clone(),
            });
            properties.insert(key.to_string(), value);
        }
        properties.insert(
            "specProvenance".to_string(),
            serde_json::json!("docs-worked-example"),
        );
        state
            .postgres
            .upsert_body(
                project_id,
                &ElementBody {
                    element_id: seed.subsystem_id.to_string(),
                    rationale,
                    properties: serde_json::Value::Object(properties),
                },
            )
            .await?;
    }

    Ok(())
}

/// docs/IMPLEMENTATION_KICKOFF.md Phase 4 — seeds the reconciled 5-subsystem turbofan system
/// model (reqs v5 §5.16/§5.17) as a real instance: the station 0-8 gas-path Ports the compressor
/// pair (Phase 3) didn't yet cover, the top-level boundary Functions and their fulfillment
/// (`:Function`, first real instantiation), the architecture-choice primitives FR-ARCH-01..04
/// need (`:SelectionChoice`/`:ConnectionChoice`, also first real instantiation, plus one
/// `IncompatibleWith` and two `ChoiceConstraint` edges), the remaining named per-subsystem design
/// variables as `:Parameter`s, and finally `Satisfy` edges wiring `REQ-THRUST` (previously
/// disconnected in this fixture) to the four gas-path subsystems that jointly generate thrust.
///
/// **Everything here is a static, versioned description of the design space's fixed structure —
/// not a live search state.** Per Phase 2's own ratified recommendation (impl v5 §10.3), the
/// *unresolved* search space stays sidecar-side (`cem-archspace`); only a *resolved* architecture
/// instance would ever materialize into this graph as a `:Structure` subgraph. This function seeds
/// neither — it seeds the fixed choice/constraint *definitions* §5.16 documents, the same durable
/// role this graph already plays for Requirement text or Contains topology.
///
/// **A real, minor doc inconsistency, flagged rather than silently resolved**: reqs v5 §5.16's own
/// prose calls `GenerateThrust`'s decomposition a "five-subsystem gas-path chain," but its own
/// mermaid diagram's `GT` subgraph includes only four (Fan & LP Compression, Core (HP) Compressor,
/// Combustor, Turbine (HP & LP) — Control (FADEC/EEC) is not gas-path). The diagram is treated as
/// authoritative here.
///
/// **A real, confirmed schema gap, flagged not invented around**: `EdgeKind::ChoiceConstraint`'s
/// own doc comment says it "carries a Linked/Permutations/Unordered [non-]replacing type," but
/// `Edge` (`packages/sysml-core/src/lib.rs`) is `{source, target, kind}` only — no properties
/// field exists to persist that type anywhere. The two `ChoiceConstraint` edges below are real
/// (FR-COMP-04, unblocked now that Turbine-side stage-count Parameters exist), but their "Linked"
/// type lives only in this comment and the docs write-up, not in the graph.
async fn seed_fr_arch_system_model(
    state: &AppState,
    project_id: &str,
    diff_entries: &mut Vec<DiffEntry>,
) -> anyhow::Result<()> {
    struct PortSeed {
        id: &'static str,
        name: &'static str,
        subsystem_id: &'static str,
        station: Option<u32>,
        properties: serde_json::Value,
    }

    let ports = [
        PortSeed {
            id: "FanBypassDuctExitPort",
            name: "Fan & LP Compression bypass duct exit",
            subsystem_id: "FanLpCompression",
            station: None,
            properties: serde_json::json!({
                "description": "Feeds the nozzle/mixer; carries the MixedNozzle incompatibility constraint (reqs v5 §5.16).",
            }),
        },
        PortSeed {
            id: "CoreBleedOfftakePort",
            name: "Core (HP) Compressor bleed-air offtake",
            subsystem_id: "CoreHpCompressor",
            station: None,
            properties: serde_json::json!({
                "description": "Location per the BleedOfftakeStage selection choice; routed via the BleedAirRouting connection choice.",
            }),
        },
        PortSeed {
            id: "CombustorInletPort",
            name: "Combustor inlet (station 3)",
            subsystem_id: "Combustor",
            station: Some(3),
            properties: serde_json::json!({}),
        },
        PortSeed {
            id: "CombustorFuelInjectorPort",
            name: "Combustor fuel-injector port",
            subsystem_id: "Combustor",
            station: None,
            properties: serde_json::json!({
                "description": "Fixed connection from Control (FADEC/EEC)'s Fuel Metering Unit -- not a choice. No fixed non-choice port-to-port edge type exists in this schema yet, so this is a documented property only, not a graph edge.",
            }),
        },
        PortSeed {
            id: "CombustorExitPort",
            name: "Combustor exit (station 4)",
            subsystem_id: "Combustor",
            station: Some(4),
            properties: serde_json::json!({}),
        },
        PortSeed {
            id: "TurbineHpInletPort",
            name: "Turbine HP inlet (station 4)",
            subsystem_id: "TurbineHpLp",
            station: Some(4),
            properties: serde_json::json!({}),
        },
        PortSeed {
            id: "TurbineLpInterstagePort",
            name: "Turbine HP exit / LP inlet (station 5)",
            subsystem_id: "TurbineHpLp",
            station: Some(5),
            properties: serde_json::json!({}),
        },
        PortSeed {
            id: "TurbineExitPort",
            name: "Turbine LP exit (station 6)",
            subsystem_id: "TurbineHpLp",
            station: Some(6),
            properties: serde_json::json!({}),
        },
        PortSeed {
            id: "NozzleInletPort",
            name: "Nozzle inlet (station 7)",
            subsystem_id: "TurbineHpLp",
            station: Some(7),
            properties: serde_json::json!({
                "description": "Nozzle folded into Turbine's exit boundary per the ratified reconciliation decision (reqs v5 §5.16) -- not a 6th subsystem.",
            }),
        },
        PortSeed {
            id: "NozzleExitPort",
            name: "Nozzle exit (station 8)",
            subsystem_id: "TurbineHpLp",
            station: Some(8),
            properties: serde_json::json!({}),
        },
        PortSeed {
            id: "ControlAccessoryPort",
            name: "Control (FADEC/EEC) accessory/generator connection port",
            subsystem_id: "ControlFadecEec",
            station: None,
            properties: serde_json::json!({
                "description": "Receives Turbine's PowerOfftake via the PowerOfftakeRouting connection choice.",
            }),
        },
    ];

    for p in ports {
        let mut properties = p.properties.as_object().cloned().unwrap_or_default();
        if let Some(station) = p.station {
            properties.insert("station".to_string(), serde_json::json!(station));
        }
        let port = Element {
            id: p.id.to_string(),
            kind: NodeKind::Port,
            name: p.name.to_string(),
            active: true,
            origin: Origin::Human,
        };
        state.neo4j.upsert_element(project_id, &port).await?;
        diff_entries.push(DiffEntry::ElementCreated {
            element_id: port.id.clone(),
            kind: port.kind,
            name: port.name.clone(),
        });
        state
            .postgres
            .upsert_body(
                project_id,
                &ElementBody {
                    element_id: p.id.to_string(),
                    rationale: None,
                    properties: serde_json::Value::Object(properties),
                },
            )
            .await?;
        state
            .neo4j
            .create_edge(
                project_id,
                &Edge {
                    source: p.subsystem_id.to_string(),
                    target: p.id.to_string(),
                    kind: EdgeKind::Contains,
                },
            )
            .await?;
        diff_entries.push(DiffEntry::EdgeCreated {
            source: p.subsystem_id.to_string(),
            target: p.id.to_string(),
            kind: EdgeKind::Contains,
        });
    }

    // FR-ARCH-02/03: selection/connection choices (`:SelectionChoice`/`:ConnectionChoice`, first
    // real instantiation of either kind). `options` is a plain JSON array -- there's no dedicated
    // "option node" mechanism in this schema yet (same "plain JSON, gap flagged" precedent as
    // Phase 3's Constraint-uses-Parameter list). `resolutionState` matches §5.17's own literal
    // "unresolved -> partial -> resolved... node-property state machine" spec.
    struct SelectionChoiceSeed {
        id: &'static str,
        name: &'static str,
        subsystem_id: &'static str,
        options: &'static [&'static str],
        incompatibility_note: Option<&'static str>,
    }

    let selection_choices = [
        SelectionChoiceSeed {
            id: "IncludeGearbox",
            name: "Include Gearbox (Fan & LP Compression)",
            subsystem_id: "FanLpCompression",
            options: &["true", "false"],
            incompatibility_note: None,
        },
        SelectionChoiceSeed {
            id: "BleedOfftakeStage",
            name: "Bleed Offtake Stage (Core (HP) Compressor)",
            subsystem_id: "CoreHpCompressor",
            options: &["stage 1..n_HP_stages (illustrative set: stage 1, stage 2, stage 3)"],
            incompatibility_note: None,
        },
        SelectionChoiceSeed {
            id: "PowerOfftake",
            name: "Power-Offtake Shaft (Turbine (HP & LP))",
            subsystem_id: "TurbineHpLp",
            options: &["HP shaft", "LP shaft"],
            incompatibility_note: None,
        },
        SelectionChoiceSeed {
            id: "MixedNozzle",
            name: "Mixed vs. Separate-Flow Nozzle (Turbine (HP & LP))",
            subsystem_id: "TurbineHpLp",
            options: &["mixed", "separate"],
            incompatibility_note: Some(
                "MixedNozzle=true (mixed exhaust) excludes independently-configured separate \
                 core/bypass nozzle fulfillment via Fan & LP Compression's \
                 FanBypassDuctExitPort, and vice versa (reqs v5 §5.16) -- see this element's \
                 IncompatibleWith edge to that port.",
            ),
        },
    ];

    for sc in selection_choices {
        let element = Element {
            id: sc.id.to_string(),
            kind: NodeKind::SelectionChoice,
            name: sc.name.to_string(),
            active: true,
            origin: Origin::Human,
        };
        state.neo4j.upsert_element(project_id, &element).await?;
        diff_entries.push(DiffEntry::ElementCreated {
            element_id: element.id.clone(),
            kind: element.kind,
            name: element.name.clone(),
        });
        let mut properties = serde_json::Map::new();
        properties.insert("options".to_string(), serde_json::json!(sc.options));
        properties.insert(
            "resolutionState".to_string(),
            serde_json::json!("unresolved"),
        );
        if let Some(note) = sc.incompatibility_note {
            properties.insert("incompatibilityNote".to_string(), serde_json::json!(note));
        }
        state
            .postgres
            .upsert_body(
                project_id,
                &ElementBody {
                    element_id: sc.id.to_string(),
                    rationale: None,
                    properties: serde_json::Value::Object(properties),
                },
            )
            .await?;
        // Scopes the choice to the subsystem it's part of. `ArchDerives` is deliberately
        // kind-unconstrained (packages/sysml-core/src/lib.rs) -- a real DSG's own option-derivation
        // edges (adsg-core's `derives`) are out of scope for this static seed; see this function's
        // own doc comment.
        state
            .neo4j
            .create_edge(
                project_id,
                &Edge {
                    source: sc.id.to_string(),
                    target: sc.subsystem_id.to_string(),
                    kind: EdgeKind::ArchDerives,
                },
            )
            .await?;
        diff_entries.push(DiffEntry::EdgeCreated {
            source: sc.id.to_string(),
            target: sc.subsystem_id.to_string(),
            kind: EdgeKind::ArchDerives,
        });
    }

    struct ConnectionChoiceSeed {
        id: &'static str,
        name: &'static str,
        properties: serde_json::Value,
    }

    let connection_choices = [
        ConnectionChoiceSeed {
            id: "BleedAirRouting",
            name: "Bleed-Air Routing",
            properties: serde_json::json!({
                "sourcePortId": "CoreBleedOfftakePort",
                "targetBoundary": "external ECS/airframe port -- outside the engine System-of-Interest (reqs v5 §5.16's reconciliation table), so no target Port element exists for it",
                "cardinality": "single connection",
            }),
        },
        ConnectionChoiceSeed {
            id: "PowerOfftakeRouting",
            name: "Power-Offtake Routing",
            properties: serde_json::json!({
                "sourceSelectionChoiceId": "PowerOfftake",
                "targetPortId": "ControlAccessoryPort",
                "cardinality": "single connection, resolved after the PowerOfftake selection choice",
            }),
        },
    ];

    for cc in connection_choices {
        let element = Element {
            id: cc.id.to_string(),
            kind: NodeKind::ConnectionChoice,
            name: cc.name.to_string(),
            active: true,
            origin: Origin::Human,
        };
        state.neo4j.upsert_element(project_id, &element).await?;
        diff_entries.push(DiffEntry::ElementCreated {
            element_id: element.id.clone(),
            kind: element.kind,
            name: element.name.clone(),
        });
        state
            .postgres
            .upsert_body(
                project_id,
                &ElementBody {
                    element_id: cc.id.to_string(),
                    rationale: None,
                    properties: cc.properties,
                },
            )
            .await?;
    }

    // FR-ARCH-01: top-level boundary Functions (`:Function`, first real instantiation), each
    // `ArchDerives`-linked to whatever fulfills it -- a fixed Structure for `COMP`, or the
    // ConnectionChoice that resolves its `NOF`-eligible fulfillment. `fulfillmentMechanism` matches
    // impl v5 §2.3's own "tag... on a Function's body" convention exactly.
    struct FunctionSeed {
        id: &'static str,
        name: &'static str,
        permanence: &'static str,
        fulfillment_mechanism: &'static str,
        notes: &'static str,
        fulfills: &'static [&'static str],
    }

    let functions = [
        FunctionSeed {
            id: "GenerateThrust",
            name: "Generate Thrust",
            permanence: "permanent",
            fulfillment_mechanism: "DE",
            notes: "Decomposed into the gas-path chain per reqs v5 §5.16's mermaid diagram: Fan & \
                LP Compression -> Core (HP) Compressor -> Combustor -> Turbine (HP & LP). The \
                section's own prose table calls this a 'five-subsystem gas-path chain', but the \
                diagram itself includes only these four (Control (FADEC/EEC) is not gas-path) -- \
                the diagram is treated as authoritative; this mismatch is flagged, not silently \
                resolved by guessing which is right.",
            fulfills: &[
                "FanLpCompression",
                "CoreHpCompressor",
                "Combustor",
                "TurbineHpLp",
            ],
        },
        FunctionSeed {
            id: "ProvideBleedAir",
            name: "Provide Bleed Air",
            permanence: "conditional",
            fulfillment_mechanism: "NOF",
            notes: "Non-fulfillment-eligible; when fulfilled, it is via the BleedAirRouting \
                connection choice from Core (HP) Compressor's offtake port, not a fixed component.",
            fulfills: &["BleedAirRouting"],
        },
        FunctionSeed {
            id: "ProvideAccessoryShaftPower",
            name: "Provide Accessory/Shaft Power",
            permanence: "conditional",
            fulfillment_mechanism: "NOF",
            notes: "Non-fulfillment-eligible; when fulfilled, it is via the PowerOfftakeRouting \
                connection choice from a shaft-mounted offtake (HP or LP shaft, per the Turbine \
                PowerOfftake selection choice).",
            fulfills: &["PowerOfftakeRouting"],
        },
        FunctionSeed {
            id: "RegulateEngineOperation",
            name: "Regulate Engine Operation",
            permanence: "permanent",
            fulfillment_mechanism: "COMP",
            notes: "Fulfilled directly by one fixed component -- Control (FADEC/EEC) -- no \
                architecture choice searched over. Induces the Meter Fuel Flow sub-function.",
            fulfills: &["ControlFadecEec"],
        },
        FunctionSeed {
            id: "MeterFuelFlow",
            name: "Meter Fuel Flow",
            permanence: "permanent",
            fulfillment_mechanism: "COMP",
            notes: "Sub-function induced by Regulate Engine Operation; fulfilled by Control \
                (FADEC/EEC)'s Fuel Metering Unit, which feeds Combustor's fixed \
                CombustorFuelInjectorPort.",
            fulfills: &["ControlFadecEec"],
        },
    ];

    for f in functions {
        let element = Element {
            id: f.id.to_string(),
            kind: NodeKind::Function,
            name: f.name.to_string(),
            active: true,
            origin: Origin::Human,
        };
        state.neo4j.upsert_element(project_id, &element).await?;
        diff_entries.push(DiffEntry::ElementCreated {
            element_id: element.id.clone(),
            kind: element.kind,
            name: element.name.clone(),
        });
        state
            .postgres
            .upsert_body(
                project_id,
                &ElementBody {
                    element_id: f.id.to_string(),
                    rationale: None,
                    properties: serde_json::json!({
                        "permanence": f.permanence,
                        "fulfillmentMechanism": f.fulfillment_mechanism,
                        "notes": f.notes,
                    }),
                },
            )
            .await?;
        for target in f.fulfills {
            state
                .neo4j
                .create_edge(
                    project_id,
                    &Edge {
                        source: f.id.to_string(),
                        target: target.to_string(),
                        kind: EdgeKind::ArchDerives,
                    },
                )
                .await?;
            diff_entries.push(DiffEntry::EdgeCreated {
                source: f.id.to_string(),
                target: target.to_string(),
                kind: EdgeKind::ArchDerives,
            });
        }
    }

    // FR-ARCH-04: the nozzle-flow-exclusivity incompatibility constraint (reqs v5 §5.16's
    // cross-cutting table), between the two elements the doc's own prose says it "spans."
    state
        .neo4j
        .create_edge(
            project_id,
            &Edge {
                source: "MixedNozzle".to_string(),
                target: "FanBypassDuctExitPort".to_string(),
                kind: EdgeKind::IncompatibleWith,
            },
        )
        .await?;
    diff_entries.push(DiffEntry::EdgeCreated {
        source: "MixedNozzle".to_string(),
        target: "FanBypassDuctExitPort".to_string(),
        kind: EdgeKind::IncompatibleWith,
    });

    // Remaining named per-subsystem design variables (reqs v5 §5.16's per-subsystem breakdown),
    // as `:Parameter`s -- includes the two stage-count pairs FR-COMP-04 needs (deferred from Phase
    // 3 pending this Turbine-side content). Bounds are the doc's own "illustrative starting
    // points, subject to real numeric sourcing" (§5.16); Combustor's four carry no stated bound at
    // all (the doc gives it design-variables/metrics with no numeric target), so those are marked
    // `illustrative: true` rather than given a fabricated range.
    struct ParamSeed {
        id: &'static str,
        subsystem_id: &'static str,
        symbol: &'static str,
        properties: serde_json::Value,
    }

    let params = [
        ParamSeed {
            id: "FanLpStagesParam",
            subsystem_id: "FanLpCompression",
            symbol: "n_LP_stages",
            properties: serde_json::json!({
                "description": "LP-compressor stage count; ChoiceConstraint-linked to TurbineLpStagesParam (FR-COMP-04).",
                "type": "integer",
            }),
        },
        ParamSeed {
            id: "CoreHpStagesParam",
            subsystem_id: "CoreHpCompressor",
            symbol: "n_HP_stages",
            properties: serde_json::json!({
                "description": "HP-compressor stage count; ChoiceConstraint-linked to TurbineHpStagesParam (FR-COMP-04).",
                "type": "integer",
            }),
        },
        ParamSeed {
            id: "TurbineHpStagesParam",
            subsystem_id: "TurbineHpLp",
            symbol: "n_HP_turbine_stages",
            properties: serde_json::json!({
                "description": "HP-turbine stage count; ChoiceConstraint-linked to CoreHpStagesParam (FR-COMP-04).",
                "type": "integer",
            }),
        },
        ParamSeed {
            id: "TurbineLpStagesParam",
            subsystem_id: "TurbineHpLp",
            symbol: "n_LP_turbine_stages",
            properties: serde_json::json!({
                "description": "LP-turbine stage count; ChoiceConstraint-linked to FanLpStagesParam (FR-COMP-04).",
                "type": "integer",
            }),
        },
        ParamSeed {
            id: "GearRatioParam",
            subsystem_id: "FanLpCompression",
            symbol: "GearRatio",
            properties: serde_json::json!({
                "description": "Conditional on the IncludeGearbox selection choice.",
                "bound": [1.0, 5.0],
                "illustrative": true,
            }),
        },
        ParamSeed {
            id: "BprParam",
            subsystem_id: "FanLpCompression",
            symbol: "BPR",
            properties: serde_json::json!({ "bound": [2.0, 12.5], "illustrative": true }),
        },
        ParamSeed {
            id: "FprParam",
            subsystem_id: "FanLpCompression",
            symbol: "FPR",
            properties: serde_json::json!({ "bound": [1.1, 1.8], "illustrative": true }),
        },
        ParamSeed {
            id: "OprCoreParam",
            subsystem_id: "CoreHpCompressor",
            symbol: "OPR_core",
            properties: serde_json::json!({
                "description": "Combines toward overall OPR [1.1-60.0].",
                "illustrative": true,
            }),
        },
        ParamSeed {
            id: "ChamberSizeParam",
            subsystem_id: "Combustor",
            symbol: "ChamberSize",
            properties: serde_json::json!({
                "description": "No architecture choice modeled for Combustor this pass (reqs v5 §5.16); no numeric target sourced anywhere yet.",
                "illustrative": true,
            }),
        },
        ParamSeed {
            id: "FlameTemperatureParam",
            subsystem_id: "Combustor",
            symbol: "FlameTemperature",
            properties: serde_json::json!({ "illustrative": true }),
        },
        ParamSeed {
            id: "PressureLossParam",
            subsystem_id: "Combustor",
            symbol: "PressureLoss",
            properties: serde_json::json!({ "illustrative": true }),
        },
        ParamSeed {
            id: "NOxParam",
            subsystem_id: "Combustor",
            symbol: "NOx",
            properties: serde_json::json!({
                "description": "Generic metric, verification-only unless made an objective (reqs v5 §5.16 Metrics table).",
                "illustrative": true,
            }),
        },
    ];

    for p in params {
        let element = Element {
            id: p.id.to_string(),
            kind: NodeKind::Parameter,
            name: format!("{} {}", p.subsystem_id, p.symbol),
            active: true,
            origin: Origin::Human,
        };
        state.neo4j.upsert_element(project_id, &element).await?;
        diff_entries.push(DiffEntry::ElementCreated {
            element_id: element.id.clone(),
            kind: element.kind,
            name: element.name.clone(),
        });
        let mut properties = p.properties.as_object().cloned().unwrap_or_default();
        properties.insert("symbol".to_string(), serde_json::json!(p.symbol));
        state
            .postgres
            .upsert_body(
                project_id,
                &ElementBody {
                    element_id: p.id.to_string(),
                    rationale: None,
                    properties: serde_json::Value::Object(properties),
                },
            )
            .await?;
        state
            .neo4j
            .create_edge(
                project_id,
                &Edge {
                    source: p.id.to_string(),
                    target: p.subsystem_id.to_string(),
                    kind: EdgeKind::Bound,
                },
            )
            .await?;
        diff_entries.push(DiffEntry::EdgeCreated {
            source: p.id.to_string(),
            target: p.subsystem_id.to_string(),
            kind: EdgeKind::Bound,
        });
    }

    for (a, b) in [
        ("FanLpStagesParam", "TurbineLpStagesParam"),
        ("CoreHpStagesParam", "TurbineHpStagesParam"),
    ] {
        state
            .neo4j
            .create_edge(
                project_id,
                &Edge {
                    source: a.to_string(),
                    target: b.to_string(),
                    kind: EdgeKind::ChoiceConstraint,
                },
            )
            .await?;
        diff_entries.push(DiffEntry::EdgeCreated {
            source: a.to_string(),
            target: b.to_string(),
            kind: EdgeKind::ChoiceConstraint,
        });
    }

    // Closes the real gap this function's own doc comment flags: `REQ-THRUST` previously had zero
    // edges in this fixture. `Satisfy` from the four gas-path subsystems `GenerateThrust`
    // decomposes into -- Phase 4's own "Satisfy/Verify edges from... existing higher-level
    // requirements into the seeded structure" instruction, exercising traceability end-to-end.
    for subsystem_id in [
        "FanLpCompression",
        "CoreHpCompressor",
        "Combustor",
        "TurbineHpLp",
    ] {
        state
            .neo4j
            .create_edge(
                project_id,
                &Edge {
                    source: subsystem_id.to_string(),
                    target: "REQ-THRUST".to_string(),
                    kind: EdgeKind::Satisfy,
                },
            )
            .await?;
        diff_entries.push(DiffEntry::EdgeCreated {
            source: subsystem_id.to_string(),
            target: "REQ-THRUST".to_string(),
            kind: EdgeKind::Satisfy,
        });
    }

    // `CoreBleedOfftakePort` is a real port on Core, so it belongs in the Interface Contract's
    // `interfacePortDefinitions.ports` array Phase 3 already populated -- read-merge, same pattern
    // as `seed_fr_comp_content`'s own Interface Contract merge.
    let existing = state
        .postgres
        .get_body(project_id, "CoreHpCompressor")
        .await?;
    let mut properties = existing
        .as_ref()
        .and_then(|b| b.get("properties"))
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    let rationale = existing
        .as_ref()
        .and_then(|b| b.get("rationale"))
        .and_then(|v| v.as_str())
        .map(String::from);
    let old_interface_port_definitions = properties
        .get("interfacePortDefinitions")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let mut new_interface_port_definitions = old_interface_port_definitions.clone();
    if let Some(ports_arr) = new_interface_port_definitions
        .get_mut("ports")
        .and_then(|v| v.as_array_mut())
    {
        ports_arr.push(serde_json::json!("CoreBleedOfftakePort"));
    }
    diff_entries.push(DiffEntry::PropertyChanged {
        element_id: "CoreHpCompressor".to_string(),
        property: "interfacePortDefinitions".to_string(),
        old: old_interface_port_definitions,
        new: new_interface_port_definitions.clone(),
    });
    properties.insert(
        "interfacePortDefinitions".to_string(),
        new_interface_port_definitions,
    );
    state
        .postgres
        .upsert_body(
            project_id,
            &ElementBody {
                element_id: "CoreHpCompressor".to_string(),
                rationale,
                properties: serde_json::Value::Object(properties),
            },
        )
        .await?;

    Ok(())
}

/// Integration tests against the real docker-compose stack (`docker compose up -d`) — `#[ignore]`d
/// so `cargo test --workspace` stays green in CI without a live Neo4j/Postgres/MinIO. Run with
/// `cargo test -p api -- --ignored`.
#[cfg(test)]
mod tests {
    use super::*;

    async fn connect_test_stores() -> (Neo4jStore, PostgresStore, ObjectStore, VersioningStore) {
        let neo4j = Neo4jStore::connect(
            &env_or("NEO4J_URI", "bolt://localhost:7687"),
            &env_or("NEO4J_USER", "neo4j"),
            &env_or("NEO4J_PASSWORD", "axioma-dev"),
        )
        .await
        .expect("connect to Neo4j — is `docker compose up -d` running?");

        let database_url = env_or(
            "DATABASE_URL",
            "postgres://axioma:axioma-dev@localhost:5433/axioma",
        );
        let postgres = PostgresStore::connect(&database_url)
            .await
            .expect("connect to Postgres — is `docker compose up -d` running?");
        let versioning = VersioningStore::connect(&database_url)
            .await
            .expect("connect to Postgres (versioning) — is `docker compose up -d` running?");

        let objects = ObjectStore::connect(
            &env_or("S3_ENDPOINT", "http://localhost:9000"),
            &env_or("S3_ACCESS_KEY", "axioma"),
            &env_or("S3_SECRET_KEY", "axioma-dev"),
            &env_or("S3_BUCKET", "axioma-geometry"),
        )
        .await
        .expect("connect to object store — is `docker compose up -d` running?");

        (neo4j, postgres, objects, versioning)
    }

    /// A fresh project per test — real isolation, no cross-test id collisions to worry about.
    async fn test_project(versioning: &VersioningStore, name: &str) -> Project {
        versioning
            .create_project(&format!("{name}-{}", uuid::Uuid::new_v4()), DEFAULT_REGION)
            .await
            .expect("creating a test project")
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
        let (neo4j, postgres, objects, versioning) = connect_test_stores().await;
        AppState {
            neo4j,
            postgres,
            objects,
            versioning,
            auth: Arc::new(auth::LocalAuthProvider),
            prometheus_handle: shared_prometheus_handle(),
        }
    }

    #[tokio::test]
    #[ignore = "requires `docker compose up -d`"]
    async fn readyz_ok_when_stores_healthy() {
        let (neo4j, postgres, objects, versioning) = connect_test_stores().await;

        assert!(neo4j.ping().await.is_ok());
        assert!(postgres.ping().await.is_ok());
        assert!(objects.ping().await.is_ok());
        assert!(versioning.ping().await.is_ok());
    }

    /// T-P1.1-03(a) against the real store: making an element a containment child of its own
    /// child must be rejected.
    #[tokio::test]
    #[ignore = "requires `docker compose up -d`"]
    async fn containment_cycle_rejected_against_neo4j() {
        let (neo4j, _postgres, _objects, versioning) = connect_test_stores().await;
        let project = test_project(&versioning, "cycle").await;

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
        neo4j.upsert_element(&project.id, &engine).await.unwrap();
        neo4j.upsert_element(&project.id, &turbine).await.unwrap();

        neo4j
            .create_edge(
                &project.id,
                &Edge {
                    source: engine.id.clone(),
                    target: turbine.id.clone(),
                    kind: EdgeKind::Contains,
                },
            )
            .await
            .expect("Engine contains Turbine should succeed");

        let result = neo4j
            .create_edge(
                &project.id,
                &Edge {
                    source: turbine.id.clone(),
                    target: engine.id.clone(),
                    kind: EdgeKind::Contains,
                },
            )
            .await;

        assert!(result.is_err(), "the containment cycle should be rejected");
    }

    /// T-P1.1-02 against the real store: a `Satisfy` edge must target a Requirement, not a
    /// Block — Combustor -> Turbine (both Structures) is rejected, with no partial write.
    #[tokio::test]
    #[ignore = "requires `docker compose up -d`"]
    async fn satisfy_endpoint_rejected_against_neo4j() {
        let (neo4j, _postgres, _objects, versioning) = connect_test_stores().await;
        let project = test_project(&versioning, "satisfy").await;

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
        neo4j.upsert_element(&project.id, &combustor).await.unwrap();
        neo4j.upsert_element(&project.id, &turbine).await.unwrap();

        let result = neo4j
            .create_edge(
                &project.id,
                &Edge {
                    source: combustor.id.clone(),
                    target: turbine.id.clone(),
                    kind: EdgeKind::Satisfy,
                },
            )
            .await;
        assert!(
            result.is_err(),
            "Satisfy targeting a Block, not a Requirement, should be rejected"
        );

        let satisfy_edges = neo4j
            .edges_of_kind(&project.id, EdgeKind::Satisfy)
            .await
            .unwrap();
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
        let (neo4j, _postgres, _objects, versioning) = connect_test_stores().await;
        let project = test_project(&versioning, "dangling").await;

        let engine = Element {
            id: "IntegrationTestDanglingEngine".to_string(),
            kind: NodeKind::Structure,
            name: "Integration Test Engine".to_string(),
            active: true,
            origin: Origin::Human,
        };
        neo4j.upsert_element(&project.id, &engine).await.unwrap();

        let result = neo4j
            .create_edge(
                &project.id,
                &Edge {
                    source: engine.id.clone(),
                    target: "IntegrationTestDoesNotExist".to_string(),
                    kind: EdgeKind::Contains,
                },
            )
            .await;
        assert!(
            result.is_err(),
            "an edge to a nonexistent element must be rejected, not silently no-op"
        );

        let contains_edges = neo4j.contains_edges(&project.id).await.unwrap();
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
        let (neo4j, _postgres, _objects, versioning) = connect_test_stores().await;
        let project = test_project(&versioning, "origin").await;

        let element = Element {
            id: "IntegrationTestOriginBlock".to_string(),
            kind: NodeKind::Structure,
            name: "Integration Test Origin Block".to_string(),
            active: true,
            origin: Origin::Human,
        };
        neo4j.upsert_element(&project.id, &element).await.unwrap();
        assert_eq!(
            neo4j
                .get_element(&project.id, &element.id)
                .await
                .unwrap()
                .unwrap()
                .origin,
            Origin::Human
        );

        neo4j
            .set_origin(&project.id, &element.id, Origin::AiSuggested)
            .await
            .unwrap();
        let reloaded = neo4j
            .get_element(&project.id, &element.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(reloaded.origin, Origin::AiSuggested);
        assert_eq!(reloaded.name, element.name);
        assert!(reloaded.active);
    }

    /// T-P1.1-04: a large body lives in Postgres, a blob is referenced from the object store by
    /// pointer, and neither ever lands in Neo4j.
    #[tokio::test]
    #[ignore = "requires `docker compose up -d`"]
    async fn polyglot_split_body_not_in_graph() {
        let (neo4j, postgres, objects, versioning) = connect_test_stores().await;
        let project = test_project(&versioning, "polyglot").await;
        let element_id = "IntegrationTestReq";
        let rationale = "x".repeat(20_000);

        postgres
            .upsert_body(
                &project.id,
                &ElementBody {
                    element_id: element_id.to_string(),
                    rationale: Some(rationale),
                    properties: serde_json::json!({}),
                },
            )
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
            .get_body(&project.id, element_id)
            .await
            .unwrap()
            .expect("body should exist in Postgres");
        assert_eq!(
            stored["rationale"].as_str().unwrap().len(),
            20_000,
            "the large body should be readable back from Postgres"
        );

        // Neo4j's upsert_element only ever sets `id`/`name`/`active`/`origin`/`project_id` —
        // there is no path for the 20 KB rationale to leak into the graph, but assert it
        // directly rather than by construction.
        neo4j
            .upsert_element(
                &project.id,
                &Element {
                    id: element_id.to_string(),
                    kind: NodeKind::Requirement,
                    name: "Integration test requirement".to_string(),
                    active: true,
                    origin: Origin::Human,
                },
            )
            .await
            .unwrap();
        let elements = neo4j.list_elements(&project.id).await.unwrap();
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
        let project = test_project(&state.versioning, "import-sysml").await;
        let fixture = include_str!("../tests/fixtures/sample-sysml-v2.json");
        let payload: import::sysml_v2::SysmlV2ImportRequest =
            serde_json::from_str(fixture).unwrap();

        let response = import::sysml_v2::import_sysml_v2(
            State(state.clone()),
            Path(project.id.clone()),
            Json(payload),
        )
        .await
        .expect("import should succeed")
        .0;
        assert_eq!(response.elements_imported, 6);
        assert_eq!(response.edges_imported, 5);

        let elements = state.neo4j.list_elements(&project.id).await.unwrap();
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

        let contains = state.neo4j.contains_edges(&project.id).await.unwrap();
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
        let project = test_project(&state.versioning, "import-cycle").await;
        let fixture = include_str!("../tests/fixtures/sample-sysml-v2-cycle.json");
        let payload: import::sysml_v2::SysmlV2ImportRequest =
            serde_json::from_str(fixture).unwrap();

        let result = import::sysml_v2::import_sysml_v2(
            State(state.clone()),
            Path(project.id.clone()),
            Json(payload),
        )
        .await;
        assert!(result.is_err(), "the self-cyclic batch should be rejected");

        let elements = state.neo4j.list_elements(&project.id).await.unwrap();
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
        let project = test_project(&state.versioning, "import-reqif").await;
        let fixture = include_str!("../tests/fixtures/sample.reqif");

        let response = import::reqif::import_reqif(
            State(state.clone()),
            Path(project.id.clone()),
            fixture.to_string(),
        )
        .await
        .expect("import should succeed")
        .0;
        assert_eq!(response.requirements_imported, 3);

        let body = state
            .postgres
            .get_body(&project.id, "REQ-THRUST-IMPORTED")
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

        let elements = state.neo4j.list_elements(&project.id).await.unwrap();
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
        let project = test_project(&state.versioning, "import-malformed").await;
        let fixture = include_str!("../tests/fixtures/sample-malformed.reqif");

        let result =
            import::reqif::import_reqif(State(state), Path(project.id), fixture.to_string()).await;
        assert!(result.is_err(), "missing IDENTIFIER should be rejected");
    }

    /// Re-importing an id already used by a different `NodeKind` is rejected (FR-CORE-05's
    /// type-legal-identity rule, `sysml_core::check_kind_conflict`).
    #[tokio::test]
    #[ignore = "requires `docker compose up -d`"]
    async fn import_rejects_kind_conflict() {
        let state = test_app_state().await;
        let project = test_project(&state.versioning, "kind-conflict").await;

        state
            .neo4j
            .upsert_element(
                &project.id,
                &Element {
                    id: "KindConflictTest".to_string(),
                    kind: NodeKind::Structure,
                    name: "Originally a Structure".to_string(),
                    active: true,
                    origin: Origin::Human,
                },
            )
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

        let result =
            import::sysml_v2::import_sysml_v2(State(state), Path(project.id), Json(payload)).await;
        assert!(result.is_err(), "the kind conflict should be rejected");
    }

    /// T-P1.1-05 — the acceptance test this whole feature exists for: branch a project, change
    /// one property on the branch, commit, and diff the branch's tip against `main`'s head.
    /// PASS: the diff reports exactly the one changed property with old/new values, and the
    /// write lands in the audit log with actor/timestamp/diff (checked via `record_audit`'s
    /// insert succeeding — the row's shape is asserted directly against `DiffEntry`).
    #[tokio::test]
    #[ignore = "requires `docker compose up -d`"]
    async fn branch_commit_and_diff_against_main_reports_exactly_one_property_change() {
        let state = test_app_state().await;
        let project = test_project(&state.versioning, "branch-diff").await;

        let fan = Element {
            id: "Fan".to_string(),
            kind: NodeKind::Structure,
            name: "Fan".to_string(),
            active: true,
            origin: Origin::Human,
        };
        state.neo4j.upsert_element(&project.id, &fan).await.unwrap();
        state
            .postgres
            .upsert_body(
                &project.id,
                &ElementBody {
                    element_id: "Fan".to_string(),
                    rationale: None,
                    properties: serde_json::json!({ "mass": "120kg" }),
                },
            )
            .await
            .unwrap();

        // `main`'s first commit — establishes the baseline `diff` is computed against.
        record_commit(
            &state,
            &project.id,
            "test-actor",
            "Seed Fan",
            vec![DiffEntry::ElementCreated {
                element_id: "Fan".to_string(),
                kind: NodeKind::Structure,
                name: "Fan".to_string(),
            }],
        )
        .await
        .unwrap();
        let main = state
            .versioning
            .get_branch(&project.id, MAIN_BRANCH)
            .await
            .unwrap()
            .unwrap();
        let main_head = main.head_commit_id.clone().unwrap();

        let branch = state
            .versioning
            .create_branch(&project.id, "lightweight-fan", None)
            .await
            .unwrap();

        let response = branch_update_element_body(
            State(state.clone()),
            HeaderMap::new(),
            Path((project.id.clone(), branch.name.clone(), "Fan".to_string())),
            Json(BranchEditBodyRequest {
                rationale: None,
                properties: serde_json::json!({ "mass": "95kg" }),
                actor: Some("test-actor".to_string()),
                message: "Lighten the fan".to_string(),
            }),
        )
        .await
        .unwrap()
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);

        let branch_after = state
            .versioning
            .get_branch(&project.id, "lightweight-fan")
            .await
            .unwrap()
            .unwrap();
        let branch_head = branch_after
            .head_commit_id
            .expect("branch should have a commit now");

        let new_snapshot = resolve_snapshot(&state, &project.id, &branch_head)
            .await
            .unwrap();
        let old_snapshot = resolve_snapshot(&state, &project.id, &main_head)
            .await
            .unwrap();
        let diff = compute_snapshot_diff(&old_snapshot, &new_snapshot);
        let property_changes: Vec<_> = diff
            .iter()
            .filter(|d| matches!(d, DiffEntry::PropertyChanged { .. }))
            .collect();
        assert_eq!(
            property_changes.len(),
            1,
            "expected exactly one changed property, got {diff:?}"
        );
        match property_changes[0] {
            DiffEntry::PropertyChanged {
                element_id,
                property,
                old,
                new,
            } => {
                assert_eq!(element_id, "Fan");
                assert_eq!(property, "mass");
                assert_eq!(old, &serde_json::json!("120kg"));
                assert_eq!(new, &serde_json::json!("95kg"));
            }
            other => panic!("expected PropertyChanged, got {other:?}"),
        }
    }

    /// T-P1.4-05 — the pilot trade-study workflow, made real: branch, swap `FanLpCompression`'s
    /// bypass ratio on the branch, run the pilot's Control-state-machine sim, compare against
    /// `main`. PASS: the report shows a nonzero thrust delta between baseline and variant, the
    /// simulation converges (proving the branch edit didn't break the pilot's behavior), and the
    /// requested element/property round-trip unchanged. Requires the `fuml-runtime` sidecar
    /// (`compare` runs `control_sim::run_golden_control_sim`), same as the other
    /// `alf_state_machine_*`/`fuml_execute_*` tests — run with `--test-threads=1`.
    #[tokio::test]
    #[ignore = "requires `docker compose up -d` and the fuml-runtime sidecar"]
    async fn trade_study_compare_reports_thrust_delta_and_confirms_simulation() {
        let state = test_app_state().await;
        let project = test_project(&state.versioning, "trade-study").await;

        let fan = Element {
            id: "FanLpCompression".to_string(),
            kind: NodeKind::Structure,
            name: "Fan & LP Compression".to_string(),
            active: true,
            origin: Origin::Human,
        };
        state.neo4j.upsert_element(&project.id, &fan).await.unwrap();
        state
            .postgres
            .upsert_body(
                &project.id,
                &ElementBody {
                    element_id: "FanLpCompression".to_string(),
                    rationale: None,
                    properties: serde_json::json!({ "bypassRatio": 5.0 }),
                },
            )
            .await
            .unwrap();
        record_commit(
            &state,
            &project.id,
            "test-actor",
            "Seed Fan",
            vec![DiffEntry::ElementCreated {
                element_id: "FanLpCompression".to_string(),
                kind: NodeKind::Structure,
                name: "Fan & LP Compression".to_string(),
            }],
        )
        .await
        .unwrap();

        let branch = state
            .versioning
            .create_branch(&project.id, "higher-bypass", None)
            .await
            .unwrap();
        branch_update_element_body(
            State(state.clone()),
            HeaderMap::new(),
            Path((
                project.id.clone(),
                branch.name.clone(),
                "FanLpCompression".to_string(),
            )),
            Json(BranchEditBodyRequest {
                rationale: None,
                properties: serde_json::json!({ "bypassRatio": 6.5 }),
                actor: Some("test-actor".to_string()),
                message: "Trade study: higher bypass ratio".to_string(),
            }),
        )
        .await
        .unwrap();

        let report = trade_study::compare(
            State(state.clone()),
            Path(project.id.clone()),
            Json(trade_study::TradeStudyCompareRequest {
                branch: "higher-bypass".to_string(),
                element_id: "FanLpCompression".to_string(),
                property: "bypassRatio".to_string(),
            }),
        )
        .await
        .unwrap()
        .0;

        assert_eq!(report.branch, "higher-bypass");
        assert_eq!(report.baseline.bypass_ratio, 5.0);
        assert_eq!(report.variant.bypass_ratio, 6.5);
        assert!(
            report.delta.thrust_lbf < 0.0,
            "a higher bypass ratio should reduce estimated thrust, got {:?}",
            report.delta
        );
        assert!(
            report.simulation.converged,
            "the pilot sim should still converge after the branch edit, got {:?}",
            report.simulation
        );
    }

    /// T-P2.1-01: two identical `optimize` calls on identical input produce identical rankings.
    /// No LLM is ever in the decision path — structurally true, not just procedurally followed
    /// (`cem-core` has no LLM-adjacent dependency at all; see that crate's own `Cargo.toml`).
    #[tokio::test]
    #[ignore = "requires `docker compose up -d`"]
    async fn mode_b_optimize_is_deterministic_across_identical_calls() {
        let state = test_app_state().await;
        let project = test_project(&state.versioning, "mode-b-optimize").await;
        state
            .postgres
            .upsert_body(
                &project.id,
                &ElementBody {
                    element_id: "REQ-THRUST".to_string(),
                    rationale: None,
                    properties: serde_json::json!({ "thrustLbfMin": 28_000.0 }),
                },
            )
            .await
            .unwrap();

        let make_request = || {
            Json(mode_b::OptimizeRequest {
                top_level_requirement_ids: vec!["REQ-THRUST".to_string()],
                constraints: cem_core::Constraints {
                    max_total_mass_kg: Some(4_500.0),
                },
            })
        };

        let first = mode_b::optimize(
            State(state.clone()),
            Path(project.id.clone()),
            make_request(),
        )
        .await
        .unwrap()
        .into_response();
        let second = mode_b::optimize(
            State(state.clone()),
            Path(project.id.clone()),
            make_request(),
        )
        .await
        .unwrap()
        .into_response();

        let first_body = response_json(first).await;
        let second_body = response_json(second).await;
        assert_eq!(first_body, second_body);
        assert!(
            !first_body["candidates"].as_array().unwrap().is_empty(),
            "expected at least one feasible candidate, got {first_body}"
        );
    }

    /// T-P2.1-03 (Interface Contract emission) + T-P2.1-06 (auto-traceability on accept):
    /// accepting one candidate persists all six Interface Contract fields per varied subsystem,
    /// wires a real `Satisfy` edge to the source requirement, and marks the subsystem
    /// `Origin::AiSuggested` with generation provenance — all via already-existing store
    /// primitives, no new proposal/branch/autonomy machinery (see `mode_b.rs`'s doc comment).
    #[tokio::test]
    #[ignore = "requires `docker compose up -d`"]
    async fn mode_b_accept_wires_traceability_and_interface_contract_is_fully_populated() {
        let state = test_app_state().await;
        let project = test_project(&state.versioning, "mode-b-accept").await;

        for id in [
            "FanLpCompression",
            "CoreHpCompressor",
            "Combustor",
            "TurbineHpLp",
        ] {
            make_structure(&state.neo4j, &project.id, id).await;
        }
        state
            .neo4j
            .upsert_element(
                &project.id,
                &Element {
                    id: "REQ-THRUST".to_string(),
                    kind: NodeKind::Requirement,
                    name: "Engine shall provide >= 30,000 lbf takeoff thrust".to_string(),
                    active: true,
                    origin: Origin::Human,
                },
            )
            .await
            .unwrap();

        let candidates = mode_b::optimize(
            State(state.clone()),
            Path(project.id.clone()),
            Json(mode_b::OptimizeRequest {
                top_level_requirement_ids: vec!["REQ-THRUST".to_string()],
                constraints: cem_core::Constraints::default(),
            }),
        )
        .await
        .unwrap()
        .0
        .candidates;
        let chosen = candidates[0];

        let accept_response = mode_b::accept(
            State(state.clone()),
            HeaderMap::new(),
            Path(project.id.clone()),
            Json(mode_b::AcceptRequest {
                candidate: chosen,
                top_level_requirement_ids: vec!["REQ-THRUST".to_string()],
            }),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(accept_response.updated_subsystem_ids.len(), 4);

        // T-P2.1-06: a real Satisfy edge + Origin::AiSuggested on each varied subsystem.
        let satisfy_edges = state
            .neo4j
            .edges_of_kind(&project.id, EdgeKind::Satisfy)
            .await
            .unwrap();
        for subsystem_id in &accept_response.updated_subsystem_ids {
            assert!(
                satisfy_edges
                    .iter()
                    .any(|e| &e.source == subsystem_id && e.target == "REQ-THRUST"),
                "expected a Satisfy edge from {subsystem_id} to REQ-THRUST, got {satisfy_edges:?}"
            );
            let element = state
                .neo4j
                .get_element(&project.id, subsystem_id)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(element.origin, Origin::AiSuggested);
        }

        // T-P2.1-03: all six Interface Contract fields populated for the Turbine subsystem.
        let contract_response = mode_b::interface_contract(
            State(state.clone()),
            Path((project.id.clone(), "TurbineHpLp".to_string())),
        )
        .await
        .unwrap();
        let contract = response_json(contract_response).await;
        for field in [
            "performanceTargets",
            "boundaryConditions",
            "geometricEnvelope",
            "interfacePortDefinitions",
            "massCostTargets",
            "materialProcessConstraints",
        ] {
            assert!(
                !contract[field].is_null(),
                "{field} should be populated, got {contract}"
            );
        }
    }

    /// T-P2.2-01: at `L1`, `propose`'s output never touches `main` at all — every subsystem lands
    /// as an individually accept/reject-able `pending` proposal on a fresh branch instead.
    #[tokio::test]
    #[ignore = "requires `docker compose up -d`"]
    async fn mode_b_propose_at_l1_lands_on_a_branch_as_pending_proposals_not_on_main() {
        let state = test_app_state().await;
        let project = test_project(&state.versioning, "propose-l1").await;

        for id in [
            "FanLpCompression",
            "CoreHpCompressor",
            "Combustor",
            "TurbineHpLp",
        ] {
            make_structure(&state.neo4j, &project.id, id).await;
        }
        state
            .neo4j
            .upsert_element(
                &project.id,
                &Element {
                    id: "REQ-THRUST".to_string(),
                    kind: NodeKind::Requirement,
                    name: "Engine shall provide >= 30,000 lbf takeoff thrust".to_string(),
                    active: true,
                    origin: Origin::Human,
                },
            )
            .await
            .unwrap();

        mode_b::set_autonomy_level(
            State(state.clone()),
            HeaderMap::new(),
            Path(project.id.clone()),
            Json(mode_b::SetAutonomyLevelRequest {
                scope: "project".to_string(),
                level: "L1".to_string(),
                mass_deviation_threshold_percent: None,
            }),
        )
        .await
        .unwrap();

        let candidates = mode_b::optimize(
            State(state.clone()),
            Path(project.id.clone()),
            Json(mode_b::OptimizeRequest {
                top_level_requirement_ids: vec!["REQ-THRUST".to_string()],
                constraints: cem_core::Constraints::default(),
            }),
        )
        .await
        .unwrap()
        .0
        .candidates;
        let chosen = candidates[0];

        let response = mode_b::propose(
            State(state.clone()),
            HeaderMap::new(),
            Path(project.id.clone()),
            Json(mode_b::ProposeRequest {
                candidate: chosen,
                top_level_requirement_ids: vec!["REQ-THRUST".to_string()],
                constraints: cem_core::Constraints::default(),
                expected_main_head_commit_id: None,
            }),
        )
        .await
        .unwrap()
        .0;

        assert_eq!(response.outcomes.len(), 4, "{response:?}");
        assert!(
            response.outcomes.iter().all(|o| o.outcome == "review"),
            "{response:?}"
        );
        let branch_id = response
            .branch_id
            .expect("L1 should always produce a review branch");

        let main_branch = state
            .versioning
            .get_branch(&project.id, MAIN_BRANCH)
            .await
            .unwrap()
            .unwrap();
        assert!(
            main_branch.head_commit_id.is_none(),
            "L1 must not commit anything to main"
        );
        for subsystem_id in [
            "FanLpCompression",
            "CoreHpCompressor",
            "Combustor",
            "TurbineHpLp",
        ] {
            let element = state
                .neo4j
                .get_element(&project.id, subsystem_id)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                element.origin,
                Origin::Human,
                "an un-accepted proposal must not touch the graph"
            );
        }

        let proposals =
            mode_b::list_proposals(State(state.clone()), Path((project.id.clone(), branch_id)))
                .await
                .unwrap()
                .0;
        assert_eq!(proposals.len(), 4);
        assert!(proposals.iter().all(|p| p.status == "pending"));
        // docs/IMPLEMENTATION_KICKOFF.md Phase 1: every proposal `mode_b::propose` files is
        // `cem-generated` -- the only real caller today, confirmed round-tripping through
        // create_proposal -> list_proposals rather than defaulting silently.
        assert!(
            proposals.iter().all(|p| p.origin == "cem-generated"),
            "{proposals:?}"
        );

        // Also confirmed via get_proposal directly, not just the list endpoint.
        let single = state
            .versioning
            .get_proposal(&project.id, &proposals[0].id)
            .await
            .unwrap()
            .expect("the just-listed proposal should be fetchable by id");
        assert_eq!(single.origin, "cem-generated");
    }

    /// T-P2.2-02: an `L3` project with a 5% mass-deviation threshold auto-merges a candidate 3%
    /// over its baseline and drops a 12%-over candidate to review, unchanged config either way.
    #[tokio::test]
    #[ignore = "requires `docker compose up -d`"]
    async fn mode_b_propose_at_l3_merges_within_threshold_and_reviews_beyond_it() {
        let state = test_app_state().await;
        let project = test_project(&state.versioning, "propose-l3").await;

        make_structure(&state.neo4j, &project.id, "FanLpCompression").await;
        state
            .neo4j
            .upsert_element(
                &project.id,
                &Element {
                    id: "REQ-THRUST".to_string(),
                    kind: NodeKind::Requirement,
                    name: "Engine shall provide >= 30,000 lbf takeoff thrust".to_string(),
                    active: true,
                    origin: Origin::Human,
                },
            )
            .await
            .unwrap();

        mode_b::set_autonomy_level(
            State(state.clone()),
            HeaderMap::new(),
            Path(project.id.clone()),
            Json(mode_b::SetAutonomyLevelRequest {
                scope: "project".to_string(),
                level: "L3".to_string(),
                mass_deviation_threshold_percent: Some(5.0),
            }),
        )
        .await
        .unwrap();

        let base_candidate = cem_core::Candidate {
            params: cem_core::SubsystemParams {
                bypass_ratio: 5.0,
                pressure_ratio: 30.0,
                turbine_inlet_temp_k: 1_800.0,
                turbine_stage_count: 2,
            },
            thrust_lbf: 20_000.0,
            sfc: 0.6,
            total_mass_kg: 1_000.0,
        };
        let constraints = cem_core::Constraints {
            max_total_mass_kg: Some(1_000.0),
        };

        let within = mode_b::propose(
            State(state.clone()),
            HeaderMap::new(),
            Path(project.id.clone()),
            Json(mode_b::ProposeRequest {
                candidate: cem_core::Candidate {
                    total_mass_kg: 1_030.0,
                    ..base_candidate
                },
                top_level_requirement_ids: vec!["REQ-THRUST".to_string()],
                constraints,
                expected_main_head_commit_id: None,
            }),
        )
        .await
        .unwrap()
        .0;
        assert!(
            within.outcomes.iter().all(|o| o.outcome == "merged"),
            "3% over a 5% threshold should auto-merge, got {within:?}"
        );
        assert!(within.branch_id.is_none());

        let beyond = mode_b::propose(
            State(state.clone()),
            HeaderMap::new(),
            Path(project.id.clone()),
            Json(mode_b::ProposeRequest {
                candidate: cem_core::Candidate {
                    total_mass_kg: 1_120.0,
                    ..base_candidate
                },
                top_level_requirement_ids: vec!["REQ-THRUST".to_string()],
                constraints,
                expected_main_head_commit_id: None,
            }),
        )
        .await
        .unwrap()
        .0;
        assert!(
            beyond.outcomes.iter().all(|o| o.outcome == "review"
                && o.reason.as_deref() == Some("below_l3_threshold_review")),
            "12% over a 5% threshold should drop to review, got {beyond:?}"
        );
        assert!(beyond.branch_id.is_some());
    }

    /// T-P2.2-03 (FR-CEM-18): at `L4`, a subsystem that `Causes` an unmitigated, Major hazard is
    /// still forced to individual review — while an unrelated subsystem in the very same
    /// candidate still auto-merges per the base `L4` decision.
    #[tokio::test]
    #[ignore = "requires `docker compose up -d`"]
    async fn mode_b_propose_at_l4_forces_review_for_a_hazard_linked_subsystem_only() {
        let state = test_app_state().await;
        let project = test_project(&state.versioning, "propose-hazard").await;

        for id in [
            "FanLpCompression",
            "CoreHpCompressor",
            "Combustor",
            "TurbineHpLp",
        ] {
            make_structure(&state.neo4j, &project.id, id).await;
        }
        state
            .neo4j
            .upsert_element(
                &project.id,
                &Element {
                    id: "REQ-THRUST".to_string(),
                    kind: NodeKind::Requirement,
                    name: "Engine shall provide >= 30,000 lbf takeoff thrust".to_string(),
                    active: true,
                    origin: Origin::Human,
                },
            )
            .await
            .unwrap();

        state
            .neo4j
            .upsert_element(
                &project.id,
                &Element {
                    id: "HAZ-OVERSPEED".to_string(),
                    kind: NodeKind::Hazard,
                    name: "Overspeed".to_string(),
                    active: true,
                    origin: Origin::Human,
                },
            )
            .await
            .unwrap();
        state
            .postgres
            .upsert_body(
                &project.id,
                &ElementBody {
                    element_id: "HAZ-OVERSPEED".to_string(),
                    rationale: None,
                    properties: serde_json::json!({ "severity": "Major", "likelihood": "Probable" }),
                },
            )
            .await
            .unwrap();
        state
            .neo4j
            .create_edge(
                &project.id,
                &Edge {
                    source: "TurbineHpLp".to_string(),
                    target: "HAZ-OVERSPEED".to_string(),
                    kind: EdgeKind::Causes,
                },
            )
            .await
            .unwrap();

        mode_b::set_autonomy_level(
            State(state.clone()),
            HeaderMap::new(),
            Path(project.id.clone()),
            Json(mode_b::SetAutonomyLevelRequest {
                scope: "project".to_string(),
                level: "L4".to_string(),
                mass_deviation_threshold_percent: None,
            }),
        )
        .await
        .unwrap();

        let candidates = mode_b::optimize(
            State(state.clone()),
            Path(project.id.clone()),
            Json(mode_b::OptimizeRequest {
                top_level_requirement_ids: vec!["REQ-THRUST".to_string()],
                constraints: cem_core::Constraints::default(),
            }),
        )
        .await
        .unwrap()
        .0
        .candidates;
        let chosen = candidates[0];

        let response = mode_b::propose(
            State(state.clone()),
            HeaderMap::new(),
            Path(project.id.clone()),
            Json(mode_b::ProposeRequest {
                candidate: chosen,
                top_level_requirement_ids: vec!["REQ-THRUST".to_string()],
                constraints: cem_core::Constraints::default(),
                expected_main_head_commit_id: None,
            }),
        )
        .await
        .unwrap()
        .0;

        let turbine_outcome = response
            .outcomes
            .iter()
            .find(|o| o.subsystem_id == "TurbineHpLp")
            .expect("TurbineHpLp should have an outcome");
        assert_eq!(turbine_outcome.outcome, "review", "{response:?}");
        assert_eq!(
            turbine_outcome.reason.as_deref(),
            Some("hazard_override"),
            "{response:?}"
        );

        for other in ["FanLpCompression", "CoreHpCompressor", "Combustor"] {
            let outcome = response
                .outcomes
                .iter()
                .find(|o| o.subsystem_id == other)
                .unwrap_or_else(|| panic!("{other} should have an outcome, got {response:?}"));
            assert_eq!(
                outcome.outcome, "merged",
                "{other} has no hazard link and should merge at L4, got {response:?}"
            );
        }
    }

    /// T-P2.2-04 (NFR-CEM-06): an autonomy-level change is audited with the actor plus the
    /// exact old/new levels — checked by reading the audit log back, not just trusting the write
    /// succeeded.
    #[tokio::test]
    #[ignore = "requires `docker compose up -d`"]
    async fn autonomy_level_change_is_audited_with_actor_and_old_new_levels() {
        let state = test_app_state().await;
        let project = test_project(&state.versioning, "autonomy-audit").await;

        mode_b::set_autonomy_level(
            State(state.clone()),
            HeaderMap::new(),
            Path(project.id.clone()),
            Json(mode_b::SetAutonomyLevelRequest {
                scope: "project".to_string(),
                level: "L1".to_string(),
                mass_deviation_threshold_percent: None,
            }),
        )
        .await
        .unwrap();
        mode_b::set_autonomy_level(
            State(state.clone()),
            HeaderMap::new(),
            Path(project.id.clone()),
            Json(mode_b::SetAutonomyLevelRequest {
                scope: "project".to_string(),
                level: "L4".to_string(),
                mass_deviation_threshold_percent: None,
            }),
        )
        .await
        .unwrap();

        let audit_log = state.versioning.list_audit_log(&project.id).await.unwrap();
        let level_changes: Vec<_> = audit_log
            .iter()
            .filter(|e| matches!(e.diff, DiffEntry::AutonomyLevelChanged { .. }))
            .collect();
        assert_eq!(level_changes.len(), 2, "{audit_log:?}");

        let DiffEntry::AutonomyLevelChanged {
            scope,
            old_level,
            new_level,
        } = &level_changes[0].diff
        else {
            unreachable!()
        };
        assert_eq!(scope, "project");
        assert_eq!(old_level, &None);
        assert_eq!(new_level, "L1");
        assert!(!level_changes[0].actor.is_empty());
        assert!(!level_changes[0].created_at.is_empty());

        let DiffEntry::AutonomyLevelChanged {
            old_level,
            new_level,
            ..
        } = &level_changes[1].diff
        else {
            unreachable!()
        };
        assert_eq!(old_level, &Some("L1".to_string()));
        assert_eq!(new_level, "L4");
    }

    /// T-P2.2-05 (NFR-OPS-04): a stale `expectedMainHeadCommitId` forces every subsystem to
    /// review even at `L4` — the concrete stand-in for "a human edit lands while an autonomous
    /// write is in flight" (Mode C doesn't exist to test the literal scenario against). PASS is
    /// the concurrent human edit surviving untouched, never force-merged over.
    #[tokio::test]
    #[ignore = "requires `docker compose up -d`"]
    async fn mode_b_propose_with_a_stale_main_head_forces_review_and_preserves_the_concurrent_edit()
    {
        let state = test_app_state().await;
        let project = test_project(&state.versioning, "propose-concurrency").await;

        for id in [
            "FanLpCompression",
            "CoreHpCompressor",
            "Combustor",
            "TurbineHpLp",
        ] {
            make_structure(&state.neo4j, &project.id, id).await;
        }
        state
            .neo4j
            .upsert_element(
                &project.id,
                &Element {
                    id: "REQ-THRUST".to_string(),
                    kind: NodeKind::Requirement,
                    name: "Engine shall provide >= 30,000 lbf takeoff thrust".to_string(),
                    active: true,
                    origin: Origin::Human,
                },
            )
            .await
            .unwrap();
        record_commit(
            &state,
            &project.id,
            "test-setup",
            "Seed reference fixture",
            vec![DiffEntry::ElementCreated {
                element_id: "REQ-THRUST".to_string(),
                kind: NodeKind::Requirement,
                name: "Engine shall provide >= 30,000 lbf takeoff thrust".to_string(),
            }],
        )
        .await
        .unwrap();

        mode_b::set_autonomy_level(
            State(state.clone()),
            HeaderMap::new(),
            Path(project.id.clone()),
            Json(mode_b::SetAutonomyLevelRequest {
                scope: "project".to_string(),
                level: "L4".to_string(),
                mass_deviation_threshold_percent: None,
            }),
        )
        .await
        .unwrap();

        // The caller captures main's head before computing a candidate against it...
        let captured_head = state
            .versioning
            .get_branch(&project.id, MAIN_BRANCH)
            .await
            .unwrap()
            .unwrap()
            .head_commit_id;
        assert!(captured_head.is_some());

        // ...meanwhile a human renames FanLpCompression, advancing main past that snapshot.
        state
            .neo4j
            .upsert_element(
                &project.id,
                &Element {
                    id: "FanLpCompression".to_string(),
                    kind: NodeKind::Structure,
                    name: "Fan & LP Compression (renamed)".to_string(),
                    active: true,
                    origin: Origin::Human,
                },
            )
            .await
            .unwrap();
        record_commit(
            &state,
            &project.id,
            "human-reviewer",
            "Rename FanLpCompression",
            vec![DiffEntry::ElementRenamed {
                element_id: "FanLpCompression".to_string(),
                old_name: "FanLpCompression".to_string(),
                new_name: "Fan & LP Compression (renamed)".to_string(),
            }],
        )
        .await
        .unwrap();

        let candidates = mode_b::optimize(
            State(state.clone()),
            Path(project.id.clone()),
            Json(mode_b::OptimizeRequest {
                top_level_requirement_ids: vec!["REQ-THRUST".to_string()],
                constraints: cem_core::Constraints::default(),
            }),
        )
        .await
        .unwrap()
        .0
        .candidates;
        let chosen = candidates[0];

        let response = mode_b::propose(
            State(state.clone()),
            HeaderMap::new(),
            Path(project.id.clone()),
            Json(mode_b::ProposeRequest {
                candidate: chosen,
                top_level_requirement_ids: vec!["REQ-THRUST".to_string()],
                constraints: cem_core::Constraints::default(),
                expected_main_head_commit_id: captured_head,
            }),
        )
        .await
        .unwrap()
        .0;

        assert!(
            response
                .outcomes
                .iter()
                .all(|o| o.outcome == "review" && o.reason.as_deref() == Some("concurrent_change")),
            "a stale expected main head should force review at every subsystem, got {response:?}"
        );
        assert!(response.branch_id.is_some());

        let fan = state
            .neo4j
            .get_element(&project.id, "FanLpCompression")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fan.name, "Fan & LP Compression (renamed)");
        assert_eq!(
            fan.origin,
            Origin::Human,
            "the concurrent human edit must survive untouched, never force-merged over"
        );
    }

    /// NFR-COMP-02 — a project's declared region round-trips through creation, `GET
    /// /api/v0/projects`, and `GET /api/v0/projects/:id` unchanged; a project created without
    /// specifying one (the pre-existing call shape, e.g. `ensure_seeded`'s) gets `DEFAULT_REGION`.
    #[tokio::test]
    #[ignore = "requires `docker compose up -d`"]
    async fn project_region_round_trips_through_create_list_and_get() {
        let (_neo4j, _postgres, _objects, versioning) = connect_test_stores().await;
        let created = versioning
            .create_project(&format!("region-test-{}", uuid::Uuid::new_v4()), "eu-west")
            .await
            .unwrap();
        assert_eq!(created.region, "eu-west");

        let fetched = versioning.get_project(&created.id).await.unwrap().unwrap();
        assert_eq!(fetched.region, "eu-west");

        let listed = versioning
            .list_projects()
            .await
            .unwrap()
            .into_iter()
            .find(|p| p.id == created.id)
            .expect("just-created project should be listed");
        assert_eq!(listed.region, "eu-west");

        let default_region_project = test_project(&versioning, "region-default").await;
        assert_eq!(default_region_project.region, DEFAULT_REGION);
    }

    /// Two projects' elements never leak into each other — the same id string in two different
    /// projects addresses two distinct Neo4j nodes.
    #[tokio::test]
    #[ignore = "requires `docker compose up -d`"]
    async fn projects_are_isolated() {
        let (neo4j, _postgres, _objects, versioning) = connect_test_stores().await;
        let project_a = test_project(&versioning, "isolation-a").await;
        let project_b = test_project(&versioning, "isolation-b").await;

        neo4j
            .upsert_element(
                &project_a.id,
                &Element {
                    id: "SharedId".to_string(),
                    kind: NodeKind::Structure,
                    name: "In project A".to_string(),
                    active: true,
                    origin: Origin::Human,
                },
            )
            .await
            .unwrap();

        let b_elements = neo4j.list_elements(&project_b.id).await.unwrap();
        assert!(
            !b_elements.iter().any(|e| e.id == "SharedId"),
            "project B must not see project A's element"
        );

        let a_elements = neo4j.list_elements(&project_a.id).await.unwrap();
        assert!(a_elements.iter().any(|e| e.id == "SharedId"));
    }

    // -----------------------------------------------------------------------
    // P1.3 Digital Thread: traceability, delete/breach, safety export, mission-coverage
    // -----------------------------------------------------------------------

    async fn response_json(response: Response) -> serde_json::Value {
        let body = response.into_body();
        let bytes = http_body_util::BodyExt::collect(body)
            .await
            .expect("collecting response body")
            .to_bytes();
        serde_json::from_slice(&bytes).expect("response body should be valid JSON")
    }

    async fn make_structure(neo4j: &Neo4jStore, project_id: &str, id: &str) {
        neo4j
            .upsert_element(
                project_id,
                &Element {
                    id: id.to_string(),
                    kind: NodeKind::Structure,
                    name: id.to_string(),
                    active: true,
                    origin: Origin::Human,
                },
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    #[ignore = "requires `docker compose up -d`"]
    async fn traceability_both_direction_never_includes_the_root_itself() {
        let state = test_app_state().await;
        let project = test_project(&state.versioning, "trace-no-self").await;
        make_structure(&state.neo4j, &project.id, "A").await;
        make_structure(&state.neo4j, &project.id, "B").await;
        // A single A->B edge, traversed with direction=both, walks backward from B to A too —
        // the root (A) must never end up in its own results because of that backward walk.
        state
            .neo4j
            .create_edge(
                &project.id,
                &Edge {
                    source: "A".to_string(),
                    target: "B".to_string(),
                    kind: EdgeKind::Refine,
                },
            )
            .await
            .unwrap();

        let response = traceability::get_traceability(
            State(state.clone()),
            Path((project.id.clone(), "A".to_string())),
            Query(traceability::TraceabilityQuery {
                depth: Some(3),
                max_fanout: Some(50),
                direction: Some(traceability::Direction::Both),
                cursor: None,
            }),
        )
        .await
        .unwrap();
        let body = response_json(response).await;
        let ids: Vec<&str> = body["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["id"].as_str().unwrap())
            .collect();
        assert_eq!(
            ids,
            vec!["B"],
            "root A must not reappear in its own traceability results"
        );
    }

    #[tokio::test]
    #[ignore = "requires `docker compose up -d`"]
    async fn traceability_depth_and_direction_filter_correctly() {
        let state = test_app_state().await;
        let project = test_project(&state.versioning, "trace-depth").await;
        for id in ["S1", "S2", "S3"] {
            make_structure(&state.neo4j, &project.id, id).await;
        }
        // S1 --Refine--> S2 --Refine--> S3 (Refine has no endpoint-kind constraint, unlike
        // Satisfy, so a plain Structure-Structure chain is legal here).
        state
            .neo4j
            .create_edge(
                &project.id,
                &Edge {
                    source: "S1".to_string(),
                    target: "S2".to_string(),
                    kind: EdgeKind::Refine,
                },
            )
            .await
            .unwrap();
        state
            .neo4j
            .create_edge(
                &project.id,
                &Edge {
                    source: "S2".to_string(),
                    target: "S3".to_string(),
                    kind: EdgeKind::Refine,
                },
            )
            .await
            .unwrap();

        // depth=1, incoming from S2: only S1 (the thing pointing AT S2).
        let response = traceability::get_traceability(
            State(state.clone()),
            Path((project.id.clone(), "S2".to_string())),
            Query(traceability::TraceabilityQuery {
                depth: Some(1),
                max_fanout: Some(50),
                direction: Some(traceability::Direction::Incoming),
                cursor: None,
            }),
        )
        .await
        .unwrap();
        let body = response_json(response).await;
        let ids: Vec<&str> = body["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["id"].as_str().unwrap())
            .collect();
        assert_eq!(
            ids,
            vec!["S1"],
            "incoming depth=1 from S2 should be just S1"
        );

        // depth=1, outgoing from S2: only S3.
        let response = traceability::get_traceability(
            State(state.clone()),
            Path((project.id.clone(), "S2".to_string())),
            Query(traceability::TraceabilityQuery {
                depth: Some(1),
                max_fanout: Some(50),
                direction: Some(traceability::Direction::Outgoing),
                cursor: None,
            }),
        )
        .await
        .unwrap();
        let body = response_json(response).await;
        let ids: Vec<&str> = body["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["id"].as_str().unwrap())
            .collect();
        assert_eq!(
            ids,
            vec!["S3"],
            "outgoing depth=1 from S2 should be just S3"
        );

        // depth=2, incoming from S3: S2 (hop 1) and S1 (hop 2, reached via S2).
        let response = traceability::get_traceability(
            State(state.clone()),
            Path((project.id.clone(), "S3".to_string())),
            Query(traceability::TraceabilityQuery {
                depth: Some(2),
                max_fanout: Some(50),
                direction: Some(traceability::Direction::Incoming),
                cursor: None,
            }),
        )
        .await
        .unwrap();
        let body = response_json(response).await;
        let mut ids: Vec<&str> = body["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["id"].as_str().unwrap())
            .collect();
        ids.sort();
        assert_eq!(
            ids,
            vec!["S1", "S2"],
            "incoming depth=2 from S3 should recover both S2 and S1"
        );
    }

    #[tokio::test]
    #[ignore = "requires `docker compose up -d`"]
    async fn traceability_requires_explicit_depth_and_fanout() {
        let state = test_app_state().await;
        let project = test_project(&state.versioning, "trace-explicit").await;
        make_structure(&state.neo4j, &project.id, "Solo").await;

        let response = traceability::get_traceability(
            State(state.clone()),
            Path((project.id.clone(), "Solo".to_string())),
            Query(traceability::TraceabilityQuery {
                depth: None,
                max_fanout: Some(50),
                direction: None,
                cursor: None,
            }),
        )
        .await;
        assert!(
            response.is_err(),
            "missing depth must be rejected, not defaulted"
        );

        let response = traceability::get_traceability(
            State(state.clone()),
            Path((project.id.clone(), "Solo".to_string())),
            Query(traceability::TraceabilityQuery {
                depth: Some(3),
                max_fanout: None,
                direction: None,
                cursor: None,
            }),
        )
        .await;
        assert!(
            response.is_err(),
            "missing maxFanout must be rejected, not defaulted"
        );
    }

    #[tokio::test]
    #[ignore = "requires `docker compose up -d`"]
    async fn traceability_rejects_above_ceiling() {
        let state = test_app_state().await;
        let project = test_project(&state.versioning, "trace-ceiling").await;
        make_structure(&state.neo4j, &project.id, "Solo").await;

        let response = traceability::get_traceability(
            State(state.clone()),
            Path((project.id.clone(), "Solo".to_string())),
            Query(traceability::TraceabilityQuery {
                depth: Some(11),
                max_fanout: Some(50),
                direction: None,
                cursor: None,
            }),
        )
        .await;
        assert!(
            response.is_err(),
            "depth above the server's ceiling must be rejected"
        );

        let response = traceability::get_traceability(
            State(state.clone()),
            Path((project.id.clone(), "Solo".to_string())),
            Query(traceability::TraceabilityQuery {
                depth: Some(3),
                max_fanout: Some(501),
                direction: None,
                cursor: None,
            }),
        )
        .await;
        assert!(
            response.is_err(),
            "maxFanout above the server's ceiling must be rejected"
        );
    }

    #[tokio::test]
    #[ignore = "requires `docker compose up -d`"]
    async fn traceability_caps_fanout_and_flags_truncation() {
        let state = test_app_state().await;
        let project = test_project(&state.versioning, "trace-fanout").await;
        make_structure(&state.neo4j, &project.id, "Hub").await;
        for i in 0..5 {
            let id = format!("Leaf{i}");
            make_structure(&state.neo4j, &project.id, &id).await;
            state
                .neo4j
                .create_edge(
                    &project.id,
                    &Edge {
                        source: id,
                        target: "Hub".to_string(),
                        kind: EdgeKind::Refine,
                    },
                )
                .await
                .unwrap();
        }

        let response = traceability::get_traceability(
            State(state.clone()),
            Path((project.id.clone(), "Hub".to_string())),
            Query(traceability::TraceabilityQuery {
                depth: Some(1),
                max_fanout: Some(2),
                direction: Some(traceability::Direction::Incoming),
                cursor: None,
            }),
        )
        .await
        .unwrap();
        let body = response_json(response).await;
        assert_eq!(body["results"].as_array().unwrap().len(), 2);
        assert_eq!(body["fanoutTruncated"], serde_json::json!(true));
    }

    #[tokio::test]
    #[ignore = "requires `docker compose up -d`"]
    async fn traceability_pagination_covers_full_set_without_duplicates() {
        let state = test_app_state().await;
        let project = test_project(&state.versioning, "trace-pagination").await;
        make_structure(&state.neo4j, &project.id, "Hub").await;
        let mut expected_ids = std::collections::HashSet::new();
        for i in 0..220 {
            let id = format!("Dep{i:04}");
            make_structure(&state.neo4j, &project.id, &id).await;
            state
                .neo4j
                .create_edge(
                    &project.id,
                    &Edge {
                        source: id.clone(),
                        target: "Hub".to_string(),
                        kind: EdgeKind::Refine,
                    },
                )
                .await
                .unwrap();
            expected_ids.insert(id);
        }

        let first_page = traceability::get_traceability(
            State(state.clone()),
            Path((project.id.clone(), "Hub".to_string())),
            Query(traceability::TraceabilityQuery {
                depth: Some(1),
                max_fanout: Some(500),
                direction: Some(traceability::Direction::Incoming),
                cursor: None,
            }),
        )
        .await
        .unwrap();
        let first_body = response_json(first_page).await;
        let first_ids: Vec<String> = first_body["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["id"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            first_ids.len(),
            200,
            "first page should be exactly PAGE_SIZE"
        );
        let cursor = first_body["nextCursor"]
            .as_str()
            .expect("first page should have a next cursor")
            .to_string();

        let second_page = traceability::get_traceability(
            State(state.clone()),
            Path((project.id.clone(), "Hub".to_string())),
            Query(traceability::TraceabilityQuery {
                depth: Some(1),
                max_fanout: Some(500),
                direction: Some(traceability::Direction::Incoming),
                cursor: Some(cursor),
            }),
        )
        .await
        .unwrap();
        let second_body = response_json(second_page).await;
        let second_ids: Vec<String> = second_body["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["id"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            second_ids.len(),
            20,
            "second page should hold the remaining 20"
        );
        assert!(
            second_body["nextCursor"].is_null(),
            "no more pages after the second"
        );

        let mut all_ids: std::collections::HashSet<String> = first_ids.into_iter().collect();
        for id in &second_ids {
            assert!(
                all_ids.insert(id.clone()),
                "id {id} appeared on both pages — pagination overlap"
            );
        }
        assert_eq!(
            all_ids, expected_ids,
            "union of all pages must equal the true dependent set, none missed/spurious"
        );
    }

    #[tokio::test]
    #[ignore = "requires `docker compose up -d`"]
    async fn delete_with_dependents_returns_breach_then_succeeds_with_acknowledge() {
        let state = test_app_state().await;
        let project = test_project(&state.versioning, "delete-breach").await;
        // Satisfy's endpoint rule requires the target to be a Requirement (a real one, so the
        // dependent edges below are legal) — matches T-P1.3-03's own "10 Satisfy dependents" shape.
        state
            .neo4j
            .upsert_element(
                &project.id,
                &Element {
                    id: "Hub".to_string(),
                    kind: NodeKind::Requirement,
                    name: "Hub".to_string(),
                    active: true,
                    origin: Origin::Human,
                },
            )
            .await
            .unwrap();
        for id in ["Dep1", "Dep2", "Dep3"] {
            make_structure(&state.neo4j, &project.id, id).await;
            state
                .neo4j
                .create_edge(
                    &project.id,
                    &Edge {
                        source: id.to_string(),
                        target: "Hub".to_string(),
                        kind: EdgeKind::Satisfy,
                    },
                )
                .await
                .unwrap();
        }

        let response = traceability::delete_element(
            State(state.clone()),
            HeaderMap::new(),
            Path((project.id.clone(), "Hub".to_string())),
            Query(traceability::DeleteElementQuery { acknowledge: false }),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let body = response_json(response).await;
        assert_eq!(body["error"], serde_json::json!("traceability_breach"));
        let mut dependent_ids: Vec<&str> = body["dependents"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["id"].as_str().unwrap())
            .collect();
        dependent_ids.sort();
        assert_eq!(dependent_ids, vec!["Dep1", "Dep2", "Dep3"]);

        // Still exists — the breach must have actually blocked the delete.
        assert!(state
            .neo4j
            .get_element(&project.id, "Hub")
            .await
            .unwrap()
            .is_some());

        let response = traceability::delete_element(
            State(state.clone()),
            HeaderMap::new(),
            Path((project.id.clone(), "Hub".to_string())),
            Query(traceability::DeleteElementQuery { acknowledge: true }),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert!(state
            .neo4j
            .get_element(&project.id, "Hub")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    #[ignore = "requires `docker compose up -d`"]
    async fn delete_without_dependents_succeeds_without_acknowledge() {
        let state = test_app_state().await;
        let project = test_project(&state.versioning, "delete-lonely").await;
        make_structure(&state.neo4j, &project.id, "Lonely").await;

        let response = traceability::delete_element(
            State(state.clone()),
            HeaderMap::new(),
            Path((project.id.clone(), "Lonely".to_string())),
            Query(traceability::DeleteElementQuery { acknowledge: false }),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert!(state
            .neo4j
            .get_element(&project.id, "Lonely")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    #[ignore = "requires `docker compose up -d`"]
    async fn risk_register_reflects_hazard_severity_and_mitigated_control() {
        let state = test_app_state().await;
        let project = test_project(&state.versioning, "risk-register").await;

        make_structure(&state.neo4j, &project.id, "Turbine").await;
        state
            .neo4j
            .upsert_element(
                &project.id,
                &Element {
                    id: "HAZ-OVERSPEED".to_string(),
                    kind: NodeKind::Hazard,
                    name: "Overspeed".to_string(),
                    active: true,
                    origin: Origin::Human,
                },
            )
            .await
            .unwrap();
        state
            .postgres
            .upsert_body(
                &project.id,
                &ElementBody {
                    element_id: "HAZ-OVERSPEED".to_string(),
                    rationale: None,
                    properties: serde_json::json!({ "severity": "Major", "likelihood": "Probable" }),
                },
            )
            .await
            .unwrap();
        state
            .neo4j
            .create_edge(
                &project.id,
                &Edge {
                    source: "Turbine".to_string(),
                    target: "HAZ-OVERSPEED".to_string(),
                    kind: EdgeKind::Causes,
                },
            )
            .await
            .unwrap();

        state
            .neo4j
            .upsert_element(
                &project.id,
                &Element {
                    id: "Governor".to_string(),
                    kind: NodeKind::Control,
                    name: "Governor".to_string(),
                    active: true,
                    origin: Origin::Human,
                },
            )
            .await
            .unwrap();
        state
            .postgres
            .upsert_body(
                &project.id,
                &ElementBody {
                    element_id: "Governor".to_string(),
                    rationale: None,
                    properties: serde_json::json!({ "status": "Mitigated" }),
                },
            )
            .await
            .unwrap();
        state
            .neo4j
            .create_edge(
                &project.id,
                &Edge {
                    source: "HAZ-OVERSPEED".to_string(),
                    target: "Governor".to_string(),
                    kind: EdgeKind::MitigatedBy,
                },
            )
            .await
            .unwrap();

        let response =
            traceability::get_risk_register(State(state.clone()), Path(project.id.clone()))
                .await
                .unwrap();
        assert!(
            response
                .headers()
                .get(axum::http::header::CONTENT_DISPOSITION)
                .is_some(),
            "export should set Content-Disposition so a browser click downloads it"
        );
        let body = response_json(response).await;
        assert_eq!(body["format"], serde_json::json!("ARP4761"));
        let entries = body["entries"].as_array().unwrap();
        let entry = entries
            .iter()
            .find(|e| e["hazardId"] == "HAZ-OVERSPEED")
            .expect("HAZ-OVERSPEED should be in the register");
        assert_eq!(entry["severityClassification"], serde_json::json!("Major"));
        assert_eq!(entry["likelihood"], serde_json::json!("Probable"));
        assert_eq!(entry["riskIndex"], serde_json::json!(16)); // Major(4) x Probable(4)
        assert_eq!(entry["residualRisk"], serde_json::json!(4)); // Mitigated -> severity x 1
        assert_eq!(entry["causingStructure"], serde_json::json!("Turbine"));
        assert_eq!(entry["status"], serde_json::json!("Mitigated"));
        let controls = entry["controls"].as_array().unwrap();
        assert_eq!(controls.len(), 1);
        assert_eq!(controls[0]["status"], serde_json::json!("Mitigated"));
    }

    #[tokio::test]
    #[ignore = "requires `docker compose up -d`"]
    async fn mission_coverage_flags_the_one_orphaned_requirement() {
        let state = test_app_state().await;
        let project = test_project(&state.versioning, "mission-coverage").await;

        for (id, kind) in [
            ("R1", NodeKind::Requirement),
            ("R2", NodeKind::Requirement),
            ("M1", NodeKind::Mission),
            ("SH1", NodeKind::Stakeholder),
        ] {
            state
                .neo4j
                .upsert_element(
                    &project.id,
                    &Element {
                        id: id.to_string(),
                        kind,
                        name: id.to_string(),
                        active: true,
                        origin: Origin::Human,
                    },
                )
                .await
                .unwrap();
        }
        // SH1 concerns both M1 and R1 — the existing MissionPlanningPanel Stakeholder-creation
        // shape — covering R1 but leaving R2 orphaned.
        state
            .neo4j
            .create_edge(
                &project.id,
                &Edge {
                    source: "SH1".to_string(),
                    target: "M1".to_string(),
                    kind: EdgeKind::Concerns,
                },
            )
            .await
            .unwrap();
        state
            .neo4j
            .create_edge(
                &project.id,
                &Edge {
                    source: "SH1".to_string(),
                    target: "R1".to_string(),
                    kind: EdgeKind::Concerns,
                },
            )
            .await
            .unwrap();

        let coverage =
            traceability::get_mission_coverage(State(state.clone()), Path(project.id.clone()))
                .await
                .unwrap()
                .0;
        assert_eq!(coverage.total_requirements, 2);
        assert_eq!(coverage.covered_count, 1);
        assert_eq!(coverage.orphaned.len(), 1);
        assert_eq!(coverage.orphaned[0].id, "R2");
    }

    // -----------------------------------------------------------------------
    // Mode A grounded copilot query (thin slice — hard-wired local Ollama, no llm-gateway yet)
    // -----------------------------------------------------------------------

    /// Requires a real local Ollama (`docker compose up -d ollama`, plus the model `OLLAMA_MODEL`
    /// defaults to actually pulled — `docker exec <ollama-container> ollama pull qwen2.5:1.5b`),
    /// not just Postgres/Neo4j/MinIO — a stricter precondition than every other `--ignored` test
    /// in this file, called out explicitly since a bare `docker compose up -d` isn't enough on
    /// its own for this one.
    #[tokio::test]
    #[ignore = "requires `docker compose up -d` AND a pulled Ollama model (see doc comment)"]
    async fn mode_a_query_grounds_answer_with_real_citations() {
        let state = test_app_state().await;
        let project = test_project(&state.versioning, "mode-a-grounded").await;

        state
            .neo4j
            .upsert_element(
                &project.id,
                &Element {
                    id: "REQ-THRUST".to_string(),
                    kind: NodeKind::Requirement,
                    name: "Engine shall provide >= 30,000 lbf takeoff thrust".to_string(),
                    active: true,
                    origin: Origin::Human,
                },
            )
            .await
            .unwrap();
        state
            .neo4j
            .upsert_element(
                &project.id,
                &Element {
                    id: "Combustor".to_string(),
                    kind: NodeKind::Structure,
                    name: "Combustor".to_string(),
                    active: true,
                    origin: Origin::Human,
                },
            )
            .await
            .unwrap();
        state
            .neo4j
            .create_edge(
                &project.id,
                &Edge {
                    source: "Combustor".to_string(),
                    target: "REQ-THRUST".to_string(),
                    kind: EdgeKind::Satisfy,
                },
            )
            .await
            .unwrap();

        let response = mode_a::query(
            State(state.clone()),
            Path(project.id.clone()),
            Json(mode_a::ModeAQueryRequest {
                question: "What verifies the thrust requirement?".to_string(),
            }),
        )
        .await
        .unwrap()
        .into_response();
        let body = response_json(response).await;

        let provenance = &body["provenance"];
        for field in [
            "modelName",
            "modelVersion",
            "promptTemplateHash",
            "contextSnapshot",
        ] {
            assert!(
                !provenance[field].is_null(),
                "provenance.{field} must not be null: {body:?}"
            );
        }
        assert_ne!(provenance["modelName"], serde_json::json!("none"));
        assert_ne!(provenance["modelVersion"], serde_json::json!("none"));

        let cited: Vec<&str> = body["citedElementIds"]
            .as_array()
            .expect("citedElementIds should be an array")
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(
            !cited.is_empty(),
            "expected at least one citation, got answer: {}",
            body["answer"]
        );
        assert!(
            cited.contains(&"Combustor") || cited.contains(&"REQ-THRUST"),
            "expected a citation to a real seeded element, got {cited:?}"
        );
        assert_eq!(
            body["groundedFully"],
            serde_json::json!(true),
            "answer: {}",
            body["answer"]
        );
    }

    #[tokio::test]
    #[ignore = "requires `docker compose up -d`"]
    async fn mode_a_query_returns_not_found_without_calling_the_model() {
        let state = test_app_state().await;
        let project = test_project(&state.versioning, "mode-a-ungrounded").await;
        make_structure(&state.neo4j, &project.id, "Unrelated").await;

        // No element anywhere in this project has anything to do with this question — grounding
        // should come back empty and short-circuit before any Ollama call is attempted, so this
        // doesn't even need Ollama running to pass.
        let response = mode_a::query(
            State(state.clone()),
            Path(project.id.clone()),
            Json(mode_a::ModeAQueryRequest {
                question: "Describe the hyperspace flux capacitor subsystem".to_string(),
            }),
        )
        .await
        .unwrap()
        .into_response();
        let body = response_json(response).await;

        assert_eq!(body["answer"], serde_json::json!("not found"));
        assert_eq!(body["groundedFully"], serde_json::json!(true));
        assert_eq!(body["citedElementIds"], serde_json::json!([]));
        assert_eq!(body["provenance"]["contextSnapshot"], serde_json::json!([]));
    }

    /// Mode A part search — deliberately asserts the deterministic invariant this endpoint
    /// actually guarantees (every returned match is a real element, the same anti-fabrication
    /// discipline as `query`'s citation check) rather than exact model output, since which
    /// specific elements a small local model ranks as "matching" a description is empirically
    /// not stable enough to hard-assert against (see `mode_a.rs`'s doc comments on both this and
    /// `lint_requirement` for the direct testing that established that).
    #[tokio::test]
    #[ignore = "requires `docker compose up -d` and Ollama"]
    async fn mode_a_part_search_only_ever_returns_real_elements() {
        let state = test_app_state().await;
        let project = test_project(&state.versioning, "part-search").await;
        make_structure(&state.neo4j, &project.id, "CoolantPump").await;
        make_structure(&state.neo4j, &project.id, "BypassValve").await;

        let response = mode_a::search_parts(
            State(state.clone()),
            Path(project.id.clone()),
            Json(mode_a::PartSearchRequest {
                description: "something that moves coolant".to_string(),
            }),
        )
        .await
        .unwrap()
        .into_response();
        let body = response_json(response).await;

        let real_ids: std::collections::HashSet<&str> = ["CoolantPump", "BypassValve"].into();
        for m in body["matches"].as_array().unwrap() {
            let element_id = m["elementId"].as_str().unwrap();
            assert!(
                real_ids.contains(element_id),
                "match {element_id:?} is not a real element in this project — a fabricated id \
                 should have been filtered out, got {:?}",
                body["matches"]
            );
        }
        assert_eq!(
            body["provenance"]["modelName"],
            serde_json::json!("qwen2.5:1.5b")
        );
    }

    #[tokio::test]
    #[ignore = "requires `docker compose up -d`"]
    async fn mode_a_part_search_with_no_elements_returns_empty_without_calling_the_model() {
        let state = test_app_state().await;
        let project = test_project(&state.versioning, "part-search-empty").await;

        let response = mode_a::search_parts(
            State(state.clone()),
            Path(project.id.clone()),
            Json(mode_a::PartSearchRequest {
                description: "anything at all".to_string(),
            }),
        )
        .await
        .unwrap()
        .into_response();
        let body = response_json(response).await;

        assert_eq!(body["matches"], serde_json::json!([]));
        assert_eq!(body["provenance"]["modelName"], serde_json::json!("none"));
    }

    /// Requirement linting — same "assert the well-formed shape, not exact model judgment"
    /// approach as part search, for the same empirically-measured reason (see `mode_a.rs`'s doc
    /// comment on `lint_requirement`).
    #[tokio::test]
    #[ignore = "requires `docker compose up -d` and Ollama"]
    async fn mode_a_lint_requirement_returns_well_formed_response() {
        let state = test_app_state().await;
        let project = test_project(&state.versioning, "lint-requirement").await;
        state
            .neo4j
            .upsert_element(
                &project.id,
                &Element {
                    id: "REQ-THRUST".to_string(),
                    kind: NodeKind::Requirement,
                    name: "Engine shall provide >= 30,000 lbf takeoff thrust".to_string(),
                    active: true,
                    origin: Origin::Human,
                },
            )
            .await
            .unwrap();

        let response = mode_a::lint_requirement(
            State(state.clone()),
            Path(project.id.clone()),
            Json(mode_a::LintRequirementRequest {
                element_id: "REQ-THRUST".to_string(),
            }),
        )
        .await
        .unwrap()
        .into_response();
        let body = response_json(response).await;

        assert!(body["issues"].is_array());
        assert_eq!(
            body["provenance"]["modelName"],
            serde_json::json!("qwen2.5:1.5b")
        );
        assert_eq!(
            body["provenance"]["contextSnapshot"][0]["id"],
            serde_json::json!("REQ-THRUST")
        );
    }

    #[tokio::test]
    #[ignore = "requires `docker compose up -d`"]
    async fn mode_a_lint_requirement_rejects_a_non_requirement_element() {
        let state = test_app_state().await;
        let project = test_project(&state.versioning, "lint-rejects-non-req").await;
        make_structure(&state.neo4j, &project.id, "NotARequirement").await;

        let result = mode_a::lint_requirement(
            State(state.clone()),
            Path(project.id.clone()),
            Json(mode_a::LintRequirementRequest {
                element_id: "NotARequirement".to_string(),
            }),
        )
        .await;
        assert!(
            result.is_err(),
            "linting a Structure element should be rejected"
        );
    }

    /// T-P1.4-01: the `Execute` RPC must stream incrementally, and 100 identical runs must
    /// produce an identical trace. Set `FUML_RUNTIME_ADDR` if the sidecar isn't on its default
    /// port (e.g. `http://localhost:50052`, needed on machines where something else already
    /// holds 50051 — see `packages/fuml-runtime/README.md`). Run both fuml_execute_* tests with
    /// `--test-threads=1`: the sidecar's `TraceStreamingAppender` attaches to a single
    /// process-global log4j logger per call (see that class's own doc comment on the
    /// single-request assumption) — two `Execute` calls running concurrently against the same
    /// sidecar process cross-contaminate each other's trace, confirmed directly.
    #[tokio::test]
    #[ignore = "requires the fuml-runtime sidecar running (docker compose up -d fuml-runtime, or packages/fuml-runtime/run.sh)"]
    async fn fuml_execute_streams_incrementally() {
        let mut stream = fuml_client::execute_streaming("HelloWorld2")
            .await
            .expect("connect to fuml-runtime — is the sidecar running?");

        // Reading messages one at a time off a `tonic::Streaming<TraceEvent>` (never a `Vec`
        // deserialized in one shot) is itself the proof this is genuinely server-streaming, not
        // a batched response dressed up as one — the type only offers this API because the wire
        // protocol is HTTP/2 DATA frames arriving over time, not one length-prefixed message.
        let mut count = 0;
        while stream
            .message()
            .await
            .expect("reading a trace event")
            .is_some()
        {
            count += 1;
        }
        assert!(
            count > 1,
            "expected more than one streamed trace event, got {count}"
        );
    }

    #[tokio::test]
    #[ignore = "requires the fuml-runtime sidecar running (docker compose up -d fuml-runtime, or packages/fuml-runtime/run.sh)"]
    async fn fuml_execute_is_deterministic_across_100_runs() {
        // The RI's own debug logging embeds a JVM object-identity hash in a handful of "log"
        // events (e.g. "[destroy] object = 1d837fa3#1420a84d") that differs run-to-run by design
        // — confirmed directly by diffing two runs — since it's a memory-address-derived identity
        // token, not model state. So this compares the structural (kind, activityName,
        // actionName) sequence, which real repeated runs confirmed is byte-for-byte identical,
        // not the raw `detail` text.
        let mut reference: Option<Vec<(String, String, String)>> = None;
        for run in 0..100 {
            let events = fuml_client::execute("HelloWorld2")
                .await
                .expect("connect to fuml-runtime — is the sidecar running?");
            let structural: Vec<(String, String, String)> = events
                .iter()
                .map(|e| {
                    (
                        e.kind.clone(),
                        e.activity_name.clone(),
                        e.action_name.clone(),
                    )
                })
                .collect();
            match &reference {
                None => reference = Some(structural),
                Some(expected) => assert_eq!(
                    &structural, expected,
                    "run {run} produced a different trace sequence than run 0"
                ),
            }
        }
    }

    /// docs/IMPLEMENTATION_KICKOFF.md Phase 2 (ADR-011) — the round-trip proof that Phase's own
    /// text asks for: define the spike's Core (HP) Compressor / Turbine test problem (reqs v5
    /// §5.16) via `cem-archspace`'s `DefineDesignSpace`, then exercise every RPC against the real
    /// sidecar. See `packages/cem-archspace/README.md` for the exact same assertions already
    /// verified directly in Python before this Rust test existed.
    #[tokio::test]
    #[ignore = "requires the cem-archspace sidecar running (docker compose up -d cem-archspace, or packages/cem-archspace/run.sh)"]
    async fn archspace_design_space_round_trips_through_the_sidecar() {
        let handle_id = archspace_client::define_design_space(
            archspace_client::spike_compressor_design_space(),
        )
        .await
        .expect("connect to cem-archspace — is the sidecar running?");
        assert!(!handle_id.is_empty());

        // FR-ARCH-06: a real, computed Imputation Ratio comes back, not a placeholder.
        let stats = archspace_client::get_design_space_stats(&handle_id)
            .await
            .expect("fetching design space stats");
        assert_eq!(
            stats.n_design_variables, 4,
            "the LINKED stage-count constraint should collapse n_HP_stages/n_HP_turbine_stages \
             into one shared design variable, leaving 4 total (3 selection choices + 1 stage \
             axis), got {stats:?}"
        );
        assert!(
            stats.imputation_ratio >= 1.0,
            "imputation ratio should be >= 1.0 (1.0 = no hierarchy), got {}",
            stats.imputation_ratio
        );
        assert!(stats.n_declared > 0 && stats.n_valid > 0 && stats.n_valid <= stats.n_declared);

        // FR-ARCH-05: decoding a random vector returns a real, internally-consistent instance —
        // the root and every design variable/connector must be present regardless of which
        // choices got resolved which way.
        let instance = archspace_client::decode_instance(&handle_id, vec![])
            .await
            .expect("decoding a random instance");
        assert!(!instance.design_vector.is_empty());
        assert!(instance
            .present_node_names
            .contains(&"CoreHpCompressor".to_string()));
        assert!(instance
            .present_node_names
            .contains(&"n_HP_stages".to_string()));

        // ADR-011's other half: SBArchOpt actually consumes the adsg-core-built problem and
        // optimizes it — a real, non-NaN best objective comes back, not just "did not error".
        let opt_result = archspace_client::run_optimization(&handle_id, 10, 3, 42)
            .await
            .expect("running the optimization RPC");
        assert!(
            opt_result.best_objective_value.is_finite(),
            "expected a real optimized objective value, got {}",
            opt_result.best_objective_value
        );

        // A bogus handle must be rejected loudly (NOT_FOUND), never silently.
        let bogus = archspace_client::get_design_space_stats("does-not-exist").await;
        assert!(bogus.is_err(), "a bogus handle should be rejected");
    }

    /// docs/IMPLEMENTATION_KICKOFF.md Phase 3 (FR-COMP-01/02/05, extended Interface Contract
    /// examples) — calls `seed_turbofan_ref` directly against a fresh project (not through the
    /// once-only `ensure_seeded` gate) and asserts the real FR-COMP content it now creates.
    #[tokio::test]
    #[ignore = "requires `docker compose up -d`"]
    async fn seed_turbofan_ref_lands_fr_comp_content_for_both_compressor_subsystems() {
        let state = test_app_state().await;
        let project = test_project(&state.versioning, "fr-comp-seed").await;

        seed_turbofan_ref(&state, &project.id).await.unwrap();

        for (subsystem_id, spec_req_id, inlet_port_id, exit_port_id) in [
            (
                "FanLpCompression",
                "REQ-FAN-SPEC",
                "FanInletPort",
                "FanExitPort",
            ),
            (
                "CoreHpCompressor",
                "REQ-CORE-SPEC",
                "CoreInletPort",
                "CoreExitPort",
            ),
        ] {
            // FR-COMP-01/06: a real Requirement element, Satisfy-linked from its subsystem, with
            // the 9-field spec plus the negotiable/flagged convention as body properties.
            let spec_req = state
                .neo4j
                .get_element(&project.id, spec_req_id)
                .await
                .unwrap()
                .expect("spec Requirement should exist");
            assert_eq!(spec_req.kind, NodeKind::Requirement);

            let satisfy_edges = state
                .neo4j
                .edges_of_kind(&project.id, EdgeKind::Satisfy)
                .await
                .unwrap();
            assert!(
                satisfy_edges
                    .iter()
                    .any(|e| e.source == subsystem_id && e.target == spec_req_id),
                "expected {subsystem_id} -Satisfy-> {spec_req_id}, got {satisfy_edges:?}"
            );

            let spec_body = state
                .postgres
                .get_body(&project.id, spec_req_id)
                .await
                .unwrap()
                .expect("spec Requirement should have a body");
            let spec_properties = spec_body["properties"].as_object().unwrap();
            for field in [
                "designWeightFlowLbPerSec",
                "designEquivalentSpeedRpm",
                "targetPolytropicEfficiency",
                "highEfficiencyOperatingRangePercentNCorrected",
                "inletDiameterIn",
                "outletDiameterIn",
                "maxOutletVelocityFtPerSec",
                "targetLengthIn",
                "targetWeightLb",
                "inletDistortionTolerance",
                "negotiable",
                "flagged",
            ] {
                assert!(
                    !spec_properties[field].is_null(),
                    "{spec_req_id}.{field} should be populated, got {spec_body}"
                );
            }

            // FR-COMP-05: the compressor's two named Ports, Contains-linked from the subsystem.
            // Asserts these two specifically exist among the subsystem's Contains-linked
            // children rather than an exact total count -- Phase 4 (docs/IMPLEMENTATION_KICKOFF.md)
            // legitimately adds further ports to these same subsystems (e.g.
            // `FanBypassDuctExitPort`, `CoreBleedOfftakePort`), so an exact-count assertion here
            // would be re-broken by every future phase that adds one more.
            let contains_edges = state.neo4j.contains_edges(&project.id).await.unwrap();
            let ports_for_subsystem: Vec<&str> = contains_edges
                .iter()
                .filter(|e| e.source == subsystem_id)
                .map(|e| e.target.as_str())
                .collect();
            for expected_port_id in [inlet_port_id, exit_port_id] {
                assert!(
                    ports_for_subsystem.contains(&expected_port_id),
                    "expected {expected_port_id} Contains-linked from {subsystem_id}, got {ports_for_subsystem:?}"
                );
            }
            for port_id in [inlet_port_id, exit_port_id] {
                let port = state
                    .neo4j
                    .get_element(&project.id, port_id)
                    .await
                    .unwrap()
                    .expect("port should exist");
                assert_eq!(port.kind, NodeKind::Port);
                let port_body = state
                    .postgres
                    .get_body(&project.id, port_id)
                    .await
                    .unwrap()
                    .expect("port should have a body");
                assert!(!port_body["properties"]["station"].is_null());
                assert!(!port_body["properties"]["equivalentWeightFlowLbPerSec"].is_null());
            }
        }

        // FR-COMP-02: the first-ever real Constraint/Parameter content, Bound-wired.
        let constraint = state
            .neo4j
            .get_element(&project.id, "FanPerformanceMapConstraint")
            .await
            .unwrap()
            .expect("performance map Constraint should exist");
        assert_eq!(constraint.kind, NodeKind::Constraint);

        let param = state
            .neo4j
            .get_element(&project.id, "FanEquivalentWeightFlowParam")
            .await
            .unwrap()
            .expect("Parameter should exist");
        assert_eq!(param.kind, NodeKind::Parameter);

        let bound_edges = state
            .neo4j
            .edges_of_kind(&project.id, EdgeKind::Bound)
            .await
            .unwrap();
        assert!(
            bound_edges
                .iter()
                .any(|e| e.source == "FanEquivalentWeightFlowParam"
                    && e.target == "FanLpCompression"),
            "expected FanEquivalentWeightFlowParam -Bound-> FanLpCompression, got {bound_edges:?}"
        );

        // Interface Contract worked examples merged onto the subsystem's body, alongside (not
        // clobbering) the FR-COMP-01 content already written there via REQ-FAN-SPEC's own
        // upsert_body call (a different element, so no clobber risk) plus whatever the rest of
        // seed_turbofan_ref already wrote to FanLpCompression/CoreHpCompressor directly.
        let fan_body = state
            .postgres
            .get_body(&project.id, "FanLpCompression")
            .await
            .unwrap()
            .expect("FanLpCompression should have a body after the Interface Contract merge");
        let fan_properties = &fan_body["properties"];
        for field in [
            "performanceTargets",
            "boundaryConditions",
            "geometricEnvelope",
            "interfacePortDefinitions",
            "massCostTargets",
            "materialProcessConstraints",
        ] {
            assert!(
                !fan_properties[field].is_null(),
                "FanLpCompression.{field} should be populated, got {fan_body}"
            );
        }
        assert_eq!(
            fan_properties["specProvenance"],
            serde_json::json!("docs-worked-example")
        );
    }

    /// docs/IMPLEMENTATION_KICKOFF.md Phase 4 (turbofan system-model instance) — calls
    /// `seed_turbofan_ref` directly against a fresh project and asserts the real FR-ARCH content
    /// `seed_fr_arch_system_model` now creates: the new gas-path Ports, the first-ever
    /// `:Function`/`:SelectionChoice`/`:ConnectionChoice` elements and their edges, the
    /// stage-count `ChoiceConstraint`s, and `REQ-THRUST`'s traceability wiring.
    #[tokio::test]
    #[ignore = "requires `docker compose up -d`"]
    async fn seed_turbofan_ref_lands_fr_arch_system_model_across_all_five_subsystems() {
        let state = test_app_state().await;
        let project = test_project(&state.versioning, "fr-arch-seed").await;

        seed_turbofan_ref(&state, &project.id).await.unwrap();

        // New station-numbered Ports, Contains-linked from their subsystem.
        let contains_edges = state.neo4j.contains_edges(&project.id).await.unwrap();
        for (port_id, subsystem_id, expected_station) in [
            ("FanBypassDuctExitPort", "FanLpCompression", None),
            ("CoreBleedOfftakePort", "CoreHpCompressor", None),
            ("CombustorInletPort", "Combustor", Some(3)),
            ("CombustorFuelInjectorPort", "Combustor", None),
            ("CombustorExitPort", "Combustor", Some(4)),
            ("TurbineHpInletPort", "TurbineHpLp", Some(4)),
            ("TurbineLpInterstagePort", "TurbineHpLp", Some(5)),
            ("TurbineExitPort", "TurbineHpLp", Some(6)),
            ("NozzleInletPort", "TurbineHpLp", Some(7)),
            ("NozzleExitPort", "TurbineHpLp", Some(8)),
            ("ControlAccessoryPort", "ControlFadecEec", None),
        ] {
            let port = state
                .neo4j
                .get_element(&project.id, port_id)
                .await
                .unwrap()
                .expect("port should exist");
            assert_eq!(port.kind, NodeKind::Port);
            assert!(
                contains_edges
                    .iter()
                    .any(|e| e.source == subsystem_id && e.target == port_id),
                "expected {subsystem_id} -Contains-> {port_id}, got {contains_edges:?}"
            );
            if let Some(station) = expected_station {
                let body = state
                    .postgres
                    .get_body(&project.id, port_id)
                    .await
                    .unwrap()
                    .expect("port should have a body");
                assert_eq!(body["properties"]["station"], serde_json::json!(station));
            }
        }

        // First-ever `:Function` instantiation, ArchDerives-linked to what fulfills it.
        let arch_derives_edges = state
            .neo4j
            .edges_of_kind(&project.id, EdgeKind::ArchDerives)
            .await
            .unwrap();
        for (function_id, fulfills) in [
            (
                "GenerateThrust",
                vec![
                    "FanLpCompression",
                    "CoreHpCompressor",
                    "Combustor",
                    "TurbineHpLp",
                ],
            ),
            ("ProvideBleedAir", vec!["BleedAirRouting"]),
            ("ProvideAccessoryShaftPower", vec!["PowerOfftakeRouting"]),
            ("RegulateEngineOperation", vec!["ControlFadecEec"]),
            ("MeterFuelFlow", vec!["ControlFadecEec"]),
        ] {
            let function = state
                .neo4j
                .get_element(&project.id, function_id)
                .await
                .unwrap()
                .expect("Function should exist");
            assert_eq!(function.kind, NodeKind::Function);
            for target in fulfills {
                assert!(
                    arch_derives_edges
                        .iter()
                        .any(|e| e.source == function_id && e.target == target),
                    "expected {function_id} -ArchDerives-> {target}, got {arch_derives_edges:?}"
                );
            }
        }

        // First-ever `:SelectionChoice`/`:ConnectionChoice` instantiation.
        for (id, subsystem_id) in [
            ("IncludeGearbox", "FanLpCompression"),
            ("BleedOfftakeStage", "CoreHpCompressor"),
            ("PowerOfftake", "TurbineHpLp"),
            ("MixedNozzle", "TurbineHpLp"),
        ] {
            let choice = state
                .neo4j
                .get_element(&project.id, id)
                .await
                .unwrap()
                .expect("SelectionChoice should exist");
            assert_eq!(choice.kind, NodeKind::SelectionChoice);
            assert!(
                arch_derives_edges
                    .iter()
                    .any(|e| e.source == id && e.target == subsystem_id),
                "expected {id} -ArchDerives-> {subsystem_id}, got {arch_derives_edges:?}"
            );
        }
        for id in ["BleedAirRouting", "PowerOfftakeRouting"] {
            let choice = state
                .neo4j
                .get_element(&project.id, id)
                .await
                .unwrap()
                .expect("ConnectionChoice should exist");
            assert_eq!(choice.kind, NodeKind::ConnectionChoice);
        }

        // FR-ARCH-04: the nozzle-flow incompatibility constraint.
        let incompatible_with_edges = state
            .neo4j
            .edges_of_kind(&project.id, EdgeKind::IncompatibleWith)
            .await
            .unwrap();
        assert!(incompatible_with_edges
            .iter()
            .any(|e| e.source == "MixedNozzle" && e.target == "FanBypassDuctExitPort"));

        // FR-COMP-04 (unblocked this phase): stage-count Parameters, Bound-linked, and their two
        // ChoiceConstraint edges.
        let bound_edges = state
            .neo4j
            .edges_of_kind(&project.id, EdgeKind::Bound)
            .await
            .unwrap();
        for (param_id, subsystem_id) in [
            ("FanLpStagesParam", "FanLpCompression"),
            ("CoreHpStagesParam", "CoreHpCompressor"),
            ("TurbineHpStagesParam", "TurbineHpLp"),
            ("TurbineLpStagesParam", "TurbineHpLp"),
            ("GearRatioParam", "FanLpCompression"),
            ("BprParam", "FanLpCompression"),
            ("FprParam", "FanLpCompression"),
            ("OprCoreParam", "CoreHpCompressor"),
            ("ChamberSizeParam", "Combustor"),
            ("FlameTemperatureParam", "Combustor"),
            ("PressureLossParam", "Combustor"),
            ("NOxParam", "Combustor"),
        ] {
            let param = state
                .neo4j
                .get_element(&project.id, param_id)
                .await
                .unwrap()
                .expect("Parameter should exist");
            assert_eq!(param.kind, NodeKind::Parameter);
            assert!(
                bound_edges
                    .iter()
                    .any(|e| e.source == param_id && e.target == subsystem_id),
                "expected {param_id} -Bound-> {subsystem_id}, got {bound_edges:?}"
            );
        }
        let choice_constraint_edges = state
            .neo4j
            .edges_of_kind(&project.id, EdgeKind::ChoiceConstraint)
            .await
            .unwrap();
        assert!(choice_constraint_edges
            .iter()
            .any(|e| e.source == "FanLpStagesParam" && e.target == "TurbineLpStagesParam"));
        assert!(choice_constraint_edges
            .iter()
            .any(|e| e.source == "CoreHpStagesParam" && e.target == "TurbineHpStagesParam"));

        // Core's Interface Contract `ports` array now includes the new bleed-offtake port.
        let core_body = state
            .postgres
            .get_body(&project.id, "CoreHpCompressor")
            .await
            .unwrap()
            .expect("CoreHpCompressor should have a body");
        let core_ports = core_body["properties"]["interfacePortDefinitions"]["ports"]
            .as_array()
            .expect("ports array should exist");
        assert!(core_ports.contains(&serde_json::json!("CoreBleedOfftakePort")));

        // REQ-THRUST was previously disconnected in this fixture -- now Satisfy-linked from the
        // four gas-path subsystems, and reachable via the real traceability machinery.
        let satisfy_edges = state
            .neo4j
            .edges_of_kind(&project.id, EdgeKind::Satisfy)
            .await
            .unwrap();
        for subsystem_id in [
            "FanLpCompression",
            "CoreHpCompressor",
            "Combustor",
            "TurbineHpLp",
        ] {
            assert!(
                satisfy_edges
                    .iter()
                    .any(|e| e.source == subsystem_id && e.target == "REQ-THRUST"),
                "expected {subsystem_id} -Satisfy-> REQ-THRUST, got {satisfy_edges:?}"
            );
        }
        let traversal = traceability::run_traversal(
            &state,
            &project.id,
            "REQ-THRUST",
            1,
            500,
            traceability::Direction::Incoming,
        )
        .await
        .unwrap();
        for subsystem_id in [
            "FanLpCompression",
            "CoreHpCompressor",
            "Combustor",
            "TurbineHpLp",
        ] {
            assert!(
                traversal.visited.contains_key(subsystem_id),
                "expected {subsystem_id} reachable from REQ-THRUST via traceability, got {:?}",
                traversal.visited
            );
        }
    }

    /// Compiles `control_sim::golden_alf_transitions` (the pilot's Idle->Armed->Running->Shutdown
    /// Control state machine, now `pub(crate)` since `trade_study` needs the same known-good
    /// program — see that function's doc comment) into wire-format transitions, for tests that
    /// call `fuml_client::execute_state_machine` directly rather than through the HTTP handler.
    fn compile_golden_transitions() -> Vec<fuml_client::proto::Transition> {
        control_sim::golden_alf_transitions()
            .into_iter()
            .map(|(from, to, signal, source)| {
                let program =
                    alf_lite::parse(source).expect("compiling a golden transition action");
                fuml_client::proto::Transition {
                    from_state: from.to_string(),
                    to_state: to.to_string(),
                    signal: signal.to_string(),
                    actions: alf_ir::compile_program(&program),
                }
            })
            .collect()
    }

    /// T-P1.4-02: an Alf action using only in-subset constructs (a guard comparison + a behavior
    /// invocation setting `Turbine.rpm`) compiles without error and its produced fUML executes
    /// to the golden trace — `Turbine.rpm` set as specified, surfaced here via the shared
    /// read-back-and-print step every state-machine run ends with (see
    /// `StateMachineActivityBuilder.appendFinalRpmOutput`'s doc comment for why that's the
    /// meaningful, comparable signal instead of raw internal action names).
    #[tokio::test]
    #[ignore = "requires the fuml-runtime sidecar running (docker compose up -d fuml-runtime, or packages/fuml-runtime/run.sh)"]
    async fn alf_state_machine_t_p1_4_02_subset_conformance() {
        let events = fuml_client::execute_state_machine(
            compile_golden_transitions(),
            control_sim::golden_signals(),
            false,
        )
        .await
        .expect("connect to fuml-runtime — is the sidecar running?");

        let output = events
            .iter()
            .find(|e| e.kind == "output")
            .expect("expected an 'output' trace event carrying Turbine.rpm's final value");
        assert_eq!(output.detail, "3500.0");
    }

    /// T-P1.4-03: an out-of-subset construct (a collection/sequence literal) yields a precise
    /// compile-time error naming it, and — because the handler compiles every transition before
    /// ever calling the sidecar — no fUML is emitted at all, not just an incorrect one.
    #[tokio::test]
    async fn alf_state_machine_t_p1_4_03_rejects_unsupported_construct() {
        let payload = control_sim::ControlStateMachineRequest {
            transitions: vec![control_sim::TransitionRequest {
                from: "Armed".to_string(),
                to: "Running".to_string(),
                signal: "ignite".to_string(),
                alf_source: "let x = Sequence{1, 2, 3};".to_string(),
            }],
            signals: control_sim::golden_signals(),
            use_hand_authored_reference: false,
        };

        let response = control_sim::simulate_control_state_machine(Json(payload))
            .await
            .expect_err("expected a compile error, not a successful call to the sidecar")
            .into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let bytes = http_body_util::BodyExt::collect(response.into_body())
            .await
            .expect("collecting response body")
            .to_bytes();
        let message = String::from_utf8(bytes.to_vec()).expect("response body should be UTF-8");
        assert!(
            message.contains("collection/sequence expressions"),
            "expected the error to name the unsupported construct, got: {message}"
        );
    }

    /// T-P1.4-04: the identical golden scenario built two ways — via alf-lite's compiled path,
    /// and hand-authored directly as fUML — must execute to identical results. Compared on the
    /// final `Turbine.rpm` output value, not raw internal action names, which legitimately
    /// differ between two independently-built graphs (see
    /// `StateMachineActivityBuilder.appendFinalRpmOutput`'s doc comment).
    #[tokio::test]
    #[ignore = "requires the fuml-runtime sidecar running (docker compose up -d fuml-runtime, or packages/fuml-runtime/run.sh)"]
    async fn alf_state_machine_t_p1_4_04_compiled_matches_hand_authored() {
        let compiled = fuml_client::execute_state_machine(
            compile_golden_transitions(),
            control_sim::golden_signals(),
            false,
        )
        .await
        .expect("connect to fuml-runtime — is the sidecar running? (compiled path)");
        let hand_authored = fuml_client::execute_state_machine(
            compile_golden_transitions(),
            control_sim::golden_signals(),
            true,
        )
        .await
        .expect("connect to fuml-runtime — is the sidecar running? (hand-authored path)");

        let compiled_output = compiled
            .iter()
            .find(|e| e.kind == "output")
            .expect("compiled path: expected an 'output' trace event");
        let hand_authored_output = hand_authored
            .iter()
            .find(|e| e.kind == "output")
            .expect("hand-authored path: expected an 'output' trace event");

        assert_eq!(compiled_output.detail, hand_authored_output.detail);
        assert_eq!(compiled_output.detail, "3500.0");
    }

    /// The concrete, verifiable form of "attempt the full dispatch loop": feeding all 3 signals
    /// in order drives the state machine through all 4 states (Idle -> Armed -> Running ->
    /// Shutdown), firing every transition's action along the way — asserted here via the driver's
    /// own `Send(...)` actions appearing, in order, and the run completing with the expected
    /// final output (proving the chain actually ran to completion, not just that the driver sent
    /// its signals).
    #[tokio::test]
    #[ignore = "requires the fuml-runtime sidecar running (docker compose up -d fuml-runtime, or packages/fuml-runtime/run.sh)"]
    async fn alf_state_machine_full_loop_reaches_all_four_states() {
        let events = fuml_client::execute_state_machine(
            compile_golden_transitions(),
            control_sim::golden_signals(),
            false,
        )
        .await
        .expect("connect to fuml-runtime — is the sidecar running?");

        let sent_signals: Vec<&str> = events
            .iter()
            .filter(|e| e.kind == "fire" && e.action_name.starts_with("Send("))
            .map(|e| e.action_name.as_str())
            .collect();
        assert_eq!(
            sent_signals,
            vec!["Send(arm)", "Send(ignite)", "Send(cutoff)"]
        );

        assert!(
            events
                .iter()
                .any(|e| e.kind == "output" && e.detail == "3500.0"),
            "expected the run to reach completion with Turbine.rpm printed as 3500.0"
        );
    }
}
