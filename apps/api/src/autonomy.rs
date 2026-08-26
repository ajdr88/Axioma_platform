//! P2.2 autonomy decision logic (FR-CEM-16/17/18, NFR-CEM-06, NFR-OPS-04) — `apps/api/src/mode_b.rs`'s
//! `propose` endpoint is the only caller. Everything here reads already-fetched store state and
//! returns a decision; it never writes anything itself, so applying that decision (committing to
//! `main`, creating a proposal row) stays the caller's explicit responsibility, not buried here.

use std::str::FromStr;

use cem_core::{Candidate, Constraints};
use sysml_core::EdgeKind;

use crate::AppState;

/// L0-L4 per implementation doc §5.6 — ordered from most to least conservative. `L0` is the safe
/// default a project starts on before anyone configures autonomy at all.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum Level {
    L0,
    L1,
    L2,
    L3,
    L4,
}

impl Level {
    pub const DEFAULT: Level = Level::L0;
}

impl std::fmt::Display for Level {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Level::L0 => "L0",
            Level::L1 => "L1",
            Level::L2 => "L2",
            Level::L3 => "L3",
            Level::L4 => "L4",
        };
        write!(f, "{s}")
    }
}

impl FromStr for Level {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "L0" => Ok(Level::L0),
            "L1" => Ok(Level::L1),
            "L2" => Ok(Level::L2),
            "L3" => Ok(Level::L3),
            "L4" => Ok(Level::L4),
            other => Err(format!("unknown autonomy level {other:?} (expected L0-L4)")),
        }
    }
}

/// Reads a scope's configured level/threshold, defaulting to `(L0, None)` when nothing has ever
/// been set for it — matching `GET .../cem/autonomy-level/:scope`'s own same-default behavior.
pub async fn resolve_level(
    state: &AppState,
    project_id: &str,
    scope: &str,
) -> anyhow::Result<(Level, Option<f64>)> {
    match state
        .versioning
        .get_autonomy_config(project_id, scope)
        .await?
    {
        Some(config) => {
            let level = config
                .level
                .parse::<Level>()
                .map_err(|e| anyhow::anyhow!(e))?;
            Ok((level, config.mass_deviation_threshold_percent))
        }
        None => Ok((Level::DEFAULT, None)),
    }
}

/// FR-CEM-18, the non-negotiable exception: true if `subsystem_id` `Causes` a Hazard that is
/// either unmitigated (no `MitigatedBy`-linked Control with `status: "Mitigated"`) or classified
/// `Major`/`Catastrophic`. The implementation doc names "High or Catastrophic"; this project's
/// existing 5-level severity scale (`traceability.rs::SEVERITY_LEVELS`, the same one
/// `HazardRiskPanel.tsx` renders) has no literal "High" value, so its top two levels
/// (`Major`/`Catastrophic`) are treated as that interpretive mapping — same precedent as the risk
/// register's own severity-scale interpretation.
pub async fn hazard_override(
    state: &AppState,
    project_id: &str,
    subsystem_id: &str,
) -> anyhow::Result<bool> {
    let causes_edges = state
        .neo4j
        .edges_of_kind(project_id, EdgeKind::Causes)
        .await?;
    let mitigated_by_edges = state
        .neo4j
        .edges_of_kind(project_id, EdgeKind::MitigatedBy)
        .await?;

    for edge in causes_edges.iter().filter(|e| e.source == subsystem_id) {
        let hazard_id = edge.target.as_str();
        let hazard_body = state.postgres.get_body(project_id, hazard_id).await?;
        let severity = hazard_body
            .as_ref()
            .and_then(|b| b.get("properties"))
            .and_then(|p| p.get("severity"))
            .and_then(|v| v.as_str());
        let is_high_or_catastrophic = matches!(severity, Some("Major") | Some("Catastrophic"));

        let mut any_mitigated = false;
        for control_edge in mitigated_by_edges.iter().filter(|e| e.source == hazard_id) {
            let control_body = state
                .postgres
                .get_body(project_id, &control_edge.target)
                .await?;
            let status = control_body
                .as_ref()
                .and_then(|b| b.get("properties"))
                .and_then(|v| v.get("status"))
                .and_then(|v| v.as_str())
                .unwrap_or("Open");
            if status == "Mitigated" {
                any_mitigated = true;
                break;
            }
        }

        if is_high_or_catastrophic || !any_mitigated {
            return Ok(true);
        }
    }
    Ok(false)
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(tag = "outcome", rename_all = "camelCase")]
pub enum Decision {
    Merge,
    Review { reason: String },
}

/// The level-only decision (before any per-subsystem hazard override is applied on top — see
/// `hazard_override` and the plan's own note that a hazard-linked subsystem can be downgraded
/// from `Merge` to `Review` while unrelated subsystems in the same candidate still merge).
///
/// `main_head_stale` is `true` when the caller's `expectedMainHeadCommitId` no longer matches
/// `main`'s actual head (T-P2.2-05/NFR-OPS-04) — always forces `Review` regardless of level, so a
/// human edit that landed while this candidate was being computed never gets force-merged over.
pub fn decide(
    level: Level,
    threshold_percent: Option<f64>,
    candidate: &Candidate,
    constraints: &Constraints,
    main_head_stale: bool,
) -> Decision {
    if main_head_stale {
        return Decision::Review {
            reason: "concurrent_change".to_string(),
        };
    }
    match level {
        Level::L0 | Level::L1 | Level::L2 => Decision::Review {
            reason: "autonomy_level_requires_review".to_string(),
        },
        Level::L3 => {
            let (Some(max_mass), Some(threshold)) =
                (constraints.max_total_mass_kg, threshold_percent)
            else {
                return Decision::Review {
                    reason: "missing_threshold_config".to_string(),
                };
            };
            let deviation_percent = (candidate.total_mass_kg - max_mass) / max_mass * 100.0;
            if deviation_percent <= threshold {
                Decision::Merge
            } else {
                Decision::Review {
                    reason: "below_l3_threshold_review".to_string(),
                }
            }
        }
        Level::L4 => Decision::Merge,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(total_mass_kg: f64) -> Candidate {
        Candidate {
            params: cem_core::SubsystemParams {
                bypass_ratio: 5.0,
                pressure_ratio: 30.0,
                turbine_inlet_temp_k: 1800.0,
                turbine_stage_count: 2,
            },
            thrust_lbf: 20_000.0,
            sfc: 0.6,
            total_mass_kg,
        }
    }

    #[test]
    fn level_round_trips_through_display_and_from_str() {
        for level in [Level::L0, Level::L1, Level::L2, Level::L3, Level::L4] {
            assert_eq!(level.to_string().parse::<Level>().unwrap(), level);
        }
        assert!("L5".parse::<Level>().is_err());
    }

    #[test]
    fn l0_l1_l2_always_review_regardless_of_candidate() {
        for level in [Level::L0, Level::L1, Level::L2] {
            let decision = decide(
                level,
                None,
                &candidate(100.0),
                &Constraints::default(),
                false,
            );
            assert_eq!(
                decision,
                Decision::Review {
                    reason: "autonomy_level_requires_review".to_string()
                }
            );
        }
    }

    #[test]
    fn l3_merges_within_threshold_and_reviews_beyond_it() {
        let constraints = Constraints {
            max_total_mass_kg: Some(1_000.0),
        };
        // 3% over a 1000kg baseline, 5% threshold -> merges.
        assert_eq!(
            decide(
                Level::L3,
                Some(5.0),
                &candidate(1_030.0),
                &constraints,
                false
            ),
            Decision::Merge
        );
        // 12% over the same baseline/threshold -> review.
        assert_eq!(
            decide(
                Level::L3,
                Some(5.0),
                &candidate(1_120.0),
                &constraints,
                false
            ),
            Decision::Review {
                reason: "below_l3_threshold_review".to_string()
            }
        );
    }

    #[test]
    fn l3_without_a_configured_threshold_never_auto_merges() {
        let constraints = Constraints {
            max_total_mass_kg: Some(1_000.0),
        };
        assert_eq!(
            decide(Level::L3, None, &candidate(1_000.0), &constraints, false),
            Decision::Review {
                reason: "missing_threshold_config".to_string()
            }
        );
    }

    #[test]
    fn l4_merges() {
        assert_eq!(
            decide(
                Level::L4,
                None,
                &candidate(100.0),
                &Constraints::default(),
                false
            ),
            Decision::Merge
        );
    }

    #[test]
    fn stale_main_head_forces_review_even_at_l4() {
        assert_eq!(
            decide(
                Level::L4,
                None,
                &candidate(100.0),
                &Constraints::default(),
                true
            ),
            Decision::Review {
                reason: "concurrent_change".to_string()
            }
        );
    }
}
