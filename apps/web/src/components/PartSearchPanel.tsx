"use client";

import { Button, Panel } from "@axioma/ui-components";
import { useState } from "react";

interface PartMatch {
  elementId: string;
  kind: string;
  name: string;
  reason: string;
}

interface PartSearchResponse {
  matches: PartMatch[];
  provenance: { modelName: string };
}

interface PartSearchPanelProps {
  projectId: string;
  onClose: () => void;
  /** Selects the matched element on canvas (opens `ElementInspector`) — the same
   * `setSelectedNodeId` the canvas's own click-to-select already uses. */
  onSelectElement: (elementId: string) => void;
}

/**
 * Mode A part search (roadmap: Mode A fast-follow) — describes a part in natural language, the
 * LLM ranks/returns matching elements from the whole project. This is in-context LLM ranking,
 * not real vector-embedding search (none exists) — see `apps/api/src/mode_a.rs`'s doc comment
 * for the scope/limitation this implies (works at reference-fixture size, not verified at
 * `Turbofan-Scale`'s 1M elements). Every returned match is a real element in the project — a
 * fabricated id would have been filtered out server-side before this panel ever sees it.
 */
export function PartSearchPanel({ projectId, onClose, onSelectElement }: PartSearchPanelProps) {
  const [description, setDescription] = useState("");
  const [matches, setMatches] = useState<PartMatch[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function handleSearch() {
    if (!description.trim()) {
      return;
    }
    setBusy(true);
    setError(null);
    setMatches(null);
    try {
      const res = await fetch(`/api/projects/${projectId}/cem/mode-a/part-search`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ description: description.trim() }),
      });
      if (!res.ok) {
        const err = await res.json().catch(() => null);
        throw new Error(err?.error ?? `search failed with status ${res.status}`);
      }
      const data: PartSearchResponse = await res.json();
      setMatches(data.matches);
    } catch (err) {
      setError(err instanceof Error ? err.message : "part search failed");
    } finally {
      setBusy(false);
    }
  }

  return (
    <Panel className="absolute right-4 top-4 z-10 w-96 max-w-[calc(100vw-2rem)] max-h-[calc(100vh-2rem)] overflow-y-auto p-4">
      <div className="mb-3 flex items-start justify-between gap-2">
        <p className="text-sm font-semibold text-white/90">Part Search</p>
        <Button variant="ghost" onClick={onClose} className="!px-2 !py-1 text-xs">
          Close
        </Button>
      </div>

      <p className="mb-3 text-[11px] text-white/60">
        Describe a part in plain language — an LLM ranks matching elements from this project.
      </p>

      <div className="flex gap-1.5">
        <input
          data-part-search-input
          value={description}
          onChange={(event) => setDescription(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              handleSearch();
            }
          }}
          placeholder="e.g. something that spins to compress air"
          className="flex-1 rounded border border-white/10 bg-obsidian/60 px-1.5 py-1 text-xs text-white/80 outline-none"
        />
        <Button
          variant="primary"
          className="!px-3 !py-1 text-xs"
          disabled={busy || !description.trim()}
          onClick={handleSearch}
        >
          {busy ? "Searching…" : "Search"}
        </Button>
      </div>

      {error && <p className="mt-2 text-xs text-alert">{error}</p>}

      {matches && (
        <div data-part-search-results className="mt-3 space-y-1.5">
          {matches.length === 0 && (
            <p className="text-xs text-white/40">No matching elements found.</p>
          )}
          {matches.map((match) => (
            <button
              key={match.elementId}
              type="button"
              data-part-search-result-id={match.elementId}
              onClick={() => onSelectElement(match.elementId)}
              className="block w-full rounded border border-white/10 p-2 text-left text-xs hover:border-cobalt-glow/60"
            >
              <p className="font-semibold text-white/90">{match.name}</p>
              <p className="font-mono text-[10px] text-graphite">
                {match.elementId} &middot; {match.kind}
              </p>
              {match.reason && <p className="mt-1 text-[11px] text-white/60">{match.reason}</p>}
            </button>
          ))}
        </div>
      )}
    </Panel>
  );
}
