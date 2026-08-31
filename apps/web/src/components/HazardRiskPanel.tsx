"use client";

import type { AxiomaBlockData } from "@axioma/diagram-engine";
import type { Edge as ApiEdge } from "@axioma/shared-types";
import { Button, Panel } from "@axioma/ui-components";
import type { Node as FlowNode } from "@xyflow/react";
import { useMemo, useState } from "react";
import { useElementBodies } from "@/lib/useElementBodies";

/** FR-SAFE-02: "Configurable Severity x Likelihood scales (e.g. 5x5, MIL-STD-882 / ARP4761
 * conventions)". Fixed at 5x5 for this first pass — index+1 is the 1-5 score. */
const SEVERITY_LEVELS = ["Negligible", "Minor", "Moderate", "Major", "Catastrophic"] as const;
const LIKELIHOOD_LEVELS = ["Improbable", "Remote", "Occasional", "Probable", "Frequent"] as const;

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
  projectId: string;
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
  projectId,
  onClose,
  onCreateHazard,
  onCreateControl,
}: HazardRiskPanelProps) {
  const nodesById = useMemo(() => new Map(nodes.map((n) => [n.id, n])), [nodes]);
  const hazards = useMemo(() => nodes.filter((n) => n.data.kind === "Hazard"), [nodes]);
  const structures = useMemo(() => nodes.filter((n) => n.data.kind === "Structure"), [nodes]);

  const [subsystemFilter, setSubsystemFilter] = useState<string>("all");
  const [newHazardName, setNewHazardName] = useState("");
  const [newHazardStructureId, setNewHazardStructureId] = useState("");
  const [newControlName, setNewControlName] = useState<Record<string, string>>({});

  const controlIds = useMemo(() => mitigatedByEdges.map((e) => e.target), [mitigatedByEdges]);
  const trackedIds = useMemo(
    () => [...hazards.map((h) => h.id), ...controlIds],
    [hazards, controlIds],
  );
  const { bodies, updateProperty, error } = useElementBodies(trackedIds, projectId);
  const [exportingReport, setExportingReport] = useState(false);
  const [reportError, setReportError] = useState<string | null>(null);

  /** FR-EXPORT-03 — a real downloadable HTML document via `/export/report`, not the JSON
   * `/safety/risk-register` endpoint this link used to point at (which just navigated the
   * browser to raw JSON despite its "Export" label — a real, pre-existing mismatch fixed here
   * alongside the new capability). Mirrors `page.tsx`'s `handleExportPng`'s `link.download`/
   * `link.click()` pattern, sourcing the blob from a fetch response instead of canvas capture. */
  async function handleExportReport() {
    setExportingReport(true);
    setReportError(null);
    try {
      const res = await fetch(`/api/projects/${projectId}/export/report`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ templateId: "risk-register" }),
      });
      if (!res.ok) {
        const err = await res.json().catch(() => null);
        throw new Error(err?.error ?? `request failed with status ${res.status}`);
      }
      const blob = await res.blob();
      const url = URL.createObjectURL(blob);
      const link = document.createElement("a");
      link.href = url;
      link.download = `risk-register-${projectId}.html`;
      link.click();
      URL.revokeObjectURL(url);
    } catch (err) {
      setReportError(err instanceof Error ? err.message : "failed to export report");
    } finally {
      setExportingReport(false);
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

      <Button
        variant="ghost"
        disabled={exportingReport}
        onClick={handleExportReport}
        className="mb-3 w-full justify-center !py-1 text-xs"
      >
        {exportingReport ? "Exporting…" : "Export Report (ARP4761, HTML)"}
      </Button>
      {reportError && <p className="-mt-2 mb-3 text-xs text-alert">{reportError}</p>}

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
