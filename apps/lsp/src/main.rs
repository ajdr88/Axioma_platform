//! `lsp` — the LSP server for FR-CORE-02 / T-P1.2-01's text↔diagram sync (NFR-OPS-01 names "LSP"
//! as its own peer service). Browser Monaco can't spawn a stdio process, so this bridges
//! `tower-lsp`'s standard stdio-oriented `Server` over a WebSocket: each connection gets its own
//! pair of in-memory duplex pipes carrying real LSP base-protocol framing (`Content-Length:
//! N\r\n\r\n<body>`), translated to/from one JSON-RPC message per WebSocket text frame at the
//! boundary (browser LSP transports — e.g. `vscode-ws-jsonrpc` — send one message per frame with
//! no header framing of their own).
//!
//! Has no direct store access — every read/write goes through `apps/api`'s existing HTTP surface
//! (`api_client.rs`), which stays the single authoritative owner of the polyglot stores (ADR-003).

mod api_client;
mod backend;

use api_client::ApiClient;
use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::State,
    response::IntoResponse,
    routing::get,
    Router,
};
use backend::Backend;
use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tower_lsp::{LspService, Server};

#[derive(Clone)]
struct AppState {
    api_base_url: String,
    /// Roadmap: Git-backed model versioning — every read/write is project-scoped now. This
    /// server stays pointed at one project (env-var override, else `apps/api`'s first project)
    /// per the plan's deliberate scope trim; see `api_client`'s doc comment.
    project_id_override: Option<String>,
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let state = AppState {
        api_base_url: env_or("API_BASE_URL", "http://localhost:8080"),
        project_id_override: std::env::var("LSP_PROJECT_ID").ok(),
    };

    let app = Router::new()
        .route("/lsp", get(ws_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8090").await?;
    tracing::info!("lsp listening on ws://0.0.0.0:8090/lsp");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: AppState) {
    let api = ApiClient::new(state.api_base_url, state.project_id_override);
    let (service, socket_handle) = LspService::build(|client| Backend::new(client, api))
        .custom_method("axioma/elementChanged", Backend::element_changed)
        .finish();

    // Two unidirectional pipes carrying real Content-Length-framed LSP bytes — `Server` reads
    // `inbound_read`/writes `outbound_write`; this task writes `inbound_write`/reads
    // `outbound_read` (the other end of each pair), translating to/from WebSocket frames.
    let (inbound_write, inbound_read) = tokio::io::duplex(64 * 1024);
    let (outbound_write, outbound_read) = tokio::io::duplex(64 * 1024);

    let server_task = tokio::spawn(async move {
        Server::new(inbound_read, outbound_write, socket_handle)
            .serve(service)
            .await;
    });

    let (mut ws_sink, mut ws_stream) = socket.split();

    let outbound_task = tokio::spawn(async move {
        let mut reader = BufReader::new(outbound_read);
        while let Ok(Some(text)) = read_framed_message(&mut reader).await {
            if ws_sink.send(Message::Text(text)).await.is_err() {
                break;
            }
        }
    });

    let mut inbound_write = inbound_write;
    while let Some(Ok(msg)) = ws_stream.next().await {
        let text = match msg {
            Message::Text(t) => t,
            Message::Close(_) => break,
            _ => continue,
        };
        if write_framed_message(&mut inbound_write, &text)
            .await
            .is_err()
        {
            break;
        }
    }

    drop(inbound_write);
    let _ = outbound_task.await;
    let _ = server_task.await;
}

async fn write_framed_message(
    writer: &mut (impl AsyncWriteExt + Unpin),
    text: &str,
) -> std::io::Result<()> {
    let header = format!("Content-Length: {}\r\n\r\n", text.len());
    writer.write_all(header.as_bytes()).await?;
    writer.write_all(text.as_bytes()).await?;
    writer.flush().await
}

/// Reads one LSP base-protocol frame (`Content-Length: N\r\n\r\n<N bytes>`) — the header-parsing
/// half of the same framing `write_framed_message` produces.
async fn read_framed_message(
    reader: &mut (impl AsyncBufReadExt + Unpin),
) -> std::io::Result<Option<String>> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            return Ok(None);
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some(value) = trimmed.strip_prefix("Content-Length:") {
            content_length = value.trim().parse().ok();
        }
    }
    let Some(len) = content_length else {
        return Ok(None);
    };
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body).await?;
    Ok(Some(String::from_utf8_lossy(&body).into_owned()))
}
