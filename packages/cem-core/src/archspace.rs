//! Mode B architecture design-space encode/decode (FR-ARCH-05, reqs v5 §5.17) — the "`cem-core`
//! can encode a resolved (or partially resolved) architecture into a design vector and decode a
//! design vector back into a graph instance" half of that requirement. **No I/O, no protobuf, no
//! `tonic` here** — same discipline as this crate's own top-level doc comment (`lib.rs`): this
//! module takes already-fetched graph content as plain data and produces plain data shaped to
//! convert 1:1 into `apps/api/src/archspace_client.rs`'s real proto messages, which is where the
//! actual gRPC call to the `cem-archspace` sidecar happens. Keeping the conversion logic here
//! rather than in `apps/api` makes it independently unit-testable with no Docker/DB/network.
//!
//! **Every encode rule below is deliberately conservative and itemized, not best-effort-silent.**
//! Real seeded Turbofan-Ref content (`apps/api/src/main.rs::seed_fr_arch_system_model`) doesn't
//! uniformly fit adsg-core's expected shapes — e.g. `adsg_core`/the sidecar's own
//! `IncompatibilityConstraint.node_names` needs two **option names** (a `SelectionChoice`'s
//! individual pick, like `"mixed"`), not a whole element id or an unrelated `Port` — so an edge
//! like the real seeded `MixedNozzle -> FanBypassDuctExitPort` genuinely cannot be encoded under
//! adsg-core's own semantics. `encode_design_space` reports exactly what it skipped and why
//! (`EncodeResult::skipped`) rather than silently dropping it or force-fitting a wrong shape.

use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq)]
pub struct DesignVariableInput {
    pub name: String,
    pub lower_bound: f64,
    pub upper_bound: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SelectionChoiceInput {
    pub choice_id: String,
    pub option_names: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConnectionChoiceInput {
    pub choice_id: String,
    pub source_connector_names: Vec<String>,
    pub target_connector_names: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IncompatibilityConstraintInput {
    pub node_names: Vec<String>,
}

/// Mirrors `cem_archspace.proto`'s `ChoiceConstraintKind` exactly (see
/// `apps/api/src/archspace_client.rs`'s own doc comment for where that enum itself was confirmed
/// against the real `adsg_core.graph.choice_constraints.ChoiceConstraintType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChoiceConstraintKindInput {
    Linked,
    Permutation,
    Unordered,
    UnorderedNorepl,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChoiceConstraintInput {
    pub kind: ChoiceConstraintKindInput,
    pub node_names: Vec<String>,
}

/// FR-ARCH-08 needs every real design space to have *some* objective for
/// `EvaluateViability`/`RunOptimization` to evaluate against (both RPCs return
/// `FAILED_PRECONDITION` for a space with none) -- `encode_design_space` always sets one whenever
/// it encodes anything at all, mirroring the sidecar's own spike fixture convention exactly
/// (`archspace_client::spike_compressor_design_space`'s `Objective { direction: -1, .. }` —
/// minimize, matching that same precedent, not re-derived).
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectiveInput {
    pub name: String,
    pub direction: i32,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct DesignSpaceDefinitionInput {
    pub root_name: String,
    pub connector_names: Vec<String>,
    pub design_variables: Vec<DesignVariableInput>,
    pub selection_choices: Vec<SelectionChoiceInput>,
    pub connection_choices: Vec<ConnectionChoiceInput>,
    pub incompatibility_constraints: Vec<IncompatibilityConstraintInput>,
    pub choice_constraints: Vec<ChoiceConstraintInput>,
    pub objective: Option<ObjectiveInput>,
}

// --- Inputs: already-fetched graph content, plain data only ---------------------------------

/// A `:Parameter` element `Bound`-linked to the subsystem being encoded. `bound` mirrors the seed
/// content's own `bound: [lo, hi]` property (`apps/api/src/main.rs::ParamSeed`) — `None` when
/// absent or not a well-formed 2-number range.
pub struct ParameterInput {
    pub id: String,
    pub bound: Option<(f64, f64)>,
}

/// A `:SelectionChoice` element `ArchDerives`-linked to the subsystem being encoded. `options`
/// mirrors its raw Postgres body property exactly (a `serde_json::Value`, not pre-validated) —
/// `encode_design_space` itself does the shape-checking, so a caller doesn't need to pre-filter.
pub struct SelectionChoiceElement {
    pub id: String,
    pub options: serde_json::Value,
}

/// A `:ConnectionChoice` element in scope. `properties` mirrors its raw Postgres body.
pub struct ConnectionChoiceElement {
    pub id: String,
    pub properties: serde_json::Value,
}

pub struct IncompatibleWithEdge {
    pub source: String,
    pub target: String,
}

/// `kind` mirrors `Edge::metadata`'s `choiceConstraintType` string
/// (`"Linked"|"Permutation"|"Unordered"|"UnorderedNorepl"`) — `None` when the edge carries no
/// metadata at all (a real, pre-existing possibility this codebase's own schema allows).
pub struct ChoiceConstraintEdge {
    pub source: String,
    pub target: String,
    pub kind: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SkippedItem {
    pub element_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct EncodeResult {
    pub definition: DesignSpaceDefinitionInput,
    pub skipped: Vec<SkippedItem>,
}

fn parse_choice_constraint_kind(kind: &str) -> Option<ChoiceConstraintKindInput> {
    match kind {
        "Linked" => Some(ChoiceConstraintKindInput::Linked),
        "Permutation" => Some(ChoiceConstraintKindInput::Permutation),
        "Unordered" => Some(ChoiceConstraintKindInput::Unordered),
        "UnorderedNorepl" => Some(ChoiceConstraintKindInput::UnorderedNorepl),
        _ => None,
    }
}

/// Builds a `DesignSpaceDefinitionInput` from real, already-fetched graph content, reporting every
/// item it could not encode rather than silently dropping or force-fitting it. See this module's
/// own doc comment for the reasoning behind each rule below.
pub fn encode_design_space(
    root_name: &str,
    connector_names: &[String],
    parameters: &[ParameterInput],
    selection_choices: &[SelectionChoiceElement],
    connection_choices: &[ConnectionChoiceElement],
    incompatibilities: &[IncompatibleWithEdge],
    choice_constraints: &[ChoiceConstraintEdge],
) -> EncodeResult {
    let mut skipped = Vec::new();
    let mut design_variables = Vec::new();

    for p in parameters {
        match p.bound {
            Some((lo, hi)) if lo < hi => design_variables.push(DesignVariableInput {
                name: p.id.clone(),
                lower_bound: lo,
                upper_bound: hi,
            }),
            Some((lo, hi)) => skipped.push(SkippedItem {
                element_id: p.id.clone(),
                reason: format!("bound [{lo}, {hi}] is not a valid lower<upper range"),
            }),
            None => skipped.push(SkippedItem {
                element_id: p.id.clone(),
                reason: "no bound property".to_string(),
            }),
        }
    }

    let mut selection_choice_inputs = Vec::new();
    for sc in selection_choices {
        let options: Option<Vec<String>> = sc.options.as_array().and_then(|arr| {
            let names: Vec<String> = arr
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .filter(|s| !s.is_empty())
                .collect();
            let distinct: BTreeSet<&str> = names.iter().map(String::as_str).collect();
            if names.len() >= 2 && distinct.len() == names.len() {
                Some(names)
            } else {
                None
            }
        });
        match options {
            Some(option_names) => selection_choice_inputs.push(SelectionChoiceInput {
                choice_id: sc.id.clone(),
                option_names,
            }),
            None => skipped.push(SkippedItem {
                element_id: sc.id.clone(),
                reason: "options is not an array of \u{2265}2 distinct non-empty strings"
                    .to_string(),
            }),
        }
    }

    let connector_set: BTreeSet<&str> = connector_names.iter().map(String::as_str).collect();
    let mut connection_choice_inputs = Vec::new();
    for cc in connection_choices {
        let names_at = |key: &str| -> Option<Vec<String>> {
            let arr = cc.properties.get(key)?.as_array()?;
            let names: Vec<String> = arr
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect();
            if names.is_empty() || names.iter().any(|n| !connector_set.contains(n.as_str())) {
                None
            } else {
                Some(names)
            }
        };
        match (
            names_at("sourceConnectorNames"),
            names_at("targetConnectorNames"),
        ) {
            (Some(source_connector_names), Some(target_connector_names)) => {
                connection_choice_inputs.push(ConnectionChoiceInput {
                    choice_id: cc.id.clone(),
                    source_connector_names,
                    target_connector_names,
                })
            }
            _ => skipped.push(SkippedItem {
                element_id: cc.id.clone(),
                reason: "missing or unresolvable sourceConnectorNames/targetConnectorNames \
                         (each name must appear in the design space's declared connector_names)"
                    .to_string(),
            }),
        }
    }

    // Every option name and design-variable name actually present in the definition being built
    // -- an incompatibility/choice-constraint edge is only encodable if both endpoints resolve to
    // one of these (adsg-core's own option-name-level semantics, confirmed against
    // `apps/api/src/archspace_client.rs`'s spike fixture -- see this module's doc comment).
    let mut known_names: BTreeSet<&str> = BTreeSet::new();
    for dv in &design_variables {
        known_names.insert(dv.name.as_str());
    }
    for sc in &selection_choice_inputs {
        for option in &sc.option_names {
            known_names.insert(option.as_str());
        }
    }

    let mut incompatibility_constraints = Vec::new();
    for edge in incompatibilities {
        if known_names.contains(edge.source.as_str()) && known_names.contains(edge.target.as_str())
        {
            incompatibility_constraints.push(IncompatibilityConstraintInput {
                node_names: vec![edge.source.clone(), edge.target.clone()],
            });
        } else {
            let unknown = if !known_names.contains(edge.source.as_str()) {
                &edge.source
            } else {
                &edge.target
            };
            skipped.push(SkippedItem {
                element_id: format!("{}->{}", edge.source, edge.target),
                reason: format!("{unknown} is not a known option or design-variable name"),
            });
        }
    }

    let mut choice_constraint_inputs = Vec::new();
    for edge in choice_constraints {
        let kind = edge.kind.as_deref().and_then(parse_choice_constraint_kind);
        match kind {
            Some(kind)
                if known_names.contains(edge.source.as_str())
                    && known_names.contains(edge.target.as_str()) =>
            {
                choice_constraint_inputs.push(ChoiceConstraintInput {
                    kind,
                    node_names: vec![edge.source.clone(), edge.target.clone()],
                });
            }
            Some(_) => {
                let unknown = if !known_names.contains(edge.source.as_str()) {
                    &edge.source
                } else {
                    &edge.target
                };
                skipped.push(SkippedItem {
                    element_id: format!("{}->{}", edge.source, edge.target),
                    reason: format!("{unknown} is not a known option or design-variable name"),
                });
            }
            None => skipped.push(SkippedItem {
                element_id: format!("{}->{}", edge.source, edge.target),
                reason: format!(
                    "unrecognized or missing choiceConstraintType metadata: {:?}",
                    edge.kind
                ),
            }),
        }
    }

    let has_any_content = !design_variables.is_empty() || !selection_choice_inputs.is_empty();
    let objective = has_any_content.then(|| ObjectiveInput {
        name: format!("{root_name}Objective"),
        direction: -1,
    });

    EncodeResult {
        definition: DesignSpaceDefinitionInput {
            root_name: root_name.to_string(),
            connector_names: connector_names.to_vec(),
            design_variables,
            selection_choices: selection_choice_inputs,
            connection_choices: connection_choice_inputs,
            incompatibility_constraints,
            choice_constraints: choice_constraint_inputs,
            objective,
        },
        skipped,
    }
}

/// One design/selection-choice's contribution to a decoded instance — which of its own
/// options/value is actually present, per the sidecar's `present_node_names`.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedChoice {
    pub choice_id: String,
    pub present_option: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct InstanceSummary {
    pub design_vector: Vec<f64>,
    pub choices: Vec<DecodedChoice>,
    pub other_present_nodes: Vec<String>,
}

/// FR-ARCH-05's "decode a design vector back into a graph instance" half — a light, pure
/// transform of the sidecar's raw `ArchitectureInstance` (design vector + which named nodes are
/// present) into a per-choice summary, using the same `DesignSpaceDefinitionInput` the request was
/// encoded from. Deliberately does **not** write anything to the graph — materializing a decoded
/// instance as new, persisted candidate elements is FR-ARCH-07 (architecture instance generation
/// entering the `/cem/proposals/*` review-gate flow), explicitly the next pass, not this one.
pub fn summarize_instance(
    definition: &DesignSpaceDefinitionInput,
    design_vector: &[f64],
    present_node_names: &[String],
) -> InstanceSummary {
    let present: BTreeSet<&str> = present_node_names.iter().map(String::as_str).collect();
    let mut choices = Vec::new();
    let mut accounted_for: BTreeSet<String> = BTreeSet::new();

    for sc in &definition.selection_choices {
        let present_option = sc
            .option_names
            .iter()
            .find(|option| present.contains(option.as_str()))
            .cloned();
        if let Some(option) = &present_option {
            accounted_for.insert(option.clone());
        }
        choices.push(DecodedChoice {
            choice_id: sc.choice_id.clone(),
            present_option,
        });
    }

    let other_present_nodes: Vec<String> = present_node_names
        .iter()
        .filter(|name| !accounted_for.contains(name.as_str()))
        .cloned()
        .collect();

    InstanceSummary {
        design_vector: design_vector.to_vec(),
        choices,
        other_present_nodes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        connector_names: Vec<String>,
        parameters: Vec<ParameterInput>,
        selection_choices: Vec<SelectionChoiceElement>,
        connection_choices: Vec<ConnectionChoiceElement>,
        incompatibilities: Vec<IncompatibleWithEdge>,
        choice_constraints: Vec<ChoiceConstraintEdge>,
    }

    fn spike_fixture() -> Fixture {
        let connector_names = vec!["BleedOfftakeConnector".to_string(), "EcsPort".to_string()];
        let parameters = vec![
            ParameterInput {
                id: "n_HP_stages".to_string(),
                bound: Some((1.0, 4.0)),
            },
            ParameterInput {
                id: "n_HP_turbine_stages".to_string(),
                bound: Some((1.0, 4.0)),
            },
        ];
        let selection_choices = vec![
            SelectionChoiceElement {
                id: "BleedOfftakeStage".to_string(),
                options: serde_json::json!(["Stage1", "Stage2", "Stage3", "Stage4"]),
            },
            SelectionChoiceElement {
                id: "NozzleConfig".to_string(),
                options: serde_json::json!(["MixedNozzle", "SeparateNozzle"]),
            },
            SelectionChoiceElement {
                id: "Gearbox".to_string(),
                options: serde_json::json!(["Geared", "DirectDrive"]),
            },
        ];
        let connection_choices = vec![ConnectionChoiceElement {
            id: "BleedRouting".to_string(),
            properties: serde_json::json!({
                "sourceConnectorNames": ["BleedOfftakeConnector"],
                "targetConnectorNames": ["EcsPort"],
            }),
        }];
        let incompatibilities = vec![IncompatibleWithEdge {
            source: "MixedNozzle".to_string(),
            target: "Geared".to_string(),
        }];
        let choice_constraints = vec![ChoiceConstraintEdge {
            source: "n_HP_stages".to_string(),
            target: "n_HP_turbine_stages".to_string(),
            kind: Some("Linked".to_string()),
        }];
        Fixture {
            connector_names,
            parameters,
            selection_choices,
            connection_choices,
            incompatibilities,
            choice_constraints,
        }
    }

    #[test]
    fn encode_design_space_cleanly_encodes_every_primitive_when_all_are_well_formed() {
        let f = spike_fixture();
        let result = encode_design_space(
            "CoreHpCompressor",
            &f.connector_names,
            &f.parameters,
            &f.selection_choices,
            &f.connection_choices,
            &f.incompatibilities,
            &f.choice_constraints,
        );
        assert!(result.skipped.is_empty(), "{:?}", result.skipped);
        assert_eq!(result.definition.design_variables.len(), 2);
        assert_eq!(result.definition.selection_choices.len(), 3);
        assert_eq!(result.definition.connection_choices.len(), 1);
        assert_eq!(result.definition.incompatibility_constraints.len(), 1);
        assert_eq!(result.definition.choice_constraints.len(), 1);
        assert_eq!(
            result.definition.choice_constraints[0].kind,
            ChoiceConstraintKindInput::Linked
        );
        // FR-ARCH-08 needs every real design space to carry an objective (EvaluateViability/
        // RunOptimization both fail without one) -- confirmed always set whenever anything real
        // was encoded.
        let objective = result
            .definition
            .objective
            .as_ref()
            .expect("a design space with real content should always get a real objective");
        assert_eq!(objective.name, "CoreHpCompressorObjective");
        assert_eq!(objective.direction, -1);
    }

    #[test]
    fn encode_design_space_skips_a_parameter_with_no_bound() {
        let result = encode_design_space(
            "X",
            &[],
            &[ParameterInput {
                id: "UnboundedParam".to_string(),
                bound: None,
            }],
            &[],
            &[],
            &[],
            &[],
        );
        assert!(result.definition.design_variables.is_empty());
        assert_eq!(result.skipped.len(), 1);
        assert_eq!(result.skipped[0].element_id, "UnboundedParam");
        assert!(result.skipped[0].reason.contains("no bound"));
        // Nothing encodable at all -- no design variables, no selection choices -- means no real
        // objective either, not a fabricated one over an empty design space.
        assert!(result.definition.objective.is_none());
    }

    #[test]
    fn encode_design_space_skips_a_selection_choice_with_a_single_illustrative_string_option() {
        let result = encode_design_space(
            "X",
            &[],
            &[],
            &[SelectionChoiceElement {
                id: "BleedOfftakeStage".to_string(),
                options: serde_json::json!(
                    "stage 1..n_HP_stages (illustrative set: stage 1, stage 2, stage 3)"
                ),
            }],
            &[],
            &[],
            &[],
        );
        assert!(result.definition.selection_choices.is_empty());
        assert_eq!(result.skipped.len(), 1);
        assert!(result.skipped[0].reason.contains("array"));
    }

    #[test]
    fn encode_design_space_skips_a_connection_choice_missing_connector_name_properties() {
        let result = encode_design_space(
            "X",
            &["RealConnector".to_string()],
            &[],
            &[],
            &[ConnectionChoiceElement {
                id: "BleedAirRouting".to_string(),
                properties: serde_json::json!({
                    "sourcePortId": "CoreBleedOfftakePort",
                    "targetBoundary": "external ECS/airframe port",
                }),
            }],
            &[],
            &[],
        );
        assert!(result.definition.connection_choices.is_empty());
        assert_eq!(result.skipped.len(), 1);
        assert_eq!(result.skipped[0].element_id, "BleedAirRouting");
    }

    #[test]
    fn encode_design_space_skips_an_incompatibility_edge_referencing_an_unknown_name() {
        let result = encode_design_space(
            "X",
            &[],
            &[],
            &[SelectionChoiceElement {
                id: "MixedNozzle".to_string(),
                options: serde_json::json!(["mixed", "separate"]),
            }],
            &[],
            &[IncompatibleWithEdge {
                source: "mixed".to_string(),
                target: "FanBypassDuctExitPort".to_string(),
            }],
            &[],
        );
        assert!(result.definition.incompatibility_constraints.is_empty());
        assert_eq!(result.skipped.len(), 1);
        assert!(result.skipped[0].reason.contains("FanBypassDuctExitPort"));
    }

    #[test]
    fn encode_design_space_skips_a_choice_constraint_with_unrecognized_metadata() {
        let result = encode_design_space(
            "X",
            &[],
            &[
                ParameterInput {
                    id: "A".to_string(),
                    bound: Some((0.0, 1.0)),
                },
                ParameterInput {
                    id: "B".to_string(),
                    bound: Some((0.0, 1.0)),
                },
            ],
            &[],
            &[],
            &[],
            &[ChoiceConstraintEdge {
                source: "A".to_string(),
                target: "B".to_string(),
                kind: None,
            }],
        );
        assert!(result.definition.choice_constraints.is_empty());
        assert_eq!(result.skipped.len(), 1);
        assert!(result.skipped[0].reason.contains("metadata"));
    }

    #[test]
    fn summarize_instance_reports_which_option_is_present_per_choice() {
        let f = spike_fixture();
        let result = encode_design_space(
            "CoreHpCompressor",
            &f.connector_names,
            &f.parameters,
            &f.selection_choices,
            &f.connection_choices,
            &f.incompatibilities,
            &f.choice_constraints,
        );
        let present_node_names = vec![
            "Stage2".to_string(),
            "SeparateNozzle".to_string(),
            "DirectDrive".to_string(),
            "SomeOtherStructuralNode".to_string(),
        ];
        let summary = summarize_instance(&result.definition, &[2.0, 3.0], &present_node_names);
        assert_eq!(summary.design_vector, vec![2.0, 3.0]);
        assert_eq!(summary.choices.len(), 3);
        let bleed_choice = summary
            .choices
            .iter()
            .find(|c| c.choice_id == "BleedOfftakeStage")
            .unwrap();
        assert_eq!(bleed_choice.present_option, Some("Stage2".to_string()));
        let nozzle_choice = summary
            .choices
            .iter()
            .find(|c| c.choice_id == "NozzleConfig")
            .unwrap();
        assert_eq!(
            nozzle_choice.present_option,
            Some("SeparateNozzle".to_string())
        );
        assert_eq!(
            summary.other_present_nodes,
            vec!["SomeOtherStructuralNode".to_string()]
        );
    }
}
