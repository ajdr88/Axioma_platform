//! `POST /api/v0/projects/:projectId/simulate/control-state-machine` — compiles each of the
//! pilot's Control-subsystem transitions from raw Alf source via `alf-lite`, then runs the
//! resulting state machine through `fuml-runtime` (roadmap: P1.4, FR-CORE-09).
//!
//! No graph/`sysml-core` wiring yet — Alf source is supplied directly in the request body, not
//! read from any `Element`/property. `sysml-core` has no `StateMachine`/`Transition`/`Signal`
//! concepts to read it from yet, and no test requires this wiring; see
//! `packages/alf-lite/README.md` for the full scope note.

use axum::Json;

use crate::{alf_ir, fuml_client, import::BadRequest, ApiError};

#[derive(Debug, serde::Deserialize)]
pub(crate) struct TransitionRequest {
    pub(crate) from: String,
    pub(crate) to: String,
    pub(crate) signal: String,
    #[serde(rename = "alfSource")]
    pub(crate) alf_source: String,
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct ControlStateMachineRequest {
    pub(crate) transitions: Vec<TransitionRequest>,
    pub(crate) signals: Vec<String>,
    #[serde(default, rename = "useHandAuthoredReference")]
    pub(crate) use_hand_authored_reference: bool,
}

pub(crate) async fn simulate_control_state_machine(
    Json(payload): Json<ControlStateMachineRequest>,
) -> Result<Json<Vec<fuml_client::TraceEventDto>>, ApiError> {
    let mut transitions = Vec::with_capacity(payload.transitions.len());
    for transition in &payload.transitions {
        let program = alf_lite::parse(&transition.alf_source).map_err(|err| {
            BadRequest(format!(
                "compiling {}->{} transition action: {err}",
                transition.from, transition.to
            ))
        })?;
        transitions.push(fuml_client::proto::Transition {
            from_state: transition.from.clone(),
            to_state: transition.to.clone(),
            signal: transition.signal.clone(),
            actions: alf_ir::compile_program(&program),
        });
    }

    let events = fuml_client::execute_state_machine(
        transitions,
        payload.signals.clone(),
        payload.use_hand_authored_reference,
    )
    .await?;
    Ok(Json(events))
}
