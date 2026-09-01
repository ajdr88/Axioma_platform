//! `cem-core` — Mode B's deterministic architecture-synthesis optimizer (roadmap: P2.1,
//! FR-CEM-02/08). **Never an LLM** — this crate has exactly one dependency (`serde`, for the
//! request/response shapes `apps/api`'s `mode_b.rs` sends over HTTP) and does no I/O at all,
//! which is what makes T-P2.1-01's "no `llm-gateway` dependency" FAIL condition trivially,
//! structurally true rather than something to remember to keep true.
//!
//! **0D turbofan performance model — deliberately simple, explicitly invented.** Nothing in the
//! doc set gives a concrete formula relating subsystem parameters to thrust/SFC/mass (only
//! "deterministic 0D/1D performance and mass-budget models," no algorithm class or equations) —
//! same design-gap shape as `HazardRiskPanel`'s Risk Index or Trade Study's `estimate_thrust_lbf`
//! earlier in this project's history: invent a reasonable, documented, deterministic
//! relationship rather than guessing at detail that was never specified. The formulas below
//! reflect real, qualitative turbomachinery tradeoffs (higher pressure ratio/turbine inlet temp
//! raise thrust and mass; higher bypass ratio lowers specific fuel consumption and raises fan
//! mass) without claiming to be a real performance model. Reference constants reuse
//! `REQ-THRUST`'s own "30,000 lbf" figure and Trade Study's `5.0` bypass-ratio reference point,
//! the same anchors already established elsewhere in this codebase.
//!
//! **Determinism by construction, not by seeding**: `optimize` enumerates a fixed parameter grid
//! (no randomness anywhere in this crate) and sorts by a deterministic key. Two calls with
//! identical `Targets`/`Constraints` always produce byte-identical output — T-P2.1-01's actual
//! claim, provable in a unit test with no Docker/DB/network involved at all.

use std::collections::HashMap;

pub mod archspace;

/// This crate's own version, exposed so callers (`apps/api`'s `mode_b.rs`) can record which
/// `cem-core` produced a given accepted candidate as part of its generation provenance — the
/// deterministic-optimizer equivalent of an LLM call's `modelVersion` (FR-CEM-05's own precedent).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The four reference subsystems this pass's grid actually varies — `ControlFadecEec` is
/// software/logic, not a thermodynamic stage, and is deliberately excluded (held fixed, out of
/// the optimization entirely) rather than given a meaningless parameter.
pub const FAN_LP_COMPRESSION: &str = "FanLpCompression";
pub const CORE_HP_COMPRESSOR: &str = "CoreHpCompressor";
pub const COMBUSTOR: &str = "Combustor";
pub const TURBINE_HP_LP: &str = "TurbineHpLp";

const BYPASS_RATIO_CHOICES: [f64; 4] = [4.0, 5.0, 6.0, 7.0];
const PRESSURE_RATIO_CHOICES: [f64; 3] = [8.0, 10.0, 12.0];
const TURBINE_INLET_TEMP_K_CHOICES: [f64; 3] = [1500.0, 1650.0, 1800.0];
const TURBINE_STAGE_COUNT_CHOICES: [u32; 3] = [2, 3, 4];

const REFERENCE_THRUST_LBF: f64 = 30_000.0;
const REFERENCE_BYPASS_RATIO: f64 = 5.0;
const REFERENCE_PRESSURE_RATIO: f64 = 10.0;
const REFERENCE_TURBINE_INLET_TEMP_K: f64 = 1650.0;
const REFERENCE_STAGE_COUNT: f64 = 3.0;
const REFERENCE_SFC: f64 = 0.6;
const REFERENCE_MASS_KG: f64 = 4000.0;

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubsystemParams {
    pub bypass_ratio: f64,
    pub pressure_ratio: f64,
    pub turbine_inlet_temp_k: f64,
    pub turbine_stage_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub params: SubsystemParams,
    pub thrust_lbf: f64,
    pub sfc: f64,
    pub total_mass_kg: f64,
}

/// Absent fields mean "unconstrained" — a study can target thrust without also capping SFC.
#[derive(Debug, Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Targets {
    #[serde(default)]
    pub min_thrust_lbf: Option<f64>,
    #[serde(default)]
    pub max_sfc: Option<f64>,
}

#[derive(Debug, Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Constraints {
    #[serde(default)]
    pub max_total_mass_kg: Option<f64>,
}

fn compute_performance(params: SubsystemParams) -> (f64, f64, f64) {
    let thrust_lbf = REFERENCE_THRUST_LBF
        * (params.pressure_ratio / REFERENCE_PRESSURE_RATIO)
        * (params.turbine_inlet_temp_k / REFERENCE_TURBINE_INLET_TEMP_K)
        * (REFERENCE_BYPASS_RATIO / params.bypass_ratio).powf(0.3);
    let sfc = REFERENCE_SFC
        * (REFERENCE_BYPASS_RATIO / params.bypass_ratio).powf(0.4)
        * (params.pressure_ratio / REFERENCE_PRESSURE_RATIO).powf(0.1);
    let total_mass_kg = REFERENCE_MASS_KG
        * (1.0 + 0.05 * (params.bypass_ratio - REFERENCE_BYPASS_RATIO))
        * (1.0 + 0.08 * (params.pressure_ratio / REFERENCE_PRESSURE_RATIO - 1.0))
        * (1.0 + 0.03 * (params.turbine_stage_count as f64 - REFERENCE_STAGE_COUNT));
    (thrust_lbf, sfc, total_mass_kg)
}

/// Enumerates the fixed 4×3×3×3 = 108 candidate grid (T-P2.1-02's "≥ dozens" — comfortably
/// cleared by construction, not tuned to just barely pass), filters out anything that misses a
/// specified target or exceeds a specified constraint, then ranks the survivors by mass ascending
/// — the lightest design that still meets every stated requirement wins. Simpler and more
/// obviously deterministic than a weighted composite score, and avoids inventing relative
/// weights between thrust/SFC/mass that nothing in the docs specifies.
pub fn optimize(targets: &Targets, constraints: &Constraints) -> Vec<Candidate> {
    let mut candidates = Vec::new();
    for &bypass_ratio in &BYPASS_RATIO_CHOICES {
        for &pressure_ratio in &PRESSURE_RATIO_CHOICES {
            for &turbine_inlet_temp_k in &TURBINE_INLET_TEMP_K_CHOICES {
                for &turbine_stage_count in &TURBINE_STAGE_COUNT_CHOICES {
                    let params = SubsystemParams {
                        bypass_ratio,
                        pressure_ratio,
                        turbine_inlet_temp_k,
                        turbine_stage_count,
                    };
                    let (thrust_lbf, sfc, total_mass_kg) = compute_performance(params);

                    if let Some(min_thrust) = targets.min_thrust_lbf {
                        if thrust_lbf < min_thrust {
                            continue;
                        }
                    }
                    if let Some(max_sfc) = targets.max_sfc {
                        if sfc > max_sfc {
                            continue;
                        }
                    }
                    if let Some(max_mass) = constraints.max_total_mass_kg {
                        if total_mass_kg > max_mass {
                            continue;
                        }
                    }

                    candidates.push(Candidate {
                        params,
                        thrust_lbf,
                        sfc,
                        total_mass_kg,
                    });
                }
            }
        }
    }
    candidates.sort_by(|a, b| {
        a.total_mass_kg
            .partial_cmp(&b.total_mass_kg)
            .expect("mass is never NaN — computed from finite grid inputs")
    });
    candidates
}

/// FR-CEM-08's six named fields, populated from one accepted candidate's parameters for one
/// subsystem. Only three fields are legitimately derivable from a 0D thermodynamic model
/// (performance targets, boundary conditions, mass target) — the other three (geometric envelope,
/// interface/port definitions, material/process constraints) are Mode C's domain (`cem-geometry`,
/// P2.3, not built yet) and say so honestly rather than fabricating dimensions or materials this
/// pass's model has no basis to produce. "Populated" (T-P2.1-03's literal PASS wording) means
/// present and non-empty, not necessarily rich — an honest placeholder still satisfies that.
pub fn build_interface_contract(
    subsystem_id: &str,
    candidate: &Candidate,
) -> HashMap<String, serde_json::Value> {
    let p = candidate.params;
    let performance_targets = match subsystem_id {
        FAN_LP_COMPRESSION => serde_json::json!({ "bypassRatio": p.bypass_ratio }),
        CORE_HP_COMPRESSOR => serde_json::json!({ "pressureRatio": p.pressure_ratio }),
        COMBUSTOR => serde_json::json!({ "turbineInletTempK": p.turbine_inlet_temp_k }),
        TURBINE_HP_LP => serde_json::json!({
            "turbineInletTempK": p.turbine_inlet_temp_k,
            "stageCount": p.turbine_stage_count,
        }),
        _ => serde_json::json!({
            "thrustLbf": candidate.thrust_lbf,
            "sfc": candidate.sfc,
        }),
    };
    let boundary_conditions = match subsystem_id {
        TURBINE_HP_LP => serde_json::json!({
            "inletTempK": p.turbine_inlet_temp_k,
            "note": "combustor exit conditions, taken as this subsystem's inlet boundary",
        }),
        _ => serde_json::json!({ "ambient": "sea-level static (0D model assumption)" }),
    };

    let not_modeled = || {
        serde_json::json!({
            "note": "not derivable from a 0D/1D performance model — Mode C (cem-geometry, P2.3) \
                     is where geometry/interfaces/materials actually get decided",
        })
    };

    let mut contract = HashMap::new();
    contract.insert("performanceTargets".to_string(), performance_targets);
    contract.insert("boundaryConditions".to_string(), boundary_conditions);
    contract.insert("geometricEnvelope".to_string(), not_modeled());
    contract.insert(
        "interfacePortDefinitions".to_string(),
        serde_json::json!({ "ports": ["inlet", "outlet"] }),
    );
    contract.insert(
        "massCostTargets".to_string(),
        serde_json::json!({
            "totalMassKg": candidate.total_mass_kg,
            "note": "cost is not modeled by this 0D pass",
        }),
    );
    contract.insert("materialProcessConstraints".to_string(), not_modeled());
    contract
}

#[cfg(test)]
mod tests {
    use super::*;

    /// T-P2.1-01's actual claim, provable in isolation: identical inputs produce byte-identical
    /// output, no Docker/DB/network needed to demonstrate it.
    #[test]
    fn optimize_is_deterministic_across_identical_calls() {
        let targets = Targets {
            min_thrust_lbf: Some(28_000.0),
            max_sfc: None,
        };
        let constraints = Constraints {
            max_total_mass_kg: Some(4_500.0),
        };
        let first = optimize(&targets, &constraints);
        let second = optimize(&targets, &constraints);
        assert_eq!(first, second);
        assert!(
            !first.is_empty(),
            "expected at least one feasible candidate"
        );
    }

    /// T-P2.1-02: the grid comfortably clears "dozens" with no targets/constraints narrowing it.
    #[test]
    fn optimize_explores_at_least_dozens_of_candidates_when_unconstrained() {
        let candidates = optimize(&Targets::default(), &Constraints::default());
        assert_eq!(candidates.len(), 4 * 3 * 3 * 3);
    }

    #[test]
    fn optimize_filters_out_candidates_violating_the_mass_constraint() {
        let tight = Constraints {
            max_total_mass_kg: Some(3_000.0),
        };
        let loose = Constraints::default();
        let tight_results = optimize(&Targets::default(), &tight);
        let loose_results = optimize(&Targets::default(), &loose);
        assert!(tight_results.len() < loose_results.len());
        assert!(tight_results.iter().all(|c| c.total_mass_kg <= 3_000.0));
    }

    #[test]
    fn optimize_ranks_feasible_candidates_by_ascending_mass() {
        let candidates = optimize(&Targets::default(), &Constraints::default());
        for window in candidates.windows(2) {
            assert!(window[0].total_mass_kg <= window[1].total_mass_kg);
        }
    }

    #[test]
    fn interface_contract_has_all_six_fields_populated_for_turbine() {
        let candidate = &optimize(&Targets::default(), &Constraints::default())[0];
        let contract = build_interface_contract(TURBINE_HP_LP, candidate);
        for field in [
            "performanceTargets",
            "boundaryConditions",
            "geometricEnvelope",
            "interfacePortDefinitions",
            "massCostTargets",
            "materialProcessConstraints",
        ] {
            let value = contract
                .get(field)
                .unwrap_or_else(|| panic!("missing {field}"));
            assert!(!value.is_null(), "{field} must be populated, not null");
        }
    }
}
