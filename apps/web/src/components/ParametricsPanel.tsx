"use client";

import type { Element as ApiElement } from "@axioma/shared-types";
import { Button, Panel } from "@axioma/ui-components";
import { useState } from "react";

interface EvaluateResult {
  constraintId: string;
  pressureRatio?: number;
  error?: string;
}

interface ParametricsPanelProps {
  projectId: string;
  /** Filtered locally to `kind === "Constraint"` — no dedicated list endpoint exists, the same
   * pattern `ElementInspector`'s Collection-members section and `InteractionPanel` already use
   * for id→name lookups over the already-loaded element list. */
  elements: ApiElement[];
  onClose: () => void;
}

/**
 * FR-PARAM-03 — evaluates one or more Constraints against a single scalar input
 * (`equivalentWeightFlowLbPerSec`, the only input field the seeded compressor performance-map
 * Constraints are keyed on). A pure, synchronous, server-side computation — never touches
 * `cem-core`/`cem-connectors`/`scheduler`.
 */
export function ParametricsPanel({ projectId, elements, onClose }: ParametricsPanelProps) {
  const constraints = elements.filter((e) => e.kind === "Constraint");
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [inputValue, setInputValue] = useState(550);
  const [results, setResults] = useState<EvaluateResult[] | null>(null);
  const [evaluating, setEvaluating] = useState(false);
  const [error, setError] = useState<string | null>(null);

  function toggle(id: string) {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  }

  async function handleEvaluate() {
    setEvaluating(true);
    setError(null);
    try {
      const res = await fetch(`/api/projects/${projectId}/parametrics/evaluate`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          constraintIds: [...selectedIds],
          equivalentWeightFlowLbPerSec: inputValue,
        }),
      });
      if (!res.ok) {
        const err = await res.json().catch(() => null);
        throw new Error(err?.error ?? `request failed with status ${res.status}`);
      }
      const data = await res.json();
      setResults(data.results);
    } catch (err) {
      setError(err instanceof Error ? err.message : "failed to evaluate");
    } finally {
      setEvaluating(false);
    }
  }

  const nameOf = (id: string) => elements.find((e) => e.id === id)?.name ?? id;

  return (
    <Panel className="absolute right-4 top-4 z-10 w-80 max-w-[calc(100vw-2rem)] max-h-[calc(100vh-2rem)] overflow-y-auto p-4">
      <div className="mb-3 flex items-start justify-between gap-2">
        <p className="text-sm font-semibold text-white/90">Parametrics</p>
        <Button variant="ghost" onClick={onClose} className="!px-2 !py-1 text-xs">
          Close
        </Button>
      </div>

      {constraints.length === 0 && (
        <p className="text-xs text-white/40">No Constraint elements exist in this project.</p>
      )}

      {constraints.length > 0 && (
        <>
          <p className="mb-1 text-[10px] uppercase tracking-widest text-white/40">Constraints</p>
          <div className="mb-3 space-y-1">
            {constraints.map((c) => (
              <label key={c.id} className="flex items-center gap-2 text-xs text-white/80">
                <input
                  type="checkbox"
                  checked={selectedIds.has(c.id)}
                  onChange={() => toggle(c.id)}
                />
                <span className="truncate">{c.name}</span>
              </label>
            ))}
          </div>

          <label
            htmlFor="param-input-value"
            className="mb-1 block text-[10px] uppercase tracking-widest text-white/40"
          >
            Equivalent Weight Flow (lb/sec)
          </label>
          <input
            id="param-input-value"
            type="number"
            value={inputValue}
            onChange={(event) => setInputValue(Number(event.target.value))}
            className="mb-3 w-full rounded border border-white/10 bg-obsidian/60 px-1.5 py-1 text-xs text-white/80"
          />

          <Button
            onClick={handleEvaluate}
            disabled={evaluating || selectedIds.size === 0}
            className="mb-3 w-full justify-center !py-1 text-xs"
          >
            {evaluating ? "Evaluating…" : "Evaluate"}
          </Button>

          {error && <p className="mb-2 text-xs text-alert">{error}</p>}

          {results && (
            <div className="space-y-1.5" data-parametrics-results>
              {results.map((r) => (
                <div
                  key={r.constraintId}
                  data-result-constraint-id={r.constraintId}
                  className="rounded border border-white/10 p-1.5"
                >
                  <p className="truncate text-xs text-white/80">{nameOf(r.constraintId)}</p>
                  {r.error ? (
                    <p className="text-[11px] text-alert">{r.error}</p>
                  ) : (
                    <p className="font-mono text-[11px] text-cobalt-glow">
                      pressureRatio = {r.pressureRatio}
                    </p>
                  )}
                </div>
              ))}
            </div>
          )}
        </>
      )}
    </Panel>
  );
}
