//! Mode B deterministic architecture-synthesis optimizer (roadmap: P2.1, FR-CEM-02/08) — thin
//! HTTP wiring around `cem_core`'s pure computation. **Never an LLM** — no handler here ever
//! calls Ollama/any LLM provider, and `cem-core` itself has no such dependency either (see that
//! crate's own doc comment) — what makes T-P2.1-01's "no LLM in the decision path" requirement
//! structurally true, not just procedurally followed.
//!
//! Three endpoints mirror three distinct moments in a trade study:
//! - `optimize` — read-only exploration, no graph writes at all.
//! - `accept` — commits one chosen candidate's parameters to the graph directly, unconditionally
//!   (T-P2.1-06's original scope) — no autonomy check, no review. Still useful on its own (e.g. a
//!   reviewer's final confirm action), and it's what `apply_candidate_to_main` factors out for
//!   `propose`/`accept_proposal` (P2.2) to reuse rather than duplicate.
//! - `interface_contract` — reads back whatever `accept` (or a merged proposal) last persisted for
//!   a subsystem, formatted per FR-CEM-08's six named fields.
//!
//! P2.2 (Contract + Autonomy + Review, FR-CEM-16/17/18) adds the autonomy-aware path on top:
//! `propose` runs `crate::autonomy::decide` once per candidate plus a per-subsystem hazard
//! override, auto-merging what it can and filing everything else as an individually
//! accept/reject-able `proposals` row; `list_proposals`/`accept_proposal`/`reject_proposal` are
//! the review side of that; `set_autonomy_level`/`get_autonomy_level` own the L0–L4 config itself.
//!
//! `accept_proposal` (docs/IMPLEMENTATION_KICKOFF.md Phase 5) is also the one shared accept
//! endpoint for `document-import`-origin proposals (`document_import.rs`) — it dispatches to that
//! module's own materialization, never calling an LLM itself (the drafting LLM call already ran
//! earlier, during that pipeline's own async Structuring stage, well before any human accept
//! decision) — the "no LLM in the decision path" invariant above stays structurally true.

use anyhow::Context;
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use cem_core::{Candidate, Constraints, Targets};
use sysml_core::{Edge, EdgeKind, ElementBody, Origin};

use crate::{
    autonomy, document_import, import::BadRequest, record_commit, store::versioning::Proposal,
    ApiError, AppState, DiffEntry, MAIN_BRANCH,
};

/// The four subsystems `cem_core`'s grid actually varies — `accept`/`interface_contract` only
/// ever operate on these (`ControlFadecEec` is deliberately outside the grid, see `cem-core`'s
/// own doc comment).
const VARIED_SUBSYSTEMS: [&str; 4] = [
    cem_core::FAN_LP_COMPRESSION,
    cem_core::CORE_HP_COMPRESSOR,
    cem_core::COMBUSTOR,
    cem_core::TURBINE_HP_LP,
];

#[derive(Debug, serde::Deserialize)]
pub struct OptimizeRequest {
    #[serde(rename = "topLevelRequirementIds")]
    pub(crate) top_level_requirement_ids: Vec<String>,
    #[serde(default)]
    pub(crate) constraints: Constraints,
}

#[derive(Debug, serde::Serialize)]
pub struct OptimizeResponse {
    pub(crate) candidates: Vec<Candidate>,
}

/// `POST /api/v0/projects/:projectId/cem/mode-b/optimize` — reads each referenced requirement's
/// Postgres body properties for numeric targets (`thrustLbfMin`/`sfcMax`, same property-bag
/// pattern Trade Study's `bypassRatio` already established), merges them into one `Targets`, and
/// returns `cem_core::optimize`'s ranked candidates directly. **Pure read — no graph write
/// happens here at all**, matching T-P2.1-02's "Mode C not invoked during exploration" spirit
/// (nothing beyond reads is invoked here, period).
pub async fn optimize(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    Json(payload): Json<OptimizeRequest>,
) -> Result<Json<OptimizeResponse>, ApiError> {
    let mut targets = Targets::default();
    for requirement_id in &payload.top_level_requirement_ids {
        let Some(body) = state.postgres.get_body(&project_id, requirement_id).await? else {
            continue;
        };
        let Some(properties) = body.get("properties").and_then(|v| v.as_object()) else {
            continue;
        };
        if let Some(min_thrust) = properties.get("thrustLbfMin").and_then(|v| v.as_f64()) {
            targets.min_thrust_lbf = Some(min_thrust);
        }
        if let Some(max_sfc) = properties.get("sfcMax").and_then(|v| v.as_f64()) {
            targets.max_sfc = Some(max_sfc);
        }
    }

    let candidates = cem_core::optimize(&targets, &payload.constraints);
    Ok(Json(OptimizeResponse { candidates }))
}

#[derive(Debug, serde::Deserialize)]
pub struct AcceptRequest {
    pub(crate) candidate: Candidate,
    #[serde(rename = "topLevelRequirementIds")]
    pub(crate) top_level_requirement_ids: Vec<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct AcceptResponse {
    #[serde(rename = "updatedSubsystemIds")]
    pub(crate) updated_subsystem_ids: Vec<String>,
}

/// Merges `cem_core::build_interface_contract`'s six fields plus generation provenance into
/// whatever properties a subsystem Block already has — **merged, not wholesale-replaced**,
/// deliberately: `PostgresStore::upsert_body` replaces a row's entire `properties` object, and a
/// naive replace here would silently destroy an unrelated feature's properties on the same
/// element (e.g. Trade Study's `bypassRatio` on this exact same `FanLpCompression` element).
async fn upsert_subsystem_contract(
    state: &AppState,
    project_id: &str,
    subsystem_id: &str,
    candidate: &Candidate,
) -> anyhow::Result<()> {
    let existing = state.postgres.get_body(project_id, subsystem_id).await?;
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

    for (key, value) in cem_core::build_interface_contract(subsystem_id, candidate) {
        properties.insert(key, value);
    }
    properties.insert(
        "modeBProvenance".to_string(),
        serde_json::json!({
            "generatedBy": "cem-core",
            "generatorVersion": cem_core::VERSION,
            "totalMassKg": candidate.total_mass_kg,
            "thrustLbf": candidate.thrust_lbf,
            "sfc": candidate.sfc,
        }),
    );

    state
        .postgres
        .upsert_body(
            project_id,
            &ElementBody {
                element_id: subsystem_id.to_string(),
                rationale,
                properties: serde_json::Value::Object(properties),
            },
        )
        .await?;
    Ok(())
}

/// Shared merge primitive — writes `candidate` to every subsystem in `subsystem_ids` that
/// actually exists in the project, records one commit, and returns which ones were written.
/// Extracted out of what was originally `accept`'s own body (P2.1) so `propose`'s auto-merge path
/// and `accept_proposal` both call this same code instead of duplicating it (P2.2).
async fn apply_candidate_to_main(
    state: &AppState,
    project_id: &str,
    actor: &str,
    candidate: &Candidate,
    top_level_requirement_ids: &[String],
    subsystem_ids: &[&str],
) -> anyhow::Result<Vec<String>> {
    let mut diff_entries = Vec::new();
    let mut updated_subsystem_ids = Vec::new();

    for &subsystem_id in subsystem_ids {
        if state
            .neo4j
            .get_element(project_id, subsystem_id)
            .await?
            .is_none()
        {
            continue;
        }

        upsert_subsystem_contract(state, project_id, subsystem_id, candidate).await?;
        state
            .neo4j
            .set_origin(project_id, subsystem_id, Origin::AiSuggested)
            .await?;
        diff_entries.push(DiffEntry::ElementOriginChanged {
            element_id: subsystem_id.to_string(),
            origin: Origin::AiSuggested,
        });

        for requirement_id in top_level_requirement_ids {
            let edge = Edge {
                source: subsystem_id.to_string(),
                target: requirement_id.clone(),
                kind: EdgeKind::Satisfy,
                metadata: None,
            };
            state.neo4j.create_edge(project_id, &edge).await?;
            diff_entries.push(DiffEntry::EdgeCreated {
                source: edge.source,
                target: edge.target,
                kind: edge.kind,
            });
        }

        updated_subsystem_ids.push(subsystem_id.to_string());
    }

    record_commit(
        state,
        project_id,
        actor,
        "Accept Mode B candidate",
        diff_entries,
    )
    .await?;

    Ok(updated_subsystem_ids)
}

/// `POST /api/v0/projects/:projectId/cem/mode-b/accept` — see the module doc comment for exactly
/// what this does and doesn't do. Subsystems the project doesn't have (a project without the
/// reference fixture's exact ids) are silently skipped rather than failing the whole request —
/// `updatedSubsystemIds` reports which ones actually got written.
pub async fn accept(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project_id): Path<String>,
    Json(payload): Json<AcceptRequest>,
) -> Result<Json<AcceptResponse>, ApiError> {
    if payload.top_level_requirement_ids.is_empty() {
        return Err(BadRequest("topLevelRequirementIds must not be empty".to_string()).into());
    }

    let updated_subsystem_ids = apply_candidate_to_main(
        &state,
        &project_id,
        &state.auth.resolve_actor(&headers)?,
        &payload.candidate,
        &payload.top_level_requirement_ids,
        &VARIED_SUBSYSTEMS,
    )
    .await?;

    Ok(Json(AcceptResponse {
        updated_subsystem_ids,
    }))
}

/// P2.2's project-wide autonomy scope — the only scope this pass's UI/tests exercise (see
/// `store::versioning`'s `autonomy_config` table doc comment for why finer-grained scopes are a
/// natural, not-yet-built extension of the same `scope` column).
const PROJECT_SCOPE: &str = "project";

#[derive(Debug, serde::Deserialize)]
pub struct ProposeRequest {
    pub(crate) candidate: Candidate,
    #[serde(rename = "topLevelRequirementIds")]
    pub(crate) top_level_requirement_ids: Vec<String>,
    #[serde(default)]
    pub(crate) constraints: Constraints,
    /// T-P2.2-05/NFR-OPS-04: the `main` head commit id the caller computed `candidate` against.
    /// `None` opts out of the staleness check entirely (caller doesn't care); `Some` that no
    /// longer matches `main`'s actual head forces every subsystem to `Review`, regardless of
    /// autonomy level — see `autonomy::decide`.
    #[serde(rename = "expectedMainHeadCommitId", default)]
    pub(crate) expected_main_head_commit_id: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct ProposeOutcome {
    #[serde(rename = "subsystemId")]
    pub(crate) subsystem_id: String,
    pub(crate) outcome: &'static str,
    pub(crate) reason: Option<String>,
    #[serde(rename = "proposalId")]
    pub(crate) proposal_id: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct ProposeResponse {
    pub(crate) outcomes: Vec<ProposeOutcome>,
    #[serde(rename = "branchId")]
    pub(crate) branch_id: Option<String>,
}

/// `POST /api/v0/projects/:projectId/cem/mode-b/propose` (T-P2.2-01/02/03/05) — the
/// autonomy-aware alternative to `accept`'s unconditional write. Runs one level/threshold
/// decision for the whole candidate (`autonomy::decide`), then applies FR-CEM-18's hazard
/// override per subsystem on top of it: a subsystem that `Causes` an unmitigated or
/// Major/Catastrophic Hazard is downgraded to individual review even when the base decision (and
/// every other subsystem in this same candidate) is `Merge`. Subsystems landing on `Merge` are
/// written to `main` in one commit via `apply_candidate_to_main`; subsystems landing on `Review`
/// get one `proposals` row each on a single lazily-created branch (never created if nothing needs
/// it) — this is what makes "each element individually accept/reject-able" real per T-P2.2-01.
pub async fn propose(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project_id): Path<String>,
    Json(payload): Json<ProposeRequest>,
) -> Result<Json<ProposeResponse>, ApiError> {
    if payload.top_level_requirement_ids.is_empty() {
        return Err(BadRequest("topLevelRequirementIds must not be empty".to_string()).into());
    }

    let (level, threshold) = autonomy::resolve_level(&state, &project_id, PROJECT_SCOPE).await?;

    let main_branch = state
        .versioning
        .get_branch(&project_id, MAIN_BRANCH)
        .await?
        .ok_or_else(|| anyhow::anyhow!("project has no main branch"))?;
    let main_head_stale = match &payload.expected_main_head_commit_id {
        Some(expected) => Some(expected.as_str()) != main_branch.head_commit_id.as_deref(),
        None => false,
    };
    let base_decision = autonomy::decide(
        level,
        threshold,
        &payload.candidate,
        &payload.constraints,
        main_head_stale,
    );

    let mut merge_ids: Vec<&str> = Vec::new();
    let mut review_entries: Vec<(&str, String)> = Vec::new();

    for &subsystem_id in &VARIED_SUBSYSTEMS {
        if state
            .neo4j
            .get_element(&project_id, subsystem_id)
            .await?
            .is_none()
        {
            continue;
        }

        let decision = if autonomy::hazard_override(&state, &project_id, subsystem_id).await? {
            autonomy::Decision::Review {
                reason: "hazard_override".to_string(),
            }
        } else {
            base_decision.clone()
        };

        match decision {
            autonomy::Decision::Merge => merge_ids.push(subsystem_id),
            autonomy::Decision::Review { reason } => review_entries.push((subsystem_id, reason)),
        }
    }

    let actor = state.auth.resolve_actor(&headers)?;
    let merged_subsystem_ids = if merge_ids.is_empty() {
        Vec::new()
    } else {
        apply_candidate_to_main(
            &state,
            &project_id,
            &actor,
            &payload.candidate,
            &payload.top_level_requirement_ids,
            &merge_ids,
        )
        .await?
    };

    let branch_id = if review_entries.is_empty() {
        None
    } else {
        let branch_name = format!("cem-proposal-{}", uuid::Uuid::new_v4());
        let branch = state
            .versioning
            .create_branch(
                &project_id,
                &branch_name,
                main_branch.head_commit_id.as_deref(),
            )
            .await?;
        Some(branch.id)
    };

    let candidate_json =
        serde_json::to_value(payload.candidate).context("serializing candidate")?;
    let mut outcomes: Vec<ProposeOutcome> = merged_subsystem_ids
        .into_iter()
        .map(|subsystem_id| ProposeOutcome {
            subsystem_id,
            outcome: "merged",
            reason: None,
            proposal_id: None,
        })
        .collect();

    for (subsystem_id, reason) in review_entries {
        let branch_id_ref = branch_id
            .as_ref()
            .expect("branch was created above whenever review_entries is non-empty");
        let proposal = state
            .versioning
            .create_proposal(
                &project_id,
                branch_id_ref,
                subsystem_id,
                &candidate_json,
                &payload.top_level_requirement_ids,
                &reason,
                "cem-generated",
            )
            .await?;
        outcomes.push(ProposeOutcome {
            subsystem_id: subsystem_id.to_string(),
            outcome: "review",
            reason: Some(reason),
            proposal_id: Some(proposal.id),
        });
    }

    Ok(Json(ProposeResponse {
        outcomes,
        branch_id,
    }))
}

/// `GET /api/v0/projects/:projectId/cem/proposals/:branchId` (T-P2.2-01).
pub async fn list_proposals(
    State(state): State<AppState>,
    Path((project_id, branch_id)): Path<(String, String)>,
) -> Result<Json<Vec<Proposal>>, ApiError> {
    Ok(Json(
        state
            .versioning
            .list_proposals(&project_id, &branch_id)
            .await?,
    ))
}

fn proposal_not_found(proposal_id: &str) -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({ "error": format!("no proposal {proposal_id}") })),
    )
        .into_response()
}

/// `POST /api/v0/projects/:projectId/cem/proposals/:proposalId/accept` — the routing/endpoint
/// shape reqs v5 §5.6 calls "reused unchanged" across all three proposal origins, but the
/// materialization it dispatches to is origin-specific: `cem-generated`/`human-authored` (not yet
/// a real producer) apply a Mode B `Candidate` to one subsystem via `apply_candidate_to_main`
/// (`accept`/`propose`'s own auto-merge path reuses the same helper); `document-import`
/// (docs/IMPLEMENTATION_KICKOFF.md Phase 5, FR-CORE-16) creates a batch of new `:Requirement`
/// elements via `document_import::materialize_proposal` instead — a `Candidate` deserialize would
/// simply fail against that origin's real candidate shape (an array of drafted requirements, not a
/// Mode B parameter set), so this dispatch is necessary, not optional.
pub async fn accept_proposal(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project_id, proposal_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let Some(proposal) = state
        .versioning
        .get_proposal(&project_id, &proposal_id)
        .await?
    else {
        return Ok(proposal_not_found(&proposal_id));
    };
    if proposal.status != "pending" {
        return Err(BadRequest(format!(
            "proposal {proposal_id} is already {}",
            proposal.status
        ))
        .into());
    }

    let actor = state.auth.resolve_actor(&headers)?;
    if proposal.origin == "document-import" {
        document_import::materialize_proposal(&state, &project_id, &actor, &proposal.candidate)
            .await?;
    } else {
        let candidate: Candidate = serde_json::from_value(proposal.candidate)
            .context("parsing stored proposal candidate")?;
        apply_candidate_to_main(
            &state,
            &project_id,
            &actor,
            &candidate,
            &proposal.top_level_requirement_ids,
            &[proposal.subsystem_id.as_str()],
        )
        .await?;
    }
    state
        .versioning
        .set_proposal_status(&project_id, &proposal_id, "accepted")
        .await?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// `POST /api/v0/projects/:projectId/cem/proposals/:proposalId/reject` — no graph write, just
/// marks the row so it stops showing up as pending.
pub async fn reject_proposal(
    State(state): State<AppState>,
    Path((project_id, proposal_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let Some(proposal) = state
        .versioning
        .get_proposal(&project_id, &proposal_id)
        .await?
    else {
        return Ok(proposal_not_found(&proposal_id));
    };
    if proposal.status != "pending" {
        return Err(BadRequest(format!(
            "proposal {proposal_id} is already {}",
            proposal.status
        ))
        .into());
    }

    state
        .versioning
        .set_proposal_status(&project_id, &proposal_id, "rejected")
        .await?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Debug, serde::Deserialize)]
pub struct SetAutonomyLevelRequest {
    pub(crate) scope: String,
    pub(crate) level: String,
    #[serde(rename = "massDeviationThresholdPercent", default)]
    pub(crate) mass_deviation_threshold_percent: Option<f64>,
}

/// `PUT /api/v0/projects/:projectId/cem/autonomy-level` (FR-CEM-16/17, NFR-CEM-06). Every change
/// — including the very first one away from the implicit `L0` default — is audited via
/// `record_audit` directly (never `record_commit`: this isn't a graph mutation, see
/// `DiffEntry::AutonomyLevelChanged`'s own doc comment) with the actor/old/new level T-P2.2-04
/// checks for.
pub async fn set_autonomy_level(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project_id): Path<String>,
    Json(payload): Json<SetAutonomyLevelRequest>,
) -> Result<Response, ApiError> {
    payload
        .level
        .parse::<autonomy::Level>()
        .map_err(BadRequest)?;

    let old_level = state
        .versioning
        .get_autonomy_config(&project_id, &payload.scope)
        .await?
        .map(|c| c.level);
    let actor = state.auth.resolve_actor(&headers)?;

    state
        .versioning
        .set_autonomy_config(
            &project_id,
            &payload.scope,
            &payload.level,
            payload.mass_deviation_threshold_percent,
            &actor,
        )
        .await?;
    state
        .versioning
        .record_audit(
            &project_id,
            &actor,
            &DiffEntry::AutonomyLevelChanged {
                scope: payload.scope,
                old_level,
                new_level: payload.level,
            },
        )
        .await?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// `GET /api/v0/projects/:projectId/cem/autonomy-level/:scope` — reports the implicit `L0`/`None`
/// default rather than 404ing when nothing has ever been configured for this scope, matching
/// `autonomy::resolve_level`'s own same default.
pub async fn get_autonomy_level(
    State(state): State<AppState>,
    Path((project_id, scope)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let config = state
        .versioning
        .get_autonomy_config(&project_id, &scope)
        .await?;
    Ok(Json(match config {
        Some(c) => serde_json::json!({
            "scope": c.scope,
            "level": c.level,
            "massDeviationThresholdPercent": c.mass_deviation_threshold_percent,
        }),
        None => serde_json::json!({
            "scope": scope,
            "level": autonomy::Level::DEFAULT.to_string(),
            "massDeviationThresholdPercent": null,
        }),
    }))
}

#[derive(Debug, serde::Serialize)]
pub struct InterfaceContractResponse {
    #[serde(rename = "subsystemId")]
    subsystem_id: String,
    #[serde(rename = "performanceTargets")]
    performance_targets: serde_json::Value,
    #[serde(rename = "boundaryConditions")]
    boundary_conditions: serde_json::Value,
    #[serde(rename = "geometricEnvelope")]
    geometric_envelope: serde_json::Value,
    #[serde(rename = "interfacePortDefinitions")]
    interface_port_definitions: serde_json::Value,
    #[serde(rename = "massCostTargets")]
    mass_cost_targets: serde_json::Value,
    #[serde(rename = "materialProcessConstraints")]
    material_process_constraints: serde_json::Value,
}

fn contract_not_found(subsystem_id: &str) -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({
            "error": format!("no accepted Mode B candidate for {subsystem_id}")
        })),
    )
        .into_response()
}

/// `GET /api/v0/projects/:projectId/cem/mode-b/interface-contract/:subsystemId` — 404 if
/// `accept` was never called for this subsystem (no persisted contract to read back), the same
/// "nothing here yet" shape as this codebase's other GET-by-id endpoints.
pub async fn interface_contract(
    State(state): State<AppState>,
    Path((project_id, subsystem_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let Some(body) = state.postgres.get_body(&project_id, &subsystem_id).await? else {
        return Ok(contract_not_found(&subsystem_id));
    };
    let Some(properties) = body.get("properties").and_then(|v| v.as_object()) else {
        return Ok(contract_not_found(&subsystem_id));
    };
    let Some(performance_targets) = properties.get("performanceTargets").cloned() else {
        return Ok(contract_not_found(&subsystem_id));
    };

    Ok(Json(InterfaceContractResponse {
        subsystem_id,
        performance_targets,
        boundary_conditions: properties
            .get("boundaryConditions")
            .cloned()
            .unwrap_or(serde_json::Value::Null),
        geometric_envelope: properties
            .get("geometricEnvelope")
            .cloned()
            .unwrap_or(serde_json::Value::Null),
        interface_port_definitions: properties
            .get("interfacePortDefinitions")
            .cloned()
            .unwrap_or(serde_json::Value::Null),
        mass_cost_targets: properties
            .get("massCostTargets")
            .cloned()
            .unwrap_or(serde_json::Value::Null),
        material_process_constraints: properties
            .get("materialProcessConstraints")
            .cloned()
            .unwrap_or(serde_json::Value::Null),
    })
    .into_response())
}
