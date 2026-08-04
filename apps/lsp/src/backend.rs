//! The `LanguageServer` implementation (FR-CORE-02, T-P1.2-01): owns the canonical textual
//! document for one connection, and both directions of the sync —
//!
//! - **text → diagram**: `textDocument/didChange` → `sysml_textual::parse` → `diff` against the
//!   last-known graph snapshot → `ApiClient::apply_ops` (one atomic transaction) →
//!   `textDocument/publishDiagnostics` (parse errors, or the batch's validation errors).
//! - **diagram → text**: the canvas rename flow sends a custom `axioma/elementChanged` request
//!   over this same connection; the server updates its snapshot, re-prints the document, and
//!   pushes it back via a standard `workspace/applyEdit` request.

use crate::api_client::ApiClient;
use std::sync::Mutex;
use sysml_core::{Edge, Element};
use tower_lsp::jsonrpc::Result as LspResult;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

struct DocumentState {
    uri: Option<Url>,
    elements: Vec<Element>,
    contains: Vec<Edge>,
}

pub struct Backend {
    client: Client,
    api: ApiClient,
    state: Mutex<DocumentState>,
}

/// A full-document range wide enough to cover any real document — used for whole-document
/// `workspace/applyEdit` pushes (v1 syncs the entire text, not a targeted region; see the plan's
/// note on this being a deliberate v1 simplification).
fn full_document_range() -> Range {
    Range {
        start: Position {
            line: 0,
            character: 0,
        },
        end: Position {
            line: u32::MAX,
            character: 0,
        },
    }
}

impl Backend {
    pub fn new(client: Client, api: ApiClient) -> Self {
        Self {
            client,
            api,
            state: Mutex::new(DocumentState {
                uri: None,
                elements: Vec::new(),
                contains: Vec::new(),
            }),
        }
    }

    fn snapshot_text(&self) -> String {
        let state = self.state.lock().unwrap();
        sysml_textual::print_tree(&state.elements, &state.contains)
    }

    /// Pushes the current snapshot's text to the client via `workspace/applyEdit`, replacing the
    /// whole document. No-ops if no document is open yet.
    async fn push_snapshot_to_client(&self) {
        let uri = { self.state.lock().unwrap().uri.clone() };
        let Some(uri) = uri else {
            tracing::warn!("push_snapshot_to_client: no document uri yet, skipping");
            return;
        };
        let text = self.snapshot_text();
        tracing::debug!(len = text.len(), "pushing snapshot via workspace/applyEdit");
        let edit = WorkspaceEdit {
            changes: Some(std::collections::HashMap::from([(
                uri,
                vec![TextEdit {
                    range: full_document_range(),
                    new_text: text,
                }],
            )])),
            ..Default::default()
        };
        if let Err(err) = self.client.apply_edit(edit).await {
            tracing::warn!(?err, "applyEdit request failed");
        }
    }

    async fn publish_parse_error(&self, uri: &Url, err: &sysml_textual::ParseError) {
        let diagnostic = Diagnostic {
            range: Range {
                start: Position {
                    line: (err.span.line.saturating_sub(1)) as u32,
                    character: (err.span.col.saturating_sub(1)) as u32,
                },
                end: Position {
                    line: (err.span.line.saturating_sub(1)) as u32,
                    character: err.span.col as u32,
                },
            },
            severity: Some(DiagnosticSeverity::ERROR),
            source: Some("axioma-textual".to_string()),
            message: err.message.clone(),
            ..Default::default()
        };
        self.client
            .publish_diagnostics(uri.clone(), vec![diagnostic], None)
            .await;
    }

    async fn publish_textual_errors(&self, uri: &Url, errors: &[sysml_textual::TextualError]) {
        let diagnostics = errors
            .iter()
            .map(|e| {
                let range = e
                    .span
                    .map(|span| Range {
                        start: Position {
                            line: (span.line.saturating_sub(1)) as u32,
                            character: (span.col.saturating_sub(1)) as u32,
                        },
                        end: Position {
                            line: (span.line.saturating_sub(1)) as u32,
                            character: span.col as u32,
                        },
                    })
                    .unwrap_or_else(full_document_range);
                Diagnostic {
                    range,
                    severity: Some(DiagnosticSeverity::ERROR),
                    source: Some("axioma-textual".to_string()),
                    message: e.message.clone(),
                    ..Default::default()
                }
            })
            .collect();
        self.client
            .publish_diagnostics(uri.clone(), diagnostics, None)
            .await;
    }

    async fn clear_diagnostics(&self, uri: &Url) {
        self.client
            .publish_diagnostics(uri.clone(), vec![], None)
            .await;
    }

    /// Handles a full-document edit: parse → diff against the last-known snapshot → apply →
    /// refresh the snapshot from the (possibly server-mutated, e.g. new real ids) result.
    async fn handle_document_text(&self, uri: Url, text: String) {
        let parsed = match sysml_textual::parse(&text) {
            Ok(parsed) => parsed,
            Err(err) => {
                self.publish_parse_error(&uri, &err).await;
                return;
            }
        };

        let ops = {
            let state = self.state.lock().unwrap();
            sysml_textual::diff(&state.elements, &state.contains, &parsed)
        };
        let ops = match ops {
            Ok(ops) => ops,
            Err(errors) => {
                self.publish_textual_errors(&uri, &errors).await;
                return;
            }
        };

        if ops.is_empty() {
            self.clear_diagnostics(&uri).await;
            return;
        }

        match self.api.apply_ops(&ops).await {
            Ok(result) if result.ok => {
                self.clear_diagnostics(&uri).await;
                self.refresh_snapshot(Some(uri)).await;
            }
            Ok(result) => {
                let diagnostics = result
                    .errors
                    .iter()
                    .map(|e| Diagnostic {
                        range: full_document_range(),
                        severity: Some(DiagnosticSeverity::ERROR),
                        source: Some("axioma-textual".to_string()),
                        message: format!("op #{}: {}", e.op_index, e.message),
                        ..Default::default()
                    })
                    .collect();
                self.client
                    .publish_diagnostics(uri, diagnostics, None)
                    .await;
            }
            Err(err) => {
                tracing::error!(error = ?err, "apply_ops request failed");
                let diagnostic = Diagnostic {
                    range: full_document_range(),
                    severity: Some(DiagnosticSeverity::ERROR),
                    source: Some("axioma-textual".to_string()),
                    message: format!("could not reach the model backend: {err}"),
                    ..Default::default()
                };
                self.client
                    .publish_diagnostics(uri, vec![diagnostic], None)
                    .await;
            }
        }
    }

    /// Re-fetches the graph snapshot from `apps/api` and stores it — used after `didOpen` and
    /// after a successful apply (ids may have changed, e.g. a `Create`'s temp id → real id).
    async fn refresh_snapshot(&self, uri: Option<Url>) {
        let elements = self.api.fetch_elements().await.unwrap_or_default();
        let contains = self.api.fetch_contains().await.unwrap_or_default();
        {
            let mut state = self.state.lock().unwrap();
            state.elements = elements;
            state.contains = contains;
            if let Some(uri) = uri {
                state.uri = Some(uri);
            }
        }
        self.push_snapshot_to_client().await;
    }

    /// Handler for the custom `axioma/elementChanged` method (the diagram→text bridge — see the
    /// module doc comment). Registered via `custom_method` and called as a JSON-RPC *request*
    /// (not a notification) — confirmed directly that a fire-and-forget notification to a
    /// `custom_method`-registered handler never reaches it, only a request does. Applies the
    /// rename to the local snapshot directly (the caller already knows it landed, since it made
    /// the PATCH itself) rather than re-fetching from `apps/api`, keeping this on the fast,
    /// no-round-trip path.
    pub async fn element_changed(&self, params: ElementChangedParams) -> LspResult<()> {
        // Scoped so the `MutexGuard` is lexically dropped before the `.await` below — an
        // explicit `drop(state)` call instead of scope-exit was tried and, even though it
        // frees the guard at the same point, left the resulting future non-`Send` (stable
        // Rust's async lowering doesn't always narrow a held-guard's lifetime from an explicit
        // `drop()` call the way it does from a lexical scope ending), which broke
        // `tower_lsp::jsonrpc::Method`'s `Future: Send` bound on `custom_method`'s registration.
        {
            let mut state = self.state.lock().unwrap();
            match state.elements.iter_mut().find(|e| e.id == params.id) {
                Some(el) => el.name = params.name,
                None => {
                    tracing::warn!(id = %params.id, "elementChanged for an unknown element id");
                }
            }
        }
        self.push_snapshot_to_client().await;
        Ok(())
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ElementChangedParams {
    pub id: String,
    pub name: String,
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _params: InitializeParams) -> LspResult<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "axioma-sysml-textual".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    async fn shutdown(&self) -> LspResult<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        self.refresh_snapshot(Some(uri)).await;
    }

    async fn did_change(&self, mut params: DidChangeTextDocumentParams) {
        // `TextDocumentSyncKind::FULL` — exactly one change event, containing the whole document.
        let Some(change) = params.content_changes.pop() else {
            return;
        };
        self.handle_document_text(params.text_document.uri, change.text)
            .await;
    }
}
