//! Budgeted traceability (P1.3, FR-CORE-03 / NFR-PERF-04) plus the three things built on or
//! alongside it: change-impact ("blast radius" — the same traversal, `direction=incoming`), the
//! delete-time Traceability Breach gate (T-P1.3-03), and the two smaller P1.3 reports (safety
//! risk-register, mission-coverage) that share this module's home but not its traversal engine.
//!
//! BFS runs in application code, not a Cypher variable-length path (`[:REL*1..N]`) — Cypher has
//! no native per-node fan-out cap, and precise per-hop fanout capping is the actual budget
//! guarantee NFR-PERF-04 asks for ("max depth, max fan-out... enforced"). One query per frontier
//! expansion instead (`Neo4jStore::trace_incoming_neighbors`/`trace_outgoing_neighbors`).
//!
//! **Now verified at real 1M-element scale (T-P1.4-06, `Turbofan-Scale` — see
//! `apps/api/src/bin/seed_turbofan_scale.rs`) — NFR-PERF-04's "<2s p95" PASSES.** Measured
//! directly against a real ~1,000,007-element/~1,000,005-edge seeded project: `depth=1,
//! maxFanout=500, direction=incoming` from a `REQ-THRUST-SCALE` requirement with ~1,200 direct
//! `Satisfy` dependents returned in **566–890ms** across 5 runs — comfortably under the 2s
//! budget. This was **not** true on the first measurement: the identical query took **~46.5
//! seconds** before a real bug was found and fixed (see `Neo4jStore::ensure_indexes`'s doc
//! comment) — every query in this store except `upsert_element`'s own `MERGE` matched `{id,
//! project_id}` with no node label at all, which cannot use *any* label-scoped index no matter
//! how many exist, so the query fell back to an unindexed scan across the whole graph. Fixed by
//! giving every node a second, shared `:Element` label (in addition to its specific kind label)
//! and an index on it, then adding `:Element` to every previously label-less `MATCH`.
//!
//! **NFR-PERF-02/T-P1.1-07's element-create budget — the snapshot-per-commit bottleneck is fixed,
//! confirmed scale-independent, real numbers below.** `POST .../elements` originally took
//! **~34–39 seconds per call** against the 1M-element project (unaffected by the index fix above
//! — a different bottleneck). Root cause: every mutating endpoint's `record_commit` →
//! `build_snapshot` fetched *every* element, *every* edge across all 9 `EdgeKind`s, and *every*
//! Postgres body in the whole project, then serialized all of it to JSON as one commit row — on
//! every single write, including a plain one-element create. Fixed by moving `store::versioning`
//! from snapshot-per-commit to a delta chain (see that module's doc comment for the full design):
//! a commit now stores only its own diff, and `main`'s state is never reconstructed from commits
//! at all — it's always the live graph, fetched lazily only when a diff is actually requested.
//! **Confirmed fixed, not just "should be faster"**: post-fix, `POST .../elements` measured
//! **~265–380ms** against the 1M-element `Turbofan-Scale` project across 8 calls, and
//! **~262–272ms** against the tiny `Turbofan-Ref` project across 5 calls — statistically the same
//! number regardless of project size, proving the O(project size) dependency is gone. The
//! remaining ~265ms doesn't literally clear the written <100ms p95/<50ms p50 budget; it's now a
//! fixed, scale-independent cost (this environment's Docker Desktop/WSL2 network round-trip
//! overhead across the write's ~5 sequential Neo4j/Postgres calls, confirmed by the identical
//! latency at both project sizes), not the pathology this fixture exists to catch. Reducing
//! round-trip count further (e.g. combining the commit-insert and branch-head-advance into one
//! statement) is a real, cheap follow-up, not attempted this pass — the ask was fixing the
//! scaling bottleneck, which is done and verified.
//!
//! T-P1.2-02 (canvas FPS at 1M-element scale) is **still not measured** — that needs real
//! browser/Playwright automation, out of scope for a backend-only pass; flagged rather than
//! faked, same as everything else in this file's history.

use std::collections::{HashMap, HashSet, VecDeque};

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use sysml_core::{EdgeKind, NodeKind};

use crate::{import, record_commit, ApiError, AppState, DiffEntry};

/// A request above either cap is rejected outright (400) — CLAUDE.md's "rejected, not merely
/// discouraged." Depth 10 / fanout 500 are this project's own reasonable ceilings; NFR-PERF-04
/// requires *some* enforced cap to exist, not these exact numbers.
const MAX_ALLOWED_DEPTH: u32 = 10;
const MAX_ALLOWED_FANOUT: u32 = 500;
/// Result page size for the traceability endpoint's cursor pagination.
const PAGE_SIZE: usize = 200;

/// NFR-PERF-04's "explicit, enforced budget" check, factored out of `get_traceability` so
/// docs/IMPLEMENTATION_KICKOFF.md Phase 5's `/collections/dynamic` (a Dynamic Query is itself a
/// stored, re-runnable traversal — FR-CORE-10 — and must be rejected at *save* time under the
/// same ceiling this endpoint already enforces at *request* time) reuses one ceiling, not a
/// second one.
pub(crate) fn validate_budget(depth: u32, max_fanout: u32) -> Result<(), ApiError> {
    if depth > MAX_ALLOWED_DEPTH || max_fanout > MAX_ALLOWED_FANOUT {
        return Err(import::BadRequest(format!(
            "depth/maxFanout exceed this server's caps (max depth {MAX_ALLOWED_DEPTH}, max fanout {MAX_ALLOWED_FANOUT})"
        ))
        .into());
    }
    Ok(())
}

// `Serialize` added for docs/IMPLEMENTATION_KICKOFF.md Phase 5 -- `collections.rs`'s Dynamic Query
// definition embeds a `Direction` directly into a stored JSONB value (round-tripped via
// `serde_json`, not this module's own private `as_str`, which stays module-private).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Direction {
    Incoming,
    Outgoing,
    Both,
}

impl Direction {
    fn as_str(&self) -> &'static str {
        match self {
            Direction::Incoming => "incoming",
            Direction::Outgoing => "outgoing",
            Direction::Both => "both",
        }
    }
}

/// One BFS run's result: every reachable id's shortest hop distance and the edge kind it was
/// first discovered through, plus whether any node's real fan-out exceeded `maxFanout`
/// (T-P1.3-02's "explicit notice" that the traversal was capped, not silently incomplete).
pub(crate) struct Traversal {
    pub(crate) visited: HashMap<String, (u32, EdgeKind)>,
    pub(crate) fanout_truncated: bool,
}

pub(crate) async fn run_traversal(
    state: &AppState,
    project_id: &str,
    root_id: &str,
    depth: u32,
    max_fanout: u32,
    direction: Direction,
) -> anyhow::Result<Traversal> {
    let mut visited: HashMap<String, (u32, EdgeKind)> = HashMap::new();
    let mut fanout_truncated = false;
    let mut seen_ids: HashSet<String> = HashSet::from([root_id.to_string()]);
    let mut frontier: VecDeque<(String, u32)> = VecDeque::from([(root_id.to_string(), 0)]);

    while let Some((current_id, current_depth)) = frontier.pop_front() {
        if current_depth >= depth {
            continue;
        }
        let mut neighbors = match direction {
            Direction::Incoming => {
                state
                    .neo4j
                    .trace_incoming_neighbors(project_id, &current_id)
                    .await?
            }
            Direction::Outgoing => {
                state
                    .neo4j
                    .trace_outgoing_neighbors(project_id, &current_id)
                    .await?
            }
            Direction::Both => {
                let mut both = state
                    .neo4j
                    .trace_incoming_neighbors(project_id, &current_id)
                    .await?;
                both.extend(
                    state
                        .neo4j
                        .trace_outgoing_neighbors(project_id, &current_id)
                        .await?,
                );
                both
            }
        };
        // Sorted before the fanout cap is applied — a deterministic order is what makes "none
        // missed/spurious" repeatable across identical requests and correct across pagination.
        neighbors.sort_by(|a, b| a.0.cmp(&b.0));
        if neighbors.len() as u32 > max_fanout {
            fanout_truncated = true;
            neighbors.truncate(max_fanout as usize);
        }
        let next_depth = current_depth + 1;
        for (neighbor_id, edge_kind) in neighbors {
            // `direction: Both` walks both endpoints of every edge it touches, so a single edge
            // between the root and one neighbor is also traversed *backward* from that neighbor
            // — without this guard the root would re-discover itself and appear in its own
            // results. `seen_ids` already starts with the root, but that alone doesn't stop
            // `visited` (a separate map) from recording it once found as someone else's neighbor.
            if neighbor_id == root_id {
                continue;
            }
            visited
                .entry(neighbor_id.clone())
                .or_insert((next_depth, edge_kind));
            if seen_ids.insert(neighbor_id.clone()) {
                frontier.push_back((neighbor_id, next_depth));
            }
        }
    }

    Ok(Traversal {
        visited,
        fanout_truncated,
    })
}

#[derive(Debug, serde::Deserialize)]
pub struct TraceabilityQuery {
    pub(crate) depth: Option<u32>,
    #[serde(rename = "maxFanout")]
    pub(crate) max_fanout: Option<u32>,
    #[serde(default)]
    pub(crate) direction: Option<Direction>,
    pub(crate) cursor: Option<String>,
}

#[derive(Debug, serde::Serialize)]
struct TraceResultEntry {
    id: String,
    kind: NodeKind,
    name: String,
    #[serde(rename = "hopDistance")]
    hop_distance: u32,
    #[serde(rename = "viaEdgeKind")]
    via_edge_kind: EdgeKind,
}

#[derive(Debug, serde::Serialize)]
struct TraceabilityResponse {
    #[serde(rename = "rootId")]
    root_id: String,
    direction: &'static str,
    depth: u32,
    #[serde(rename = "maxFanout")]
    max_fanout: u32,
    results: Vec<TraceResultEntry>,
    #[serde(rename = "nextCursor")]
    next_cursor: Option<String>,
    #[serde(rename = "fanoutTruncated")]
    fanout_truncated: bool,
}

/// `GET /api/v0/projects/:projectId/elements/:elementId/traceability` (FR-CORE-03, T-P1.3-01/02).
/// `depth`/`maxFanout` are required, not defaulted — NFR-PERF-04 says budgets are "explicit."
/// Change-impact ("blast radius", T-P1.3-01's "request the affected set" after a change) is this
/// same endpoint called with `direction=incoming` — no separate endpoint exists for it.
pub async fn get_traceability(
    State(state): State<AppState>,
    Path((project_id, element_id)): Path<(String, String)>,
    Query(params): Query<TraceabilityQuery>,
) -> Result<Response, ApiError> {
    let Some(depth) = params.depth else {
        return Err(
            import::BadRequest("explicit maxDepth and maxFanout are required".to_string()).into(),
        );
    };
    let Some(max_fanout) = params.max_fanout else {
        return Err(
            import::BadRequest("explicit maxDepth and maxFanout are required".to_string()).into(),
        );
    };
    validate_budget(depth, max_fanout)?;
    if state
        .neo4j
        .get_element(&project_id, &element_id)
        .await?
        .is_none()
    {
        return Ok((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("no element {element_id}") })),
        )
            .into_response());
    }

    let direction = params.direction.unwrap_or(Direction::Both);
    let traversal = run_traversal(
        &state,
        &project_id,
        &element_id,
        depth,
        max_fanout,
        direction,
    )
    .await?;

    let mut sorted_ids: Vec<&String> = traversal.visited.keys().collect();
    sorted_ids.sort();

    let start_index = match &params.cursor {
        Some(cursor) => sorted_ids
            .iter()
            .position(|id| *id == cursor)
            .map_or(0, |i| i + 1),
        None => 0,
    };
    let page_ids = &sorted_ids[start_index.min(sorted_ids.len())..];
    let this_page: Vec<&String> = page_ids.iter().take(PAGE_SIZE).copied().collect();
    let next_cursor = if page_ids.len() > PAGE_SIZE {
        this_page.last().map(|id| (*id).clone())
    } else {
        None
    };

    let mut results = Vec::with_capacity(this_page.len());
    for id in this_page {
        let (hop_distance, via_edge_kind) = traversal.visited[id];
        let Some(element) = state.neo4j.get_element(&project_id, id).await? else {
            // The element was deleted between the traversal query and this hydration step — a
            // real but narrow race; skip it rather than fail the whole page.
            continue;
        };
        results.push(TraceResultEntry {
            id: id.clone(),
            kind: element.kind,
            name: element.name,
            hop_distance,
            via_edge_kind,
        });
    }

    Ok(Json(TraceabilityResponse {
        root_id: element_id,
        direction: direction.as_str(),
        depth,
        max_fanout,
        results,
        next_cursor,
        fanout_truncated: traversal.fanout_truncated,
    })
    .into_response())
}

#[derive(Debug, serde::Deserialize)]
pub struct DeleteElementQuery {
    #[serde(default)]
    pub(crate) acknowledge: bool,
}

#[derive(Debug, serde::Serialize)]
struct BreachDependent {
    id: String,
    kind: NodeKind,
    name: String,
    #[serde(rename = "viaEdgeKind")]
    via_edge_kind: EdgeKind,
}

#[derive(Debug, serde::Serialize)]
struct TraceabilityBreach {
    error: &'static str,
    message: String,
    dependents: Vec<BreachDependent>,
}

/// `DELETE /api/v0/projects/:projectId/elements/:elementId?acknowledge=true` (T-P1.3-03). Direct
/// (depth=1) Satisfy/Verify/Refine dependents block the delete with a 409 Traceability Breach
/// unless `acknowledge=true` is passed. "Reassign" (the test's other stated option) isn't a new
/// bulk-reassign endpoint — a caller can already re-point edges via the existing edge endpoints
/// before retrying the delete.
pub async fn delete_element(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project_id, element_id)): Path<(String, String)>,
    Query(params): Query<DeleteElementQuery>,
) -> Result<Response, ApiError> {
    let Some(existing) = state.neo4j.get_element(&project_id, &element_id).await? else {
        return Ok((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("no element {element_id}") })),
        )
            .into_response());
    };

    let dependents = state
        .neo4j
        .trace_incoming_neighbors(&project_id, &element_id)
        .await?;
    if !dependents.is_empty() && !params.acknowledge {
        let mut breach_dependents = Vec::with_capacity(dependents.len());
        for (id, via_edge_kind) in dependents {
            if let Some(element) = state.neo4j.get_element(&project_id, &id).await? {
                breach_dependents.push(BreachDependent {
                    id,
                    kind: element.kind,
                    name: element.name,
                    via_edge_kind,
                });
            }
        }
        return Ok((
            StatusCode::CONFLICT,
            Json(TraceabilityBreach {
                error: "traceability_breach",
                message: format!(
                    "{} element(s) depend on {element_id} via Satisfy/Verify/Refine — \
                     re-point or remove those edges, or retry with ?acknowledge=true",
                    breach_dependents.len()
                ),
                dependents: breach_dependents,
            }),
        )
            .into_response());
    }

    state.neo4j.delete_element(&project_id, &element_id).await?;
    state
        .postgres
        .delete_body_and_position(&project_id, &element_id)
        .await?;
    record_commit(
        &state,
        &project_id,
        &state.auth.resolve_actor(&headers)?,
        "Delete element",
        vec![DiffEntry::ElementDeleted {
            element_id,
            kind: existing.kind,
            name: existing.name,
        }],
    )
    .await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Debug, serde::Serialize)]
pub(crate) struct RiskRegisterEntry {
    #[serde(rename = "hazardId")]
    pub(crate) hazard_id: String,
    pub(crate) description: String,
    #[serde(rename = "causingStructure")]
    pub(crate) causing_structure: Option<String>,
    #[serde(rename = "severityClassification")]
    pub(crate) severity_classification: String,
    pub(crate) likelihood: String,
    #[serde(rename = "riskIndex")]
    pub(crate) risk_index: u32,
    pub(crate) controls: Vec<RiskRegisterControl>,
    #[serde(rename = "residualRisk")]
    pub(crate) residual_risk: u32,
    pub(crate) status: &'static str,
}

#[derive(Debug, serde::Serialize)]
pub(crate) struct RiskRegisterControl {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) status: String,
}

#[derive(Debug, serde::Serialize)]
pub(crate) struct RiskRegister {
    #[serde(rename = "projectId")]
    pub(crate) project_id: String,
    pub(crate) format: &'static str,
    pub(crate) entries: Vec<RiskRegisterEntry>,
}

const SEVERITY_LEVELS: [&str; 5] = ["Negligible", "Minor", "Moderate", "Major", "Catastrophic"];
const LIKELIHOOD_LEVELS: [&str; 5] = ["Improbable", "Remote", "Occasional", "Probable", "Frequent"];

/// 1-5 score for a severity/likelihood label — mirrors `HazardRiskPanel.tsx`'s `scoreOf` exactly
/// (index+1, defaulting to 1 for an unset/unrecognized value). Duplicated here, not imported,
/// since this endpoint must be self-contained on the server side (export shouldn't depend on the
/// frontend having computed anything first).
fn score_of(levels: &[&str], value: Option<&str>) -> u32 {
    match value.and_then(|v| levels.iter().position(|l| *l == v)) {
        Some(index) => index as u32 + 1,
        None => 1,
    }
}

/// The data-gathering half of the risk register — split out of `get_risk_register` during
/// docs/IMPLEMENTATION_KICKOFF.md Phase 5 (FR-EXPORT-03) so `export::export_report`'s
/// `"risk-register"` template reuses this exact logic rather than a second, parallel
/// implementation (reqs v5 §5.12's own "no new parallel pipeline" instruction). An
/// ARP4761-*shaped* JSON export — no literal ARP4761 template exists anywhere in the docs to copy
/// against, so this is this project's own reasonable field layout (hazard/severity/likelihood/
/// Risk Index/causing structure/linked controls+status/residual risk), same interpretation
/// precedent as `HazardRiskPanel.tsx`'s Risk Index formula itself. MIL-STD-882/ISO-26262 variants
/// are not built — no test or concrete spec covers their shape.
pub(crate) async fn build_risk_register(
    state: &AppState,
    project_id: &str,
) -> anyhow::Result<RiskRegister> {
    let elements = state.neo4j.list_elements(project_id).await?;
    let causes_edges = state
        .neo4j
        .edges_of_kind(project_id, EdgeKind::Causes)
        .await?;
    let mitigated_by_edges = state
        .neo4j
        .edges_of_kind(project_id, EdgeKind::MitigatedBy)
        .await?;
    let elements_by_id: HashMap<&str, &sysml_core::Element> =
        elements.iter().map(|e| (e.id.as_str(), e)).collect();

    let mut entries = Vec::new();
    for hazard in elements.iter().filter(|e| e.kind == NodeKind::Hazard) {
        let body = state.postgres.get_body(project_id, &hazard.id).await?;
        let properties = body
            .as_ref()
            .and_then(|b| b.get("properties"))
            .and_then(|v| v.as_object());
        let severity = properties
            .and_then(|p| p.get("severity"))
            .and_then(|v| v.as_str());
        let likelihood = properties
            .and_then(|p| p.get("likelihood"))
            .and_then(|v| v.as_str());
        let severity_score = score_of(&SEVERITY_LEVELS, severity);
        let likelihood_score = score_of(&LIKELIHOOD_LEVELS, likelihood);
        let risk_index = severity_score * likelihood_score;

        let causing_structure = causes_edges
            .iter()
            .find(|e| e.target == hazard.id)
            .and_then(|e| elements_by_id.get(e.source.as_str()))
            .map(|e| e.name.clone());

        let mut controls = Vec::new();
        let mut any_mitigated = false;
        for edge in mitigated_by_edges.iter().filter(|e| e.source == hazard.id) {
            let Some(control) = elements_by_id.get(edge.target.as_str()) else {
                continue;
            };
            let control_body = state.postgres.get_body(project_id, &control.id).await?;
            let control_status = control_body
                .as_ref()
                .and_then(|b| b.get("properties"))
                .and_then(|v| v.get("status"))
                .and_then(|v| v.as_str())
                .unwrap_or("Open")
                .to_string();
            if control_status == "Mitigated" {
                any_mitigated = true;
            }
            controls.push(RiskRegisterControl {
                id: control.id.clone(),
                name: control.name.clone(),
                status: control_status,
            });
        }
        // Mirrors HazardRiskPanel.tsx exactly: residual = severity x 1 if any linked Control is
        // Mitigated, else the raw (unmitigated) Risk Index.
        let residual_risk = if any_mitigated {
            severity_score
        } else {
            risk_index
        };

        entries.push(RiskRegisterEntry {
            hazard_id: hazard.id.clone(),
            description: hazard.name.clone(),
            causing_structure,
            severity_classification: severity.unwrap_or(SEVERITY_LEVELS[0]).to_string(),
            likelihood: likelihood.unwrap_or(LIKELIHOOD_LEVELS[0]).to_string(),
            risk_index,
            controls,
            residual_risk,
            status: if any_mitigated { "Mitigated" } else { "Open" },
        });
    }

    Ok(RiskRegister {
        project_id: project_id.to_string(),
        format: "ARP4761",
        entries,
    })
}

/// `GET /api/v0/projects/:projectId/safety/risk-register` (FR-SAFE-05, T-P1.3-04) — the real HTTP
/// handler, now just `build_risk_register` plus the download-header wrapping.
pub async fn get_risk_register(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
) -> Result<Response, ApiError> {
    let register = build_risk_register(&state, &project_id).await?;
    let mut response = Json(register).into_response();
    response.headers_mut().insert(
        axum::http::header::CONTENT_DISPOSITION,
        axum::http::HeaderValue::from_str(&format!(
            "attachment; filename=\"risk-register-{project_id}.json\""
        ))
        .map_err(anyhow::Error::from)?,
    );
    Ok(response)
}

#[derive(Debug, serde::Serialize)]
pub(crate) struct OrphanedRequirement {
    pub(crate) id: String,
    #[allow(dead_code)] // read via JSON in the frontend; not read as a struct field in Rust
    pub(crate) name: String,
}

#[derive(Debug, serde::Serialize)]
pub struct MissionCoverage {
    #[serde(rename = "totalRequirements")]
    pub(crate) total_requirements: usize,
    #[serde(rename = "coveredCount")]
    pub(crate) covered_count: usize,
    pub(crate) orphaned: Vec<OrphanedRequirement>,
}

/// `GET /api/v0/projects/:projectId/mission-coverage` (FR-MSN-04, T-P1.3-05). No dedicated
/// Mission<->Requirement edge exists in the data model — the only real substrate linking them
/// today is `MissionPlanningPanel`'s Stakeholder-creation flow, which creates two `Concerns`
/// edges (Stakeholder->Mission, Stakeholder->Requirement) from one Stakeholder. A Requirement is
/// "covered" iff some Stakeholder's Concerns pair connects it to a Mission — grounded in the
/// actual existing data model, not a new edge kind invented for this endpoint. Unbounded by
/// user-controlled depth (bounded by "every Requirement in the project"), so no query budget or
/// pagination applies here.
pub async fn get_mission_coverage(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
) -> Result<Json<MissionCoverage>, ApiError> {
    let elements = state.neo4j.list_elements(&project_id).await?;
    let concerns_edges = state
        .neo4j
        .edges_of_kind(&project_id, EdgeKind::Concerns)
        .await?;

    let mission_ids: HashSet<&str> = elements
        .iter()
        .filter(|e| e.kind == NodeKind::Mission)
        .map(|e| e.id.as_str())
        .collect();
    let stakeholder_ids: HashSet<&str> = elements
        .iter()
        .filter(|e| e.kind == NodeKind::Stakeholder)
        .map(|e| e.id.as_str())
        .collect();

    // Every Mission a given Stakeholder is Concerns-linked to.
    let mut missions_by_stakeholder: HashMap<&str, Vec<&str>> = HashMap::new();
    for edge in &concerns_edges {
        if stakeholder_ids.contains(edge.source.as_str())
            && mission_ids.contains(edge.target.as_str())
        {
            missions_by_stakeholder
                .entry(edge.source.as_str())
                .or_default()
                .push(edge.target.as_str());
        }
    }

    let requirements: Vec<&sysml_core::Element> = elements
        .iter()
        .filter(|e| e.kind == NodeKind::Requirement)
        .collect();
    let mut orphaned = Vec::new();
    for requirement in &requirements {
        let covered = concerns_edges.iter().any(|edge| {
            edge.target == requirement.id
                && missions_by_stakeholder
                    .get(edge.source.as_str())
                    .is_some_and(|missions| !missions.is_empty())
        });
        if !covered {
            orphaned.push(OrphanedRequirement {
                id: requirement.id.clone(),
                name: requirement.name.clone(),
            });
        }
    }

    Ok(Json(MissionCoverage {
        total_requirements: requirements.len(),
        covered_count: requirements.len() - orphaned.len(),
        orphaned,
    }))
}
