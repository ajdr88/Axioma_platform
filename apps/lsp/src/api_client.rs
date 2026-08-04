//! Thin REST client to `apps/api` — this server has no direct store access of its own (ADR-003's
//! polyglot stores stay owned by `apps/api`); it reuses the exact same HTTP surface `apps/web`
//! already calls for reads, and the new atomic batch endpoint for writes.
//!
//! Every read/write is scoped to one project (roadmap: Git-backed model versioning). This
//! server stays pointed at a single project per the plan's deliberate scope trim — making the
//! text panel itself project-switch-aware is a small follow-up once the canvas's own switcher
//! (built alongside multi-project support) needs it. `resolve_project_id` picks an explicit
//! `LSP_PROJECT_ID` override if set, else the first project `apps/api` reports — the same
//! "default to the first project" the frontend does on its own initial load.

use anyhow::{Context, Result};
use std::collections::HashMap;
use sysml_core::{Edge, Element};
use sysml_textual::GraphOp;

#[derive(Clone)]
pub struct ApiClient {
    base_url: String,
    project_id_override: Option<String>,
    http: reqwest::Client,
}

#[derive(Debug, serde::Deserialize)]
struct ProjectSummary {
    id: String,
}

impl ApiClient {
    pub fn new(base_url: String, project_id_override: Option<String>) -> Self {
        Self {
            base_url,
            project_id_override,
            http: reqwest::Client::new(),
        }
    }

    /// Resolves which project this connection talks to — an explicit override if configured,
    /// else the first project `GET /api/v0/projects` returns.
    pub async fn resolve_project_id(&self) -> Result<String> {
        if let Some(id) = &self.project_id_override {
            return Ok(id.clone());
        }
        let url = format!("{}/api/v0/projects", self.base_url);
        let projects: Vec<ProjectSummary> = self
            .http
            .get(&url)
            .send()
            .await
            .context("fetching projects")?
            .error_for_status()
            .context("projects request failed")?
            .json()
            .await
            .context("parsing projects response")?;
        projects
            .into_iter()
            .next()
            .map(|p| p.id)
            .context("no projects exist yet")
    }

    pub async fn fetch_elements(&self, project_id: &str) -> Result<Vec<Element>> {
        let url = format!("{}/api/v0/projects/{project_id}/elements", self.base_url);
        self.http
            .get(&url)
            .send()
            .await
            .context("fetching elements")?
            .error_for_status()
            .context("elements request failed")?
            .json()
            .await
            .context("parsing elements response")
    }

    pub async fn fetch_contains(&self, project_id: &str) -> Result<Vec<Edge>> {
        let url = format!("{}/api/v0/projects/{project_id}/contains", self.base_url);
        self.http
            .get(&url)
            .send()
            .await
            .context("fetching contains edges")?
            .error_for_status()
            .context("contains request failed")?
            .json()
            .await
            .context("parsing contains response")
    }

    pub async fn apply_ops(&self, project_id: &str, ops: &[GraphOp]) -> Result<ApplyResult> {
        #[derive(serde::Serialize)]
        struct Body<'a> {
            ops: &'a [GraphOp],
        }
        let url = format!(
            "{}/api/v0/projects/{project_id}/text-model/apply",
            self.base_url
        );
        self.http
            .post(&url)
            .json(&Body { ops })
            .send()
            .await
            .context("posting text-model apply")?
            .error_for_status()
            .context("apply request failed")?
            .json()
            .await
            .context("parsing apply response")
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResult {
    pub ok: bool,
    /// Not consulted today — `Backend::handle_document_text` re-fetches the full snapshot on
    /// success instead, which already reflects any newly-minted real ids. Kept because it's a
    /// faithful part of the endpoint's actual response contract (useful to a caller that wants
    /// to avoid the extra round trip).
    #[serde(default)]
    #[allow(dead_code)]
    pub id_map: HashMap<String, String>,
    #[serde(default)]
    pub errors: Vec<ApplyOpError>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyOpError {
    pub op_index: usize,
    pub message: String,
}
