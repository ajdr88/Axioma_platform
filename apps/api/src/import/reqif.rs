//! `POST /import/reqif` — imports a flat list of requirements from a ReqIF XML document.
//!
//! **Supported subset** (deliberately restricted, grown only if a real import needs more — same
//! discipline `alf-lite` uses for its Alf subset, impl §9.6): `<SPEC-OBJECT IDENTIFIER="...">`
//! elements, each with zero or more `<ATTRIBUTE-VALUE-STRING THE-VALUE="...">` children. The
//! attribute's name is taken directly from its `ATTRIBUTE-DEFINITION-STRING-REF` text — it is
//! *not* resolved against the document's `SPEC-TYPES`/`DATATYPES` sections, which is the part of
//! real ReqIF this subset deliberately doesn't implement yet. `SPEC-HIERARCHY` (nested/ordered
//! requirement structure) is also out of scope — this is a flat import.
//!
//! A `SPEC-OBJECT` missing `IDENTIFIER` is a hard parse error naming the problem — never a silent
//! partial import (the same "precise error, never silent partial compile" rule `alf-lite` follows
//! for unsupported constructs).

use axum::{extract::State, Json};
use serde::Serialize;
use sysml_core::{Element, ElementBody, NodeKind, Origin};

use super::BadRequest;
use crate::{ApiError, AppState};

/// One imported requirement, prior to the name/rationale split described below.
struct ParsedRequirement {
    identifier: String,
    /// `(attribute name, value)` pairs, in document order. The first becomes the element's
    /// `name` (truncated) and the body's `rationale` (untruncated); the rest land in the body's
    /// `properties`.
    attributes: Vec<(String, String)>,
}

const NAME_TRUNCATE_LEN: usize = 80;

fn parse_reqif(xml: &str) -> Result<Vec<ParsedRequirement>, BadRequest> {
    let doc = roxmltree::Document::parse(xml)
        .map_err(|err| BadRequest(format!("malformed ReqIF XML: {err}")))?;

    let mut requirements = Vec::new();

    for spec_object in doc.descendants().filter(|n| n.has_tag_name("SPEC-OBJECT")) {
        let identifier = spec_object.attribute("IDENTIFIER").ok_or_else(|| {
            BadRequest("SPEC-OBJECT is missing its required IDENTIFIER attribute".to_string())
        })?;

        let mut attributes = Vec::new();
        for value_node in spec_object
            .descendants()
            .filter(|n| n.has_tag_name("ATTRIBUTE-VALUE-STRING"))
        {
            let value = value_node.attribute("THE-VALUE").ok_or_else(|| {
                BadRequest(format!(
                    "ATTRIBUTE-VALUE-STRING on SPEC-OBJECT {identifier} is missing THE-VALUE"
                ))
            })?;
            let attr_name = value_node
                .descendants()
                .find(|n| n.has_tag_name("ATTRIBUTE-DEFINITION-STRING-REF"))
                .and_then(|n| n.text())
                .unwrap_or("value")
                .to_string();
            attributes.push((attr_name, value.to_string()));
        }

        requirements.push(ParsedRequirement {
            identifier: identifier.to_string(),
            attributes,
        });
    }

    Ok(requirements)
}

#[derive(Debug, Serialize)]
pub struct ImportSummary {
    pub requirements_imported: usize,
}

pub async fn import_reqif(
    State(state): State<AppState>,
    body: String,
) -> Result<Json<ImportSummary>, ApiError> {
    let requirements = parse_reqif(&body)?;

    // Every requirement's body is written to Postgres first — see this module's doc comment and
    // the plan's ordering rationale: each upsert is individually atomic (INSERT ... ON CONFLICT),
    // and a harmless orphaned row (if the Neo4j step below fails) is safe to leave for a retry to
    // overwrite, unlike a dangling graph edge.
    for req in &requirements {
        let rationale = req.attributes.first().map(|(_, value)| value.clone());
        let properties = serde_json::to_value(
            req.attributes
                .iter()
                .skip(1)
                .cloned()
                .collect::<std::collections::HashMap<_, _>>(),
        )
        .expect("a HashMap<String, String> always serializes");

        state
            .postgres
            .upsert_body(&ElementBody {
                element_id: req.identifier.clone(),
                rationale,
                properties,
            })
            .await?;
    }

    let elements: Vec<Element> = requirements
        .iter()
        .map(|req| {
            let full_text = req.attributes.first().map(|(_, value)| value.as_str());
            let name = match full_text {
                Some(text) if text.chars().count() > NAME_TRUNCATE_LEN => {
                    let truncated: String = text.chars().take(NAME_TRUNCATE_LEN).collect();
                    format!("{truncated}…")
                }
                Some(text) => text.to_string(),
                None => req.identifier.clone(),
            };
            Element {
                id: req.identifier.clone(),
                kind: NodeKind::Requirement,
                name,
                active: true,
                origin: Origin::Human,
            }
        })
        .collect();

    // No containment edges — this importer is flat (SPEC-HIERARCHY out of scope). Kind-conflict
    // validation still runs inside `import_elements_and_edges`, so re-importing a ReqIF
    // identifier already used by a non-Requirement element is rejected.
    state
        .neo4j
        .import_elements_and_edges(&elements, &[])
        .await?;

    Ok(Json(ImportSummary {
        requirements_imported: elements.len(),
    }))
}
