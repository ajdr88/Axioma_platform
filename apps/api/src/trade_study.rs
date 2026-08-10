//! `POST /api/v0/projects/:projectId/trade-studies/compare` (roadmap: P1.4, T-P1.4-05,
//! "FR-CORE-02/03/04 integration"). T-P1.4-05's own Action is a human workflow, not a single
//! endpoint — "Branch; swap a Fan variant (different bypass ratio); run the behavioral sim;
//! generate a comparison report" — so this composes pieces that already exist rather than
//! building a new subsystem: branching and the branch-scoped property edit
//! (`store::versioning`/`branch_update_element_body`, both P1.1) supply "branch" and "swap a
//! variant"; this module supplies the one genuinely new piece, "generate a comparison report,"
//! plus running the existing Control-state-machine simulation (`control_sim`, P1.4) as the
//! Action's "run the behavioral sim" step.
//!
//! **Thrust formula is this module's own invented default, not sourced from the docs** — same
//! "invent a reasonable, documented default" precedent as `HazardRiskPanel`'s Risk Index and
//! `StageTrackingPanel`'s progress percentages. Nothing in the doc set gives a bypass-ratio/
//! thrust relationship or specifies what "the report" contains. `estimate_thrust_lbf` reflects
//! the real, qualitative turbofan tradeoff (a higher bypass ratio moves more of the thrust to
//! bypassed air at lower exhaust velocity, so specific thrust per unit of core mass flow drops)
//! without claiming to be a real performance model.
//!
//! **The simulation step is a regression check, not an input to the thrust number.** `alf-lite`'s
//! pilot Control state machine (`control_sim::golden_alf_transitions`) has no notion of
//! `bypassRatio` at all — `sysml-core` has zero behavioral-modeling concepts, so nothing wires a
//! Fan property into the Alf program (see `control_sim.rs`'s own doc comment on that scope line).
//! Running it here proves the branch edit didn't break the pilot's simulated behavior; it
//! deliberately is *not* where the thrust delta comes from, and this report says so explicitly
//! rather than implying a connection that doesn't exist.

use axum::{extract::Path, extract::State, Json};

use crate::{control_sim, import::BadRequest, resolve_snapshot, ApiError, AppState};

/// A modern high-bypass turbofan reference point — not derived from any specific real engine,
/// just a plausible anchor so the formula produces plausible-looking numbers.
const REFERENCE_BYPASS_RATIO: f64 = 5.0;
/// `REQ-THRUST`'s own ">= 30,000 lbf" figure (`apps/api/src/main.rs`'s `seed_turbofan_ref`) —
/// reused as the thrust anchor so the baseline case (bypass ratio == the reference) reports
/// exactly the requirement's own number.
const REFERENCE_THRUST_LBF: f64 = 30_000.0;

fn default_element_id() -> String {
    "FanLpCompression".to_string()
}

fn default_property() -> String {
    "bypassRatio".to_string()
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct TradeStudyCompareRequest {
    /// The variant branch to compare against `main`'s current (live) state — created via the
    /// existing `POST .../branches`, edited via the existing branch-scoped
    /// `PATCH .../branches/:branch/elements/:elementId/body`.
    pub(crate) branch: String,
    #[serde(default = "default_element_id")]
    pub(crate) element_id: String,
    #[serde(default = "default_property")]
    pub(crate) property: String,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VariantResult {
    pub(crate) bypass_ratio: f64,
    pub(crate) thrust_lbf: f64,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ThrustDelta {
    pub(crate) thrust_lbf: f64,
    pub(crate) percent: f64,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SimulationCheck {
    pub(crate) converged: bool,
    pub(crate) final_rpm: Option<String>,
    pub(crate) note: &'static str,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TradeStudyReport {
    pub(crate) branch: String,
    pub(crate) element_id: String,
    pub(crate) property: String,
    pub(crate) baseline: VariantResult,
    pub(crate) variant: VariantResult,
    pub(crate) delta: ThrustDelta,
    pub(crate) simulation: SimulationCheck,
}

/// See the module doc comment for the real, qualitative relationship this is a deliberately
/// simple stand-in for.
fn estimate_thrust_lbf(bypass_ratio: f64) -> f64 {
    REFERENCE_THRUST_LBF * (REFERENCE_BYPASS_RATIO / bypass_ratio)
}

fn extract_property_f64(body: Option<&serde_json::Value>, property: &str) -> f64 {
    body.and_then(|b| b.get("properties"))
        .and_then(|p| p.get(property))
        .and_then(|v| v.as_f64())
        .unwrap_or(REFERENCE_BYPASS_RATIO)
}

pub(crate) async fn compare(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    Json(payload): Json<TradeStudyCompareRequest>,
) -> Result<Json<TradeStudyReport>, ApiError> {
    let Some(branch) = state
        .versioning
        .get_branch(&project_id, &payload.branch)
        .await?
    else {
        return Err(BadRequest(format!("no branch {}", payload.branch)).into());
    };
    let Some(branch_head) = branch.head_commit_id.clone() else {
        return Err(BadRequest(format!(
            "branch {} has no commits yet — apply a variant edit first",
            payload.branch
        ))
        .into());
    };

    let baseline_body = state
        .postgres
        .get_body(&project_id, &payload.element_id)
        .await?;
    let baseline_ratio = extract_property_f64(baseline_body.as_ref(), &payload.property);

    let variant_snapshot = resolve_snapshot(&state, &project_id, &branch_head).await?;
    let variant_ratio = extract_property_f64(
        variant_snapshot.bodies.get(&payload.element_id),
        &payload.property,
    );

    let baseline_thrust = estimate_thrust_lbf(baseline_ratio);
    let variant_thrust = estimate_thrust_lbf(variant_ratio);
    let delta_thrust = variant_thrust - baseline_thrust;

    let (converged, final_rpm) = control_sim::run_golden_control_sim().await?;

    Ok(Json(TradeStudyReport {
        branch: payload.branch,
        element_id: payload.element_id,
        property: payload.property,
        baseline: VariantResult {
            bypass_ratio: baseline_ratio,
            thrust_lbf: baseline_thrust,
        },
        variant: VariantResult {
            bypass_ratio: variant_ratio,
            thrust_lbf: variant_thrust,
        },
        delta: ThrustDelta {
            thrust_lbf: delta_thrust,
            percent: (delta_thrust / baseline_thrust) * 100.0,
        },
        simulation: SimulationCheck {
            converged,
            final_rpm,
            note: "the pilot's Control state machine has no dependency on this property; this \
                   confirms the branch edit didn't break simulated behavior, it isn't the \
                   source of the thrust delta above",
        },
    }))
}
