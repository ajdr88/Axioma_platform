//! Mode B deterministic architecture-synthesis optimizer (roadmap: P2.1, FR-CEM-02/08) — thin
//! HTTP wiring around `cem_core`'s pure computation. **Never an LLM** — no handler here ever
//! calls Ollama/any LLM provider, and `cem-core` itself has no such dependency either (see that
//! crate's own doc comment) — what makes T-P2.1-01's "no LLM in the decision path" requirement
//! structurally true, not just procedurally followed.
//!
//! Three endpoints mirror three distinct moments in a trade study:
//! - `optimize` — read-only exploration, no graph writes at all.
//! - `accept` — commits one chosen candidate's parameters to the graph. **This is T-P2.1-06's
//!   entire scope**: a direct, unconditional write (with provenance + an auto-wired `Satisfy`
//!   edge), not a reviewable proposal. The full L0–L4 autonomy policy engine and proposal/branch
//!   review workflow is P2.2's own deliverable ("Contract + Autonomy + Review") — not attempted
//!   here, on purpose, not as an oversight.
//! - `interface_contract` — reads back whatever `accept` last persisted for a subsystem,
//!   formatted per FR-CEM-08's six named fields.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use cem_core::{Candidate, Constraints, Targets};
use sysml_core::{Edge, EdgeKind, ElementBody, Origin};

use crate::{import::BadRequest, record_commit, ApiError, AppState, DiffEntry};

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

    let mut diff_entries = Vec::new();
    let mut updated_subsystem_ids = Vec::new();

    for &subsystem_id in &VARIED_SUBSYSTEMS {
        if state
            .neo4j
            .get_element(&project_id, subsystem_id)
            .await?
            .is_none()
        {
            continue;
        }

        upsert_subsystem_contract(&state, &project_id, subsystem_id, &payload.candidate).await?;
        state
            .neo4j
            .set_origin(&project_id, subsystem_id, Origin::AiSuggested)
            .await?;
        diff_entries.push(DiffEntry::ElementOriginChanged {
            element_id: subsystem_id.to_string(),
            origin: Origin::AiSuggested,
        });

        for requirement_id in &payload.top_level_requirement_ids {
            let edge = Edge {
                source: subsystem_id.to_string(),
                target: requirement_id.clone(),
                kind: EdgeKind::Satisfy,
            };
            state.neo4j.create_edge(&project_id, &edge).await?;
            diff_entries.push(DiffEntry::EdgeCreated {
                source: edge.source,
                target: edge.target,
                kind: edge.kind,
            });
        }

        updated_subsystem_ids.push(subsystem_id.to_string());
    }

    record_commit(
        &state,
        &project_id,
        &state.auth.resolve_actor(&headers)?,
        "Accept Mode B candidate",
        diff_entries,
    )
    .await?;

    Ok(Json(AcceptResponse {
        updated_subsystem_ids,
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
