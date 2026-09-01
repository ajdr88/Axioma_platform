//! FR-ARCH-01…08's real build-out (reqs v5 §5.17) — thin HTTP wiring around
//! `cem_core::archspace`'s pure encode/decode logic and the `cem-archspace` gRPC sidecar
//! (`archspace_client.rs`), plus the resolution state machine (FR-ARCH-02/03) and its
//! incompatibility/choice-constraint enforcement (FR-ARCH-04), FR-ARCH-05/06's define/decode/stats
//! endpoints, and FR-ARCH-07/08's instance generation/comparison + typed-viability evaluation and
//! proposal/materialization flow. Mirrors `mode_b.rs`'s own shape (thin HTTP wiring around a pure
//! computation crate).
//!
//! **`define`/`decode`/`evaluate`/`generate_instances` don't materialize anything into the
//! graph** — a decoded/evaluated architecture instance is returned as data until a caller
//! explicitly `propose`s one specific instance, which lands a real, review-gated `proposals` row
//! (`origin: "archspace-instance"`); only accepting that proposal (`mode_b.rs::accept_proposal`,
//! dispatching to this module's own `materialize_proposal`) actually writes new graph elements.
//! The sidecar's own live constructed graph object stays process-lifetime/in-memory
//! sidecar-side, exactly as `cem_archspace.proto`'s own doc comment scopes it — but **the
//! definition that produced it is now really persisted** (Tier 1 pass, item 6,
//! `VersioningStore::persist_archspace_definition`), so a stale `handle_id` after a sidecar
//! restart is transparently recovered rather than just 404ing forever — see
//! `ensure_live_handle`'s own doc comment for the exact recovery flow.

use anyhow::Context;
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use cem_core::archspace::{
    self as core_archspace, ChoiceConstraintEdge, ChoiceConstraintKindInput,
    ConnectionChoiceElement, IncompatibleWithEdge, ParameterInput, SelectionChoiceElement,
};
use sysml_core::{Edge, EdgeKind, Element, ElementBody, NodeKind, Origin};

use crate::{
    archspace_client, import::BadRequest, record_commit, ApiError, AppState, DiffEntry, MAIN_BRANCH,
};

fn body_properties(body: Option<serde_json::Value>) -> serde_json::Value {
    body.and_then(|b| b.get("properties").cloned())
        .unwrap_or(serde_json::Value::Null)
}

/// Gathers a subsystem's real graph content for encoding: `:Parameter`s `Bound` to it,
/// `:SelectionChoice`s `ArchDerives`-linked to it, every `:ConnectionChoice` in the project
/// (connection choices route *between* subsystems in this schema — e.g. bleed air from Core (HP)
/// Compressor to an external boundary — so they are never single-subsystem-scoped, unlike
/// Parameters/SelectionChoices), and every `IncompatibleWith`/`ChoiceConstraint` edge touching
/// anything already in scope. A `ChoiceConstraint`/`IncompatibleWith` edge's *other* endpoint is
/// pulled in too even when it belongs to a different subsystem (a one-hop closure) — otherwise a
/// real cross-subsystem constraint like FR-COMP-04's stage-count `LINKED` pair (Core (HP)
/// Compressor's `CoreHpStagesParam` / Turbine's `TurbineHpStagesParam`) would always show up as
/// unencodable for either subsystem alone, defeating the entire point of proving that primitive.
struct SubsystemContent {
    parameters: Vec<ParameterInput>,
    selection_choices: Vec<SelectionChoiceElement>,
    connection_choices: Vec<ConnectionChoiceElement>,
    incompatibilities: Vec<IncompatibleWithEdge>,
    choice_constraints: Vec<ChoiceConstraintEdge>,
    connector_names: Vec<String>,
}

async fn fetch_subsystem_content(
    state: &AppState,
    project_id: &str,
    subsystem_id: &str,
) -> anyhow::Result<SubsystemContent> {
    let kinds = state.neo4j.element_kinds(project_id).await?;
    let bound_edges = state
        .neo4j
        .edges_of_kind(project_id, EdgeKind::Bound)
        .await?;
    let arch_derives = state
        .neo4j
        .edges_of_kind(project_id, EdgeKind::ArchDerives)
        .await?;
    let incompatible_with = state
        .neo4j
        .edges_of_kind(project_id, EdgeKind::IncompatibleWith)
        .await?;
    let choice_constraint_edges = state
        .neo4j
        .edges_of_kind(project_id, EdgeKind::ChoiceConstraint)
        .await?;
    let bodies = state.postgres.list_bodies(project_id).await?;

    let param_ids: std::collections::HashSet<String> = bound_edges
        .iter()
        .filter(|e| e.target == subsystem_id && kinds.get(&e.source) == Some(&NodeKind::Parameter))
        .map(|e| e.source.clone())
        .collect();
    let choice_ids: std::collections::HashSet<String> = arch_derives
        .iter()
        .filter(|e| {
            e.target == subsystem_id && kinds.get(&e.source) == Some(&NodeKind::SelectionChoice)
        })
        .map(|e| e.source.clone())
        .collect();

    // One-hop closure: pull in the other endpoint of any constraint edge touching what's already
    // in scope, so a real cross-subsystem constraint stays encodable (see this function's own doc
    // comment).
    let mut in_scope_names: std::collections::HashSet<String> =
        param_ids.iter().chain(choice_ids.iter()).cloned().collect();
    let mut extra_param_ids = std::collections::HashSet::new();
    let mut extra_choice_ids = std::collections::HashSet::new();
    for edge in incompatible_with
        .iter()
        .chain(choice_constraint_edges.iter())
    {
        let (touches, other) = if in_scope_names.contains(&edge.source) {
            (true, &edge.target)
        } else if in_scope_names.contains(&edge.target) {
            (true, &edge.source)
        } else {
            (false, &edge.source)
        };
        if touches && !in_scope_names.contains(other) {
            match kinds.get(other) {
                Some(NodeKind::Parameter) => {
                    extra_param_ids.insert(other.clone());
                }
                Some(NodeKind::SelectionChoice) => {
                    extra_choice_ids.insert(other.clone());
                }
                _ => {}
            }
        }
    }
    in_scope_names.extend(extra_param_ids.iter().cloned());
    in_scope_names.extend(extra_choice_ids.iter().cloned());
    let param_ids: std::collections::HashSet<String> =
        param_ids.into_iter().chain(extra_param_ids).collect();
    let choice_ids: std::collections::HashSet<String> =
        choice_ids.into_iter().chain(extra_choice_ids).collect();

    let parameters: Vec<ParameterInput> = param_ids
        .iter()
        .map(|id| {
            let properties = body_properties(bodies.get(id).cloned());
            let bound = properties
                .get("bound")
                .and_then(|v| v.as_array())
                .filter(|arr| arr.len() == 2)
                .and_then(|arr| Some((arr[0].as_f64()?, arr[1].as_f64()?)));
            ParameterInput {
                id: id.clone(),
                bound,
            }
        })
        .collect();

    let selection_choices: Vec<SelectionChoiceElement> = choice_ids
        .iter()
        .map(|id| {
            let properties = body_properties(bodies.get(id).cloned());
            let options = properties
                .get("options")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            SelectionChoiceElement {
                id: id.clone(),
                options,
            }
        })
        .collect();

    let connection_choice_ids: Vec<String> = kinds
        .iter()
        .filter(|(_, kind)| **kind == NodeKind::ConnectionChoice)
        .map(|(id, _)| id.clone())
        .collect();
    let connection_choices: Vec<ConnectionChoiceElement> = connection_choice_ids
        .iter()
        .map(|id| ConnectionChoiceElement {
            id: id.clone(),
            properties: body_properties(bodies.get(id).cloned()),
        })
        .collect();
    let mut connector_names: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for cc in &connection_choices {
        for key in ["sourceConnectorNames", "targetConnectorNames"] {
            if let Some(arr) = cc.properties.get(key).and_then(|v| v.as_array()) {
                for name in arr.iter().filter_map(|v| v.as_str()) {
                    connector_names.insert(name.to_string());
                }
            }
        }
    }

    let incompatibilities: Vec<IncompatibleWithEdge> = incompatible_with
        .iter()
        .filter(|e| in_scope_names.contains(&e.source) || in_scope_names.contains(&e.target))
        .map(|e| IncompatibleWithEdge {
            source: e.source.clone(),
            target: e.target.clone(),
        })
        .collect();
    let choice_constraints: Vec<ChoiceConstraintEdge> = choice_constraint_edges
        .iter()
        .filter(|e| in_scope_names.contains(&e.source) || in_scope_names.contains(&e.target))
        .map(|e| ChoiceConstraintEdge {
            source: e.source.clone(),
            target: e.target.clone(),
            kind: e
                .metadata
                .as_ref()
                .and_then(|m| m.get("choiceConstraintType"))
                .and_then(|v| v.as_str())
                .map(str::to_string),
        })
        .collect();

    Ok(SubsystemContent {
        parameters,
        selection_choices,
        connection_choices,
        incompatibilities,
        choice_constraints,
        connector_names: connector_names.into_iter().collect(),
    })
}

fn to_proto_kind(kind: ChoiceConstraintKindInput) -> archspace_client::proto::ChoiceConstraintKind {
    use archspace_client::proto::ChoiceConstraintKind as ProtoKind;
    match kind {
        ChoiceConstraintKindInput::Linked => ProtoKind::Linked,
        ChoiceConstraintKindInput::Permutation => ProtoKind::Permutation,
        ChoiceConstraintKindInput::Unordered => ProtoKind::Unordered,
        ChoiceConstraintKindInput::UnorderedNorepl => ProtoKind::UnorderedNorepl,
    }
}

fn to_proto_definition(
    def: &core_archspace::DesignSpaceDefinitionInput,
) -> archspace_client::proto::DesignSpaceDefinition {
    archspace_client::proto::DesignSpaceDefinition {
        root_name: def.root_name.clone(),
        connector_names: def.connector_names.clone(),
        design_variables: def
            .design_variables
            .iter()
            .map(|dv| archspace_client::proto::DesignVariable {
                name: dv.name.clone(),
                lower_bound: dv.lower_bound,
                upper_bound: dv.upper_bound,
            })
            .collect(),
        selection_choices: def
            .selection_choices
            .iter()
            .map(|sc| archspace_client::proto::SelectionChoice {
                choice_id: sc.choice_id.clone(),
                option_names: sc.option_names.clone(),
            })
            .collect(),
        connection_choices: def
            .connection_choices
            .iter()
            .map(|cc| archspace_client::proto::ConnectionChoice {
                choice_id: cc.choice_id.clone(),
                source_connector_names: cc.source_connector_names.clone(),
                target_connector_names: cc.target_connector_names.clone(),
            })
            .collect(),
        incompatibility_constraints: def
            .incompatibility_constraints
            .iter()
            .map(|ic| archspace_client::proto::IncompatibilityConstraint {
                node_names: ic.node_names.clone(),
            })
            .collect(),
        choice_constraints: def
            .choice_constraints
            .iter()
            .map(|cc| archspace_client::proto::ChoiceConstraint {
                kind: to_proto_kind(cc.kind) as i32,
                node_names: cc.node_names.clone(),
            })
            .collect(),
        objective: def
            .objective
            .as_ref()
            .map(|o| archspace_client::proto::Objective {
                name: o.name.clone(),
                direction: o.direction,
            }),
    }
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SkippedItemDto {
    pub(crate) element_id: String,
    pub(crate) reason: String,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DesignSpaceStatsDto {
    pub(crate) n_design_variables: i32,
    pub(crate) n_declared: i64,
    pub(crate) n_valid: i64,
    pub(crate) imputation_ratio: f64,
    /// FR-ARCH-06's other three real metrics — real sb_arch_opt properties (`Option::None`, not a
    /// fake `0.0`, when this design space has no objective — see the proto's own doc comment).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) correction_ratio: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) discrete_correction_ratio: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) continuous_correction_ratio: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) correction_fraction: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) max_rate_diversity: Option<f64>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DefineResponse {
    pub(crate) handle_id: String,
    pub(crate) stats: DesignSpaceStatsDto,
    pub(crate) skipped: Vec<SkippedItemDto>,
}

/// `POST /api/v0/projects/:projectId/cem/archspace/:subsystemId/define` — FR-ARCH-05's encode
/// half plus FR-ARCH-06 (bundles `GetDesignSpaceStats` into the same round trip, not a second
/// user action). Fetches the subsystem's real graph content, encodes it via
/// `cem_core::archspace::encode_design_space`, defines it against the real sidecar.
pub(crate) async fn define(
    State(state): State<AppState>,
    Path((project_id, subsystem_id)): Path<(String, String)>,
) -> Result<Json<DefineResponse>, ApiError> {
    let content = fetch_subsystem_content(&state, &project_id, &subsystem_id).await?;
    let result = core_archspace::encode_design_space(
        &subsystem_id,
        &content.connector_names,
        &content.parameters,
        &content.selection_choices,
        &content.connection_choices,
        &content.incompatibilities,
        &content.choice_constraints,
    );
    if result.definition.design_variables.is_empty()
        && result.definition.selection_choices.is_empty()
    {
        return Err(BadRequest(format!(
            "subsystem {subsystem_id} has no encodable design-space content \
             (design variables/selection choices) -- {} item(s) were skipped, see the request \
             against an equivalent /define call with a broader subsystem for the reasons",
            result.skipped.len()
        ))
        .into());
    }

    let definition = to_proto_definition(&result.definition);
    let handle_id = archspace_client::define_design_space(definition).await?;
    let stats = archspace_client::get_design_space_stats(&handle_id).await?;

    // Tier 1 pass (item 6) — real, durable persistence of the definition itself (not the
    // sidecar's own live constructed graph object), so a stale handle after a sidecar restart can
    // be transparently recovered — see `resolve_or_redefine`'s own doc comment.
    state
        .versioning
        .persist_archspace_definition(&handle_id, &project_id, &subsystem_id, &result.definition)
        .await?;

    // Also cached in-process so a later `decode` call for this same handle can group its result
    // by choice (`DecodeResponse::choices`) instead of always reporting an empty summary --
    // `AppState::archspace_definitions` stays a same-process fast path now that Postgres is the
    // real durable copy, not a replacement for it.
    state
        .archspace_definitions
        .lock()
        .expect("archspace_definitions mutex poisoned")
        .insert(handle_id.clone(), result.definition);

    Ok(Json(DefineResponse {
        handle_id,
        stats: DesignSpaceStatsDto {
            n_design_variables: stats.n_design_variables,
            n_declared: stats.n_declared,
            n_valid: stats.n_valid,
            imputation_ratio: stats.imputation_ratio,
            correction_ratio: stats.correction_ratio,
            discrete_correction_ratio: stats.discrete_correction_ratio,
            continuous_correction_ratio: stats.continuous_correction_ratio,
            correction_fraction: stats.correction_fraction,
            max_rate_diversity: stats.max_rate_diversity,
        },
        skipped: result
            .skipped
            .into_iter()
            .map(|s| SkippedItemDto {
                element_id: s.element_id,
                reason: s.reason,
            })
            .collect(),
    }))
}

/// Tier 1 pass (item 6) — probes whether the sidecar still recognizes `handle_id` (a lightweight
/// `GetDesignSpaceStats` call), and if not (`NotFound` — e.g. after a sidecar restart wiped its
/// in-memory `_design_spaces` dict), transparently re-defines the design space from its real,
/// persisted `DesignSpaceDefinitionInput` (`VersioningStore::persist_archspace_definition`) and
/// returns the fresh handle instead. Checked **once per request**, not once per individual RPC
/// call — a handler that makes several sidecar calls in one request (`generate_instances`'s loop)
/// only pays the redefinition cost once, not once per call inside it. Any other error (including
/// "no persisted definition to recover from" — a handle from before this pass, or a genuinely
/// unknown one) propagates unchanged. Returns `(effective_handle_id, refreshed)` — `refreshed` is
/// `Some(fresh_handle_id)` only when recovery actually happened, so callers can tell their own
/// client which handle to use from now on (`None` on the normal, no-recovery path).
async fn ensure_live_handle(
    state: &AppState,
    handle_id: &str,
) -> anyhow::Result<(String, Option<String>)> {
    match archspace_client::get_design_space_stats(handle_id).await {
        Ok(_) => Ok((handle_id.to_string(), None)),
        Err(err) => {
            let is_not_found = err
                .downcast_ref::<tonic::Status>()
                .is_some_and(|status| status.code() == tonic::Code::NotFound);
            if !is_not_found {
                return Err(err);
            }
            let Some((project_id, subsystem_id, definition)) =
                state.versioning.get_archspace_definition(handle_id).await?
            else {
                return Err(err.context(format!(
                    "handle {handle_id} not found and no persisted definition to recover from"
                )));
            };
            let fresh_handle =
                archspace_client::define_design_space(to_proto_definition(&definition)).await?;
            state
                .versioning
                .persist_archspace_definition(
                    &fresh_handle,
                    &project_id,
                    &subsystem_id,
                    &definition,
                )
                .await?;
            state
                .archspace_definitions
                .lock()
                .expect("archspace_definitions mutex poisoned")
                .insert(fresh_handle.clone(), definition);
            Ok((fresh_handle.clone(), Some(fresh_handle)))
        }
    }
}

/// A definition for `handle_id`, checked in-process first (the common-case fast path), falling
/// back to the real persisted copy (Tier 1 pass, item 6) — so choice-grouping (`decode`/
/// `generate_instances`/`propose`'s own `summarize_instance` calls) stays accurate even after an
/// apps/api restart alone (no sidecar restart, so `handle_id` itself is still valid, but this
/// process's own in-memory cache is empty). Falls back to `Default` only when truly nothing is
/// known about this handle anywhere — same graceful "report present nodes as ungrouped" behavior
/// as before this pass.
async fn definition_for_handle(
    state: &AppState,
    handle_id: &str,
) -> anyhow::Result<cem_core::archspace::DesignSpaceDefinitionInput> {
    if let Some(definition) = state
        .archspace_definitions
        .lock()
        .expect("archspace_definitions mutex poisoned")
        .get(handle_id)
        .cloned()
    {
        return Ok(definition);
    }
    Ok(state
        .versioning
        .get_archspace_definition(handle_id)
        .await?
        .map(|(_, _, definition)| definition)
        .unwrap_or_default())
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DecodeRequestDto {
    #[serde(default)]
    pub(crate) design_vector: Vec<f64>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DecodedChoiceDto {
    choice_id: String,
    present_option: Option<String>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DecodeResponse {
    pub(crate) design_vector: Vec<f64>,
    pub(crate) is_active: Vec<bool>,
    pub(crate) present_node_names: Vec<String>,
    pub(crate) choices: Vec<DecodedChoiceDto>,
    pub(crate) other_present_nodes: Vec<String>,
    /// Tier 1 pass (item 6) — set only when `handleId` had gone stale (e.g. a sidecar restart)
    /// and was transparently recovered; the caller should use this handle for any further calls
    /// in this session instead of the one it sent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) refreshed_handle_id: Option<String>,
}

/// `POST /api/v0/projects/:projectId/cem/archspace/:handleId/decode` — FR-ARCH-05's decode half.
/// An empty/omitted `designVector` asks the sidecar to sample a random valid vector first
/// (matches `cem_archspace.proto`'s own documented `DecodeInstance` behavior). `handleId` no
/// longer has to come from a still-live sidecar handle — `ensure_live_handle` transparently
/// recovers a stale one from its real persisted definition (Tier 1 pass, item 6).
pub(crate) async fn decode(
    State(state): State<AppState>,
    Path((_project_id, handle_id)): Path<(String, String)>,
    Json(payload): Json<DecodeRequestDto>,
) -> Result<Json<DecodeResponse>, ApiError> {
    let (handle_id, refreshed_handle_id) = ensure_live_handle(&state, &handle_id).await?;
    let instance = archspace_client::decode_instance(&handle_id, payload.design_vector).await?;
    let definition = definition_for_handle(&state, &handle_id).await?;
    let summary = core_archspace::summarize_instance(
        &definition,
        &instance.design_vector,
        &instance.present_node_names,
    );
    Ok(Json(DecodeResponse {
        design_vector: instance.design_vector,
        is_active: instance.is_active,
        present_node_names: instance.present_node_names,
        choices: summary
            .choices
            .into_iter()
            .map(|c| DecodedChoiceDto {
                choice_id: c.choice_id,
                present_option: c.present_option,
            })
            .collect(),
        other_present_nodes: summary.other_present_nodes,
        refreshed_handle_id,
    }))
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ViabilityDto {
    pub(crate) state: &'static str,
    pub(crate) probability_of_viability: f64,
    pub(crate) objective_value: Option<f64>,
    pub(crate) training_samples_used: i32,
}

/// FR-ARCH-08 -- calls the real sidecar `EvaluateViability` RPC (a real
/// `sb_arch_opt.algo.arch_sbo.hc_strategy.RandomForestClassifier`, trained fresh on freshly
/// sampled/evaluated points from the same design space, see `archspace_client::evaluate_viability`'s
/// own doc comment) and maps its raw signal into `sysml_core::SolverResultState` — FR-CEM-13's
/// typed-outcome pattern, reused rather than a bespoke failure shape. `objective_computed == false`
/// is a real, existing non-convergence signal (the sidecar's placeholder evaluator's own documented
/// NaN case) -> `Diverged`. Otherwise, `probability_of_viability < 0.5` -> `SuspectNumerical` (the
/// literal "plausibility check... before any graph write" NFR-REL-03 calls for, using the trained
/// classifier's own signal as that check). Anything else -> `Converged`.
async fn evaluate_viability_typed(
    handle_id: &str,
    design_vector: Vec<f64>,
    n_training_samples: i32,
    seed: i32,
) -> anyhow::Result<ViabilityDto> {
    let result =
        archspace_client::evaluate_viability(handle_id, design_vector, n_training_samples, seed)
            .await?;
    let state = if !result.objective_computed {
        sysml_core::SolverResultState::Diverged
    } else if result.probability_of_viability < 0.5 {
        sysml_core::SolverResultState::SuspectNumerical
    } else {
        sysml_core::SolverResultState::Converged
    };
    Ok(ViabilityDto {
        state: state.as_str(),
        probability_of_viability: result.probability_of_viability,
        objective_value: result.objective_computed.then_some(result.objective_value),
        training_samples_used: result.training_samples_used,
    })
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EvaluateResponseDto {
    #[serde(flatten)]
    pub(crate) viability: ViabilityDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) refreshed_handle_id: Option<String>,
}

/// `POST /api/v0/projects/:projectId/cem/archspace/:handleId/evaluate` — FR-ARCH-08's own direct
/// HTTP surface: evaluate one specific candidate's typed viability outcome. `handleId` recovery —
/// see `ensure_live_handle`'s own doc comment (Tier 1 pass, item 6).
pub(crate) async fn evaluate(
    State(state): State<AppState>,
    Path((_project_id, handle_id)): Path<(String, String)>,
    Json(payload): Json<DecodeRequestDto>,
) -> Result<Json<EvaluateResponseDto>, ApiError> {
    let (handle_id, refreshed_handle_id) = ensure_live_handle(&state, &handle_id).await?;
    let viability = evaluate_viability_typed(&handle_id, payload.design_vector, 0, 0).await?;
    Ok(Json(EvaluateResponseDto {
        viability,
        refreshed_handle_id,
    }))
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GeneratedInstanceDto {
    pub(crate) design_vector: Vec<f64>,
    pub(crate) present_node_names: Vec<String>,
    pub(crate) choices: Vec<DecodedChoiceDto>,
    pub(crate) viability: ViabilityDto,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GenerateInstancesRequest {
    #[serde(default)]
    pub(crate) count: Option<u32>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GenerateInstancesResponseDto {
    pub(crate) instances: Vec<GeneratedInstanceDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) refreshed_handle_id: Option<String>,
}

/// `POST /api/v0/projects/:projectId/cem/archspace/:handleId/generate-instances` — FR-ARCH-07's
/// "browsable, comparable set of architecture instances" half, literally: decodes `count` (default
/// 5) random instances and evaluates each one's real FR-ARCH-08 viability, so a caller can compare
/// them before proposing any one of them (`propose`, below). `handleId` recovery happens once, up
/// front (Tier 1 pass, item 6, `ensure_live_handle`) — not once per instance in the loop below.
pub(crate) async fn generate_instances(
    State(state): State<AppState>,
    Path((_project_id, handle_id)): Path<(String, String)>,
    Json(payload): Json<GenerateInstancesRequest>,
) -> Result<Json<GenerateInstancesResponseDto>, ApiError> {
    let count = payload.count.unwrap_or(5).clamp(1, 20);
    let (handle_id, refreshed_handle_id) = ensure_live_handle(&state, &handle_id).await?;
    let definition = definition_for_handle(&state, &handle_id).await?;

    let mut instances = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let instance = archspace_client::decode_instance(&handle_id, vec![]).await?;
        let summary = core_archspace::summarize_instance(
            &definition,
            &instance.design_vector,
            &instance.present_node_names,
        );
        let viability =
            evaluate_viability_typed(&handle_id, instance.design_vector.clone(), 0, 0).await?;
        instances.push(GeneratedInstanceDto {
            design_vector: instance.design_vector,
            present_node_names: instance.present_node_names,
            choices: summary
                .choices
                .into_iter()
                .map(|c| DecodedChoiceDto {
                    choice_id: c.choice_id,
                    present_option: c.present_option,
                })
                .collect(),
            viability,
        });
    }
    Ok(Json(GenerateInstancesResponseDto {
        instances,
        refreshed_handle_id,
    }))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProposeInstanceRequest {
    pub(crate) design_vector: Vec<f64>,
    pub(crate) subsystem_id: String,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProposeInstanceResponse {
    pub(crate) proposal_id: String,
    pub(crate) branch_id: String,
    pub(crate) viability: ViabilityDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) refreshed_handle_id: Option<String>,
}

/// `POST /api/v0/projects/:projectId/cem/archspace/:handleId/propose` — FR-ARCH-07's "enterable
/// into the existing proposal/review-gate flow" half. Re-decodes and re-evaluates the named design
/// vector server-side rather than trusting whatever the client last saw from `generate-instances`
/// (same "server recomputes, never passed through" discipline as FR-COMP-06's `flagged` field).
/// Always lands on a fresh review branch -- no auto-merge path exists for architecture instances
/// (no autonomy-decision story for this shape anywhere in the docs, and `SolverResultState::
/// satisfies_autonomy_gate` means a non-`Converged` state could never qualify for one anyway, per
/// CLAUDE.md's own non-negotiable rule #3).
pub(crate) async fn propose(
    State(state): State<AppState>,
    Path((project_id, handle_id)): Path<(String, String)>,
    Json(payload): Json<ProposeInstanceRequest>,
) -> Result<Json<ProposeInstanceResponse>, ApiError> {
    if state
        .neo4j
        .get_element(&project_id, &payload.subsystem_id)
        .await?
        .is_none()
    {
        return Err(BadRequest(format!("no such subsystem {}", payload.subsystem_id)).into());
    }

    let (handle_id, refreshed_handle_id) = ensure_live_handle(&state, &handle_id).await?;
    let instance =
        archspace_client::decode_instance(&handle_id, payload.design_vector.clone()).await?;
    let definition = definition_for_handle(&state, &handle_id).await?;
    let summary = core_archspace::summarize_instance(
        &definition,
        &instance.design_vector,
        &instance.present_node_names,
    );
    let viability =
        evaluate_viability_typed(&handle_id, instance.design_vector.clone(), 0, 0).await?;

    let candidate = serde_json::json!({
        "subsystemId": payload.subsystem_id,
        "handleId": handle_id,
        "designVector": instance.design_vector,
        "presentNodeNames": instance.present_node_names,
        "choices": summary.choices.iter().map(|c| serde_json::json!({
            "choiceId": c.choice_id,
            "presentOption": c.present_option,
        })).collect::<Vec<_>>(),
        "viability": viability.clone(),
    });
    let reason = format!(
        "Mode B architecture instance ({}) — satisfiesAutonomyGate={}",
        viability.state,
        viability.state == sysml_core::SolverResultState::Converged.as_str()
    );

    let main_branch = state
        .versioning
        .get_branch(&project_id, MAIN_BRANCH)
        .await?
        .ok_or_else(|| anyhow::anyhow!("project has no main branch"))?;
    let branch_name = format!("archspace-instance-{}", uuid::Uuid::new_v4());
    let branch = state
        .versioning
        .create_branch(
            &project_id,
            &branch_name,
            main_branch.head_commit_id.as_deref(),
        )
        .await?;
    let proposal = state
        .versioning
        .create_proposal(
            &project_id,
            &branch.id,
            &payload.subsystem_id,
            &candidate,
            &[],
            &reason,
            "archspace-instance",
        )
        .await?;

    Ok(Json(ProposeInstanceResponse {
        proposal_id: proposal.id,
        branch_id: branch.id,
        viability,
        refreshed_handle_id,
    }))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResolveChoiceRequest {
    #[serde(default)]
    pub(crate) selected_option: Option<String>,
    /// FR-ARCH-03 real build-out — only meaningful for `:ConnectionChoice`. Absent/empty means
    /// "resolving zero connections," validated against `cardinality` like any other count.
    #[serde(default)]
    pub(crate) connections: Vec<ConnectionPair>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConnectionPair {
    pub(crate) source: String,
    pub(crate) target: String,
}

/// `PATCH /api/v0/projects/:projectId/cem/archspace/choices/:id/resolve` — FR-ARCH-02/03's real,
/// sidecar-independent capability: no resolution endpoint existed anywhere before this pass
/// (`resolutionState` was set once at seed time and never touched again). Also enforces FR-ARCH-04
/// (incompatibility/choice-constraint validity) and FR-ARCH-03's "resolved after selection
/// choices" ordering for `:ConnectionChoice`.
pub(crate) async fn resolve_choice(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project_id, id)): Path<(String, String)>,
    Json(payload): Json<ResolveChoiceRequest>,
) -> Result<StatusCode, ApiError> {
    let kind = state
        .neo4j
        .element_kinds(&project_id)
        .await?
        .get(&id)
        .copied()
        .ok_or_else(|| BadRequest(format!("no such element {id}")))?;
    if kind != NodeKind::SelectionChoice && kind != NodeKind::ConnectionChoice {
        return Err(BadRequest(format!(
            "{id} is a {kind:?}, not a SelectionChoice/ConnectionChoice"
        ))
        .into());
    }

    let old_body = state.postgres.get_body(&project_id, &id).await?;
    let mut properties = old_body
        .as_ref()
        .and_then(|b| b.get("properties"))
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();

    if kind == NodeKind::SelectionChoice {
        let options: Vec<String> = properties
            .get("options")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        let selected = payload
            .selected_option
            .as_ref()
            .ok_or_else(|| BadRequest("selectedOption is required for a SelectionChoice".into()))?;
        if !options.contains(selected) {
            return Err(BadRequest(format!(
                "{selected} is not one of {id}'s options ({options:?})"
            ))
            .into());
        }
        check_constraints(&state, &project_id, &id, selected).await?;
        properties.insert(
            "selectedOption".to_string(),
            serde_json::Value::String(selected.clone()),
        );
    } else {
        // ConnectionChoice -- FR-ARCH-03's "resolved after selection choices" ordering.
        for key in ["sourceSelectionChoiceId", "targetSelectionChoiceId"] {
            if let Some(prereq_id) = properties.get(key).and_then(|v| v.as_str()) {
                let prereq_body = state.postgres.get_body(&project_id, prereq_id).await?;
                let prereq_state = prereq_body
                    .as_ref()
                    .and_then(|b| b.get("properties"))
                    .and_then(|p| p.get("resolutionState"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unresolved");
                if prereq_state != "resolved" {
                    return Err(BadRequest(format!(
                        "{id} cannot resolve before its prerequisite selection choice \
                         {prereq_id} (currently {prereq_state})"
                    ))
                    .into());
                }
            }
        }

        // FR-ARCH-03's cardinality-*enforcement* half (the ordering check above is its other
        // half). `cardinality` stays untouched if absent/malformed (see
        // `check_connection_cardinality`'s own doc comment) -- this only rejects a genuine,
        // stated violation.
        let cardinality = properties
            .get("cardinality")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        sysml_core::check_connection_cardinality(&id, &cardinality, payload.connections.len())?;
        properties.insert(
            "resolvedConnections".to_string(),
            serde_json::to_value(&payload.connections)
                .expect("Vec<ConnectionPair> always serializes"),
        );
    }

    let old_state = properties
        .get("resolutionState")
        .and_then(|v| v.as_str())
        .unwrap_or("unresolved")
        .to_string();
    properties.insert(
        "resolutionState".to_string(),
        serde_json::Value::String("resolved".to_string()),
    );

    let rationale = old_body
        .as_ref()
        .and_then(|b| b.get("rationale"))
        .and_then(|v| v.as_str())
        .map(String::from);
    state
        .postgres
        .upsert_body(
            &project_id,
            &ElementBody {
                element_id: id.clone(),
                rationale,
                properties: serde_json::Value::Object(properties),
            },
        )
        .await?;
    record_commit(
        &state,
        &project_id,
        &state.auth.resolve_actor(&headers)?,
        "Resolve architecture choice",
        vec![DiffEntry::PropertyChanged {
            element_id: id.clone(),
            property: "resolutionState".to_string(),
            old: serde_json::Value::String(old_state),
            new: serde_json::Value::String("resolved".to_string()),
        }],
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// FR-ARCH-04 enforcement: rejects a `SelectionChoice` resolution that would conflict with an
/// already-resolved element it has a real `IncompatibleWith`/`ChoiceConstraint` edge to.
///
/// **Element-level semantics, matching FR-ARCH-04's own literal wording** ("A user can define an
/// incompatibility constraint (mutual exclusion) between two **elements/choices**" — reqs v5
/// §2.11, not "between two options") and how these edges are actually creatable in this graph in
/// the first place: `Neo4jStore::create_edge` rejects a dangling edge, so an `IncompatibleWith`/
/// `ChoiceConstraint` edge can only ever connect two real elements (confirmed directly against the
/// real seeded `MixedNozzle -> FanBypassDuctExitPort` edge, which connects a whole
/// `:SelectionChoice` element to a `:Port` element, not two option-value strings — option values
/// aren't graph elements at all in this schema, so an edge could never reference one). This is a
/// different, independent concern from `cem_core::archspace::encode_design_space`'s own
/// option-name-level matching against adsg-core's real library API — that module is solving "what
/// does the *sidecar* need," this function is solving "what does *this graph's own resolution
/// state* allow," and the two don't need to agree.
///
/// The rule: once one of two `IncompatibleWith`-linked elements is resolved to anything, the
/// other cannot also be resolved (element-to-Port edges like the real seeded one never actually
/// block anything under this rule, since a `:Port` never carries a `selectedOption` in the first
/// place — checked, not just assumed). `LINKED` `ChoiceConstraint` uses the same "already resolved
/// on the other side" trigger, additionally requiring the two selected values to match — a
/// reasonable rule for two choices sharing an option vocabulary, though no real seeded `LINKED`
/// pair currently connects two `SelectionChoice`s this way (the real ones are Parameter↔Parameter,
/// never resolved through this SelectionChoice/ConnectionChoice-only endpoint) — exercised
/// directly by this module's own synthetic-fixture tests, not by a naturally-occurring scenario.
async fn check_constraints(
    state: &AppState,
    project_id: &str,
    id: &str,
    selected: &str,
) -> Result<(), ApiError> {
    let bodies = state.postgres.list_bodies(project_id).await?;
    let selected_option_of = |element_id: &str| -> Option<String> {
        bodies
            .get(element_id)?
            .get("properties")?
            .get("selectedOption")?
            .as_str()
            .map(str::to_string)
    };

    let incompatible_with = state
        .neo4j
        .edges_of_kind(project_id, EdgeKind::IncompatibleWith)
        .await?;
    for edge in incompatible_with
        .iter()
        .filter(|e| e.source == id || e.target == id)
    {
        let other_id = if edge.source == id {
            &edge.target
        } else {
            &edge.source
        };
        if selected_option_of(other_id).is_some() {
            return Err(BadRequest(format!(
                "resolving {id} to {selected} conflicts with {other_id}, which is already \
                 resolved (IncompatibleWith)"
            ))
            .into());
        }
    }

    let choice_constraints = state
        .neo4j
        .edges_of_kind(project_id, EdgeKind::ChoiceConstraint)
        .await?;
    for edge in choice_constraints
        .iter()
        .filter(|e| e.source == id || e.target == id)
    {
        let is_linked = edge
            .metadata
            .as_ref()
            .and_then(|m| m.get("choiceConstraintType"))
            .and_then(|v| v.as_str())
            == Some("Linked");
        if !is_linked {
            continue;
        }
        let other_id = if edge.source == id {
            &edge.target
        } else {
            &edge.source
        };
        if let Some(other_selected) = selected_option_of(other_id) {
            if other_selected != selected {
                return Err(BadRequest(format!(
                    "resolving {id} to {selected} violates the LINKED choice constraint with \
                     {other_id} (currently resolved to {other_selected})"
                ))
                .into());
            }
        }
    }
    Ok(())
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResolutionStatusResponse {
    pub(crate) state: &'static str,
    pub(crate) resolved: usize,
    pub(crate) total: usize,
}

/// `GET /api/v0/projects/:projectId/cem/archspace/:subsystemId/resolution-status` — where §5.17's
/// literal "unresolved → partial → resolved" state-machine language actually lives: aggregated
/// across every SelectionChoice/ConnectionChoice scoped to a subsystem, not invented as a fake
/// per-choice intermediate state (a single pick-one-of-N choice has no meaningful "partial" of its
/// own).
pub(crate) async fn resolution_status(
    State(state): State<AppState>,
    Path((project_id, subsystem_id)): Path<(String, String)>,
) -> Result<Json<ResolutionStatusResponse>, ApiError> {
    let content = fetch_subsystem_content(&state, &project_id, &subsystem_id).await?;
    let mut ids: Vec<String> = content
        .selection_choices
        .iter()
        .map(|sc| sc.id.clone())
        .collect();
    ids.extend(content.connection_choices.iter().map(|cc| cc.id.clone()));

    let bodies = state.postgres.list_bodies(&project_id).await?;
    let resolved = ids
        .iter()
        .filter(|id| {
            bodies
                .get(*id)
                .and_then(|b| b.get("properties"))
                .and_then(|p| p.get("resolutionState"))
                .and_then(|v| v.as_str())
                == Some("resolved")
        })
        .count();
    let total = ids.len();
    let state_label = if total == 0 || resolved == 0 {
        "unresolved"
    } else if resolved == total {
        "resolved"
    } else {
        "partial"
    };
    Ok(Json(ResolutionStatusResponse {
        state: state_label,
        resolved,
        total,
    }))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DerivedExistenceQuery {
    /// Comma-separated element ids "directly asserted to exist" — the DFS's own seed set.
    pub(crate) seed_ids: String,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DerivedExistenceResponse {
    pub(crate) derived_element_ids: Vec<String>,
    pub(crate) within_cycle: Vec<String>,
}

/// `GET /api/v0/projects/:projectId/cem/archspace/:subsystemId/derived-existence?seedIds=A,B,C` —
/// FR-ARCH-02's own direct HTTP surface: given a caller-supplied seed set (e.g. the project's
/// currently-resolved `SelectionChoice` ids), evaluates `sysml_core::compute_derived_existence`
/// against the project's real `ArchDerives` edges. `subsystemId` is accepted for URL-shape
/// consistency with `define`/`resolution-status` above but not otherwise used — `ArchDerives`
/// derivation genuinely crosses subsystem boundaries (the spec's own literal example is
/// Compressor/Combustor/Turbine, three different subsystems), so this deliberately evaluates over
/// every `ArchDerives` edge in the project, not just one subsystem's.
pub(crate) async fn derived_existence(
    State(state): State<AppState>,
    Path((project_id, _subsystem_id)): Path<(String, String)>,
    axum::extract::Query(query): axum::extract::Query<DerivedExistenceQuery>,
) -> Result<Json<DerivedExistenceResponse>, ApiError> {
    let seed_ids: Vec<&str> = query
        .seed_ids
        .split(',')
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .collect();
    let edges = state
        .neo4j
        .edges_of_kind(&project_id, EdgeKind::ArchDerives)
        .await?;
    let result = sysml_core::compute_derived_existence(&seed_ids, &edges);
    Ok(Json(DerivedExistenceResponse {
        derived_element_ids: result.derived_ids.into_iter().collect(),
        within_cycle: result.within_cycle.into_iter().collect(),
    }))
}

/// FR-ARCH-07's materialization half, dispatched from `mode_b.rs::accept_proposal` for
/// `origin: "archspace-instance"` proposals (`propose`, above) — the exact extension pattern
/// `document_import::materialize_proposal` already established for its own origin, mirrored here.
/// Creates one real new `:Structure` element (the literal "candidate Blocks/subgraphs" FR-ARCH-07
/// names, matching reqs v5's own bridging note: "a candidate `:Structure` subgraph tagged
/// `source: ai-generated`"), `Contains`-linked under the subsystem the design space was defined
/// for, carrying real generation provenance shaped like `SimulationRun`/solver provenance (this
/// isn't LLM-driven, so FR-CEM-05's LLM-shaped fields don't apply — its own text calls itself "the
/// LLM analog of `SimulationRun` provenance," making the solver-shaped analog the correct one
/// here), plus the real `viability` this candidate was proposed with.
pub(crate) async fn materialize_proposal(
    state: &AppState,
    project_id: &str,
    actor: &str,
    candidate: &serde_json::Value,
) -> anyhow::Result<()> {
    let subsystem_id = candidate
        .get("subsystemId")
        .and_then(|v| v.as_str())
        .context("archspace-instance candidate missing subsystemId")?
        .to_string();
    let handle_id = candidate
        .get("handleId")
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    let element = Element {
        id: uuid::Uuid::new_v4().to_string(),
        kind: NodeKind::Structure,
        name: format!(
            "Architecture Instance — {subsystem_id} — {}",
            &uuid::Uuid::new_v4().to_string()[..8]
        ),
        active: true,
        origin: Origin::AiSuggested,
    };
    state.neo4j.upsert_element(project_id, &element).await?;
    let mut diff_entries = vec![DiffEntry::ElementCreated {
        element_id: element.id.clone(),
        kind: element.kind,
        name: element.name.clone(),
    }];

    let contains_edge = Edge {
        source: subsystem_id.clone(),
        target: element.id.clone(),
        kind: EdgeKind::Contains,
        metadata: None,
    };
    state.neo4j.create_edge(project_id, &contains_edge).await?;
    diff_entries.push(DiffEntry::EdgeCreated {
        source: contains_edge.source,
        target: contains_edge.target,
        kind: contains_edge.kind,
    });

    state
        .postgres
        .upsert_body(
            project_id,
            &ElementBody {
                element_id: element.id.clone(),
                rationale: None,
                properties: serde_json::json!({
                    "designVector": candidate.get("designVector"),
                    "presentNodeNames": candidate.get("presentNodeNames"),
                    "choices": candidate.get("choices"),
                    "viability": candidate.get("viability"),
                    // No manual timestamp field -- the commit/audit-log `record_commit` writes
                    // below already captures "when," same convention every other provenance shape
                    // in this codebase already follows (confirmed: none of them embed one either).
                    "provenance": {
                        "tool": "cem-archspace",
                        "adsgCoreVersion": "1.4.1",
                        "designSpaceHandleId": handle_id,
                    },
                }),
            },
        )
        .await?;

    record_commit(
        state,
        project_id,
        actor,
        "Accept Mode B architecture instance proposal",
        diff_entries,
    )
    .await?;
    Ok(())
}
