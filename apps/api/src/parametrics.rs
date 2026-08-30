//! docs/IMPLEMENTATION_KICKOFF.md Phase 5 (FR-PARAM-03) — Constraint evaluation. "Pure,
//! synchronous, server-side computation... architecturally closer to a spreadsheet formula
//! evaluation than to a CEM run" (reqs v5 §5.9) — no `cem-core`/`cem-connectors`/`scheduler`
//! involved, so a Product-1-only deployment can evaluate Constraints with zero Product-2 services
//! running.
//!
//! Deliberately **not** a general arithmetic-expression parser — reqs v5 doesn't concretely
//! specify one (§5.15 itself says the real constitutive equations "still need sourcing"), and
//! inventing an expression grammar mid-feature would be exactly the "guessing ahead of the spec"
//! `sysml-core`'s own validation-layer discipline already commits against. This evaluates the one
//! shape reqs v5 §5.15 already gives real content for: a Constraint's tabulated
//! `sampledPointsAtDesignSpeed` curve (Phase 3's real `FanPerformanceMapConstraint`/
//! `CorePerformanceMapConstraint`), linearly interpolated at a caller-supplied input value.
//!
//! `/parametrics/constraints`, `/parametrics/bindings` (impl v5 §1.4) are deliberately not built
//! as dedicated endpoints — Constraint/Parameter creation and `Bound` edges are already fully
//! covered by the generic `POST /elements` (any `NodeKind`) and `POST /edges` (any `EdgeKind`)
//! endpoints in `main.rs`; a thin wrapper duplicating an endpoint that already exists would be
//! pure redundancy, not new capability.

use axum::{
    extract::{Path, State},
    Json,
};
use serde::{Deserialize, Serialize};

use crate::{import, ApiError, AppState};

#[derive(Debug, Deserialize)]
pub(crate) struct EvaluateRequest {
    #[serde(rename = "constraintIds")]
    pub(crate) constraint_ids: Vec<String>,
    /// The tabulated curve's x-axis input — matches Phase 3's own `sampledPointsAtDesignSpeed`
    /// shape (`{equivalentWeightFlowLbPerSec, pressureRatio}`), the only tabulated Constraint
    /// content that exists anywhere in this codebase today.
    #[serde(rename = "equivalentWeightFlowLbPerSec")]
    pub(crate) equivalent_weight_flow_lb_per_sec: f64,
}

#[derive(Debug, Serialize)]
pub(crate) struct EvaluateResult {
    #[serde(rename = "constraintId")]
    pub(crate) constraint_id: String,
    #[serde(rename = "pressureRatio", skip_serializing_if = "Option::is_none")]
    pub(crate) pressure_ratio: Option<f64>,
    /// Set instead of `pressureRatio` when the Constraint isn't evaluable this way (missing
    /// tabulated data, input outside the sampled range) — a typed "not evaluable" reason, never a
    /// silent/wrong extrapolation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct EvaluateResponse {
    pub(crate) results: Vec<EvaluateResult>,
}

/// `POST /api/v0/projects/:projectId/parametrics/evaluate` (FR-PARAM-03).
pub(crate) async fn evaluate(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    Json(payload): Json<EvaluateRequest>,
) -> Result<Json<EvaluateResponse>, ApiError> {
    if payload.constraint_ids.is_empty() {
        return Err(import::BadRequest("constraintIds must not be empty".to_string()).into());
    }
    let mut results = Vec::with_capacity(payload.constraint_ids.len());
    for constraint_id in payload.constraint_ids {
        results.push(
            evaluate_one(
                &state,
                &project_id,
                &constraint_id,
                payload.equivalent_weight_flow_lb_per_sec,
            )
            .await?,
        );
    }
    Ok(Json(EvaluateResponse { results }))
}

fn not_evaluable(constraint_id: &str, reason: impl Into<String>) -> EvaluateResult {
    EvaluateResult {
        constraint_id: constraint_id.to_string(),
        pressure_ratio: None,
        error: Some(reason.into()),
    }
}

async fn evaluate_one(
    state: &AppState,
    project_id: &str,
    constraint_id: &str,
    input: f64,
) -> Result<EvaluateResult, ApiError> {
    let Some(body) = state.postgres.get_body(project_id, constraint_id).await? else {
        return Ok(not_evaluable(constraint_id, "no such Constraint"));
    };
    let Some(points) = body["properties"]["sampledPointsAtDesignSpeed"].as_array() else {
        return Ok(not_evaluable(
            constraint_id,
            "Constraint has no sampledPointsAtDesignSpeed -- not evaluable",
        ));
    };
    let mut samples: Vec<(f64, f64)> = points
        .iter()
        .filter_map(|p| {
            let x = p.get("equivalentWeightFlowLbPerSec")?.as_f64()?;
            let y = p.get("pressureRatio")?.as_f64()?;
            Some((x, y))
        })
        .collect();
    if samples.len() < 2 {
        return Ok(not_evaluable(
            constraint_id,
            "fewer than 2 usable sample points",
        ));
    }
    samples.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let (min_x, max_x) = (samples[0].0, samples[samples.len() - 1].0);
    if input < min_x || input > max_x {
        return Ok(not_evaluable(
            constraint_id,
            format!("input {input} outside the tabulated range [{min_x}, {max_x}]"),
        ));
    }
    for window in samples.windows(2) {
        let (x0, y0) = window[0];
        let (x1, y1) = window[1];
        if input >= x0 && input <= x1 {
            let t = if (x1 - x0).abs() < f64::EPSILON {
                0.0
            } else {
                (input - x0) / (x1 - x0)
            };
            return Ok(EvaluateResult {
                constraint_id: constraint_id.to_string(),
                pressure_ratio: Some(y0 + t * (y1 - y0)),
                error: None,
            });
        }
    }
    // Unreachable given `samples` is sorted ascending and `input` was already bounds-checked
    // against [min_x, max_x] above -- returned as a typed error rather than a panic regardless,
    // since this handler must never crash the server on malformed/adversarial stored data.
    Ok(not_evaluable(
        constraint_id,
        "input fell outside every sampled window despite passing the range check",
    ))
}
