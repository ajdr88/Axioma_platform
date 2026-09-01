//! docs/IMPLEMENTATION_KICKOFF.md Phase 2 (ADR-011) — the gRPC client to the `cem-archspace`
//! Python sidecar wrapping `adsg-core`/`SBArchOpt` for Mode B's architecture design-space
//! representation (reqs v5 §5.17, FR-ARCH). Same "thin client module in `apps/api`, not inside a
//! pure package" convention as `fuml_client.rs` — `cem-core` stays zero-I/O.
//!
//! The Phase 2 spike proved the sidecar/gRPC plumbing only (see `packages/cem-archspace/
//! README.md` for the exact spike scope and real numbers observed); `connect`/
//! `define_design_space`/`get_design_space_stats`/`decode_instance` are now real, non-test
//! callers via `archspace.rs`'s HTTP handlers (FR-ARCH-01…06's real build-out). `run_optimization`
//! stays test-only for now — wiring it into a real optimize/propose flow is FR-ARCH-07/08, not
//! this pass.

use crate::env_or;

#[allow(clippy::enum_variant_names)] // generated code (prost oneofs) — not ours to rename
pub(crate) mod proto {
    tonic::include_proto!("axioma.archspace");
}

use proto::cem_archspace_client::CemArchspaceClient;
#[cfg(test)]
use proto::{
    ChoiceConstraint, ChoiceConstraintKind, ConnectionChoice, DesignVariable,
    IncompatibilityConstraint, Objective, SelectionChoice,
};
use proto::{DecodeRequest, DesignSpaceDefinition, OptimizeRequest};

pub(crate) use proto::{ArchitectureInstance, DesignSpaceStats, OptimizeResult};

async fn connect() -> anyhow::Result<CemArchspaceClient<tonic::transport::Channel>> {
    let addr = env_or("ARCHSPACE_ADDR", "http://localhost:50052");
    Ok(CemArchspaceClient::connect(addr).await?)
}

/// Builds a real `adsg_core.BasicDSG` server-side and returns the handle id it's held under.
/// Fails if `adsg-core` itself rejects the definition as infeasible (surfaced by the sidecar as
/// `INVALID_ARGUMENT`) — propagated here as a plain `anyhow::Error`, same as every other gRPC
/// call site in this codebase (`fuml_client.rs` does the same, no bespoke error type).
pub(crate) async fn define_design_space(
    definition: DesignSpaceDefinition,
) -> anyhow::Result<String> {
    let mut client = connect().await?;
    let handle = client.define_design_space(definition).await?.into_inner();
    Ok(handle.id)
}

pub(crate) async fn get_design_space_stats(handle_id: &str) -> anyhow::Result<DesignSpaceStats> {
    let mut client = connect().await?;
    let stats = client
        .get_design_space_stats(proto::DesignSpaceHandle {
            id: handle_id.to_string(),
        })
        .await?
        .into_inner();
    Ok(stats)
}

/// `design_vector` empty asks the sidecar to sample a random valid vector before decoding.
pub(crate) async fn decode_instance(
    handle_id: &str,
    design_vector: Vec<f64>,
) -> anyhow::Result<ArchitectureInstance> {
    let mut client = connect().await?;
    let instance = client
        .decode_instance(DecodeRequest {
            handle_id: handle_id.to_string(),
            design_vector,
        })
        .await?
        .into_inner();
    Ok(instance)
}

#[allow(dead_code)]
pub(crate) async fn run_optimization(
    handle_id: &str,
    population_size: i32,
    n_generations: i32,
    seed: i32,
) -> anyhow::Result<OptimizeResult> {
    let mut client = connect().await?;
    let result = client
        .run_optimization(OptimizeRequest {
            handle_id: handle_id.to_string(),
            population_size,
            n_generations,
            seed,
        })
        .await?
        .into_inner();
    Ok(result)
}

/// Builds the spike's own test problem — Core (HP) Compressor / Turbine stage-count and
/// bleed-offtake choices (reqs v5 §5.16) — the same definition proven directly against the
/// sidecar (see `packages/cem-archspace/README.md`) before this Rust wrapper existed. Exercises
/// all four primitives `docs/IMPLEMENTATION_KICKOFF.md` Phase 2 names: a selection choice, a
/// connection choice, an incompatibility constraint, and a `LINKED` choice constraint.
#[cfg(test)]
pub(crate) fn spike_compressor_design_space() -> DesignSpaceDefinition {
    DesignSpaceDefinition {
        root_name: "CoreHpCompressor".to_string(),
        connector_names: vec!["BleedOfftakeConnector".to_string(), "EcsPort".to_string()],
        design_variables: vec![
            DesignVariable {
                name: "n_HP_stages".to_string(),
                lower_bound: 1.0,
                upper_bound: 4.0,
            },
            DesignVariable {
                name: "n_HP_turbine_stages".to_string(),
                lower_bound: 1.0,
                upper_bound: 4.0,
            },
        ],
        selection_choices: vec![
            SelectionChoice {
                choice_id: "BleedOfftakeStage".to_string(),
                option_names: vec![
                    "Stage1".to_string(),
                    "Stage2".to_string(),
                    "Stage3".to_string(),
                    "Stage4".to_string(),
                ],
            },
            SelectionChoice {
                choice_id: "NozzleConfig".to_string(),
                option_names: vec!["MixedNozzle".to_string(), "SeparateNozzle".to_string()],
            },
            SelectionChoice {
                choice_id: "Gearbox".to_string(),
                option_names: vec!["Geared".to_string(), "DirectDrive".to_string()],
            },
        ],
        connection_choices: vec![ConnectionChoice {
            choice_id: "BleedRouting".to_string(),
            source_connector_names: vec!["BleedOfftakeConnector".to_string()],
            target_connector_names: vec!["EcsPort".to_string()],
        }],
        incompatibility_constraints: vec![IncompatibilityConstraint {
            node_names: vec!["MixedNozzle".to_string(), "Geared".to_string()],
        }],
        choice_constraints: vec![ChoiceConstraint {
            kind: ChoiceConstraintKind::Linked as i32,
            node_names: vec!["n_HP_stages".to_string(), "n_HP_turbine_stages".to_string()],
        }],
        objective: Some(Objective {
            name: "TotalStages".to_string(),
            direction: -1,
        }),
    }
}
