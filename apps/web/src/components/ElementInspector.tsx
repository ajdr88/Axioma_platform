"use client";

import { Button, Panel } from "@axioma/ui-components";
import { useEffect, useState } from "react";

interface PropertyRow {
  key: string;
  value: string;
}

type LoadState = { status: "loading" } | { status: "error"; message: string } | { status: "ready" };

interface ElementInspectorProps {
  elementId: string;
  elementLabel: string;
  projectId: string;
  onClose: () => void;
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
  projectId,
  onClose,
}: ElementInspectorProps) {
  const [state, setState] = useState<LoadState>({ status: "loading" });
  const [rationale, setRationale] = useState("");
  const [rows, setRows] = useState<PropertyRow[]>([]);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setState({ status: "loading" });

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
        </div>
      )}
    </Panel>
  );
}
