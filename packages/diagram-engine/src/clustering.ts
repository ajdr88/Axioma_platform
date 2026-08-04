import { type Node as FlowNode, getNodesBounds, type Rect } from "@xyflow/react";
import { NODE_HEIGHT, NODE_WIDTH } from "./dimensions";
import type { AxiomaBlockData } from "./nodes/AxiomaBlockNode";
import type { SubsystemClusterData } from "./nodes/SubsystemClusterNode";

interface ClusterEdge {
  source: string;
  target: string;
}

/** How far beyond the exact viewport a subsystem's bounding box must fall before it's
 * considered off-screen and collapsed — avoids a cluster flickering in/out right at the
 * viewport edge during a pan. Matches impl §4.3's "only viewport + margin live" framing;
 * expressed as a multiple of the viewport's own size on each side, but capped in absolute
 * flow-units (see `MAX_MARGIN_UNITS`) — without the cap, zooming out far enough to view
 * NFR-PERF-01's target of 10,000+ simultaneous elements makes the viewport itself huge, so a
 * proportional margin balloons right along with it and stops collapsing anything. Confirmed via
 * the large-fixture test during this work: at `minZoom`'s extreme (0.01), an uncapped 1x margin
 * left 43,000+ of 100,000 elements "real" and dropped FPS to under 1 — the cap is what makes
 * clustering actually engage at the scale it's meant for. */
const VIEWPORT_MARGIN_FACTOR = 1;
const MAX_MARGIN_UNITS = 3000;

function expandRect(rect: Rect, marginFactor: number): Rect {
  const marginX = Math.min(rect.width * marginFactor, MAX_MARGIN_UNITS);
  const marginY = Math.min(rect.height * marginFactor, MAX_MARGIN_UNITS);
  return {
    x: rect.x - marginX,
    y: rect.y - marginY,
    width: rect.width + 2 * marginX,
    height: rect.height + 2 * marginY,
  };
}

function rectsIntersect(a: Rect, b: Rect): boolean {
  return a.x < b.x + b.width && a.x + a.width > b.x && a.y < b.y + b.height && a.y + a.height > b.y;
}

/** A cluster only exists once its subsystem's bbox fails to intersect the viewport — which by
 * definition puts the bbox's own center outside the viewport too, for any margin ≥ 0. Left at
 * its true position, a cluster node's on-screen coordinates would always fall outside the
 * browser's visible pixels, making `SubsystemClusterNode`'s click-to-expand geometrically
 * unreachable (confirmed via a Playwright pan/zoom probe during this work — the subsystem
 * un-clusters from proximity before its placeholder's true position ever entered the viewport).
 * Clamping to the nearest point on the viewport's edge keeps the placeholder visible as a
 * directional indicator, the way an off-screen radar blip works — `onExpandSubsystem` below still
 * receives the *true* bbox, so clicking still `fitBounds`-navigates to the real location. */
function clampToViewportEdge(bbox: Rect, viewport: Rect): { x: number; y: number } {
  const centerX = bbox.x + bbox.width / 2;
  const centerY = bbox.y + bbox.height / 2;
  const insetX = Math.min(viewport.width / 2, NODE_WIDTH / 2 + 24);
  const insetY = Math.min(viewport.height / 2, NODE_HEIGHT / 2 + 24);
  const clampedCenterX = Math.min(
    viewport.x + viewport.width - insetX,
    Math.max(viewport.x + insetX, centerX),
  );
  const clampedCenterY = Math.min(
    viewport.y + viewport.height - insetY,
    Math.max(viewport.y + insetY, centerY),
  );
  return { x: clampedCenterX - NODE_WIDTH / 2, y: clampedCenterY - NODE_HEIGHT / 2 };
}

/**
 * Viewport-driven subsystem clustering (NFR-PERF-01, T-P1.2-02) — the array-shrinking half of
 * canvas virtualization. `onlyRenderVisibleElements` (set on `<ReactFlow>` in `page.tsx`) only
 * DOM-culls; React Flow still holds and diffs the full `nodes` array regardless. This groups
 * every element under its top-level subsystem (a direct Contains-child of the root), and for any
 * subsystem whose full descendant bounding box doesn't intersect the (margin-expanded) viewport,
 * substitutes one `subsystemCluster` node for the whole subtree in the array actually passed to
 * `<ReactFlow>`. Single-level: an off-screen subsystem's cluster absorbs *all* of its
 * descendants regardless of nesting depth — there's no separate cluster per intermediate level.
 * Dormant at small scale: a subsystem with no descendants has nothing to collapse.
 */
export function computeClusteredNodes(
  nodes: FlowNode<AxiomaBlockData>[],
  containsEdges: ClusterEdge[],
  viewportBounds: Rect,
  onExpandSubsystem: (bbox: Rect) => void,
): (FlowNode<AxiomaBlockData> | FlowNode<SubsystemClusterData>)[] {
  const nodesById = new Map(nodes.map((n) => [n.id, n]));
  const parentOf = new Map<string, string>();
  const childrenOf = new Map<string, string[]>();
  for (const edge of containsEdges) {
    parentOf.set(edge.target, edge.source);
    const children = childrenOf.get(edge.source) ?? [];
    children.push(edge.target);
    childrenOf.set(edge.source, children);
  }

  const roots = nodes.filter((n) => !parentOf.has(n.id));
  const topLevelSubsystemIds = new Set<string>();
  for (const root of roots) {
    for (const childId of childrenOf.get(root.id) ?? []) {
      topLevelSubsystemIds.add(childId);
    }
  }

  function collectDescendants(id: string): string[] {
    const result: string[] = [];
    const stack = [...(childrenOf.get(id) ?? [])];
    while (stack.length > 0) {
      const next = stack.pop();
      if (!next) {
        continue;
      }
      result.push(next);
      stack.push(...(childrenOf.get(next) ?? []));
    }
    return result;
  }

  const clusteredAwayIds = new Set<string>();
  const clusterNodes: FlowNode<SubsystemClusterData>[] = [];
  const expandedViewport = expandRect(viewportBounds, VIEWPORT_MARGIN_FACTOR);

  for (const subsystemId of topLevelSubsystemIds) {
    const descendantIds = collectDescendants(subsystemId);
    const descendantNodes = descendantIds
      .map((id) => nodesById.get(id))
      .filter((n): n is FlowNode<AxiomaBlockData> => n !== undefined);
    if (descendantNodes.length === 0) {
      continue;
    }

    const bbox = getNodesBounds(descendantNodes);
    if (rectsIntersect(bbox, expandedViewport)) {
      continue;
    }

    for (const id of descendantIds) {
      clusteredAwayIds.add(id);
    }
    const subsystemNode = nodesById.get(subsystemId);
    clusterNodes.push({
      id: `cluster-${subsystemId}`,
      type: "subsystemCluster",
      position: clampToViewportEdge(bbox, viewportBounds),
      width: NODE_WIDTH,
      height: NODE_HEIGHT,
      data: {
        label: subsystemNode?.data.label ?? subsystemId,
        count: descendantIds.length,
        onExpand: () => onExpandSubsystem(bbox),
      },
    });
  }

  const realNodes = nodes.filter((n) => !clusteredAwayIds.has(n.id));
  return [...realNodes, ...clusterNodes];
}
