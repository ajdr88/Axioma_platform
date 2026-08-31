import type { Node as FlowNode } from "@xyflow/react";
import { NODE_HEIGHT } from "./dimensions";
import type { SwimlaneLaneData } from "./nodes/SwimlaneLaneNode";

const LANE_WIDTH = 260;
const LANE_HEADER_HEIGHT = 36;
const LANE_GAP = 24;
const MEMBER_GAP = 16;
const MEMBER_INSET_X = 20;

/** Every element with no outgoing `Allocate` edge lands here — a real, visible lane, never
 * silently hidden (FR-CORE-12 doesn't say allocation is mandatory). */
export const UNALLOCATED_LANE_ID = "swimlane-unallocated";

export interface SwimlaneElement {
  id: string;
  name: string;
}

export interface SwimlaneEdge {
  source: string;
  target: string;
}

export interface SwimlaneMemberPosition {
  parentId: string;
  x: number;
  y: number;
}

export interface SwimlaneLayoutResult {
  laneNodes: FlowNode<SwimlaneLaneData>[];
  /** Keyed by member element id — `page.tsx` applies each entry on top of its own
   * `AxiomaBlockData` node for that element (`parentId`/`extent: "parent"`/relative position). */
  memberPositions: Map<string, SwimlaneMemberPosition>;
}

/**
 * docs/IMPLEMENTATION_KICKOFF.md Phase 5 (FR-CORE-12) — a manual grid layout, not ELK: ELK's flat
 * `layered` algorithm (`layout.ts`) has no partition-aware mode wired in here, and building one
 * would be a bigger lift than FR-CORE-12's own "vertical/horizontal partitions" ask needs. Each
 * lane is a real React Flow parent node (`SwimlaneLaneNode`); each member gets `parentId`/
 * `extent: "parent"` — the actual nested-child primitive this feature reuses, confirmed already
 * shipped in the pinned `@xyflow/react` version and unused anywhere else in this codebase before
 * this pass.
 */
export function computeSwimlaneLayout(
  elements: SwimlaneElement[],
  allocateEdges: SwimlaneEdge[],
): SwimlaneLayoutResult {
  const elementsById = new Map(elements.map((e) => [e.id, e]));
  const laneOf = new Map<string, string>();
  for (const edge of allocateEdges) {
    if (elementsById.has(edge.source) && elementsById.has(edge.target)) {
      laneOf.set(edge.source, edge.target);
    }
  }

  const membersByLane = new Map<string, string[]>();
  for (const element of elements) {
    const laneId = laneOf.get(element.id) ?? UNALLOCATED_LANE_ID;
    const list = membersByLane.get(laneId) ?? [];
    list.push(element.id);
    membersByLane.set(laneId, list);
  }

  // Deterministic lane order: real lanes sorted by their owner's name, "Unallocated" always last.
  const realLaneIds = [...membersByLane.keys()]
    .filter((id) => id !== UNALLOCATED_LANE_ID)
    .sort((a, b) => (elementsById.get(a)?.name ?? a).localeCompare(elementsById.get(b)?.name ?? b));
  const laneIds = membersByLane.has(UNALLOCATED_LANE_ID)
    ? [...realLaneIds, UNALLOCATED_LANE_ID]
    : realLaneIds;

  const laneNodes: FlowNode<SwimlaneLaneData>[] = [];
  const memberPositions = new Map<string, SwimlaneMemberPosition>();

  let x = 0;
  for (const laneId of laneIds) {
    const memberIds = membersByLane.get(laneId) ?? [];
    const label =
      laneId === UNALLOCATED_LANE_ID ? "Unallocated" : (elementsById.get(laneId)?.name ?? laneId);
    const laneNodeId = `swimlane-lane-${laneId}`;
    const laneHeight =
      LANE_HEADER_HEIGHT + Math.max(memberIds.length, 1) * (NODE_HEIGHT + MEMBER_GAP) + MEMBER_GAP;

    laneNodes.push({
      id: laneNodeId,
      type: "swimlaneLane",
      position: { x, y: 0 },
      width: LANE_WIDTH,
      height: laneHeight,
      draggable: false,
      selectable: false,
      data: { label },
    });

    memberIds.forEach((memberId, index) => {
      memberPositions.set(memberId, {
        parentId: laneNodeId,
        x: MEMBER_INSET_X,
        y: LANE_HEADER_HEIGHT + MEMBER_GAP + index * (NODE_HEIGHT + MEMBER_GAP),
      });
    });

    x += LANE_WIDTH + LANE_GAP;
  }

  return { laneNodes, memberPositions };
}
