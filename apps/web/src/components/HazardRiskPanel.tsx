"use client";

import type { AxiomaBlockData } from "@axioma/diagram-engine";
import type { Edge as ApiEdge } from "@axioma/shared-types";
import { Button, Panel } from "@axioma/ui-components";
import type { Node as FlowNode } from "@xyflow/react";
import { useEffect, useMemo, useRef, useState } from "react";

/** FR-SAFE-02: "Configurable Severity x Likelihood scales (e.g. 5x5, MIL-STD-882 / ARP4761
 * conventions)". Fixed at 5x5 for this first pass — index+1 is the 1-5 score. */
const SEVERITY_LEVELS = ["Negligible", "Minor", "Moderate", "Major", "Catastrophic"] as const;
const LIKELIHOOD_LEVELS = ["Improbable", "Remote", "Occasional", "Probable", "Frequent"] as const;

interface ElementBody {
  rationale: string | null;
  properties: Record<string, string>;
}

function scoreOf(levels: readonly string[], value: string | undefined): number {
  const index = levels.indexOf(value ?? "");
  return index === -1 ? 1 : index + 1;
}

interface HazardRiskPanelProps {
  nodes: FlowNode<AxiomaBlockData>[];
  /** source=Structure, target=Hazard (FR-SAFE-01). */
  causesEdges: ApiEdge[];
  /** source=Hazard, target=Control (FR-SAFE-01/03). */
  mitigatedByEdges: ApiEdge[];
  editMode: boolean;
  onClose: () => void;
  onCreateHazard: (name: string, causingStructureId: string) => Promise<void>;
  onCreateControl: (hazardId: string, name: string) => Promise<void>;
}

/**
 * FR-SAFE-01..04 / T-P1.2-04, T-P1.2-07: a Severity x Likelihood risk matrix over every `Hazard`
 * element, each hazard's `causes`-linked subsystem, and its `mitigatedBy`-linked Control(s) with
 * status tracking and residual-risk recomputation. Severity/likelihood/status are stored as plain
 * string properties on the element's Postgres body — the same `GET`/`PUT /api/elements/:id/body`
 * path `ElementInspector` uses — so scoring here is just a purpose-built (dropdown-based) editor
 * over the same generic property bag, not a new storage mechanism.
 */
export function HazardRiskPanel({
  nodes,
  causesEdges,
  mitigatedByEdges,
  editMode,
  onClose,
  onCreateHazard,
  onCreateControl,
}: HazardRiskPanelProps) {
  const nodesById = useMemo(() => new Map(nodes.map((n) => [n.id, n])), [nodes]);
  const hazards = useMemo(() => nodes.filter((n) => n.data.kind === "Hazard"), [nodes]);
  const structures = useMemo(() => nodes.filter((n) => n.data.kind === "Structure"), [nodes]);

  const [bodies, setBodies] = useState<Record<string, ElementBody>>({});
  const [subsystemFilter, setSubsystemFilter] = useState<string>("all");
  const [newHazardName, setNewHazardName] = useState("");
  const [newHazardStructureId, setNewHazardStructureId] = useState("");
  const [newControlName, setNewControlName] = useState<Record<string, string>>({});
  const [error, setError] = useState<string | null>(null);

  const controlIds = useMemo(() => mitigatedByEdges.map((e) => e.target), [mitigatedByEdges]);
  const trackedIds = useMemo(
    () => [...hazards.map((h) => h.id), ...controlIds].join(","),
    [hazards, controlIds],
  );

  /** Mirrors `bodies` synchronously (not just after the next render) so `updateProperty` below
   * always reads the latest write instead of a snapshot that may already be out of date — see
   * its own doc comment. */
  const bodiesRef = useRef<Record<string, ElementBody>>({});

  useEffect(() => {
    let cancelled = false;
    async function loadBodies() {
      // Hydrate each id's body exactly once. Re-fetching (and blindly overwriting) an id that's
      // already cached would race an in-flight or just-landed `updateProperty` optimistic write
      // for that same id — this GET reflects a snapshot from whenever it was issued, so if it
      // resolves after a newer local write, it would silently roll that write back in the UI
      // even though the server itself still has it (as happened when this used a full replace).
      const ids = (trackedIds ? trackedIds.split(",") : []).filter(
        (id) => !(id in bodiesRef.current),
      );
      if (ids.length === 0) {
        return;
      }
      const entries = await Promise.all(
        ids.map(async (id): Promise<[string, ElementBody]> => {
          const res = await fetch(`/api/elements/${id}/body`);
          if (!res.ok) {
            return [id, { rationale: null, properties: {} }];
          }
          const body = await res.json();
          return [id, { rationale: body.rationale ?? null, properties: body.properties ?? {} }];
        }),
      );
      if (!cancelled) {
        // Re-check the cache now, not just at fetch-issue time above: an `updateProperty` call
        // for one of these ids may have landed while this fetch was in flight. That local write
        // must win — applying this (now-stale) response on top of it would silently roll it back.
        const freshEntries = entries.filter(([id]) => !(id in bodiesRef.current));
        if (freshEntries.length > 0) {
          bodiesRef.current = { ...bodiesRef.current, ...Object.fromEntries(freshEntries) };
          setBodies((b) => ({ ...b, ...Object.fromEntries(freshEntries) }));
        }
      }
    }
    loadBodies();
    return () => {
      cancelled = true;
    };
  }, [trackedIds]);

  /** `updateProperty` reads/writes `bodiesRef` (not `bodies` state) so two calls fired
   * back-to-back — e.g. scoring severity then likelihood — each see the other's write
   * immediately; reading component state directly would race, since both calls would close over
   * the same pre-update snapshot and the PUT below replaces the whole property bag. */
  async function updateProperty(elementId: string, patch: Record<string, string>) {
    const previous = bodiesRef.current[elementId] ?? { rationale: null, properties: {} };
    const nextBody: ElementBody = {
      rationale: previous.rationale,
      properties: { ...previous.properties, ...patch },
    };
    bodiesRef.current = { ...bodiesRef.current, [elementId]: nextBody };
    setBodies((b) => ({ ...b, [elementId]: nextBody }));

    const res = await fetch(`/api/elements/${elementId}/body`, {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(nextBody),
    });
    if (!res.ok) {
      const err = await res.json().catch(() => null);
      setError(err?.error ?? `save failed with status ${res.status}`);
      // Roll back the optimistic write — only if nothing newer has landed on top of it since.
      if (bodiesRef.current[elementId] === nextBody) {
        bodiesRef.current = { ...bodiesRef.current, [elementId]: previous };
        setBodies((b) => ({ ...b, [elementId]: previous }));
      }
    }
  }

  const visibleHazards = hazards.filter((hazard) => {
    if (subsystemFilter === "all") {
      return true;
    }
    return causesEdges.some((e) => e.source === subsystemFilter && e.target === hazard.id);
  });

  return (
    <Panel className="absolute right-4 top-4 z-10 w-96 max-w-[calc(100vw-2rem)] max-h-[calc(100vh-2rem)] overflow-y-auto p-4">
      <div className="mb-3 flex items-start justify-between gap-2">
        <p className="text-sm font-semibold text-white/90">Hazard / Risk</p>
        <Button variant="ghost" onClick={onClose} className="!px-2 !py-1 text-xs">
          Close
        </Button>
      </div>

      {error && <p className="mb-2 text-xs text-alert">{error}</p>}

      <div className="mb-3">
        <label
          htmlFor="hazard-subsystem-filter"
          className="mb-1 block text-[10px] uppercase tracking-widest text-white/40"
        >
          Filter by subsystem
        </label>
        <select
          id="hazard-subsystem-filter"
          value={subsystemFilter}
          onChange={(event) => setSubsystemFilter(event.target.value)}
          className="w-full rounded border border-white/10 bg-obsidian/60 px-1.5 py-1 text-xs text-white/80 outline-none"
        >
          <option value="all">All subsystems</option>
          {structures
            .filter((s) => causesEdges.some((e) => e.source === s.id))
            .map((s) => (
              <option key={s.id} value={s.id}>
                {s.data.label}
              </option>
            ))}
        </select>
      </div>

      <div className="mb-3">
        <p className="mb-1 text-[10px] uppercase tracking-widest text-white/40">
          Risk matrix (Severity &times; Likelihood)
        </p>
        <div className="grid grid-cols-6 gap-0.5 text-[9px]">
          <div />
          {SEVERITY_LEVELS.map((s) => (
            <div key={s} className="truncate text-center text-white/40" title={s}>
              {s.slice(0, 3)}
            </div>
          ))}
          {[...LIKELIHOOD_LEVELS].reverse().map((likelihood) => (
            <div key={likelihood} className="contents">
              <div className="truncate text-right text-white/40" title={likelihood}>
                {likelihood.slice(0, 3)}
              </div>
              {SEVERITY_LEVELS.map((severity) => {
                const cellHazards = visibleHazards.filter((h) => {
                  const props = bodies[h.id]?.properties ?? {};
                  return props.severity === severity && props.likelihood === likelihood;
                });
                const riskIndex =
                  scoreOf(SEVERITY_LEVELS, severity) * scoreOf(LIKELIHOOD_LEVELS, likelihood);
                const heat =
                  riskIndex >= 15 ? "bg-alert/40" : riskIndex >= 8 ? "bg-alert/15" : "bg-white/5";
                return (
                  <div
                    key={severity}
                    className={`flex min-h-[1.5rem] items-center justify-center rounded ${heat}`}
                    title={`${severity} x ${likelihood} = ${riskIndex}`}
                  >
                    {cellHazards.length > 0 && (
                      <span className="text-[9px] text-white/80">{cellHazards.length}</span>
                    )}
                  </div>
                );
              })}
            </div>
          ))}
        </div>
      </div>

      <div className="space-y-3">
        {visibleHazards.map((hazard) => {
          const props = bodies[hazard.id]?.properties ?? {};
          const severity = props.severity ?? SEVERITY_LEVELS[0];
          const likelihood = props.likelihood ?? LIKELIHOOD_LEVELS[0];
          const rawRisk =
            scoreOf(SEVERITY_LEVELS, severity) * scoreOf(LIKELIHOOD_LEVELS, likelihood);
          const causingStructureId = causesEdges.find((e) => e.target === hazard.id)?.source;
          const causingStructure = causingStructureId
            ? nodesById.get(causingStructureId)?.data.label
            : undefined;
          const controls = mitigatedByEdges
            .filter((e) => e.source === hazard.id)
            .map((e) => nodesById.get(e.target))
            .filter((n): n is FlowNode<AxiomaBlockData> => n !== undefined);
          const isMitigated = controls.some(
            (c) => bodies[c.id]?.properties?.status === "Mitigated",
          );
          const residualRisk = isMitigated ? scoreOf(SEVERITY_LEVELS, severity) * 1 : rawRisk;

          return (
            <div
              key={hazard.id}
              data-hazard-id={hazard.id}
              className="rounded border border-white/10 p-2"
            >
              <p className="text-xs font-semibold text-white/90">{hazard.data.label}</p>
              {causingStructure && (
                <p className="font-mono text-[10px] text-graphite">causes: {causingStructure}</p>
              )}
              <div className="mt-1.5 flex gap-1.5">
                <select
                  value={severity}
                  onChange={(event) => updateProperty(hazard.id, { severity: event.target.value })}
                  className="flex-1 rounded border border-white/10 bg-obsidian/60 px-1 py-0.5 text-[11px] text-white/80"
                >
                  {SEVERITY_LEVELS.map((level) => (
                    <option key={level} value={level}>
                      {level}
                    </option>
                  ))}
                </select>
                <select
                  value={likelihood}
                  onChange={(event) =>
                    updateProperty(hazard.id, { likelihood: event.target.value })
                  }
                  className="flex-1 rounded border border-white/10 bg-obsidian/60 px-1 py-0.5 text-[11px] text-white/80"
                >
                  {LIKELIHOOD_LEVELS.map((level) => (
                    <option key={level} value={level}>
                      {level}
                    </option>
                  ))}
                </select>
              </div>
              <p className="mt-1 font-mono text-[10px] text-white/60">
                Risk Index: {rawRisk} &middot; Residual: {residualRisk}
              </p>

              {controls.length > 0 && (
                <div className="mt-1.5 space-y-1">
                  {controls.map((control) => {
                    const status = bodies[control.id]?.properties?.status ?? "Open";
                    return (
                      <div
                        key={control.id}
                        data-control-id={control.id}
                        className="flex items-center gap-1.5"
                      >
                        <span className="flex-1 truncate text-[11px] text-white/70">
                          {control.data.label}
                        </span>
                        <select
                          value={status}
                          onChange={(event) =>
                            updateProperty(control.id, { status: event.target.value })
                          }
                          className="rounded border border-white/10 bg-obsidian/60 px-1 py-0.5 text-[10px] text-white/80"
                        >
                          <option value="Open">Open</option>
                          <option value="Mitigated">Mitigated</option>
                          <option value="Accepted">Accepted</option>
                        </select>
                      </div>
                    );
                  })}
                </div>
              )}

              {editMode && (
                <div className="mt-1.5 flex gap-1.5">
                  <input
                    value={newControlName[hazard.id] ?? ""}
                    onChange={(event) =>
                      setNewControlName((n) => ({ ...n, [hazard.id]: event.target.value }))
                    }
                    placeholder="New Control name"
                    className="flex-1 rounded border border-white/10 bg-obsidian/60 px-1.5 py-0.5 text-[11px] text-white/80 outline-none"
                  />
                  <Button
                    variant="ghost"
                    className="!px-2 !py-0.5 text-[11px]"
                    disabled={!newControlName[hazard.id]?.trim()}
                    onClick={async () => {
                      const name = newControlName[hazard.id]?.trim();
                      if (!name) {
                        return;
                      }
                      await onCreateControl(hazard.id, name);
                      setNewControlName((n) => ({ ...n, [hazard.id]: "" }));
                    }}
                  >
                    + Control
                  </Button>
                </div>
              )}
            </div>
          );
        })}
        {visibleHazards.length === 0 && (
          <p className="text-xs text-white/40">No hazards for this filter yet.</p>
        )}
      </div>

      {editMode && (
        <div className="mt-3 border-t border-white/10 pt-3">
          <p className="mb-1 text-[10px] uppercase tracking-widest text-white/40">New Hazard</p>
          <div className="flex flex-col gap-1.5">
            <input
              value={newHazardName}
              onChange={(event) => setNewHazardName(event.target.value)}
              placeholder="Hazard name"
              className="rounded border border-white/10 bg-obsidian/60 px-1.5 py-1 text-xs text-white/80 outline-none"
            />
            <select
              id="new-hazard-structure"
              value={newHazardStructureId}
              onChange={(event) => setNewHazardStructureId(event.target.value)}
              className="rounded border border-white/10 bg-obsidian/60 px-1.5 py-1 text-xs text-white/80"
            >
              <option value="">Causing subsystem…</option>
              {structures.map((s) => (
                <option key={s.id} value={s.id}>
                  {s.data.label}
                </option>
              ))}
            </select>
            <Button
              variant="ghost"
              className="!px-2 !py-1 text-xs"
              disabled={!newHazardName.trim() || !newHazardStructureId}
              onClick={async () => {
                await onCreateHazard(newHazardName.trim(), newHazardStructureId);
                setNewHazardName("");
                setNewHazardStructureId("");
              }}
            >
              + Add Hazard
            </Button>
          </div>
        </div>
      )}
    </Panel>
  );
}
