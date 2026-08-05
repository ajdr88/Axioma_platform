"use client";

import type { AxiomaBlockData } from "@axioma/diagram-engine";
import { Button, Panel } from "@axioma/ui-components";
import type { Node as FlowNode } from "@xyflow/react";
import { useEffect, useState } from "react";

type Direction = "both" | "incoming" | "outgoing";

interface TraceResultEntry {
  id: string;
  kind: string;
  name: string;
  hopDistance: number;
  viaEdgeKind: string;
}

interface TraceabilityResponse {
  results: TraceResultEntry[];
  nextCursor: string | null;
  fanoutTruncated: boolean;
}

interface TraceabilityPanelProps {
  selectedNode: FlowNode<AxiomaBlockData> | null;
  projectId: string;
  onClose: () => void;
}

/**
 * FR-CORE-03 / NFR-PERF-04 (T-P1.3-01/02): a budgeted traceability viewer over the currently
 * selected canvas node. `depth`/`maxFanout` are required by `apps/api` — there's no server-side
 * default to silently fall back to, matching NFR-PERF-04's "explicit" budget requirement.
 * `direction=incoming` is the same query T-P1.3-01's "change-impact"/"blast radius" is answered
 * with — this panel doesn't special-case that, it's just this endpoint with that direction picked.
 */
export function TraceabilityPanel({ selectedNode, projectId, onClose }: TraceabilityPanelProps) {
  const [depth, setDepth] = useState(3);
  const [maxFanout, setMaxFanout] = useState(50);
  const [direction, setDirection] = useState<Direction>("both");
  const [results, setResults] = useState<TraceResultEntry[]>([]);
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const [fanoutTruncated, setFanoutTruncated] = useState(false);
  const [status, setStatus] = useState<"idle" | "loading" | "error">("idle");
  const [errorMessage, setErrorMessage] = useState("");

  // Stale results from a previous root would otherwise sit under the new root's label until the
  // user notices and re-runs the query by hand. selectedNode?.id is a deliberate reset trigger,
  // not a value the effect body reads.
  // biome-ignore lint/correctness/useExhaustiveDependencies: intentional reset-on-root-change
  useEffect(() => {
    setResults([]);
    setNextCursor(null);
    setFanoutTruncated(false);
    setStatus("idle");
  }, [selectedNode?.id]);

  async function runQuery(cursor: string | null, append: boolean) {
    if (!selectedNode) {
      return;
    }
    setStatus("loading");
    try {
      const params = new URLSearchParams({
        depth: String(depth),
        maxFanout: String(maxFanout),
        direction,
      });
      if (cursor) {
        params.set("cursor", cursor);
      }
      const res = await fetch(
        `/api/projects/${projectId}/elements/${selectedNode.id}/traceability?${params}`,
      );
      if (!res.ok) {
        const err = await res.json().catch(() => null);
        throw new Error(err?.error ?? `request failed with status ${res.status}`);
      }
      const body: TraceabilityResponse = await res.json();
      setResults((prev) => (append ? [...prev, ...body.results] : body.results));
      setNextCursor(body.nextCursor);
      setFanoutTruncated(body.fanoutTruncated);
      setStatus("idle");
    } catch (error) {
      setErrorMessage(error instanceof Error ? error.message : "failed to load traceability");
      setStatus("error");
    }
  }

  return (
    <Panel className="absolute right-4 top-4 z-10 w-96 max-w-[calc(100vw-2rem)] max-h-[calc(100vh-2rem)] overflow-y-auto p-4">
      <div className="mb-3 flex items-start justify-between gap-2">
        <p className="text-sm font-semibold text-white/90">Traceability</p>
        <Button variant="ghost" onClick={onClose} className="!px-2 !py-1 text-xs">
          Close
        </Button>
      </div>

      {!selectedNode && (
        <p className="text-xs text-white/40">Select an element on the canvas to trace it.</p>
      )}

      {selectedNode && (
        <>
          <p className="mb-2 font-mono text-[10px] uppercase tracking-widest text-white/40">
            Root: {selectedNode.data.label}
          </p>

          <div className="mb-3 flex gap-1.5">
            <label className="flex-1 text-[10px] uppercase tracking-widest text-white/40">
              Depth
              <input
                type="number"
                min={0}
                max={10}
                value={depth}
                onChange={(event) => setDepth(Number(event.target.value))}
                className="mt-0.5 w-full rounded border border-white/10 bg-obsidian/60 px-1.5 py-1 text-xs text-white/80"
              />
            </label>
            <label className="flex-1 text-[10px] uppercase tracking-widest text-white/40">
              Max fanout
              <input
                type="number"
                min={1}
                max={500}
                value={maxFanout}
                onChange={(event) => setMaxFanout(Number(event.target.value))}
                className="mt-0.5 w-full rounded border border-white/10 bg-obsidian/60 px-1.5 py-1 text-xs text-white/80"
              />
            </label>
            <label className="flex-1 text-[10px] uppercase tracking-widest text-white/40">
              Direction
              <select
                value={direction}
                onChange={(event) => setDirection(event.target.value as Direction)}
                className="mt-0.5 w-full rounded border border-white/10 bg-obsidian/60 px-1 py-1 text-xs text-white/80"
              >
                <option value="both">Both</option>
                <option value="incoming">Incoming (dependents)</option>
                <option value="outgoing">Outgoing</option>
              </select>
            </label>
          </div>

          <Button
            onClick={() => runQuery(null, false)}
            disabled={status === "loading"}
            className="mb-3 w-full justify-center !py-1 text-xs"
          >
            {status === "loading" ? "Running…" : "Run traceability query"}
          </Button>

          {status === "error" && <p className="mb-2 text-xs text-alert">{errorMessage}</p>}
          {fanoutTruncated && (
            <p className="mb-2 text-[11px] text-alert">
              Fanout exceeded {maxFanout} at one or more nodes — results are capped, not complete.
            </p>
          )}

          <div className="space-y-1">
            {results.map((entry) => (
              <div
                key={entry.id}
                data-trace-result-id={entry.id}
                className="flex items-center justify-between gap-2 rounded border border-white/10 p-1.5"
              >
                <div className="min-w-0">
                  <p className="truncate text-xs text-white/80">{entry.name}</p>
                  <p className="font-mono text-[10px] text-graphite">
                    {entry.kind} · hop {entry.hopDistance} · via {entry.viaEdgeKind}
                  </p>
                </div>
              </div>
            ))}
            {results.length === 0 && status === "idle" && (
              <p className="text-xs text-white/40">Run a query to see results.</p>
            )}
          </div>

          {nextCursor && (
            <Button
              variant="ghost"
              onClick={() => runQuery(nextCursor, true)}
              disabled={status === "loading"}
              className="mt-2 w-full justify-center !py-1 text-xs"
            >
              {status === "loading" ? "Loading…" : "Load more"}
            </Button>
          )}
        </>
      )}
    </Panel>
  );
}
