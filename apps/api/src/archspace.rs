//! FR-ARCH-01…06's real build-out (reqs v5 §5.17) — thin HTTP wiring around
//! `cem_core::archspace`'s pure encode/decode logic and the `cem-archspace` gRPC sidecar
//! (`archspace_client.rs`), plus the resolution state machine (FR-ARCH-02/03) and its
//! incompatibility/choice-constraint enforcement (FR-ARCH-04) — real, new logic that doesn't
//! touch the sidecar at all. Mirrors `mode_b.rs`'s own shape (thin HTTP wiring around a pure
//! computation crate).
//!
//! **`define`/`decode` don't materialize anything into the graph.** A decoded architecture
//! instance is returned as data (`design vector` + `present node names` + a per-choice summary),
//! not written back as new persisted elements — that's FR-ARCH-07 (architecture instances
//! entering the `/cem/proposals/*` review-gate flow), explicitly the next requested pass, not
//! this one. Design-space handles stay process-lifetime/in-memory sidecar-side, exactly as
//! `cem_archspace.proto`'s own doc comment already scopes the spike.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use cem_core::archspace::{
    self as core_archspace, ChoiceConstraintEdge, ChoiceConstraintKindInput,
    ConnectionChoiceElement, IncompatibleWithEdge, ParameterInput, SelectionChoiceElement,
};
use sysml_core::{EdgeKind, ElementBody, NodeKind};

use crate::{archspace_client, import::BadRequest, record_commit, ApiError, AppState, DiffEntry};

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
        objective: None,
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

    // Cached so a later `decode` call for this same handle can group its result by choice
    // (`DecodeResponse::choices`) instead of always reporting an empty summary -- see
    // `AppState::archspace_definitions`'s own doc comment for why this is in-process bookkeeping,
    // not new handle persistence.
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
}

/// `POST /api/v0/projects/:projectId/cem/archspace/:handleId/decode` — FR-ARCH-05's decode half.
/// An empty/omitted `designVector` asks the sidecar to sample a random valid vector first
/// (matches `cem_archspace.proto`'s own documented `DecodeInstance` behavior). The `handleId`
/// must come from a prior `define` call in this pass -- the sidecar holds it process-lifetime,
/// in-memory only (deliberately not persisted, see this module's own doc comment).
pub(crate) async fn decode(
    State(state): State<AppState>,
    Path((_project_id, handle_id)): Path<(String, String)>,
    Json(payload): Json<DecodeRequestDto>,
) -> Result<Json<DecodeResponse>, ApiError> {
    let instance = archspace_client::decode_instance(&handle_id, payload.design_vector).await?;
    // Grouping the result by choice needs the same definition `define` encoded for this handle --
    // `AppState::archspace_definitions` caches it (in-process only, not new handle persistence,
    // see its own doc comment). A handle this process never `define`d (e.g. from a restart, or
    // one built by a different caller) falls back to an empty definition, same as before this
    // cache existed -- every present node just reports as "other" rather than grouped.
    let definition = state
        .archspace_definitions
        .lock()
        .expect("archspace_definitions mutex poisoned")
        .get(&handle_id)
        .cloned()
        .unwrap_or_default();
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
    }))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResolveChoiceRequest {
    #[serde(default)]
    pub(crate) selected_option: Option<String>,
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
