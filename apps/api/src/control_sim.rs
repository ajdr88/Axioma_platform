//! `POST /api/v0/projects/:projectId/simulate/control-state-machine` — compiles each of the
//! pilot's Control-subsystem transitions from raw Alf source via `alf-lite`, then runs the
//! resulting state machine through `fuml-runtime` (roadmap: P1.4, FR-CORE-09).
//!
//! No graph/`sysml-core` wiring yet — Alf source is supplied directly in the request body, not
//! read from any `Element`/property. `sysml-core` has no `StateMachine`/`Transition`/`Signal`
//! concepts to read it from yet, and no test requires this wiring; see
//! `packages/alf-lite/README.md` for the full scope note.

use axum::Json;
use sysml_core::SolverResultState;

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

/// The pilot's Control state machine, exactly as described in the test specs: Idle -> Armed
/// (`arm`) -> Running (`ignite`, the one transition with a real guard+effect) -> Shutdown
/// (`cutoff`). The Idle->Armed and Running->Shutdown transitions are trivial (empty actions) —
/// the docs only ever give one concrete transition action to compile. `pub(crate)` (not test-only
/// anymore) since `trade_study`'s "run the behavioral sim" step (T-P1.4-05) needs the same known-
/// good program the P1.4 tests already verify against, not a second copy.
pub(crate) fn golden_alf_transitions(
) -> Vec<(&'static str, &'static str, &'static str, &'static str)> {
    vec![
        ("Idle", "Armed", "arm", ""),
        (
            "Armed",
            "Running",
            "ignite",
            "if (Turbine.rpm < 3500.0) { SetTurbineRpm(3500.0); }",
        ),
        ("Running", "Shutdown", "cutoff", ""),
    ]
}

pub(crate) fn golden_signals() -> Vec<String> {
    vec![
        "arm".to_string(),
        "ignite".to_string(),
        "cutoff".to_string(),
    ]
}

/// Runs `golden_alf_transitions` end to end through `fuml-runtime` and returns FR-CEM-13's typed
/// `SolverResultState` (reused here rather than a bespoke bool, the "one shared typed-outcome
/// vocabulary" this pass retrofits) plus the final reading, for callers (e.g. `trade_study`) that
/// just need a real result state + final reading, not the full trace.
///
/// Classified from the only real signal `fuml_client`'s trace stream actually gives us — a
/// genuine RPC/connection failure stays a real `Err` (infrastructural, not one of the six typed
/// solver outcomes; conflating "sidecar unreachable" with "sidecar ran and diverged" would lose
/// real information):
/// - An `"output"` event with the golden expected value (`Turbine.rpm` = 3500.0) → `Converged`.
/// - An `"output"` event with any other value → `SuspectNumerical` — it ran and produced *a*
///   result, just not the expected one, exactly the "plausibility pass... before any graph write"
///   NFR-REL-03 already calls for.
/// - No `"output"` event at all → `Diverged` — the RPC itself succeeded (events came back), but
///   the state machine never reached its final send/output step; a real non-convergence, distinct
///   from an infra failure.
pub(crate) async fn run_golden_control_sim() -> Result<(SolverResultState, Option<String>), ApiError>
{
    let request = ControlStateMachineRequest {
        transitions: golden_alf_transitions()
            .into_iter()
            .map(|(from, to, signal, alf_source)| TransitionRequest {
                from: from.to_string(),
                to: to.to_string(),
                signal: signal.to_string(),
                alf_source: alf_source.to_string(),
            })
            .collect(),
        signals: golden_signals(),
        use_hand_authored_reference: false,
    };
    let Json(events) = simulate_control_state_machine(Json(request)).await?;
    let final_rpm = events
        .iter()
        .rev()
        .find(|e| e.kind == "output")
        .map(|e| e.detail.clone());
    let state = match final_rpm.as_deref() {
        Some("3500.0") => SolverResultState::Converged,
        Some(_) => SolverResultState::SuspectNumerical,
        None => SolverResultState::Diverged,
    };
    Ok((state, final_rpm))
}
