//! docs/IMPLEMENTATION_KICKOFF.md Phase 5 (FR-CORE-14..18) — the async "documents → draft model"
//! pipeline (reqs v5 §5.14). One deliberate scope-down, flagged rather than silently guessed:
//!
//! - **OCR is feature-gated (`ocr`, default OFF), not always available.** `pdf_extract` reads
//!   only existing text layers; for a scanned/image-only PDF, `ocr_pages` below rasterizes each
//!   page (`pdfium-render`, dynamically binds a Pdfium `.so` at runtime — no binary shipped, see
//!   `apps/api/Dockerfile`) and runs real Tesseract OCR on it (`leptess`, needs
//!   `libtesseract-dev`/`libleptonica-dev`/`clang` at **compile** time — a real `-sys` crate).
//!   Gated behind a Cargo feature because this dev workspace's default build must keep working on
//!   every contributor's machine, Windows included, without those installed — see
//!   `apps/api/Cargo.toml`'s own comment on the `ocr` feature. The default (non-`ocr`) build keeps
//!   the exact prior behavior: a scanned/image-only PDF fails the job with a precise reason, not
//!   silent garbage or a crash.
//! - **No `llm-gateway`.** `packages/llm-gateway/README.md` is still "Not started" — Mode A
//!   (`mode_a.rs`) already set the precedent of hard-wiring a direct Ollama call rather than
//!   building the pluggable-provider abstraction for its first caller; this pipeline does the
//!   same for its second. The Ollama request/response shape below is a deliberate copy of
//!   `mode_a.rs::call_ollama`'s pattern, not a shared import — `mode_a.rs` keeps its own copy
//!   scoped to its own three capabilities, and two total callers doesn't justify extracting a
//!   shared helper (same "one extra caller doesn't justify a package" reasoning already applied
//!   to `llm-gateway`/`fuml_client`/`archspace_client`).
//!
//! **Real async, not fake**: `POST /import/documents` inserts the job row and `tokio::spawn`s the
//! pipeline, returning `{jobId}` before the spawned task necessarily finishes. No jobs/worker
//! infrastructure existed anywhere in this codebase before this pass (confirmed via search) — this
//! is a real background task backed by a real status row, not a distributed queue (that's
//! `scheduler`'s separate, Product-2-scoped job).
//!
//! **A proposal per completed job, not per requirement** — reqs v5 §5.6's own amendment explicitly
//! allows "individual or **consolidated-batch** accept/reject" for `document-import`, so batching
//! every drafted Requirement from one document into one proposal is spec-conformant. `Proposal`'s
//! existing columns (`store/versioning.rs`) are shaped entirely around Mode B's "propose one
//! subsystem candidate" concept — `subsystem_id` is repurposed here to hold the job id (documented,
//! not renamed: renaming would touch every existing Mode-B call site for no functional gain),
//! `candidate` holds the full drafted-requirements array, `top_level_requirement_ids` stays empty.
//!
//! **Accept-time materialization is genuinely new**, not a reuse of `mode_b.rs`'s
//! `apply_candidate_to_main` (which only knows Mode B's `Candidate` shape) — see
//! `materialize_proposal` below, called from `mode_b::accept_proposal` once it branches on
//! `proposal.origin == "document-import"`.

use std::time::Duration;

use anyhow::Context;
use axum::{
    extract::{Multipart, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use sha2::{Digest, Sha256};
use sysml_core::{Element, ElementBody, NodeKind, Origin};

use crate::{env_or, import, record_commit, ApiError, AppState, DiffEntry};

// ---------------------------------------------------------------------------
// Ollama call (Structuring stage) — deliberate copy of mode_a.rs::call_ollama's shape, see this
// module's own doc comment for why it isn't shared.
// ---------------------------------------------------------------------------

const STRUCTURING_PROMPT_TEMPLATE: &str = "You are drafting a structured requirement from a \
candidate sentence extracted from a specification document. The candidate was already \
deterministically identified as a requirement statement -- your job is only to clean it up and \
categorize it, not to decide whether it's a real requirement. Respond with ONLY a JSON object (no \
markdown fences, no extra text) with exactly these fields: \"name\" (a short, descriptive title, \
NOT the full sentence), \"shallText\" (the requirement text itself, preserving its meaning), \
\"category\" (a best-effort one-or-two-word category, or null if unclear).\n\n\
Example:\n\
CANDIDATE: \"The system shall provide at least 30,000 lbf of takeoff thrust.\"\n\
RESPONSE: {\"name\": \"Takeoff Thrust\", \"shallText\": \"The system shall provide at least \
30,000 lbf of takeoff thrust.\", \"category\": \"Performance\"}\n\n\
CANDIDATE: \"{candidate}\"\nRESPONSE:";

/// Deterministic-leaning defaults, same reasoning as `mode_a.rs`'s identical constants: drafting a
/// requirement from an already-identified candidate isn't a creative-writing task.
const TEMPERATURE: f32 = 0.0;
const SEED: u64 = 42;

fn prompt_template_hash(template: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(template.as_bytes());
    format!("{:x}", hasher.finalize())
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
struct OllamaOptions {
    temperature: f32,
    seed: u64,
}

#[derive(Debug, serde::Serialize)]
struct OllamaGenerateRequest<'a> {
    model: &'a str,
    prompt: &'a str,
    stream: bool,
    options: OllamaOptions,
}

#[derive(Debug, serde::Deserialize)]
struct OllamaGenerateResponse {
    response: String,
}

/// Returns `(raw_response_text, model_name, model_version)` — `model_version` is the real content
/// digest from `/api/tags`, same rigor `mode_a.rs`/`SimulationRun` provenance already require.
async fn call_ollama(prompt: &str) -> anyhow::Result<(String, String, String)> {
    let base_url = env_or("OLLAMA_URL", "http://localhost:11434");
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
struct RawDraft {
    name: Option<String>,
    #[serde(rename = "shallText")]
    shall_text: Option<String>,
    #[serde(default)]
    category: Option<String>,
}

/// Finds the outermost `{...}` and parses just that slice — tolerant of markdown fences/preamble,
/// same "small models don't perfectly follow format instructions" tolerance as `mode_a.rs`'s own
/// `parse_json_array`. Unlike that function, this never returns "no result" — a candidate that
/// already passed deterministic segmentation must never be silently dropped just because the LLM's
/// output didn't parse (FR-CORE-18's "surfaces... rather than silently drops"), so a parse failure
/// falls back to the raw candidate text itself as both `name` and `shallText`.
fn parse_draft(raw_response: &str, fallback_text: &str) -> (String, String, Option<String>) {
    let parsed = raw_response
        .find('{')
        .zip(raw_response.rfind('}'))
        .filter(|(start, end)| end > start)
        .and_then(|(start, end)| serde_json::from_str::<RawDraft>(&raw_response[start..=end]).ok());
    match parsed {
        Some(draft) => (
            draft
                .name
                .unwrap_or_else(|| fallback_text.chars().take(60).collect()),
            draft
                .shall_text
                .unwrap_or_else(|| fallback_text.to_string()),
            draft.category,
        ),
        None => (
            fallback_text.chars().take(60).collect(),
            fallback_text.to_string(),
            None,
        ),
    }
}

// ---------------------------------------------------------------------------
// Segmentation (deterministic, no LLM call) and structural-noun suggestions (FR-CORE-17).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Confidence {
    High,
    Low,
}

struct SegmentedCandidate {
    text: String,
    page: u32,
    confidence: Confidence,
}

/// Splits each page's text into sentences and keeps any sentence containing "shall"
/// (case-insensitive) as a candidate requirement statement — deterministic/heuristic per reqs v5
/// §5.14 stage 2's own text ("not an LLM call, so mechanical segmentation doesn't consume LLM
/// budget"). Confidence is a simple length heuristic (too short/too long to plausibly be a
/// well-formed requirement) — flagged, never dropped (FR-CORE-18).
fn segment(pages: &[String]) -> Vec<SegmentedCandidate> {
    let mut candidates = Vec::new();
    for (page_index, page_text) in pages.iter().enumerate() {
        for sentence in page_text.split(['.', '!', '?']) {
            let trimmed = sentence.trim();
            if trimmed.is_empty() || !trimmed.to_lowercase().contains("shall") {
                continue;
            }
            let confidence = if (20..=400).contains(&trimmed.len()) {
                Confidence::High
            } else {
                Confidence::Low
            };
            candidates.push(SegmentedCandidate {
                text: trimmed.to_string(),
                page: (page_index + 1) as u32,
                confidence,
            });
        }
    }
    candidates
}

fn is_capitalized_word(word: &str) -> bool {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.is_uppercase() && chars.all(|c| c.is_alphanumeric()),
        None => false,
    }
}

/// FR-CORE-17 — "candidate structural nouns... display-only hints, never persisted as
/// `Structure`/Block elements automatically." A simple heuristic (consecutive Title-Case words),
/// explicitly not a real NLP entity extractor — flagged here, not oversold as more than it is.
fn extract_suggestions(pages: &[String]) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    for page in pages {
        let words: Vec<&str> = page.split_whitespace().collect();
        let mut i = 0;
        while i < words.len() {
            if is_capitalized_word(words[i]) {
                let mut j = i + 1;
                while j < words.len() && is_capitalized_word(words[j]) {
                    j += 1;
                }
                if j - i >= 2 {
                    seen.insert(words[i..j].join(" "));
                }
                i = j;
            } else {
                i += 1;
            }
        }
    }
    seen.into_iter().take(50).collect()
}

// ---------------------------------------------------------------------------
// Drafted-requirement shape — shared between the pipeline (below) and
// mode_b.rs::accept_proposal's document-import materialization branch.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct Citation {
    pub(crate) page: u32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct ImportProvenance {
    #[serde(rename = "modelName")]
    pub(crate) model_name: String,
    #[serde(rename = "modelVersion")]
    pub(crate) model_version: String,
    #[serde(rename = "promptTemplateHash")]
    pub(crate) prompt_template_hash: String,
    pub(crate) temperature: f32,
    pub(crate) seed: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct DraftedRequirement {
    pub(crate) name: String,
    #[serde(rename = "shallText")]
    pub(crate) shall_text: String,
    pub(crate) category: Option<String>,
    pub(crate) citation: Citation,
    pub(crate) confidence: Confidence,
    pub(crate) provenance: ImportProvenance,
}

// ---------------------------------------------------------------------------
// OCR fallback (FR-CORE-14, T-DOCIMPORT-07) — feature-gated, see this module's own doc comment.
// ---------------------------------------------------------------------------

/// Rasterizes every page of a PDF and runs real Tesseract OCR on each — only called once
/// `pdf_extract`'s text-layer extraction has already come back empty for every page. A page's
/// rasterization/OCR failure doesn't abort the whole document: it becomes an empty string for
/// that page (surfaced downstream as "still no text" if every page fails the same way), not a
/// hard error for one bad page in an otherwise-scannable document.
#[cfg(feature = "ocr")]
fn ocr_pages(pdf_bytes: &[u8]) -> anyhow::Result<Vec<String>> {
    let bindings = pdfium_render::prelude::Pdfium::bind_to_library(
        std::env::var("PDFIUM_LIB_PATH").unwrap_or_else(|_| "libpdfium.so".to_string()),
    )
    .or_else(|_| pdfium_render::prelude::Pdfium::bind_to_system_library())
    .map_err(|e| anyhow::anyhow!("binding to Pdfium library: {e}"))?;
    let pdfium = pdfium_render::prelude::Pdfium::new(bindings);
    let document = pdfium
        .load_pdf_from_byte_slice(pdf_bytes, None)
        .map_err(|e| anyhow::anyhow!("loading PDF for OCR: {e}"))?;

    let render_config = pdfium_render::prelude::PdfRenderConfig::new()
        .set_target_width(2000)
        .set_maximum_height(2000);

    let mut pages = Vec::new();
    for page in document.pages().iter() {
        let text = (|| -> anyhow::Result<String> {
            let image = page
                .render_with_config(&render_config)
                .map_err(|e| anyhow::anyhow!("rendering page for OCR: {e}"))?
                .as_image()
                .map_err(|e| {
                    anyhow::anyhow!("converting rendered page to an image for OCR: {e}")
                })?;
            let mut png_bytes = Vec::new();
            image.write_to(
                &mut std::io::Cursor::new(&mut png_bytes),
                image::ImageFormat::Png,
            )?;
            let mut ocr = leptess::LepTess::new(None, "eng")
                .map_err(|e| anyhow::anyhow!("initializing Tesseract: {e}"))?;
            ocr.set_image_from_mem(&png_bytes)
                .map_err(|e| anyhow::anyhow!("loading rasterized page into Tesseract: {e}"))?;
            Ok(ocr.get_utf8_text().unwrap_or_default())
        })();
        pages.push(text.unwrap_or_default());
    }
    Ok(pages)
}

#[cfg(not(feature = "ocr"))]
fn ocr_pages(_pdf_bytes: &[u8]) -> anyhow::Result<Vec<String>> {
    anyhow::bail!("OCR not implemented in this build (compiled without the `ocr` feature)")
}

// ---------------------------------------------------------------------------
// The pipeline itself.
// ---------------------------------------------------------------------------

async fn run_pipeline_inner(
    state: &AppState,
    project_id: &str,
    job_id: &str,
    pdf_bytes: &[u8],
) -> anyhow::Result<()> {
    // Extraction.
    let text_layer_pages = pdf_extract::extract_text_from_mem_by_pages(pdf_bytes)
        .map_err(|e| anyhow::anyhow!("PDF extraction failed: {e}"))?;
    let pages = if text_layer_pages.iter().all(|p| p.trim().is_empty()) {
        // No text layer at all — the scanned-PDF case OCR exists for. Real, CPU-bound work
        // (image rendering + Tesseract), run via `spawn_blocking` so it doesn't stall the async
        // runtime's worker thread — this already runs inside `run_pipeline`'s own `tokio::spawn`,
        // not on any request-handling path, but that doesn't exempt it from this rule.
        let pdf_bytes_owned = pdf_bytes.to_vec();
        let ocr_result = tokio::task::spawn_blocking(move || ocr_pages(&pdf_bytes_owned)).await;
        match ocr_result {
            Ok(Ok(ocr_pages)) if !ocr_pages.iter().all(|p| p.trim().is_empty()) => ocr_pages,
            Ok(Ok(_)) => {
                state
                    .postgres
                    .update_import_job_status(
                        project_id,
                        job_id,
                        "Failed",
                        Some("no extractable text layer -- OCR ran but found no text either"),
                    )
                    .await?;
                return Ok(());
            }
            Ok(Err(ocr_error)) => {
                state
                    .postgres
                    .update_import_job_status(
                        project_id,
                        job_id,
                        "Failed",
                        Some(&format!(
                            "no extractable text layer -- OCR failed: {ocr_error}"
                        )),
                    )
                    .await?;
                return Ok(());
            }
            Err(join_error) => {
                return Err(anyhow::anyhow!("OCR task panicked: {join_error}"));
            }
        }
    } else {
        text_layer_pages
    };

    // Segmentation.
    state
        .postgres
        .update_import_job_status(project_id, job_id, "Segmenting", None)
        .await?;
    let segmented = segment(&pages);
    let suggestions = extract_suggestions(&pages);
    state
        .postgres
        .set_import_job_suggestions(project_id, job_id, &serde_json::to_value(&suggestions)?)
        .await?;
    if segmented.is_empty() {
        // FR-CORE-18, implemented literally: "a reported failure state, not an empty successful
        // import."
        state
            .postgres
            .update_import_job_status(
                project_id,
                job_id,
                "Failed",
                Some("no candidate requirement statements found"),
            )
            .await?;
        return Ok(());
    }

    // Structuring (one Ollama call per candidate, sequential -- fine for a background job).
    state
        .postgres
        .update_import_job_status(project_id, job_id, "Drafting", None)
        .await?;
    let mut drafted = Vec::with_capacity(segmented.len());
    for candidate in &segmented {
        let prompt = STRUCTURING_PROMPT_TEMPLATE.replace("{candidate}", &candidate.text);
        let (raw_response, model_name, model_version) = call_ollama(&prompt).await?;
        let (name, shall_text, category) = parse_draft(&raw_response, &candidate.text);
        drafted.push(DraftedRequirement {
            name,
            shall_text,
            category,
            citation: Citation {
                page: candidate.page,
            },
            confidence: candidate.confidence,
            provenance: ImportProvenance {
                model_name,
                model_version,
                prompt_template_hash: prompt_template_hash(STRUCTURING_PROMPT_TEMPLATE),
                temperature: TEMPERATURE,
                seed: SEED,
            },
        });
    }

    // Grounding & Provenance is folded into the loop above (citation/confidence/provenance are
    // stamped as each candidate is drafted, not a separate pass).

    // Validation: an internal invariant check, not a normal-path filter -- every candidate above
    // already has citation/confidence/provenance stamped unconditionally by this same code, so
    // this should never actually trigger. Implements stage 4's literal "missing any of these
    // three is rejected before stage 5" instruction defensively rather than skipping it because
    // "it shouldn't happen."
    state
        .postgres
        .update_import_job_status(project_id, job_id, "Validating", None)
        .await?;
    for draft in &drafted {
        if draft.shall_text.trim().is_empty() {
            anyhow::bail!("drafted requirement missing shallText -- pipeline invariant violated");
        }
    }

    state
        .postgres
        .set_import_job_candidates(project_id, job_id, &serde_json::to_value(&drafted)?)
        .await?;
    state
        .postgres
        .update_import_job_status(project_id, job_id, "AwaitingReview", None)
        .await?;
    Ok(())
}

/// The `tokio::spawn`ed entry point — never propagates an error out (nothing awaits it), so a
/// failure anywhere in the pipeline is caught here and turned into the job's own `Failed` status
/// rather than a silently-lost panic/error in a detached task.
async fn run_pipeline(state: AppState, project_id: String, job_id: String, pdf_bytes: Vec<u8>) {
    if let Err(err) = run_pipeline_inner(&state, &project_id, &job_id, &pdf_bytes).await {
        tracing::error!(job_id = %job_id, error = ?err, "document import pipeline failed");
        let _ = state
            .postgres
            .update_import_job_status(&project_id, &job_id, "Failed", Some(&err.to_string()))
            .await;
    }
}

// ---------------------------------------------------------------------------
// HTTP handlers.
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Serialize)]
pub(crate) struct CreateImportJobResponse {
    #[serde(rename = "jobId")]
    pub(crate) job_id: String,
}

/// `POST /api/v0/projects/:projectId/import/documents` (FR-CORE-14) — a single PDF file per
/// request. Returns `{jobId}` immediately; the pipeline keeps running after this handler returns.
pub(crate) async fn create_import_job(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    mut multipart: Multipart,
) -> Result<Json<CreateImportJobResponse>, ApiError> {
    let Some(field) = multipart.next_field().await.map_err(anyhow::Error::from)? else {
        return Err(import::BadRequest(
            "multipart request must contain one file field".to_string(),
        )
        .into());
    };
    let file_name = field.file_name().unwrap_or("document.pdf").to_string();
    let bytes = field.bytes().await.map_err(anyhow::Error::from)?;

    let job_id = uuid::Uuid::new_v4().to_string();
    state
        .postgres
        .create_import_job(&project_id, &job_id, &file_name)
        .await?;

    tokio::spawn(run_pipeline(
        state.clone(),
        project_id,
        job_id.clone(),
        bytes.to_vec(),
    ));

    Ok(Json(CreateImportJobResponse { job_id }))
}

fn job_not_found(job_id: &str) -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({ "error": format!("no import job {job_id}") })),
    )
        .into_response()
}

#[derive(Debug, serde::Serialize)]
pub(crate) struct ImportJobStatusResponse {
    pub(crate) status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<String>,
}

/// `GET /api/v0/projects/:projectId/import/documents/:jobId` (FR-CORE-14).
pub(crate) async fn get_import_job_status(
    State(state): State<AppState>,
    Path((project_id, job_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let Some(job) = state.postgres.get_import_job(&project_id, &job_id).await? else {
        return Ok(job_not_found(&job_id));
    };
    Ok(Json(ImportJobStatusResponse {
        status: job.status,
        error: job.error,
    })
    .into_response())
}

/// `GET /api/v0/projects/:projectId/import/documents/:jobId/candidates` (FR-CORE-18).
pub(crate) async fn get_import_job_candidates(
    State(state): State<AppState>,
    Path((project_id, job_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let Some(job) = state.postgres.get_import_job(&project_id, &job_id).await? else {
        return Ok(job_not_found(&job_id));
    };
    Ok(Json(job.candidates.unwrap_or(serde_json::json!([]))).into_response())
}

/// `GET /api/v0/projects/:projectId/import/documents/:jobId/suggestions` (FR-CORE-17).
pub(crate) async fn get_import_job_suggestions(
    State(state): State<AppState>,
    Path((project_id, job_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let Some(job) = state.postgres.get_import_job(&project_id, &job_id).await? else {
        return Ok(job_not_found(&job_id));
    };
    Ok(Json(job.suggestions.unwrap_or(serde_json::json!([]))).into_response())
}

#[derive(Debug, serde::Serialize)]
pub(crate) struct CreateImportProposalResponse {
    #[serde(rename = "proposalId")]
    pub(crate) proposal_id: String,
    #[serde(rename = "branchId")]
    pub(crate) branch_id: String,
}

/// `POST /api/v0/projects/:projectId/import/documents/:jobId/proposal` (FR-CORE-16) — only valid
/// once the job is `AwaitingReview`. Creates one branch (never committed to — see this module's
/// own doc comment for why that matches `mode_b.rs::propose`'s established pattern) and one
/// `document-import`-origin proposal batching every drafted candidate.
pub(crate) async fn create_import_proposal(
    State(state): State<AppState>,
    Path((project_id, job_id)): Path<(String, String)>,
) -> Result<Json<CreateImportProposalResponse>, ApiError> {
    let Some(job) = state.postgres.get_import_job(&project_id, &job_id).await? else {
        return Err(import::BadRequest(format!("no such import job {job_id}")).into());
    };
    if job.status != "AwaitingReview" {
        return Err(import::BadRequest(format!(
            "import job {job_id} is not awaiting review (status: {})",
            job.status
        ))
        .into());
    }
    let candidates = job
        .candidates
        .context("AwaitingReview job has no candidates")?;

    let branch_name = format!("document-import-{}", uuid::Uuid::new_v4());
    let branch = state
        .versioning
        .create_branch(&project_id, &branch_name, None)
        .await?;
    let proposal = state
        .versioning
        .create_proposal(
            &project_id,
            &branch.id,
            &job_id,
            &candidates,
            &[],
            &format!("Document import: {}", job.file_name),
            "document-import",
        )
        .await?;

    Ok(Json(CreateImportProposalResponse {
        proposal_id: proposal.id,
        branch_id: branch.id,
    }))
}

/// Called from `mode_b::accept_proposal` once it branches on `proposal.origin == "document-import"`
/// — creates one real `:Requirement` element per drafted candidate (citation/confidence/category/
/// provenance as body properties), one commit for the whole batch. Genuinely new materialization
/// logic, not a reuse of `apply_candidate_to_main` (Mode-B-candidate-shape-specific).
pub(crate) async fn materialize_proposal(
    state: &AppState,
    project_id: &str,
    actor: &str,
    candidate: &serde_json::Value,
) -> anyhow::Result<()> {
    let drafted: Vec<DraftedRequirement> = serde_json::from_value(candidate.clone())
        .context("parsing document-import proposal candidate")?;

    let mut diff_entries = Vec::with_capacity(drafted.len());
    for draft in drafted {
        let element = Element {
            id: uuid::Uuid::new_v4().to_string(),
            kind: NodeKind::Requirement,
            name: draft.name.clone(),
            active: true,
            origin: Origin::AiSuggested,
        };
        state.neo4j.upsert_element(project_id, &element).await?;
        diff_entries.push(DiffEntry::ElementCreated {
            element_id: element.id.clone(),
            kind: element.kind,
            name: element.name.clone(),
        });
        state
            .postgres
            .upsert_body(
                project_id,
                &ElementBody {
                    element_id: element.id.clone(),
                    rationale: None,
                    properties: serde_json::json!({
                        "shallText": draft.shall_text,
                        "category": draft.category,
                        "citation": draft.citation,
                        "confidence": draft.confidence,
                        "provenance": draft.provenance,
                    }),
                },
            )
            .await?;
    }

    record_commit(
        state,
        project_id,
        actor,
        "Accept document-import proposal",
        diff_entries,
    )
    .await?;
    Ok(())
}
