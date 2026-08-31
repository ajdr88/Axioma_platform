"use client";

import { UNALLOCATED_LANE_ID } from "@axioma/diagram-engine";
import type { Element as ApiElement } from "@axioma/shared-types";
import { Button, Panel } from "@axioma/ui-components";
import { useEffect, useState } from "react";

interface PropertyRow {
  key: string;
  value: string;
}

interface AttachmentMeta {
  id: string;
  file_name: string;
  content_type: string;
  size_bytes: number;
}

type LoadState = { status: "loading" } | { status: "error"; message: string } | { status: "ready" };

interface BreachDependent {
  id: string;
  kind: string;
  name: string;
}

interface LintIssue {
  category: string;
  severity: string;
  message: string;
}

interface ElementInspectorProps {
  elementId: string;
  elementLabel: string;
  /** Gates the "Lint" button (Mode A requirement linting only applies to Requirements) — not
   * used for anything else here. */
  elementKind: string;
  projectId: string;
  editMode: boolean;
  onClose: () => void;
  /** Refetches the canvas's nodes/edges from the backend — reused here after a successful
   * delete, since removing a node isn't one of the local-state updates any existing handler
   * already knows how to do (every other mutation edits a node in place; this removes one). */
  reloadModel: () => Promise<void>;
  /** docs/IMPLEMENTATION_KICKOFF.md Phase 5 (FR-CORE-12) — gates the lane-assignment dropdown
   * below. A real, working allocation action (click-to-allocate), not the drag-to-allocate
   * FR-CORE-12's own text describes — see `swimlane.ts`'s doc comment for the scope-down. */
  swimlaneView?: boolean;
  /** Every Structure this element could be allocated to (the panel's own element excluded). */
  laneOptions?: { id: string; name: string }[];
  /** The lane (Structure id) this element is currently allocated to, or `null` for unallocated. */
  currentLaneId?: string | null;
  onAllocate?: (laneId: string) => Promise<void>;
  /** Already loaded by `page.tsx` — reused for the Collection-members id→name lookup, mirroring
   * `InteractionPanel`'s own `elements` prop rather than a second element fetch. */
  elements: ApiElement[];
}

/**
 * Properties editor (Edit Mode) — GETs/PUTs `apps/api`'s Postgres-backed body
 * (`ElementBody { rationale, properties }`). `properties` is edited as flat string key/value
 * rows only — that's the only shape properties actually have today (the ReqIF importer's
 * attribute map); nested/typed JSON isn't supported here.
 */
export function ElementInspector({
  elementId,
  elementLabel,
  elementKind,
  projectId,
  editMode,
  onClose,
  reloadModel,
  swimlaneView,
  laneOptions,
  currentLaneId,
  onAllocate,
  elements,
}: ElementInspectorProps) {
  const [state, setState] = useState<LoadState>({ status: "loading" });
  const [rationale, setRationale] = useState("");
  const [rows, setRows] = useState<PropertyRow[]>([]);
  const [saving, setSaving] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [allocating, setAllocating] = useState(false);
  const [memberNames, setMemberNames] = useState<string[]>([]);
  const [attachments, setAttachments] = useState<AttachmentMeta[]>([]);
  const [uploading, setUploading] = useState(false);
  const [attachmentError, setAttachmentError] = useState<string | null>(null);

  async function handleAllocateChange(laneId: string) {
    if (!onAllocate) {
      return;
    }
    setAllocating(true);
    try {
      await onAllocate(laneId);
    } finally {
      setAllocating(false);
    }
  }
  const [breach, setBreach] = useState<{ message: string; dependents: BreachDependent[] } | null>(
    null,
  );
  const [linting, setLinting] = useState(false);
  const [lintIssues, setLintIssues] = useState<LintIssue[] | null>(null);
  const [lintError, setLintError] = useState<string | null>(null);

  async function handleLint() {
    setLinting(true);
    setLintError(null);
    setLintIssues(null);
    try {
      const res = await fetch(`/api/projects/${projectId}/cem/mode-a/lint-requirement`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ elementId }),
      });
      if (!res.ok) {
        const err = await res.json().catch(() => null);
        throw new Error(err?.error ?? `lint failed with status ${res.status}`);
      }
      const data = await res.json();
      setLintIssues(data.issues);
    } catch (error) {
      setLintError(error instanceof Error ? error.message : "failed to lint requirement");
    } finally {
      setLinting(false);
    }
  }

  useEffect(() => {
    let cancelled = false;
    setState({ status: "loading" });
    setLintIssues(null);
    setLintError(null);

    async function load() {
      try {
        const res = await fetch(`/api/projects/${projectId}/elements/${elementId}/body`);
        if (res.status === 404) {
          // No body yet (e.g. a freshly-added node) — that's a valid empty starting point, not
          // an error.
          if (!cancelled) {
            setRationale("");
            setRows([]);
            setState({ status: "ready" });
          }
          return;
        }
        if (!res.ok) {
          const err = await res.json().catch(() => null);
          throw new Error(err?.error ?? `request failed with status ${res.status}`);
        }
        const body = await res.json();
        if (!cancelled) {
          setRationale(body.rationale ?? "");
          const properties = (body.properties ?? {}) as Record<string, string>;
          setRows(
            Object.entries(properties).map(([key, value]) => ({ key, value: String(value) })),
          );
          setState({ status: "ready" });
        }
      } catch (error) {
        if (!cancelled) {
          setState({
            status: "error",
            message: error instanceof Error ? error.message : "failed to load properties",
          });
        }
      }
    }

    load();
    return () => {
      cancelled = true;
    };
  }, [elementId, projectId]);

  // FR-CORE-11 — a Collection's members are `Member` edges (source=collection), not `Contains`.
  // No source-filtered edges endpoint exists, so this fetches every `Member` edge in the project
  // and filters locally, the same pattern `InteractionPanel` already uses for `Contains`.
  useEffect(() => {
    if (elementKind !== "Collection") {
      setMemberNames([]);
      return;
    }
    let cancelled = false;
    async function loadMembers() {
      const res = await fetch(`/api/projects/${projectId}/edges?kind=Member`);
      if (!res.ok || cancelled) {
        return;
      }
      const edges: { source: string; target: string }[] = await res.json();
      const names = edges
        .filter((e) => e.source === elementId)
        .map((e) => elements.find((el) => el.id === e.target)?.name ?? e.target);
      if (!cancelled) {
        setMemberNames(names);
      }
    }
    loadMembers();
    return () => {
      cancelled = true;
    };
  }, [elementId, elementKind, projectId, elements]);

  const loadAttachments = async () => {
    const res = await fetch(`/api/projects/${projectId}/elements/${elementId}/attachments`);
    if (res.ok) {
      setAttachments(await res.json());
    }
  };

  // FR-EXPORT-04 — every element (not gated by kind) can carry file attachments.
  // biome-ignore lint/correctness/useExhaustiveDependencies: loadAttachments closes over elementId/projectId, both already listed — adding the function itself (a new reference every render) would re-run this on every render instead of only on elementId/projectId change
  useEffect(() => {
    loadAttachments();
    setAttachmentError(null);
  }, [elementId, projectId]);

  async function handleUpload(event: React.ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0];
    event.target.value = "";
    if (!file) {
      return;
    }
    setUploading(true);
    setAttachmentError(null);
    try {
      const formData = new FormData();
      formData.append("file", file);
      const res = await fetch(`/api/projects/${projectId}/elements/${elementId}/attachments`, {
        method: "POST",
        body: formData,
      });
      if (!res.ok) {
        const err = await res.json().catch(() => null);
        throw new Error(err?.error ?? `upload failed with status ${res.status}`);
      }
      await loadAttachments();
    } catch (error) {
      setAttachmentError(error instanceof Error ? error.message : "failed to upload attachment");
    } finally {
      setUploading(false);
    }
  }

  async function handleSave() {
    setSaving(true);
    try {
      const properties = Object.fromEntries(
        rows.filter((row) => row.key.trim().length > 0).map((row) => [row.key.trim(), row.value]),
      );
      const res = await fetch(`/api/projects/${projectId}/elements/${elementId}/body`, {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ rationale: rationale.trim() || null, properties }),
      });
      if (!res.ok) {
        const err = await res.json().catch(() => null);
        throw new Error(err?.error ?? `save failed with status ${res.status}`);
      }
      onClose();
    } catch (error) {
      setState({
        status: "error",
        message: error instanceof Error ? error.message : "failed to save properties",
      });
    } finally {
      setSaving(false);
    }
  }

  /** T-P1.3-03: a direct Satisfy/Verify/Refine dependent blocks the delete with a 409
   * Traceability Breach unless `acknowledge=true` — `acknowledge` re-sends the same request
   * with that override once the user has seen (and accepted) the dependent list. */
  async function handleDelete(acknowledge: boolean) {
    setDeleting(true);
    try {
      const query = acknowledge ? "?acknowledge=true" : "";
      const res = await fetch(`/api/projects/${projectId}/elements/${elementId}${query}`, {
        method: "DELETE",
      });
      if (res.status === 409) {
        const body = await res.json();
        setBreach({ message: body.message, dependents: body.dependents });
        return;
      }
      if (!res.ok) {
        const err = await res.json().catch(() => null);
        throw new Error(err?.error ?? `delete failed with status ${res.status}`);
      }
      await reloadModel();
      onClose();
    } catch (error) {
      setState({
        status: "error",
        message: error instanceof Error ? error.message : "failed to delete element",
      });
    } finally {
      setDeleting(false);
    }
  }

  return (
    <Panel className="absolute right-4 top-4 z-10 w-80 max-w-[calc(100vw-2rem)] p-4">
      <div className="mb-3 flex items-start justify-between gap-2">
        <div>
          <p className="text-sm font-semibold text-white/90">{elementLabel}</p>
          <p className="font-mono text-[10px] uppercase tracking-widest text-white/40">
            {elementId}
          </p>
        </div>
        <Button variant="ghost" onClick={onClose} className="!px-2 !py-1 text-xs">
          Close
        </Button>
      </div>

      {state.status === "loading" && <p className="text-xs text-white/40">Loading…</p>}
      {state.status === "error" && <p className="text-xs text-alert">{state.message}</p>}

      {(state.status === "ready" || state.status === "error") && (
        <div className="space-y-4">
          <div>
            <label
              htmlFor="element-rationale"
              className="mb-1 block text-[10px] uppercase tracking-widest text-white/40"
            >
              Rationale
            </label>
            <textarea
              id="element-rationale"
              value={rationale}
              onChange={(event) => setRationale(event.target.value)}
              rows={4}
              className="w-full rounded border border-white/10 bg-obsidian/60 p-2 text-xs text-white/80 outline-none focus-visible:ring-1 focus-visible:ring-cobalt-glow"
            />
          </div>

          <div>
            <p className="mb-1 text-[10px] uppercase tracking-widest text-white/40">Properties</p>
            <div className="space-y-1.5">
              {rows.map((row, index) => (
                // biome-ignore lint/suspicious/noArrayIndexKey: rows are edited in place, not reordered
                <div key={index} className="flex gap-1.5">
                  <input
                    value={row.key}
                    placeholder="key"
                    onChange={(event) => {
                      const next = [...rows];
                      next[index] = { ...row, key: event.target.value };
                      setRows(next);
                    }}
                    className="w-1/3 rounded border border-white/10 bg-obsidian/60 px-1.5 py-1 font-mono text-xs text-white/80 outline-none"
                  />
                  <input
                    value={row.value}
                    placeholder="value"
                    onChange={(event) => {
                      const next = [...rows];
                      next[index] = { ...row, value: event.target.value };
                      setRows(next);
                    }}
                    className="flex-1 rounded border border-white/10 bg-obsidian/60 px-1.5 py-1 font-mono text-xs text-white/80 outline-none"
                  />
                  <Button
                    variant="ghost"
                    onClick={() => setRows(rows.filter((_, i) => i !== index))}
                    className="!px-2 !py-1 text-xs"
                  >
                    &times;
                  </Button>
                </div>
              ))}
            </div>
            <Button
              variant="ghost"
              onClick={() => setRows([...rows, { key: "", value: "" }])}
              className="mt-2 !px-2 !py-1 text-xs"
            >
              + Add property
            </Button>
          </div>

          <Button onClick={handleSave} disabled={saving} className="w-full justify-center">
            {saving ? "Saving…" : "Save"}
          </Button>

          {swimlaneView && (
            <div className="border-t border-white/10 pt-3">
              <label
                htmlFor="element-lane"
                className="mb-1 block text-[10px] uppercase tracking-widest text-white/40"
              >
                Swimlane
              </label>
              <select
                id="element-lane"
                value={currentLaneId ?? UNALLOCATED_LANE_ID}
                disabled={allocating}
                onChange={(event) => handleAllocateChange(event.target.value)}
                className="w-full rounded border border-white/10 bg-obsidian/60 p-1.5 text-xs text-white/80 outline-none focus-visible:ring-1 focus-visible:ring-cobalt-glow"
              >
                <option value={UNALLOCATED_LANE_ID}>Unallocated</option>
                {(laneOptions ?? []).map((lane) => (
                  <option key={lane.id} value={lane.id}>
                    {lane.name}
                  </option>
                ))}
              </select>
            </div>
          )}

          {elementKind === "Collection" && (
            <div className="border-t border-white/10 pt-3">
              <p className="mb-1.5 text-[10px] uppercase tracking-widest text-white/40">
                Members (FR-CORE-11)
              </p>
              {memberNames.length === 0 && <p className="text-xs text-white/40">No members.</p>}
              {memberNames.length > 0 && (
                <ul className="mb-2 space-y-0.5">
                  {memberNames.map((name) => (
                    <li key={name} className="truncate text-xs text-white/80">
                      {name}
                    </li>
                  ))}
                </ul>
              )}
              <a
                href={`/api/projects/${projectId}/export/table?collectionId=${elementId}`}
                className="block w-full rounded border border-white/10 px-2 py-1 text-center text-xs text-white/70 hover:bg-white/5"
              >
                Export as Table (CSV)
              </a>
            </div>
          )}

          <div className="border-t border-white/10 pt-3">
            <p className="mb-1.5 text-[10px] uppercase tracking-widest text-white/40">
              Attachments (FR-EXPORT-04)
            </p>
            {attachmentError && <p className="mb-1.5 text-xs text-alert">{attachmentError}</p>}
            {attachments.length === 0 && (
              <p className="mb-1.5 text-xs text-white/40">No attachments.</p>
            )}
            {attachments.length > 0 && (
              <ul className="mb-1.5 space-y-1">
                {attachments.map((a) => (
                  <li
                    key={a.id}
                    data-attachment-id={a.id}
                    className="flex items-center justify-between gap-2 rounded border border-white/10 p-1.5"
                  >
                    <span className="truncate text-xs text-white/80">
                      {a.file_name}{" "}
                      <span className="text-graphite">
                        ({Math.max(1, Math.round(a.size_bytes / 1024))} KB)
                      </span>
                    </span>
                    <a
                      href={`/api/projects/${projectId}/attachments/${a.id}`}
                      className="shrink-0 text-[11px] text-cobalt-glow hover:underline"
                    >
                      Download
                    </a>
                  </li>
                ))}
              </ul>
            )}
            <label className="block">
              <input
                type="file"
                onChange={handleUpload}
                disabled={uploading}
                className="w-full text-xs text-white/60 file:mr-2 file:rounded file:border file:border-white/10 file:bg-white/5 file:px-2 file:py-1 file:text-xs file:text-white/80"
              />
            </label>
            {uploading && <p className="mt-1 text-xs text-white/40">Uploading…</p>}
          </div>

          {elementKind === "Requirement" && (
            <div className="border-t border-white/10 pt-3">
              <div className="mb-1.5 flex items-center justify-between">
                <p className="text-[10px] uppercase tracking-widest text-white/40">
                  Mode A Wording Review
                </p>
                <Button
                  variant="ghost"
                  onClick={handleLint}
                  disabled={linting}
                  className="!px-2 !py-1 text-xs"
                >
                  {linting ? "Linting…" : "Lint"}
                </Button>
              </div>
              {lintError && <p className="text-xs text-alert">{lintError}</p>}
              {lintIssues && lintIssues.length === 0 && (
                <p className="text-xs text-white/40">No wording issues found.</p>
              )}
              {lintIssues && lintIssues.length > 0 && (
                <ul data-lint-issues className="space-y-1.5">
                  {lintIssues.map((issue, index) => (
                    <li
                      // biome-ignore lint/suspicious/noArrayIndexKey: a fresh list each lint run, never reordered
                      key={index}
                      data-lint-severity={issue.severity}
                      className="rounded border border-white/10 p-1.5 text-[11px]"
                    >
                      <span
                        className={issue.severity === "error" ? "text-alert" : "text-cobalt-glow"}
                      >
                        {issue.category}
                      </span>
                      <p className="text-white/70">{issue.message}</p>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          )}

          {editMode && (
            <div className="border-t border-white/10 pt-3">
              {breach ? (
                <div className="space-y-2">
                  <p className="text-xs text-alert">Traceability Breach</p>
                  <p className="text-[11px] text-white/70">{breach.message}</p>
                  <ul className="space-y-0.5">
                    {breach.dependents.map((dependent) => (
                      <li
                        key={dependent.id}
                        data-dependent-id={dependent.id}
                        className="font-mono text-[10px] text-graphite"
                      >
                        {dependent.name} ({dependent.kind})
                      </li>
                    ))}
                  </ul>
                  <div className="flex gap-1.5">
                    <Button
                      variant="ghost"
                      onClick={() => setBreach(null)}
                      className="flex-1 !py-1 text-xs"
                    >
                      Cancel
                    </Button>
                    <Button
                      onClick={() => handleDelete(true)}
                      disabled={deleting}
                      className="flex-1 !bg-alert !py-1 text-xs hover:!bg-alert/80"
                    >
                      {deleting ? "Deleting…" : "Acknowledge and delete anyway"}
                    </Button>
                  </div>
                </div>
              ) : (
                <Button
                  variant="ghost"
                  onClick={() => handleDelete(false)}
                  disabled={deleting}
                  className="w-full justify-center !text-alert"
                >
                  {deleting ? "Deleting…" : "Delete element"}
                </Button>
              )}
            </div>
          )}
        </div>
      )}
    </Panel>
  );
}
