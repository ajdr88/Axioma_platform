"use client";

import type { AxiomaBlockData } from "@axioma/diagram-engine";
import type { Edge as ApiEdge } from "@axioma/shared-types";
import { Button, Panel } from "@axioma/ui-components";
import type { Node as FlowNode } from "@xyflow/react";
import { useEffect, useMemo, useState } from "react";
import { useElementBodies } from "@/lib/useElementBodies";

interface MissionCoverage {
  totalRequirements: number;
  coveredCount: number;
  orphaned: { id: string; name: string }[];
}

/** FR-MSN-03: "Lifecycle phases (Concept -> Development -> Production -> Operations -> Disposal)
 * as a timeline overlay." */
const PHASES = ["Concept", "Development", "Production", "Operations", "Disposal"] as const;

interface MissionPlanningPanelProps {
  nodes: FlowNode<AxiomaBlockData>[];
  /** source=Stakeholder, target=Mission or Requirement (FR-MSN-02). */
  concernsEdges: ApiEdge[];
  editMode: boolean;
  projectId: string;
  onClose: () => void;
  onCreateMission: (name: string) => Promise<void>;
  onCreateStakeholder: (
    name: string,
    concern: string,
    missionId: string,
    requirementId: string,
  ) => Promise<void>;
}

/**
 * FR-MSN-01/02/03 / T-P1.2-05, T-P1.2-08: a lifecycle-phase timeline over every `Requirement`
 * (phase stored as a plain string property, same pattern as Hazard's severity/likelihood — see
 * `useElementBodies`), plus Stakeholder management — a Stakeholder's link to the Mission and
 * Requirement it's concerned with is a real graph edge (`Concerns`), not a property reference,
 * so it's traversable from either end (queried here by filtering `concernsEdges` by source or by
 * target — no dedicated backend query needed, the edge list is small enough to filter client-side
 * at this scale).
 */
export function MissionPlanningPanel({
  nodes,
  concernsEdges,
  editMode,
  projectId,
  onClose,
  onCreateMission,
  onCreateStakeholder,
}: MissionPlanningPanelProps) {
  const nodesById = useMemo(() => new Map(nodes.map((n) => [n.id, n])), [nodes]);
  const missions = useMemo(() => nodes.filter((n) => n.data.kind === "Mission"), [nodes]);
  const requirements = useMemo(() => nodes.filter((n) => n.data.kind === "Requirement"), [nodes]);
  const stakeholders = useMemo(() => nodes.filter((n) => n.data.kind === "Stakeholder"), [nodes]);

  const [newMissionName, setNewMissionName] = useState("");
  const [newStakeholderName, setNewStakeholderName] = useState("");
  const [newStakeholderConcern, setNewStakeholderConcern] = useState("");
  const [newStakeholderMissionId, setNewStakeholderMissionId] = useState("");
  const [newStakeholderRequirementId, setNewStakeholderRequirementId] = useState("");

  const trackedIds = useMemo(
    () => [...requirements.map((r) => r.id), ...stakeholders.map((s) => s.id)],
    [requirements, stakeholders],
  );
  const { bodies, updateProperty, error } = useElementBodies(trackedIds, projectId);

  // FR-MSN-04 / T-P1.3-05: re-fetched whenever the Concerns edge count changes (creating a
  // Stakeholder's Mission/Requirement links is the only thing that can change coverage) rather
  // than recomputed client-side, so this panel doesn't duplicate `apps/api`'s coverage rule.
  // concernsEdges.length is a deliberate re-fetch trigger, not a value the effect body reads.
  const [coverage, setCoverage] = useState<MissionCoverage | null>(null);
  // biome-ignore lint/correctness/useExhaustiveDependencies: intentional re-fetch-on-edge-change
  useEffect(() => {
    let cancelled = false;
    fetch(`/api/projects/${projectId}/mission-coverage`)
      .then((res) => (res.ok ? res.json() : null))
      .then((data: MissionCoverage | null) => {
        if (!cancelled) {
          setCoverage(data);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setCoverage(null);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [projectId, concernsEdges.length]);

  function stakeholdersConcernedWith(elementId: string): FlowNode<AxiomaBlockData>[] {
    return concernsEdges
      .filter((e) => e.target === elementId)
      .map((e) => nodesById.get(e.source))
      .filter((n): n is FlowNode<AxiomaBlockData> => n !== undefined);
  }

  return (
    <Panel className="absolute right-4 top-4 z-10 w-96 max-w-[calc(100vw-2rem)] max-h-[calc(100vh-2rem)] overflow-y-auto p-4">
      <div className="mb-3 flex items-start justify-between gap-2">
        <p className="text-sm font-semibold text-white/90">Mission Planning</p>
        <Button variant="ghost" onClick={onClose} className="!px-2 !py-1 text-xs">
          Close
        </Button>
      </div>

      {error && <p className="mb-2 text-xs text-alert">{error}</p>}

      {coverage && (
        <div className="mb-3 rounded border border-white/10 p-2">
          <p className="text-[10px] uppercase tracking-widest text-white/40">Mission Coverage</p>
          <p className="text-xs text-white/80">
            {coverage.coveredCount} of {coverage.totalRequirements} requirements traced to a mission
          </p>
          {coverage.orphaned.length > 0 && (
            <ul className="mt-1 space-y-0.5">
              {coverage.orphaned.map((requirement) => (
                <li
                  key={requirement.id}
                  data-orphaned-requirement-id={requirement.id}
                  className="font-mono text-[10px] text-alert"
                >
                  {requirement.name}
                </li>
              ))}
            </ul>
          )}
        </div>
      )}

      <div className="mb-4">
        <p className="mb-1 text-[10px] uppercase tracking-widest text-white/40">Missions</p>
        <div className="space-y-1">
          {missions.map((mission) => {
            const linkedStakeholders = stakeholdersConcernedWith(mission.id);
            return (
              <div key={mission.id} data-mission-id={mission.id} className="text-xs text-white/80">
                {mission.data.label}
                {linkedStakeholders.length > 0 && (
                  <span className="ml-1 font-mono text-[10px] text-graphite">
                    &middot; {linkedStakeholders.map((s) => s.data.label).join(", ")}
                  </span>
                )}
              </div>
            );
          })}
          {missions.length === 0 && <p className="text-xs text-white/40">No missions yet.</p>}
        </div>
        {editMode && (
          <div className="mt-2 flex gap-1.5">
            <input
              value={newMissionName}
              onChange={(event) => setNewMissionName(event.target.value)}
              placeholder="Mission name"
              className="flex-1 rounded border border-white/10 bg-obsidian/60 px-1.5 py-1 text-xs text-white/80 outline-none"
            />
            <Button
              variant="ghost"
              className="!px-2 !py-1 text-xs"
              disabled={!newMissionName.trim()}
              onClick={async () => {
                await onCreateMission(newMissionName.trim());
                setNewMissionName("");
              }}
            >
              + Mission
            </Button>
          </div>
        )}
      </div>

      <div className="mb-4">
        <p className="mb-1 text-[10px] uppercase tracking-widest text-white/40">
          Timeline (Concept &rarr; Disposal)
        </p>
        <div className="space-y-2">
          {PHASES.map((phase) => {
            const inPhase = requirements.filter(
              (r) => (bodies[r.id]?.properties.phase ?? "") === phase,
            );
            return (
              <div key={phase} data-phase={phase} className="rounded border border-white/10 p-1.5">
                <p className="text-[10px] uppercase tracking-widest text-white/40">{phase}</p>
                {inPhase.map((r) => (
                  <p
                    key={r.id}
                    data-requirement-id={r.id}
                    className="font-mono text-[11px] text-white/80"
                  >
                    {r.data.label}
                  </p>
                ))}
              </div>
            );
          })}
        </div>
        <div className="mt-2 space-y-1">
          {requirements.map((requirement) => {
            const linkedStakeholders = stakeholdersConcernedWith(requirement.id);
            return (
              <div key={requirement.id} data-requirement-row-id={requirement.id}>
                <div className="flex items-center gap-1.5">
                  <span className="flex-1 truncate text-[11px] text-white/70">
                    {requirement.data.label}
                  </span>
                  <select
                    data-requirement-phase-for={requirement.id}
                    value={bodies[requirement.id]?.properties.phase ?? ""}
                    onChange={(event) =>
                      updateProperty(requirement.id, { phase: event.target.value })
                    }
                    className="rounded border border-white/10 bg-obsidian/60 px-1 py-0.5 text-[10px] text-white/80"
                  >
                    <option value="">Untagged</option>
                    {PHASES.map((phase) => (
                      <option key={phase} value={phase}>
                        {phase}
                      </option>
                    ))}
                  </select>
                </div>
                {linkedStakeholders.length > 0 && (
                  <p className="font-mono text-[10px] text-graphite">
                    &middot; {linkedStakeholders.map((s) => s.data.label).join(", ")}
                  </p>
                )}
              </div>
            );
          })}
          {requirements.length === 0 && (
            <p className="text-xs text-white/40">No requirements to tag yet.</p>
          )}
        </div>
      </div>

      <div>
        <p className="mb-1 text-[10px] uppercase tracking-widest text-white/40">Stakeholders</p>
        <div className="space-y-1.5">
          {stakeholders.map((stakeholder) => {
            const missionId = concernsEdges.find(
              (e) =>
                e.source === stakeholder.id && nodesById.get(e.target)?.data.kind === "Mission",
            )?.target;
            const requirementId = concernsEdges.find(
              (e) =>
                e.source === stakeholder.id && nodesById.get(e.target)?.data.kind === "Requirement",
            )?.target;
            return (
              <div
                key={stakeholder.id}
                data-stakeholder-id={stakeholder.id}
                className="rounded border border-white/10 p-2 text-xs"
              >
                <p className="font-semibold text-white/90">{stakeholder.data.label}</p>
                <p className="font-mono text-[10px] text-graphite">
                  {bodies[stakeholder.id]?.properties.concern ?? "(no concern set)"}
                </p>
                <p className="text-[10px] text-white/60">
                  {missionId && `Mission: ${nodesById.get(missionId)?.data.label}`}
                  {missionId && requirementId && " · "}
                  {requirementId && `Requirement: ${nodesById.get(requirementId)?.data.label}`}
                </p>
              </div>
            );
          })}
          {stakeholders.length === 0 && (
            <p className="text-xs text-white/40">No stakeholders yet.</p>
          )}
        </div>

        {editMode && (
          <div className="mt-2 flex flex-col gap-1.5 border-t border-white/10 pt-2">
            <input
              value={newStakeholderName}
              onChange={(event) => setNewStakeholderName(event.target.value)}
              placeholder="Stakeholder name"
              className="rounded border border-white/10 bg-obsidian/60 px-1.5 py-1 text-xs text-white/80 outline-none"
            />
            <input
              value={newStakeholderConcern}
              onChange={(event) => setNewStakeholderConcern(event.target.value)}
              placeholder="Concern"
              className="rounded border border-white/10 bg-obsidian/60 px-1.5 py-1 text-xs text-white/80 outline-none"
            />
            <select
              id="new-stakeholder-mission"
              value={newStakeholderMissionId}
              onChange={(event) => setNewStakeholderMissionId(event.target.value)}
              className="rounded border border-white/10 bg-obsidian/60 px-1.5 py-1 text-xs text-white/80"
            >
              <option value="">Linked mission…</option>
              {missions.map((m) => (
                <option key={m.id} value={m.id}>
                  {m.data.label}
                </option>
              ))}
            </select>
            <select
              id="new-stakeholder-requirement"
              value={newStakeholderRequirementId}
              onChange={(event) => setNewStakeholderRequirementId(event.target.value)}
              className="rounded border border-white/10 bg-obsidian/60 px-1.5 py-1 text-xs text-white/80"
            >
              <option value="">Linked requirement…</option>
              {requirements.map((r) => (
                <option key={r.id} value={r.id}>
                  {r.data.label}
                </option>
              ))}
            </select>
            <Button
              variant="ghost"
              className="!px-2 !py-1 text-xs"
              disabled={
                !newStakeholderName.trim() ||
                !newStakeholderMissionId ||
                !newStakeholderRequirementId
              }
              onClick={async () => {
                await onCreateStakeholder(
                  newStakeholderName.trim(),
                  newStakeholderConcern.trim(),
                  newStakeholderMissionId,
                  newStakeholderRequirementId,
                );
                setNewStakeholderName("");
                setNewStakeholderConcern("");
                setNewStakeholderMissionId("");
                setNewStakeholderRequirementId("");
              }}
            >
              + Stakeholder
            </Button>
          </div>
        )}
      </div>
    </Panel>
  );
}
