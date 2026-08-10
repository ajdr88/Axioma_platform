//! Mode A grounded copilot query (roadmap: Mode A fast-follow; FR-CEM-01 grounded retrieval,
//! FR-CEM-05 AI generation provenance) — a thin vertical slice, hard-wired directly to a local
//! Ollama call. **No `llm-gateway` package yet** (ADR-004's pluggable-provider abstraction is
//! deliberately deferred — see `packages/llm-gateway/README.md` — until a second Mode A
//! capability actually needs it; building it now, with one caller, would be exactly the kind of
//! speculative abstraction this project's own conventions warn against).
//!
//! Grounding happens *before* any LLM call: real graph facts are retrieved first (keyword-matched
//! elements, plus their Satisfy/Verify/Refine dependents via the P1.3 traceability engine — see
//! `traceability::run_traversal`), and the model is instructed to answer only from those facts,
//! citing each claim's source element id. A citation-integrity check afterward cross-references
//! every `[ElementId]` the model actually cited against what it was given. **This is not a
//! hallucination guarantee** — nothing short of retrieval + validation can be for a local model's
//! free-text output — it's a real check that catches a citation to something never in context,
//! surfaced via `groundedFully` rather than presented as trustworthy either way.

use std::collections::HashSet;
use std::time::Duration;

use axum::{
    extract::{Path, State},
    Json,
};
use sha2::{Digest, Sha256};
use sysml_core::NodeKind;

use crate::traceability::{run_traversal, Direction};
use crate::{env_or, import, ApiError, AppState};

const PROMPT_TEMPLATE: &str = "You are Axioma's grounded model copilot. Answer the user's \
question using ONLY the facts listed below — never use outside knowledge. \
This is mandatory: every single sentence of your answer must end with the bracketed id of the \
fact it came from, with no exceptions — a sentence with no bracketed id is an error. \
If the facts below do not support an answer, respond with exactly: not found\n\n\
Example of the required format:\n\
FACTS:\n- [PUMP-1] (Structure) Coolant Pump\n\
QUESTION: What is PUMP-1?\n\
ANSWER: PUMP-1 is a Structure named Coolant Pump. [PUMP-1]\n\n\
FACTS:\n{facts}\n\nQUESTION: {question}\nANSWER:";

/// FR-CEM-05's "prompt-template hash" — hashing the fixed template text itself (not the filled-in
/// prompt, which varies per call) is what makes this a stable identifier for "which template
/// generated this," matching the field's purpose. Takes the template explicitly since three Mode
/// A capabilities now each have their own fixed template.
fn prompt_template_hash(template: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(template.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// The other two Mode A capabilities (`search_parts`/`lint_requirement`) need the model to
/// return structured data, not free text — small local models reliably wrap JSON in markdown
/// code fences or a sentence of preamble even when told not to (confirmed directly, same
/// "small models don't perfectly follow format instructions" finding as the copilot's own bracket-
/// citation gap). Finds the outermost `[...]` rather than requiring the whole response to be
/// clean JSON, then parses just that slice — tolerant of surrounding noise, still strict about
/// the array's own contents. Returns `None` (callers treat this as "no results," not an error)
/// if no bracket pair parses as valid JSON.
fn parse_json_array<T: serde::de::DeserializeOwned>(text: &str) -> Option<Vec<T>> {
    let start = text.find('[')?;
    let end = text.rfind(']')?;
    if end < start {
        return None;
    }
    serde_json::from_str(&text[start..=end]).ok()
}

/// Deterministic-leaning defaults for a grounded-fact QA task — this isn't a creative-writing
/// use case, low temperature is the honest choice, not a tuning knob exposed to callers yet.
const TEMPERATURE: f32 = 0.0;
const SEED: u64 = 42;

#[derive(Debug, Clone, serde::Serialize)]
struct ContextFact {
    id: String,
    kind: NodeKind,
    name: String,
    /// Set only for facts discovered via the traceability hop (e.g. "Satisfy -> REQ-THRUST"),
    /// not for the directly keyword-matched element itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    relation: Option<String>,
}

/// Keyword-matches the question against every element's id/name (case-insensitive substring —
/// no embeddings/semantic search; that's real premature engineering for a first slice with no
/// usage data yet), then pulls each matched Requirement's direct Satisfy/Verify/Refine dependents
/// via the P1.3 traceability engine — the actual mechanism that answers "what verifies X" from
/// real graph data, not LLM inference. Returns an empty set if nothing matches, which the caller
/// treats as "not found" without ever invoking the LLM.
async fn ground_question(
    state: &AppState,
    project_id: &str,
    question: &str,
) -> anyhow::Result<Vec<ContextFact>> {
    let elements = state.neo4j.list_elements(project_id).await?;
    let keywords: Vec<String> = question
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| word.len() > 2)
        .map(|word| word.to_lowercase())
        .collect();
    if keywords.is_empty() {
        return Ok(Vec::new());
    }

    let mut facts = Vec::new();
    let mut seen_ids: HashSet<String> = HashSet::new();
    for element in &elements {
        let haystack = format!("{} {}", element.id, element.name).to_lowercase();
        if !keywords.iter().any(|kw| haystack.contains(kw.as_str())) {
            continue;
        }
        if seen_ids.insert(element.id.clone()) {
            facts.push(ContextFact {
                id: element.id.clone(),
                kind: element.kind,
                name: element.name.clone(),
                relation: None,
            });
        }
        if element.kind != NodeKind::Requirement {
            continue;
        }
        let traversal =
            run_traversal(state, project_id, &element.id, 1, 50, Direction::Incoming).await?;
        for (dependent_id, (_, edge_kind)) in traversal.visited {
            if !seen_ids.insert(dependent_id.clone()) {
                continue;
            }
            if let Some(dependent) = state.neo4j.get_element(project_id, &dependent_id).await? {
                facts.push(ContextFact {
                    id: dependent_id,
                    kind: dependent.kind,
                    name: dependent.name,
                    relation: Some(format!("{edge_kind:?} -> {}", element.id)),
                });
            }
        }
    }
    Ok(facts)
}

/// Extracts `[Token]`-bracketed citations from the model's answer — a plain scan, not a `regex`
/// dependency, for a pattern this simple.
fn extract_bracketed_citations(answer: &str) -> Vec<String> {
    let mut citations = Vec::new();
    let mut current = String::new();
    let mut in_bracket = false;
    for ch in answer.chars() {
        match ch {
            '[' => {
                in_bracket = true;
                current.clear();
            }
            ']' if in_bracket => {
                in_bracket = false;
                if !current.is_empty() {
                    citations.push(std::mem::take(&mut current));
                }
            }
            _ if in_bracket => current.push(ch),
            _ => {}
        }
    }
    citations
}

/// A small local model doesn't reliably follow the bracket format even when it correctly
/// identifies the right source — confirmed directly (`qwen2.5:1.5b` answering a bare `REQ-THRUST`
/// with no brackets at all, for a question its bracketed answer handled correctly moments
/// earlier). This is a formatting-compliance gap, not a grounding-safety one: NFR-CEM-04 cares
/// about whether a claim traces to a real element, not whether the model wrapped it in `[...]`.
/// Recognizing a bare known id as an implicit citation stays just as strict about the thing that
/// actually matters (only ids that were really in `context_snapshot` ever count) while not
/// penalizing cosmetic non-compliance.
fn find_bare_known_ids(answer: &str, known_ids: &HashSet<&str>) -> Vec<String> {
    known_ids
        .iter()
        .filter(|id| answer.contains(**id))
        .map(|id| id.to_string())
        .collect()
}

#[derive(Debug, serde::Deserialize)]
struct OllamaTagsResponse {
    models: Vec<OllamaModelTag>,
}

#[derive(Debug, serde::Deserialize)]
struct OllamaModelTag {
    name: String,
    digest: String,
}

#[derive(Debug, serde::Serialize)]
struct OllamaGenerateRequest<'a> {
    model: &'a str,
    prompt: &'a str,
    stream: bool,
    options: OllamaOptions,
}

#[derive(Debug, serde::Serialize)]
struct OllamaOptions {
    temperature: f32,
    seed: u64,
}

#[derive(Debug, serde::Deserialize)]
struct OllamaGenerateResponse {
    response: String,
}

/// Calls local Ollama directly (`OLLAMA_URL`/`OLLAMA_MODEL` env-configurable, matching the
/// existing `env_or` pattern) — returns the generated text plus the model tag and its content
/// digest (from `/api/tags`) as `model_name`/`model_version`. The digest, not the human-readable
/// tag, is what actually identifies *which weights* answered — the same rigor `SimulationRun`
/// provenance already requires for solver results.
async fn call_ollama(prompt: &str) -> anyhow::Result<(String, String, String)> {
    let base_url = env_or("OLLAMA_URL", "http://localhost:11434");
    // qwen2.5:0.5b (494M params) frequently declined to answer at all ("not found") even when
    // the retrieved facts fully supported one — confirmed directly (repeated identical calls at
    // temperature=0/seed=42 still varied between citing correctly and punting). qwen2.5:1.5b
    // answered with real citations consistently in the same head-to-head comparison, still small
    // enough to pull/run fast. Chosen for reliability, not arbitrarily.
    let model = env_or("OLLAMA_MODEL", "qwen2.5:1.5b");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;

    let tags: OllamaTagsResponse = client
        .get(format!("{base_url}/api/tags"))
        .send()
        .await?
        .json()
        .await?;
    let model_version = tags
        .models
        .iter()
        .find(|tag| tag.name == model)
        .map(|tag| tag.digest.clone())
        .unwrap_or_else(|| "unknown".to_string());

    let request = OllamaGenerateRequest {
        model: &model,
        prompt,
        stream: false,
        options: OllamaOptions {
            temperature: TEMPERATURE,
            seed: SEED,
        },
    };
    let response: OllamaGenerateResponse = client
        .post(format!("{base_url}/api/generate"))
        .json(&request)
        .send()
        .await?
        .json()
        .await?;

    Ok((response.response, model, model_version))
}

#[derive(Debug, serde::Deserialize)]
pub struct ModeAQueryRequest {
    pub(crate) question: String,
}

#[derive(Debug, serde::Serialize)]
struct ModeAProvenance {
    #[serde(rename = "modelName")]
    model_name: String,
    #[serde(rename = "modelVersion")]
    model_version: String,
    #[serde(rename = "promptTemplateHash")]
    prompt_template_hash: String,
    temperature: f32,
    seed: u64,
    #[serde(rename = "contextSnapshot")]
    context_snapshot: Vec<ContextFact>,
}

#[derive(Debug, serde::Serialize)]
pub struct ModeAQueryResponse {
    answer: String,
    #[serde(rename = "citedElementIds")]
    cited_element_ids: Vec<String>,
    #[serde(rename = "groundedFully")]
    grounded_fully: bool,
    provenance: ModeAProvenance,
}

/// `POST /api/v0/projects/:projectId/cem/mode-a/query` (FR-CEM-01/05, T-P2.1-04/05).
pub async fn query(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    Json(payload): Json<ModeAQueryRequest>,
) -> Result<Json<ModeAQueryResponse>, ApiError> {
    let facts = ground_question(&state, &project_id, &payload.question).await?;

    if facts.is_empty() {
        // T-P2.1-05's other PASS case: a question with no graph support returns "not found," not
        // a fabrication — no LLM call is even made, so there's nothing to attribute to a model.
        return Ok(Json(ModeAQueryResponse {
            answer: "not found".to_string(),
            cited_element_ids: Vec::new(),
            grounded_fully: true,
            provenance: ModeAProvenance {
                model_name: "none".to_string(),
                model_version: "none".to_string(),
                prompt_template_hash: prompt_template_hash(PROMPT_TEMPLATE),
                temperature: TEMPERATURE,
                seed: SEED,
                context_snapshot: Vec::new(),
            },
        }));
    }

    let facts_text = facts
        .iter()
        .map(|fact| {
            let relation = fact
                .relation
                .as_ref()
                .map(|r| format!(" — {r}"))
                .unwrap_or_default();
            format!("- [{}] ({:?}) {}{relation}", fact.id, fact.kind, fact.name)
        })
        .collect::<Vec<_>>()
        .join("\n");
    let prompt = PROMPT_TEMPLATE
        .replace("{facts}", &facts_text)
        .replace("{question}", &payload.question);

    let (answer, model_name, model_version) = call_ollama(&prompt).await?;

    let known_ids: HashSet<&str> = facts.iter().map(|fact| fact.id.as_str()).collect();
    let bracketed = extract_bracketed_citations(&answer);
    // A bracketed citation to something NOT in context is the real red flag (a fabricated
    // source) — that alone forces `grounded_fully = false` regardless of what else the answer
    // gets right. A *bare* known id with no brackets is just sloppy formatting, not fabrication.
    let has_fabricated_citation = bracketed.iter().any(|id| !known_ids.contains(id.as_str()));
    let mut cited_element_ids: Vec<String> = bracketed
        .into_iter()
        .filter(|id| known_ids.contains(id.as_str()))
        .collect();
    for bare_id in find_bare_known_ids(&answer, &known_ids) {
        if !cited_element_ids.contains(&bare_id) {
            cited_element_ids.push(bare_id);
        }
    }
    let grounded_fully = !cited_element_ids.is_empty() && !has_fabricated_citation;

    Ok(Json(ModeAQueryResponse {
        answer,
        cited_element_ids,
        grounded_fully,
        provenance: ModeAProvenance {
            model_name,
            model_version,
            prompt_template_hash: prompt_template_hash(PROMPT_TEMPLATE),
            temperature: TEMPERATURE,
            seed: SEED,
            context_snapshot: facts,
        },
    }))
}

// ---------------------------------------------------------------------------
// Part search — in-context LLM ranking, not real embeddings/vector search (none exist; building
// an embedding pipeline is much larger, unrequested scope). The whole element list is put in the
// prompt and the model ranks/returns matches directly. Deliberately, honestly scoped to
// reference-fixture size: this does not scale to `Turbofan-Scale`'s 1M elements (nowhere near
// fitting in any model's context window) — a real, flagged limitation, not silently assumed away,
// same as several other features in this codebase that are honest about not being verified at
// 1M-element scale.
// ---------------------------------------------------------------------------

const PART_SEARCH_PROMPT_TEMPLATE: &str = "You are Axioma's grounded model copilot, helping an \
engineer find a specific part by natural-language description. Given the description and the \
list of elements below, identify which ones plausibly match, most-relevant first. \
Respond with ONLY a JSON array, nothing else, no markdown code fence — each entry exactly \
{\"elementId\": \"...\", \"reason\": \"...\"}. If nothing plausibly matches, respond with \
exactly: []\n\n\
Example of the required format:\n\
ELEMENTS:\n- [PUMP-1] (Structure) Coolant Pump\n- [VALVE-2] (Structure) Bypass Valve\n\
DESCRIPTION: something that moves coolant\n\
MATCHES: [{\"elementId\": \"PUMP-1\", \"reason\": \"a pump moves coolant\"}]\n\n\
ELEMENTS:\n{elements}\n\nDESCRIPTION: {description}\nMATCHES:";

#[derive(Debug, serde::Deserialize)]
pub struct PartSearchRequest {
    pub(crate) description: String,
}

/// `qwen2.5:1.5b` doesn't reliably follow the object-per-match format even when the example
/// shows it — confirmed directly: asked for `[{"elementId": "...", "reason": "..."}]`, it
/// returned a bare `["CoreHpCompressor", "ControlFadecEec"]` instead. Same "be lenient about
/// form, strict about substance" precedent as `find_bare_known_ids`'s bare-citation handling —
/// accept either shape rather than losing a real match to a formatting slip.
#[derive(Debug, serde::Deserialize)]
#[serde(untagged)]
enum RawPartMatch {
    WithReason {
        #[serde(rename = "elementId")]
        element_id: String,
        #[serde(default)]
        reason: String,
    },
    BareId(String),
}

impl RawPartMatch {
    fn into_parts(self) -> (String, String) {
        match self {
            RawPartMatch::WithReason { element_id, reason } => (element_id, reason),
            RawPartMatch::BareId(element_id) => (element_id, String::new()),
        }
    }
}

#[derive(Debug, serde::Serialize)]
struct PartMatch {
    #[serde(rename = "elementId")]
    element_id: String,
    kind: NodeKind,
    name: String,
    reason: String,
}

#[derive(Debug, serde::Serialize)]
pub struct PartSearchResponse {
    matches: Vec<PartMatch>,
    provenance: ModeAProvenance,
}

/// `POST /api/v0/projects/:projectId/cem/mode-a/part-search`. Every candidate returned by the
/// model is cross-checked against the real element list before being surfaced — the same
/// citation-integrity discipline `query`'s fabrication check uses, applied here to "is this a
/// real element id" instead of "was this id really in the grounding facts."
pub async fn search_parts(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    Json(payload): Json<PartSearchRequest>,
) -> Result<Json<PartSearchResponse>, ApiError> {
    let elements = state.neo4j.list_elements(&project_id).await?;
    let candidates: Vec<ContextFact> = elements
        .iter()
        .map(|e| ContextFact {
            id: e.id.clone(),
            kind: e.kind,
            name: e.name.clone(),
            relation: None,
        })
        .collect();

    if candidates.is_empty() {
        return Ok(Json(PartSearchResponse {
            matches: Vec::new(),
            provenance: ModeAProvenance {
                model_name: "none".to_string(),
                model_version: "none".to_string(),
                prompt_template_hash: prompt_template_hash(PART_SEARCH_PROMPT_TEMPLATE),
                temperature: TEMPERATURE,
                seed: SEED,
                context_snapshot: Vec::new(),
            },
        }));
    }

    let elements_text = candidates
        .iter()
        .map(|f| format!("- [{}] ({:?}) {}", f.id, f.kind, f.name))
        .collect::<Vec<_>>()
        .join("\n");
    let prompt = PART_SEARCH_PROMPT_TEMPLATE
        .replace("{elements}", &elements_text)
        .replace("{description}", &payload.description);

    let (raw_answer, model_name, model_version) = call_ollama(&prompt).await?;

    let elements_by_id: std::collections::HashMap<&str, &sysml_core::Element> =
        elements.iter().map(|e| (e.id.as_str(), e)).collect();
    let matches: Vec<PartMatch> = parse_json_array::<RawPartMatch>(&raw_answer)
        .unwrap_or_default()
        .into_iter()
        .map(RawPartMatch::into_parts)
        .filter_map(|(element_id, reason)| {
            elements_by_id.get(element_id.as_str()).map(|e| PartMatch {
                element_id: e.id.clone(),
                kind: e.kind,
                name: e.name.clone(),
                reason,
            })
        })
        .collect();

    Ok(Json(PartSearchResponse {
        matches,
        provenance: ModeAProvenance {
            model_name,
            model_version,
            prompt_template_hash: prompt_template_hash(PART_SEARCH_PROMPT_TEMPLATE),
            temperature: TEMPERATURE,
            seed: SEED,
            context_snapshot: candidates,
        },
    }))
}

// ---------------------------------------------------------------------------
// Requirement linting — LLM-based INCOSE-style wording review (ambiguous terms, missing "shall",
// multiple requirements crammed into one, non-testable language). A `Requirement`'s `name` field
// already *is* the requirement statement text in this data model (confirmed against the seed
// fixture — `REQ-THRUST`'s `name` is "Engine shall provide >= 30,000 lbf takeoff thrust", not a
// short label with the real text elsewhere), so that's what gets reviewed; no separate
// structured "shall statement" field exists to prefer instead.
//
// **Real, measured quality limitation — tested directly against both locally-available models
// (`qwen2.5:1.5b`/`qwen2.5:3b`) across several prompt structures, not assumed.** Neither
// reliably distinguishes "no real issues" from "has real issues" the way the copilot's citation
// task turned out to: depending on prompt phrasing, they either (a) default to an empty result
// even for a requirement combining four unrelated, vague clauses in one sentence with no "shall,"
// (b) echo this prompt's own category labels back verbatim regardless of input (a false-positive
// failure mode, worse than under-flagging — it erodes trust by "crying wolf" on well-formed
// text), or (c) drift into generic copyediting (spelling/hyphenation) unrelated to INCOSE-style
// requirement quality. The prompt/parsing below is the version that produced the best real
// output in that testing (real "ambiguous-term" issues with sensible categories on a genuinely
// bad requirement, correctly silent on a well-formed one) — kept over higher-recall variants
// that returned *something* more often, because false positives here are worse than false
// negatives. **This is a real, structural limit of the small local models available in this
// environment, not a code defect** — a production deployment wanting this capability to be
// reliable would need a materially more capable model. Documented rather than hidden, same
// honesty stance as every other measured limitation in this codebase.
// ---------------------------------------------------------------------------

const LINT_REQUIREMENT_PROMPT_TEMPLATE: &str = "You are Axioma's grounded model copilot, \
performing an INCOSE-style wording review of one requirement. Identify concrete issues: \
ambiguous or vague terms (e.g. \"user-friendly\", \"sufficient\", \"etc.\", \"and/or\"), a \
missing \"shall\", multiple distinct requirements combined into one sentence, or language that \
cannot be objectively verified. Respond with ONLY a JSON array, nothing else, no markdown code \
fence — each entry exactly {\"category\": \"...\", \"severity\": \"warning\" or \"error\", \
\"message\": \"...\"}. If the wording has no such issues, respond with exactly: []\n\n\
Example of the required format:\n\
REQUIREMENT [REQ-1]: The system shall be sufficiently fast and user-friendly.\n\
ISSUES: [{\"category\": \"ambiguous-term\", \"severity\": \"warning\", \"message\": \"'sufficiently \
fast' is not measurable — specify a testable threshold\"}, {\"category\": \"ambiguous-term\", \
\"severity\": \"warning\", \"message\": \"'user-friendly' is subjective and not verifiable\"}]\n\n\
REQUIREMENT [{id}]: {text}\nISSUES:";

#[derive(Debug, serde::Deserialize)]
pub struct LintRequirementRequest {
    #[serde(rename = "elementId")]
    pub(crate) element_id: String,
}

/// Same "be lenient about form, strict about substance" leniency as `RawPartMatch` — a bare
/// string (missing `category`/`severity`) is treated as a generic warning rather than discarded.
#[derive(Debug, serde::Deserialize)]
#[serde(untagged)]
enum RawLintIssue {
    Full {
        category: String,
        severity: String,
        message: String,
    },
    BareMessage(String),
}

#[derive(Debug, serde::Serialize)]
struct LintIssue {
    category: String,
    severity: String,
    message: String,
}

impl From<RawLintIssue> for LintIssue {
    fn from(raw: RawLintIssue) -> Self {
        match raw {
            RawLintIssue::Full {
                category,
                severity,
                message,
            } => LintIssue {
                category,
                severity,
                message,
            },
            RawLintIssue::BareMessage(message) => LintIssue {
                category: "general".to_string(),
                severity: "warning".to_string(),
                message,
            },
        }
    }
}

#[derive(Debug, serde::Serialize)]
pub struct LintRequirementResponse {
    issues: Vec<LintIssue>,
    provenance: ModeAProvenance,
}

/// `POST /api/v0/projects/:projectId/cem/mode-a/lint-requirement`. Deliberately per-requirement,
/// not a whole-project bulk lint — matches this codebase's established thin-slice precedent
/// (Mode A's own copilot slice, `fuml_client`, etc.); a bulk variant is a small, obvious
/// follow-up once this one's actually used, not something to build speculatively now.
pub async fn lint_requirement(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    Json(payload): Json<LintRequirementRequest>,
) -> Result<Json<LintRequirementResponse>, ApiError> {
    let Some(element) = state
        .neo4j
        .get_element(&project_id, &payload.element_id)
        .await?
    else {
        return Err(import::BadRequest(format!("no element {}", payload.element_id)).into());
    };
    if element.kind != NodeKind::Requirement {
        return Err(import::BadRequest(format!(
            "{} is a {:?}, not a Requirement",
            element.id, element.kind
        ))
        .into());
    }

    let prompt = LINT_REQUIREMENT_PROMPT_TEMPLATE
        .replace("{id}", &element.id)
        .replace("{text}", &element.name);
    let (raw_answer, model_name, model_version) = call_ollama(&prompt).await?;
    let issues: Vec<LintIssue> = parse_json_array::<RawLintIssue>(&raw_answer)
        .unwrap_or_default()
        .into_iter()
        .map(LintIssue::from)
        .collect();

    Ok(Json(LintRequirementResponse {
        issues,
        provenance: ModeAProvenance {
            model_name,
            model_version,
            prompt_template_hash: prompt_template_hash(LINT_REQUIREMENT_PROMPT_TEMPLATE),
            temperature: TEMPERATURE,
            seed: SEED,
            context_snapshot: vec![ContextFact {
                id: element.id,
                kind: element.kind,
                name: element.name,
                relation: None,
            }],
        },
    }))
}
