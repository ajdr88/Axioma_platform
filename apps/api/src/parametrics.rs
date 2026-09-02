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

use std::collections::{HashMap, HashSet, VecDeque};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use sysml_core::{EdgeKind, NodeKind};

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

// --- Pending-items Tier 1 item 10 -- reusable 0D physics Models (`NodeKind::Model`) -------------
//
// A real, embedded-formula (rhai) evaluator over a Model's `Contains`-nested `Parameter`
// (role: input/output) and `Constraint` (a bare formula string) definition subgraph, wired
// together by `EdgeKind::Uses` (a Constraint's inputs) and `EdgeKind::Produces` (a Constraint's
// one computed output). Deliberately the same "pure function of explicit caller-supplied input,
// never an implicit read from other graph elements" contract `/parametrics/evaluate` above
// already has -- a caller must supply a value for every declared input Parameter's symbol; a
// Model's own seeded `designValue`s are a UI convenience (pre-filling a form), not something this
// endpoint reads on the caller's behalf.

struct ModelParameter {
    id: String,
    name: String,
    symbol: String,
    role: String,
    unit: Option<String>,
    design_value: Option<f64>,
}

struct ModelConstraintDef {
    id: String,
    formula: String,
    uses_symbols: Vec<String>,
    produces_symbol: Option<String>,
}

struct ModelSubgraph {
    parameters: Vec<ModelParameter>,
    constraints: Vec<ModelConstraintDef>,
}

/// Loads a Model's real `Contains`/`Uses`/`Produces` subgraph. `Ok(None)` means "no such element"
/// or "that element isn't a `Model`" -- both surfaced as a 404 by the callers, not distinguished
/// further (a wrong-kind id isn't meaningfully different from a missing one to this endpoint's
/// caller).
async fn load_model_subgraph(
    state: &AppState,
    project_id: &str,
    model_id: &str,
) -> Result<Option<ModelSubgraph>, ApiError> {
    let Some(model_el) = state.neo4j.get_element(project_id, model_id).await? else {
        return Ok(None);
    };
    if model_el.kind != NodeKind::Model {
        return Ok(None);
    }

    let child_ids: HashSet<String> = state
        .neo4j
        .edges_of_kind(project_id, EdgeKind::Contains)
        .await?
        .into_iter()
        .filter(|e| e.source == model_id)
        .map(|e| e.target)
        .collect();

    let uses_edges: Vec<_> = state
        .neo4j
        .edges_of_kind(project_id, EdgeKind::Uses)
        .await?
        .into_iter()
        .filter(|e| child_ids.contains(&e.source))
        .collect();
    let produces_edges: Vec<_> = state
        .neo4j
        .edges_of_kind(project_id, EdgeKind::Produces)
        .await?
        .into_iter()
        .filter(|e| child_ids.contains(&e.source))
        .collect();

    let mut parameters = Vec::new();
    let mut parameter_symbol: HashMap<String, String> = HashMap::new();
    let mut constraint_bodies: Vec<(String, serde_json::Value)> = Vec::new();
    for id in &child_ids {
        let Some(el) = state.neo4j.get_element(project_id, id).await? else {
            continue;
        };
        let body = state
            .postgres
            .get_body(project_id, id)
            .await?
            .unwrap_or_else(|| serde_json::json!({}));
        match el.kind {
            NodeKind::Parameter => {
                let props = &body["properties"];
                let symbol = props["symbol"].as_str().unwrap_or(&el.id).to_string();
                parameter_symbol.insert(el.id.clone(), symbol.clone());
                parameters.push(ModelParameter {
                    id: el.id,
                    name: el.name,
                    symbol,
                    role: props["role"].as_str().unwrap_or("input").to_string(),
                    unit: props["unit"].as_str().map(str::to_string),
                    design_value: props["designValue"].as_f64(),
                });
            }
            NodeKind::Constraint => {
                constraint_bodies.push((el.id, body));
            }
            _ => {}
        }
    }

    let mut constraints = Vec::new();
    for (constraint_id, body) in constraint_bodies {
        let formula = body["properties"]["formula"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let uses_symbols: Vec<String> = uses_edges
            .iter()
            .filter(|e| e.source == constraint_id)
            .filter_map(|e| parameter_symbol.get(&e.target).cloned())
            .collect();
        let produces_symbol = produces_edges
            .iter()
            .find(|e| e.source == constraint_id)
            .and_then(|e| parameter_symbol.get(&e.target).cloned());
        constraints.push(ModelConstraintDef {
            id: constraint_id,
            formula,
            uses_symbols,
            produces_symbol,
        });
    }

    Ok(Some(ModelSubgraph {
        parameters,
        constraints,
    }))
}

/// Kahn's algorithm over each Constraint's real `Produces`/`Uses`-derived dependencies -- small,
/// bounded graph (one Model's own definition subgraph), no query-budget concern at this scale.
/// `Err` names a real cycle rather than silently evaluating in declaration order.
fn topological_order(constraints: &[ModelConstraintDef]) -> Result<Vec<usize>, String> {
    let mut produced_by: HashMap<&str, usize> = HashMap::new();
    for (i, c) in constraints.iter().enumerate() {
        if let Some(symbol) = &c.produces_symbol {
            produced_by.insert(symbol.as_str(), i);
        }
    }
    let mut in_degree = vec![0usize; constraints.len()];
    let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); constraints.len()];
    for (i, c) in constraints.iter().enumerate() {
        for used in &c.uses_symbols {
            if let Some(&dep_idx) = produced_by.get(used.as_str()) {
                if dep_idx != i {
                    dependents[dep_idx].push(i);
                    in_degree[i] += 1;
                }
            }
        }
    }
    let mut queue: VecDeque<usize> = (0..constraints.len())
        .filter(|&i| in_degree[i] == 0)
        .collect();
    let mut order = Vec::with_capacity(constraints.len());
    while let Some(i) = queue.pop_front() {
        order.push(i);
        for &dependent in &dependents[i] {
            in_degree[dependent] -= 1;
            if in_degree[dependent] == 0 {
                queue.push_back(dependent);
            }
        }
    }
    if order.len() != constraints.len() {
        return Err(
            "Model's Constraints have a circular Uses/Produces dependency -- cannot determine an evaluation order"
                .to_string(),
        );
    }
    Ok(order)
}

#[derive(Debug, Serialize)]
pub(crate) struct ModelParameterDto {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) symbol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) unit: Option<String>,
    #[serde(rename = "designValue", skip_serializing_if = "Option::is_none")]
    pub(crate) design_value: Option<f64>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ModelConstraintDto {
    pub(crate) id: String,
    pub(crate) formula: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct ModelDetailResponse {
    pub(crate) inputs: Vec<ModelParameterDto>,
    pub(crate) outputs: Vec<ModelParameterDto>,
    pub(crate) constraints: Vec<ModelConstraintDto>,
}

/// `GET /api/v0/projects/:projectId/parametrics/models/:modelId` -- a real, minimal read
/// convenience endpoint so a caller doesn't need to invent generic bulk-edge loading just to
/// discover a Model's declared inputs/outputs/formulas before evaluating it.
pub(crate) async fn model_detail(
    State(state): State<AppState>,
    Path((project_id, model_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let Some(subgraph) = load_model_subgraph(&state, &project_id, &model_id).await? else {
        return Ok((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("no Model {model_id}") })),
        )
            .into_response());
    };

    let mut inputs = Vec::new();
    let mut outputs = Vec::new();
    for p in subgraph.parameters {
        let is_output = p.role == "output";
        let dto = ModelParameterDto {
            id: p.id,
            name: p.name,
            symbol: p.symbol,
            unit: p.unit,
            design_value: p.design_value,
        };
        if is_output {
            outputs.push(dto);
        } else {
            inputs.push(dto);
        }
    }
    let constraints = subgraph
        .constraints
        .into_iter()
        .map(|c| ModelConstraintDto {
            id: c.id,
            formula: c.formula,
        })
        .collect();

    Ok((
        StatusCode::OK,
        Json(ModelDetailResponse {
            inputs,
            outputs,
            constraints,
        }),
    )
        .into_response())
}

#[derive(Debug, Deserialize)]
pub(crate) struct EvaluateModelRequest {
    pub(crate) inputs: HashMap<String, f64>,
}

#[derive(Debug, Serialize)]
pub(crate) struct EvaluateModelErrorDto {
    #[serde(rename = "constraintId")]
    pub(crate) constraint_id: String,
    pub(crate) message: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct EvaluateModelResponse {
    pub(crate) outputs: HashMap<String, f64>,
    #[serde(rename = "evaluationOrder")]
    pub(crate) evaluation_order: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<EvaluateModelErrorDto>,
}

/// `POST /api/v0/projects/:projectId/parametrics/models/:modelId/evaluate` -- evaluates a Model's
/// real rhai-formula Constraint chain against caller-supplied input values, in Produces/Uses
/// dependency order. A formula error aborts evaluation at that Constraint (never a panic) and is
/// returned as a typed error alongside whatever was already computed, matching this module's
/// existing "typed, never silent" convention.
pub(crate) async fn evaluate_model(
    State(state): State<AppState>,
    Path((project_id, model_id)): Path<(String, String)>,
    Json(payload): Json<EvaluateModelRequest>,
) -> Result<Response, ApiError> {
    let Some(subgraph) = load_model_subgraph(&state, &project_id, &model_id).await? else {
        return Ok((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("no Model {model_id}") })),
        )
            .into_response());
    };

    let order = match topological_order(&subgraph.constraints) {
        Ok(order) => order,
        Err(message) => {
            return Ok((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": message })),
            )
                .into_response())
        }
    };

    let mut engine = rhai::Engine::new();
    engine.set_max_operations(10_000);
    engine.set_max_expr_depths(32, 32);

    let mut scope = rhai::Scope::new();
    for (symbol, value) in &payload.inputs {
        scope.set_value(symbol.clone(), *value);
    }

    let mut outputs = HashMap::new();
    let mut evaluation_order = Vec::new();
    for &idx in &order {
        let constraint = &subgraph.constraints[idx];
        evaluation_order.push(constraint.id.clone());
        let result = engine.eval_expression_with_scope::<f64>(&mut scope, &constraint.formula);
        match result {
            Ok(value) => {
                if let Some(symbol) = &constraint.produces_symbol {
                    scope.set_value(symbol.clone(), value);
                    outputs.insert(symbol.clone(), value);
                }
            }
            Err(err) => {
                return Ok((
                    StatusCode::OK,
                    Json(EvaluateModelResponse {
                        outputs,
                        evaluation_order,
                        error: Some(EvaluateModelErrorDto {
                            constraint_id: constraint.id.clone(),
                            message: err.to_string(),
                        }),
                    }),
                )
                    .into_response());
            }
        }
    }

    Ok((
        StatusCode::OK,
        Json(EvaluateModelResponse {
            outputs,
            evaluation_order,
            error: None,
        }),
    )
        .into_response())
}
