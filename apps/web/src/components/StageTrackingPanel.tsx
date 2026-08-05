"use client";

import type { AxiomaBlockData } from "@axioma/diagram-engine";
import { Button, Panel } from "@axioma/ui-components";
import type { Node as FlowNode } from "@xyflow/react";
import { useMemo } from "react";
import { useElementBodies } from "@/lib/useElementBodies";

/** FR-PM-01/§3: each Block's current stage, ordered, each with its own controlled status vocab. */
const STAGES = [
  {
    name: "Requirements Definition",
    vocab: ["Draft", "In Review", "Edits Requested", "Approved", "Baselined"],
  },
  {
    name: "Preliminary Design",
    vocab: [
      "In Progress",
      "Internal Review",
      "Peer Review",
      "PDR Prep",
      "Approved w/ Actions",
      "PDR Complete",
    ],
  },
  {
    name: "Detailed Design",
    vocab: [
      "In Progress",
      "Internal Review",
      "Peer Review",
      "CDR Prep",
      "Approved w/ Actions",
      "Released for Fabrication",
    ],
  },
  {
    name: "Prototype Fabrication",
    vocab: [
      "Not Started",
      "Procurement",
      "In Fabrication",
      "In Assembly",
      "QA/Inspection",
      "Nonconformance/Rework",
      "Complete",
    ],
  },
  {
    name: "Testing",
    vocab: ["Test Planning", "TRR", "In Test", "Anomaly/Failed", "Passed", "Verified/Closed"],
  },
] as const;

type StageName = (typeof STAGES)[number]["name"];

/** FR-PM-02/§4: only Concept and Development are ever implied by a subsystem's stage — Production/
 * Operations/Disposal stay program-level only in this revision. */
const STAGE_TO_PHASE: Record<StageName, string> = {
  "Requirements Definition": "Concept",
  "Preliminary Design": "Concept",
  "Detailed Design": "Development",
  "Prototype Fabrication": "Development",
  Testing: "Development",
};
const PHASE_ORDER = ["Concept", "Development", "Production", "Operations", "Disposal"] as const;

function stageByName(name: string) {
  return STAGES.find((s) => s.name === name) ?? STAGES[0];
}

/** FR-PM-03/§5's general formula: position of the current status within its stage's ordered
 * vocabulary. Used for every stage except Requirements Definition, which has a real underlying
 * substrate (see `requirementsDefinitionPercent`) — Preliminary/Detailed Design, Prototype
 * Fabrication, and Testing don't (no sub-Block nesting, no Interface Contract/Parts model, no
 * SimulationRun data yet), so this is the honest interim answer for those four, not a permanent
 * design choice. */
function vocabPositionPercent(vocab: readonly string[], status: string): number {
  const index = vocab.indexOf(status);
  if (index === -1) {
    return 0;
  }
  return Math.round(((index + 1) / vocab.length) * 100);
}

interface Subsystem {
  id: string;
  label: string;
  stage: StageName;
  status: string;
}

/** Same "roots + their direct Contains-children" pattern as
 * `packages/diagram-engine/src/clustering.ts` (`computeClusteredNodes`) uses to find top-level
 * subsystems — duplicated locally rather than exported, since it's ~10 lines and this panel
 * doesn't need clustering's viewport awareness. */
function computeTopLevelSubsystemIds(
  nodeIds: Set<string>,
  containsEdges: { source: string; target: string }[],
): Set<string> {
  const parentOf = new Map<string, string>();
  const childrenOf = new Map<string, string[]>();
  for (const edge of containsEdges) {
    if (!nodeIds.has(edge.source) || !nodeIds.has(edge.target)) {
      continue;
    }
    parentOf.set(edge.target, edge.source);
    const children = childrenOf.get(edge.source) ?? [];
    children.push(edge.target);
    childrenOf.set(edge.source, children);
  }
  const roots = [...nodeIds].filter((id) => !parentOf.has(id));
  const topLevel = new Set<string>();
  for (const root of roots) {
    for (const childId of childrenOf.get(root) ?? []) {
      topLevel.add(childId);
    }
  }
  return topLevel;
}

function collectDescendantIds(
  rootId: string,
  containsEdges: { source: string; target: string }[],
): string[] {
  const childrenOf = new Map<string, string[]>();
  for (const edge of containsEdges) {
    const children = childrenOf.get(edge.source) ?? [];
    children.push(edge.target);
    childrenOf.set(edge.source, children);
  }
  const result: string[] = [];
  const stack = [...(childrenOf.get(rootId) ?? [])];
  while (stack.length > 0) {
    const id = stack.pop();
    if (id === undefined) {
      continue;
    }
    result.push(id);
    stack.push(...(childrenOf.get(id) ?? []));
  }
  return result;
}

interface StageTrackingPanelProps {
  nodes: FlowNode<AxiomaBlockData>[];
  containsEdges: { source: string; target: string }[];
  projectId: string;
  onClose: () => void;
}

/**
 * FR-PM-01..03 (Stage Tracking Amendment Rev C): a per-subsystem engineering-lifecycle stage +
 * status, and computed program-phase/progress rollups derived from it. Stage/status are stored
 * as plain string body properties (`stage`/`status` on the subsystem Structure, `reqStatus` on a
 * Requirement) — the same `GET`/`PUT /api/projects/:projectId/elements/:id/body` path
 * `ElementInspector`/`HazardRiskPanel` use, via the shared `useElementBodies` hook.
 *
 * FR-PM-04 (Testing status derived from `SimulationRun` provenance) and FR-PM-05 (Requirements/
 * Design approval gated through the CEM proposal/branch review mechanism) are NOT enforced here —
 * both real prerequisites (solver-run data, a proposal/review-gate route) don't exist yet in this
 * codebase, and there's no reviewer-identity/auth system to make a second-person approval
 * meaningful regardless. Every status in every stage's vocabulary, including "In Review"/
 * "Approved" and Testing's, is a plain free-editable dropdown for now — same honesty precedent as
 * leaving Validation/Staleness as placeholders in the provenance work until something real
 * produces them.
 */
export function StageTrackingPanel({
  nodes,
  containsEdges,
  projectId,
  onClose,
}: StageTrackingPanelProps) {
  const nodesById = useMemo(() => new Map(nodes.map((n) => [n.id, n])), [nodes]);
  const nodeIds = useMemo(() => new Set(nodes.map((n) => n.id)), [nodes]);

  const subsystemIds = useMemo(() => {
    const topLevel = computeTopLevelSubsystemIds(nodeIds, containsEdges);
    return [...topLevel].filter((id) => nodesById.get(id)?.data.kind === "Structure");
  }, [nodeIds, containsEdges, nodesById]);

  const requirementIdsBySubsystem = useMemo(() => {
    const map = new Map<string, string[]>();
    for (const subsystemId of subsystemIds) {
      const requirementIds = collectDescendantIds(subsystemId, containsEdges).filter(
        (id) => nodesById.get(id)?.data.kind === "Requirement",
      );
      map.set(subsystemId, requirementIds);
    }
    return map;
  }, [subsystemIds, containsEdges, nodesById]);

  const trackedIds = useMemo(
    () => [...subsystemIds, ...[...requirementIdsBySubsystem.values()].flat()],
    [subsystemIds, requirementIdsBySubsystem],
  );
  const { bodies, updateProperty, error } = useElementBodies(trackedIds, projectId);

  const subsystems: Subsystem[] = subsystemIds.map((id) => {
    const props = bodies[id]?.properties ?? {};
    return {
      id,
      label: nodesById.get(id)?.data.label ?? id,
      stage: (props.stage as StageName) ?? STAGES[0].name,
      status: props.status ?? STAGES[0].vocab[0],
    };
  });

  /** FR-PM-03/§5's one real refinement: proportion of the subsystem's nested Requirements that
   * are Baselined — used for the Requirements Definition slot regardless of which stage the
   * subsystem's own pointer is currently at, since it reflects real underlying data rather than
   * just "we've moved past this stage so assume it's done." */
  function requirementsDefinitionPercent(subsystemId: string): number | null {
    const requirementIds = requirementIdsBySubsystem.get(subsystemId) ?? [];
    if (requirementIds.length === 0) {
      return null;
    }
    const baselined = requirementIds.filter(
      (id) => bodies[id]?.properties.reqStatus === "Baselined",
    ).length;
    return Math.round((baselined / requirementIds.length) * 100);
  }

  function stagePercent(subsystem: Subsystem, stage: (typeof STAGES)[number]): number {
    if (stage.name === "Requirements Definition") {
      return requirementsDefinitionPercent(subsystem.id) ?? 0;
    }
    const currentIndex = STAGES.findIndex((s) => s.name === subsystem.stage);
    const stageIndex = STAGES.findIndex((s) => s.name === stage.name);
    if (stageIndex < currentIndex) {
      return 100;
    }
    if (stageIndex > currentIndex) {
      return 0;
    }
    return vocabPositionPercent(stage.vocab, subsystem.status);
  }

  function overallPercent(subsystem: Subsystem): number {
    const total = STAGES.reduce((sum, stage) => sum + stagePercent(subsystem, stage), 0);
    return Math.round(total / STAGES.length);
  }

  const programOverallPercent =
    subsystems.length === 0
      ? 0
      : Math.round(subsystems.reduce((sum, s) => sum + overallPercent(s), 0) / subsystems.length);

  const programPhase = subsystems.reduce((minPhase, subsystem) => {
    const phase = STAGE_TO_PHASE[subsystem.stage];
    return PHASE_ORDER.indexOf(phase as never) < PHASE_ORDER.indexOf(minPhase as never)
      ? phase
      : minPhase;
  }, PHASE_ORDER[0] as string);

  function handleStageChange(subsystemId: string, nextStageName: string) {
    const nextStage = stageByName(nextStageName);
    // Statuses aren't shared across vocabularies — reset to the new stage's first status rather
    // than carrying over a value that may not even be a valid option there.
    updateProperty(subsystemId, { stage: nextStage.name, status: nextStage.vocab[0] });
  }

  return (
    <Panel className="absolute right-4 top-4 z-10 w-96 max-w-[calc(100vw-2rem)] max-h-[calc(100vh-2rem)] overflow-y-auto p-4">
      <div className="mb-3 flex items-start justify-between gap-2">
        <p className="text-sm font-semibold text-white/90">Stage Tracking</p>
        <Button variant="ghost" onClick={onClose} className="!px-2 !py-1 text-xs">
          Close
        </Button>
      </div>

      {error && <p className="mb-2 text-xs text-alert">{error}</p>}

      <div className="mb-4 rounded border border-white/10 p-2">
        <p className="text-[10px] uppercase tracking-widest text-white/40">Program Phase</p>
        <p className="text-sm font-semibold text-white/90">{programPhase}</p>
        <p className="mt-1 font-mono text-[10px] text-graphite">
          Program overall: {programOverallPercent}%
        </p>
      </div>

      <div className="space-y-3">
        {subsystems.map((subsystem) => (
          <div
            key={subsystem.id}
            data-subsystem-id={subsystem.id}
            className="rounded border border-white/10 p-2"
          >
            <div className="flex items-center justify-between gap-2">
              <p className="text-xs font-semibold text-white/90">{subsystem.label}</p>
              <span className="font-mono text-[10px] text-graphite">
                {overallPercent(subsystem)}%
              </span>
            </div>
            <div className="mt-1.5 flex gap-1.5">
              <select
                value={subsystem.stage}
                onChange={(event) => handleStageChange(subsystem.id, event.target.value)}
                className="flex-1 rounded border border-white/10 bg-obsidian/60 px-1 py-0.5 text-[11px] text-white/80"
              >
                {STAGES.map((stage) => (
                  <option key={stage.name} value={stage.name}>
                    {stage.name}
                  </option>
                ))}
              </select>
              <select
                value={subsystem.status}
                onChange={(event) => updateProperty(subsystem.id, { status: event.target.value })}
                className="flex-1 rounded border border-white/10 bg-obsidian/60 px-1 py-0.5 text-[11px] text-white/80"
              >
                {stageByName(subsystem.stage).vocab.map((status) => (
                  <option key={status} value={status}>
                    {status}
                  </option>
                ))}
              </select>
            </div>
            <div className="mt-1.5 grid grid-cols-5 gap-0.5">
              {STAGES.map((stage) => {
                const percent = stagePercent(subsystem, stage);
                const isCurrent = stage.name === subsystem.stage;
                return (
                  <div
                    key={stage.name}
                    title={`${stage.name}: ${percent}%`}
                    className={`rounded px-0.5 py-1 text-center text-[8px] ${
                      isCurrent ? "bg-cobalt-glow/30 text-white/90" : "bg-white/5 text-white/40"
                    }`}
                  >
                    {percent}%
                  </div>
                );
              })}
            </div>
          </div>
        ))}
        {subsystems.length === 0 && (
          <p className="text-xs text-white/40">No top-level subsystems to track yet.</p>
        )}
      </div>
    </Panel>
  );
}
