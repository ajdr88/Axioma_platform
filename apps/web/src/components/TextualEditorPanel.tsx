"use client";

import Editor, { type OnMount } from "@monaco-editor/react";
import type * as Monaco from "monaco-editor";
import { useEffect, useRef, useState } from "react";
import { createMessageConnection, type MessageConnection } from "vscode-jsonrpc/browser";
import { toSocket, WebSocketMessageReader, WebSocketMessageWriter } from "vscode-ws-jsonrpc";

const LANGUAGE_ID = "axioma-sysml-textual";
const DOCUMENT_URI = "inmemory://axioma/model.sysml-textual";

/**
 * FR-CORE-02 / T-P1.2-01's text pane — driven by a real LSP connection to `apps/lsp` over raw
 * JSON-RPC (`vscode-jsonrpc`/`vscode-ws-jsonrpc`), not `monaco-languageclient`'s
 * `MonacoLanguageClient`/`BaseLanguageClient`. That class's automatic `vscode.workspace`
 * document tracking, diagnostics collection, and `workspace/applyEdit` handling all require
 * `monaco-languageclient`'s companion `@codingame/monaco-vscode-*` service-registry layer to be
 * initialized first (confirmed directly: constructing/starting it against plain `monaco-editor`
 * throws "Default api is not ready yet, do not forget to import 'vscode/localExtensionHost'").
 * This component was already driving every part of the protocol manually (`didOpen`/`didChange`
 * sent by hand, `workspace/applyEdit`/`publishDiagnostics` applied by hand) — none of
 * `BaseLanguageClient`'s automatic wiring was actually in use, so talking raw JSON-RPC directly
 * is a strictly smaller, equally real LSP client for what this needs.
 */
export interface TextualEditorPanelHandle {
  /** Notifies the LSP server that a rename landed via the canvas (the diagram→text bridge —
   * `AxiomaBlockNode`'s rename flow calls this after its own PATCH resolves) — a no-op if the
   * connection isn't up yet. */
  notifyElementRenamed: (id: string, name: string) => void;
}

interface TextualEditorPanelProps {
  onClose: () => void;
  onHandleReady: (handle: TextualEditorPanelHandle) => void;
  /** Called whenever the server pushes a refreshed snapshot (`workspace/applyEdit`) — i.e. a
   * text-driven edit (or the initial didOpen sync) changed the model on the backend. The canvas
   * has no other way to learn about a change that originated on this side of the bridge: unlike
   * the diagram→text direction (`notifyElementRenamed`, driven by the canvas's own PATCH), a
   * text edit is applied to `apps/api` entirely inside `apps/lsp`, so `page.tsx`'s React Flow
   * state would otherwise never refetch and silently drift from the real graph. */
  onModelChanged?: () => void;
}

function lspUrl(): string {
  if (typeof process !== "undefined" && process.env.NEXT_PUBLIC_LSP_URL) {
    return process.env.NEXT_PUBLIC_LSP_URL;
  }
  return "ws://localhost:8090/lsp";
}

export function TextualEditorPanel({
  onClose,
  onHandleReady,
  onModelChanged,
}: TextualEditorPanelProps) {
  const connectionRef = useRef<MessageConnection | null>(null);
  const editorRef = useRef<Monaco.editor.IStandaloneCodeEditor | null>(null);
  const monacoRef = useRef<typeof Monaco | null>(null);
  const [status, setStatus] = useState<"connecting" | "connected" | "error">("connecting");
  // `onModelChanged` is called from inside the connection-setup effect below, which
  // deliberately runs once (empty deps — the WebSocket connection shouldn't be torn down and
  // rebuilt just because this callback's identity changed upstream). A ref keeps the call
  // pointed at the latest callback without adding it to that effect's dependencies.
  const onModelChangedRef = useRef(onModelChanged);
  onModelChangedRef.current = onModelChanged;

  useEffect(() => {
    const socket = new WebSocket(lspUrl());

    socket.onopen = async () => {
      const wsSocket = toSocket(socket);
      const reader = new WebSocketMessageReader(wsSocket);
      const writer = new WebSocketMessageWriter(wsSocket);
      const connection = createMessageConnection(reader, writer);

      connection.onRequest("workspace/applyEdit", (params: WorkspaceApplyEditParams) => {
        applyWorkspaceEdit(editorRef.current, params);
        onModelChangedRef.current?.();
        return { applied: true };
      });
      connection.onNotification(
        "textDocument/publishDiagnostics",
        (params: PublishDiagnosticsParams) => {
          applyDiagnostics(editorRef.current, monacoRef.current, params);
        },
      );

      connection.listen();
      connectionRef.current = connection;

      try {
        await connection.sendRequest("initialize", {
          processId: null,
          rootUri: null,
          capabilities: {},
        });
        await connection.sendNotification("initialized", {});
        setStatus("connected");
        await connection.sendNotification("textDocument/didOpen", {
          textDocument: { uri: DOCUMENT_URI, languageId: LANGUAGE_ID, version: 1, text: "" },
        });
      } catch {
        setStatus("error");
      }
    };
    socket.onerror = () => setStatus("error");

    return () => {
      connectionRef.current?.dispose();
      connectionRef.current = null;
      socket.close();
    };
  }, []);

  useEffect(() => {
    onHandleReady({
      notifyElementRenamed: (id, name) => {
        // Registered on the server via `custom_method` (request/response shaped), not
        // `custom_notification` — sent as a request rather than a fire-and-forget notification
        // to match, even though the resolved value itself isn't needed here.
        connectionRef.current?.sendRequest("axioma/elementChanged", { id, name }).catch(() => {});
      },
    });
    // `onHandleReady` is expected to be a stable callback (see `Canvas`'s `useCallback`-wrapped
    // ref-setter) — only re-registering when the identity actually changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [onHandleReady]);

  const handleMount: OnMount = (editor, monacoInstance) => {
    editorRef.current = editor;
    monacoRef.current = monacoInstance;
    let version = 1;
    let debounceTimer: ReturnType<typeof setTimeout> | null = null;
    editor.onDidChangeModelContent(() => {
      const connection = connectionRef.current;
      if (!connection) {
        return;
      }
      if (debounceTimer) {
        clearTimeout(debounceTimer);
      }
      debounceTimer = setTimeout(() => {
        version += 1;
        connection.sendNotification("textDocument/didChange", {
          textDocument: { uri: DOCUMENT_URI, version },
          contentChanges: [{ text: editor.getValue() }],
        });
      }, 300);
    });
  };

  return (
    <div className="flex h-full w-[480px] flex-shrink-0 flex-col border-l border-white/10 bg-obsidian/95">
      <div className="flex items-center justify-between border-b border-white/10 px-3 py-2">
        <span className="font-mono text-[10px] uppercase tracking-widest text-white/50">
          Text View{status !== "connected" ? ` (${status})` : ""}
        </span>
        <button type="button" onClick={onClose} className="text-sm text-white/50 hover:text-white">
          ×
        </button>
      </div>
      <div className="min-h-0 flex-1">
        <Editor
          defaultLanguage={LANGUAGE_ID}
          theme="vs-dark"
          onMount={handleMount}
          options={{ minimap: { enabled: false }, fontSize: 13 }}
        />
      </div>
    </div>
  );
}

interface LspPosition {
  line: number;
  character: number;
}

interface LspRange {
  start: LspPosition;
  end: LspPosition;
}

interface WorkspaceApplyEditParams {
  edit: {
    changes?: Record<string, { range: LspRange; newText: string }[]>;
  };
}

interface PublishDiagnosticsParams {
  diagnostics: { range: LspRange; message: string }[];
}

/** `workspace/applyEdit`'s handler — v1 always replaces the whole document in one edit (see the
 * module doc comment on why this component drives the protocol manually), so this just applies
 * whatever range/text the server sent without trying to reconcile it against local cursor state. */
function applyWorkspaceEdit(
  editor: Monaco.editor.IStandaloneCodeEditor | null,
  params: WorkspaceApplyEditParams,
): void {
  const model = editor?.getModel();
  if (!model) {
    return;
  }
  const edits = params.edit.changes?.[DOCUMENT_URI] ?? [];
  for (const edit of edits) {
    model.applyEdits([
      {
        range: {
          startLineNumber: edit.range.start.line + 1,
          startColumn: edit.range.start.character + 1,
          endLineNumber: edit.range.end.line + 1,
          endColumn: edit.range.end.character + 1,
        },
        text: edit.newText,
      },
    ]);
  }
}

function applyDiagnostics(
  editor: Monaco.editor.IStandaloneCodeEditor | null,
  monacoInstance: typeof Monaco | null,
  params: PublishDiagnosticsParams,
): void {
  const model = editor?.getModel();
  if (!model || !monacoInstance) {
    return;
  }
  const markers: Monaco.editor.IMarkerData[] = params.diagnostics.map((d) => ({
    severity: monacoInstance.MarkerSeverity.Error,
    startLineNumber: d.range.start.line + 1,
    startColumn: d.range.start.character + 1,
    endLineNumber: d.range.end.line + 1,
    endColumn: d.range.end.character + 1,
    message: d.message,
  }));
  monacoInstance.editor.setModelMarkers(model, "axioma-textual", markers);
}
