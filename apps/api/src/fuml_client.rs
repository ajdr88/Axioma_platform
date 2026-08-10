//! P1.4 (roadmap: Behavioral Simulation, ADR-005/008) — the gRPC client to the `fuml-runtime`
//! JVM sidecar. This pass only proves the pipeline mechanics T-P1.4-01 actually asks about
//! (deterministic, incrementally-streamed execution over the gRPC boundary): the sidecar
//! currently supports exactly one fixed, hard-coded activity ("HelloWorld2", the same one the
//! ADR-005 spike already executed) — no XMI/model transfer yet, that's `alf-lite`'s job later.
//! No dedicated Rust workspace crate for this client (one caller — same "thin slice, no
//! premature package" reasoning already applied to Mode A/`llm-gateway`).

use axum::Json;

use crate::{env_or, ApiError};

#[allow(clippy::enum_variant_names)] // generated code (prost oneofs) — not ours to rename
pub(crate) mod proto {
    tonic::include_proto!("axioma.fuml");
}

use proto::fuml_runtime_client::FumlRuntimeClient;
use proto::ExecuteRequest;

/// The only activity the sidecar supports this pass — see `FumlRuntimeServiceImpl`'s own
/// `HELLO_WORLD_ACTIVITY` constant, which this must match exactly.
const HELLO_WORLD_ACTIVITY: &str = "HelloWorld2";

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct TraceEventDto {
    #[serde(rename = "activityName")]
    pub(crate) activity_name: String,
    #[serde(rename = "actionName")]
    pub(crate) action_name: String,
    pub(crate) kind: String,
    // Read by `control_sim::run_golden_control_sim` (the final `"output"`-kind event's `detail`
    // carries the golden run's `Turbine.rpm` reading) as well as being carried in the JSON
    // response for every other caller.
    pub(crate) detail: String,
}

impl From<proto::TraceEvent> for TraceEventDto {
    fn from(event: proto::TraceEvent) -> Self {
        Self {
            activity_name: event.activity_name,
            action_name: event.action_name,
            kind: event.kind,
            detail: event.detail,
        }
    }
}

/// Calls the sidecar's `Execute` RPC and collects the streamed trace into a `Vec` — collecting
/// here is honest scope for this pass (the plan's own "true incremental HTTP-to-browser
/// streaming is a separate, later concern"); the RPC itself is still genuinely server-streaming,
/// which is what `execute_streaming` (used by the T-P1.4-01 test) verifies directly.
pub(crate) async fn execute(activity_name: &str) -> anyhow::Result<Vec<TraceEventDto>> {
    let mut events = Vec::new();
    let mut stream = execute_streaming(activity_name).await?;
    while let Some(event) = stream.message().await? {
        events.push(TraceEventDto::from(event));
    }
    Ok(events)
}

/// Opens the raw `Streaming<TraceEvent>` response without draining it — kept separate from
/// `execute` so the integration test can assert on the transport itself (a `Streaming` handle,
/// not a pre-collected `Vec`) rather than just its already-collected contents.
pub(crate) async fn execute_streaming(
    activity_name: &str,
) -> anyhow::Result<tonic::Streaming<proto::TraceEvent>> {
    let addr = env_or("FUML_RUNTIME_ADDR", "http://localhost:50051");
    let mut client = FumlRuntimeClient::connect(addr).await?;
    let response = client
        .execute(ExecuteRequest {
            activity_name: activity_name.to_string(),
        })
        .await?;
    Ok(response.into_inner())
}

pub(crate) async fn simulate_hello_world() -> Result<Json<Vec<TraceEventDto>>, ApiError> {
    Ok(Json(execute(HELLO_WORLD_ACTIVITY).await?))
}

/// Runs the pilot's Control state machine (alf-lite's compiled transitions, or the
/// hand-authored Java reference path when `use_hand_authored_reference` is set — T-P1.4-04's
/// comparison mechanism) and collects the streamed trace. See `execute`'s doc comment for why
/// collecting server-side is honest scope for this pass.
pub(crate) async fn execute_state_machine(
    transitions: Vec<proto::Transition>,
    signals_to_fire: Vec<String>,
    use_hand_authored_reference: bool,
) -> anyhow::Result<Vec<TraceEventDto>> {
    let addr = env_or("FUML_RUNTIME_ADDR", "http://localhost:50051");
    let mut client = FumlRuntimeClient::connect(addr).await?;
    let response = client
        .execute_state_machine(proto::StateMachineRequest {
            transitions,
            signals_to_fire,
            use_hand_authored_reference,
        })
        .await?;
    let mut stream = response.into_inner();
    let mut events = Vec::new();
    while let Some(event) = stream.message().await? {
        events.push(TraceEventDto::from(event));
    }
    Ok(events)
}
