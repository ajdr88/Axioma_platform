//! docs/IMPLEMENTATION_KICKOFF.md Phase 5 (FR-INTX-01..04) — ratifies ADR-009 as its own
//! recommended option 2: Interactions/messages/fragments are pure content on the existing
//! `:Interaction`/`:InteractionFragment` elements (Phase 1's own placeholder `NodeKind`s, added
//! specifically so this phase had "something concrete to reference"); the Lifeline/Message
//! diagram itself is a separate, new `diagram-engine` view (`InteractionPanel.tsx`) rendering that
//! content — "looks like a Sequence Diagram" stays a **view concern**, decoupled from storage,
//! exactly as ADR-009's own text recommends. No `Lifeline`/`Message` `NodeKind` was added — per
//! that same recommendation, a message is data (one entry in a JSON array on its owning
//! `:Interaction`/`:InteractionFragment`), not a new stored graph node.
//!
//! FR-INTX-02 (timing constraints) and FR-INTX-04 (reusable sub-interaction references) are both
//! just fields on a message — no separate mechanism. This captures and displays timing
//! constraints; it doesn't build a latency-analysis engine (no spec text asks for one yet).

use anyhow::Context;
use axum::{
    extract::{Path, State},
    http::HeaderMap,
    Json,
};
use sysml_core::{Edge, EdgeKind, Element, ElementBody, NodeKind, Origin};

use crate::{import, record_commit, ApiError, AppState, DiffEntry};

#[derive(Debug, serde::Deserialize)]
pub(crate) struct CreateInteractionRequest {
    pub(crate) name: String,
    #[serde(rename = "participantIds")]
    pub(crate) participant_ids: Vec<String>,
}

/// `POST /api/v0/projects/:projectId/interactions` (FR-INTX-01) — a real `:Interaction` element;
/// every participant id must already exist (a Lifeline with nothing behind it isn't meaningful).
pub(crate) async fn create_interaction(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project_id): Path<String>,
    Json(payload): Json<CreateInteractionRequest>,
) -> Result<Json<Element>, ApiError> {
    if payload.name.trim().is_empty() {
        return Err(import::BadRequest("name must not be empty".to_string()).into());
    }
    if payload.participant_ids.is_empty() {
        return Err(import::BadRequest("participantIds must not be empty".to_string()).into());
    }
    for participant_id in &payload.participant_ids {
        if state
            .neo4j
            .get_element(&project_id, participant_id)
            .await?
            .is_none()
        {
            return Err(import::BadRequest(format!("no such participant {participant_id}")).into());
        }
    }

    let element = Element {
        id: uuid::Uuid::new_v4().to_string(),
        kind: NodeKind::Interaction,
        name: payload.name,
        active: true,
        origin: Origin::Human,
    };
    state.neo4j.upsert_element(&project_id, &element).await?;
    state
        .postgres
        .upsert_body(
            &project_id,
            &ElementBody {
                element_id: element.id.clone(),
                rationale: None,
                properties: serde_json::json!({
                    "participantIds": payload.participant_ids,
                    "messages": [],
                }),
            },
        )
        .await?;
    record_commit(
        &state,
        &project_id,
        &state.auth.resolve_actor(&headers)?,
        "Create interaction",
        vec![DiffEntry::ElementCreated {
            element_id: element.id.clone(),
            kind: element.kind,
            name: element.name.clone(),
        }],
    )
    .await?;
    Ok(Json(element))
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub(crate) struct TimingConstraint {
    #[serde(rename = "minMs")]
    pub(crate) min_ms: Option<f64>,
    #[serde(rename = "maxMs")]
    pub(crate) max_ms: Option<f64>,
}

fn default_message_kind() -> String {
    "sync".to_string()
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub(crate) struct AddMessageRequest {
    pub(crate) from: String,
    pub(crate) to: String,
    pub(crate) text: String,
    /// `"sync"` | `"async"` | `"reply"` — not a Rust enum: this is display-only content the
    /// Lifeline panel switches its arrow style on, not something the backend validates against a
    /// closed set (same "plain JSON, no schema-per-field" convention every other free-form
    /// body-content field in this codebase already uses).
    #[serde(default = "default_message_kind")]
    pub(crate) kind: String,
    /// Nests this message inside a fragment's own sub-sequence (FR-INTX-03) — not validated
    /// against a real `:InteractionFragment` id here; the panel is responsible for only ever
    /// sending an id it created via `add_fragment`.
    #[serde(rename = "fragmentId", default)]
    pub(crate) fragment_id: Option<String>,
    /// FR-INTX-04 — references another `:Interaction` by id as a reusable sub-sequence.
    #[serde(rename = "refInteractionId", default)]
    pub(crate) ref_interaction_id: Option<String>,
    /// FR-INTX-02.
    #[serde(rename = "timingConstraint", default)]
    pub(crate) timing_constraint: Option<TimingConstraint>,
}

/// `POST /api/v0/projects/:projectId/interactions/:id/messages` (FR-INTX-01/02/03/04) — appends
/// to the Interaction's own `messages` array (read-merge, the established body-array-mutation
/// pattern this codebase already uses for every other multi-entry property — e.g.
/// `seed_fr_arch_system_model`'s Interface Contract merge). `order` is server-assigned (the
/// array's current length), not caller-supplied — keeps ordering unambiguous regardless of
/// request arrival order.
pub(crate) async fn add_message(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project_id, interaction_id)): Path<(String, String)>,
    Json(payload): Json<AddMessageRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let Some(element) = state
        .neo4j
        .get_element(&project_id, &interaction_id)
        .await?
    else {
        return Err(import::BadRequest(format!("no such interaction {interaction_id}")).into());
    };
    if element.kind != NodeKind::Interaction {
        return Err(import::BadRequest(format!("{interaction_id} is not an Interaction")).into());
    }

    let existing = state
        .postgres
        .get_body(&project_id, &interaction_id)
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

    let old_messages = properties
        .get("messages")
        .cloned()
        .unwrap_or(serde_json::json!([]));
    let mut messages = old_messages.as_array().cloned().unwrap_or_default();
    let order = messages.len() as u32;
    let mut message_json = serde_json::to_value(&payload).context("serializing message")?;
    message_json["order"] = serde_json::json!(order);
    messages.push(message_json.clone());
    let new_messages = serde_json::Value::Array(messages);
    properties.insert("messages".to_string(), new_messages.clone());

    state
        .postgres
        .upsert_body(
            &project_id,
            &ElementBody {
                element_id: interaction_id.clone(),
                rationale,
                properties: serde_json::Value::Object(properties),
            },
        )
        .await?;
    record_commit(
        &state,
        &project_id,
        &state.auth.resolve_actor(&headers)?,
        "Add interaction message",
        vec![DiffEntry::PropertyChanged {
            element_id: interaction_id,
            property: "messages".to_string(),
            old: old_messages,
            new: new_messages,
        }],
    )
    .await?;
    Ok(Json(message_json))
}

const FRAGMENT_KINDS: [&str; 4] = ["alt", "opt", "par", "loop"];

#[derive(Debug, serde::Deserialize)]
pub(crate) struct AddFragmentRequest {
    #[serde(rename = "fragmentKind")]
    pub(crate) fragment_kind: String,
    #[serde(default)]
    pub(crate) guard: Option<String>,
}

/// `POST /api/v0/projects/:projectId/interactions/:id/fragments` (FR-INTX-03) — a real
/// `:InteractionFragment` element, `Contains`-edged from its parent Interaction (the existing
/// containment-hierarchy convention every other element already uses). Messages nested inside
/// this fragment's sub-sequence are added via the same `add_message` endpoint with `fragmentId`
/// set to this element's id.
pub(crate) async fn add_fragment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project_id, interaction_id)): Path<(String, String)>,
    Json(payload): Json<AddFragmentRequest>,
) -> Result<Json<Element>, ApiError> {
    let Some(element) = state
        .neo4j
        .get_element(&project_id, &interaction_id)
        .await?
    else {
        return Err(import::BadRequest(format!("no such interaction {interaction_id}")).into());
    };
    if element.kind != NodeKind::Interaction {
        return Err(import::BadRequest(format!("{interaction_id} is not an Interaction")).into());
    }
    if !FRAGMENT_KINDS.contains(&payload.fragment_kind.as_str()) {
        return Err(import::BadRequest(format!(
            "unknown fragmentKind {:?} -- expected one of {FRAGMENT_KINDS:?}",
            payload.fragment_kind
        ))
        .into());
    }

    let name = match &payload.guard {
        Some(guard) if !guard.trim().is_empty() => {
            format!("{} [{guard}]", payload.fragment_kind)
        }
        _ => payload.fragment_kind.clone(),
    };
    let fragment = Element {
        id: uuid::Uuid::new_v4().to_string(),
        kind: NodeKind::InteractionFragment,
        name,
        active: true,
        origin: Origin::Human,
    };
    state.neo4j.upsert_element(&project_id, &fragment).await?;
    let mut diff_entries = vec![DiffEntry::ElementCreated {
        element_id: fragment.id.clone(),
        kind: fragment.kind,
        name: fragment.name.clone(),
    }];
    state
        .postgres
        .upsert_body(
            &project_id,
            &ElementBody {
                element_id: fragment.id.clone(),
                rationale: None,
                properties: serde_json::json!({
                    "fragmentKind": payload.fragment_kind,
                    "guard": payload.guard,
                }),
            },
        )
        .await?;
    state
        .neo4j
        .create_edge(
            &project_id,
            &Edge {
                source: interaction_id.clone(),
                target: fragment.id.clone(),
                kind: EdgeKind::Contains,
            },
        )
        .await?;
    diff_entries.push(DiffEntry::EdgeCreated {
        source: interaction_id,
        target: fragment.id.clone(),
        kind: EdgeKind::Contains,
    });
    record_commit(
        &state,
        &project_id,
        &state.auth.resolve_actor(&headers)?,
        "Add interaction fragment",
        diff_entries,
    )
    .await?;
    Ok(Json(fragment))
}
