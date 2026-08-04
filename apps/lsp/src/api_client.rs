//! Thin REST client to `apps/api` — this server has no direct store access of its own (ADR-003's
//! polyglot stores stay owned by `apps/api`); it reuses the exact same HTTP surface `apps/web`
//! already calls for reads, and the new atomic batch endpoint for writes.

use anyhow::{Context, Result};
use std::collections::HashMap;
use sysml_core::{Edge, Element};
use sysml_textual::GraphOp;

#[derive(Clone)]
pub struct ApiClient {
    base_url: String,
    http: reqwest::Client,
}

impl ApiClient {
    pub fn new(base_url: String) -> Self {
        Self {
            base_url,
            http: reqwest::Client::new(),
        }
    }

    pub async fn fetch_elements(&self) -> Result<Vec<Element>> {
        let url = format!("{}/api/v0/elements", self.base_url);
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

    pub async fn fetch_contains(&self) -> Result<Vec<Edge>> {
        let url = format!("{}/api/v0/contains", self.base_url);
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

    pub async fn apply_ops(&self, ops: &[GraphOp]) -> Result<ApplyResult> {
        #[derive(serde::Serialize)]
        struct Body<'a> {
            ops: &'a [GraphOp],
        }
        let url = format!("{}/api/v0/text-model/apply", self.base_url);
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
