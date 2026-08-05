// The default `elkjs` entry point conditionally `require`s the Node-only `web-worker` package
// for off-thread layout — bundlers (Next.js/Turbopack here) try to resolve that at build time
// and fail even though the code path never runs in a browser. `elk.bundled.js` is the
// self-contained browser/bundler-safe build (runs the algorithm synchronously, no worker).
import ELK from "elkjs/lib/elk.bundled.js";
import { NODE_HEIGHT, NODE_WIDTH } from "./dimensions";

interface LayoutElement {
  id: string;
}

interface LayoutEdge {
  source: string;
  target: string;
}

const elk = new ELK();

/**
 * Real ELK auto-layout (FR-CORE-02, T-P1.2-03) — replaces the earlier grid-placeholder. Builds a
 * flat (single-level, no nested containers) layered graph: every element is a direct child of an
 * implicit root, every edge (regardless of kind — Contains, Causes, Concerns, whatever the caller
 * passes) feeds ELK's layer assignment and edge routing. Every node uses the same fixed
 * `NODE_WIDTH`/`NODE_HEIGHT` — the actual `AxiomaBlockNode` footprint (see `dimensions.ts`), so
 * ELK's overlap-avoidance is computed against the same box that's actually on screen.
 */
export async function computeElkLayout(
  elements: LayoutElement[],
  edges: LayoutEdge[],
): Promise<Map<string, { x: number; y: number }>> {
  const elementIds = new Set(elements.map((el) => el.id));
  // ELK errors on an edge referencing an unknown node — guard against edges pointing at an
  // element outside the current set (e.g. a filtered view) rather than letting layout fail.
  const validEdges = edges.filter((e) => elementIds.has(e.source) && elementIds.has(e.target));

  // An element with no edge at all (an untraced Requirement, a freshly-created Hazard) forms its
  // own trivial single-node connected component — ELK's `layered` algorithm places these at the
  // whole layout's origin corner (confirmed directly: (12, 12) for a 220x92 node with the spacing
  // options below), which is also where the app's fixed top-left toolbar panel sits once the
  // canvas fits the graph into view. Relocated below, after ELK runs, rather than trying to steer
  // ELK's own placement for them.
  const connectedIds = new Set<string>();
  for (const edge of validEdges) {
    connectedIds.add(edge.source);
    connectedIds.add(edge.target);
  }
  const isolatedIds = elements.map((el) => el.id).filter((id) => !connectedIds.has(id));

  const graph = await elk.layout({
    id: "root",
    layoutOptions: {
      "elk.algorithm": "layered",
      "elk.direction": "DOWN",
      "elk.layered.spacing.nodeNodeBetweenLayers": "80",
      "elk.spacing.nodeNode": "40",
      // Tuned for T-P1.2-03's < 500ms budget at 500 nodes / 1,000 edges — the layered
      // algorithm's defaults (full crossing minimization, BRANDES_KOEPF node placement,
      // orthogonal routing) took ~1.4s at that scale in measurement. This combination measured
      // ~220-450ms warm and ~390ms on a genuinely cold first call, with zero node-node or
      // edge-through-node overlaps at the same scale (verified with a throwaway script — no
      // frontend test runner exists in this repo to commit this as a real test against).
      "elk.layered.thoroughness": "1",
      "elk.layered.crossingMinimization.strategy": "NONE",
      "elk.edgeRouting": "POLYLINE",
      "elk.layered.nodePlacement.strategy": "SIMPLE",
      "elk.layered.cycleBreaking.strategy": "GREEDY",
    },
    children: elements.map((el) => ({ id: el.id, width: NODE_WIDTH, height: NODE_HEIGHT })),
    edges: validEdges.map((e, index) => ({
      id: `e${index}`,
      sources: [e.source],
      targets: [e.target],
    })),
  });

  const positions = new Map<string, { x: number; y: number }>();
  for (const child of graph.children ?? []) {
    if (child.id && child.x !== undefined && child.y !== undefined) {
      positions.set(child.id, { x: child.x, y: child.y });
    }
  }

  if (isolatedIds.length > 0) {
    const connectedYs = [...positions.entries()]
      .filter(([id]) => connectedIds.has(id))
      .map(([, p]) => p.y);
    // No connected graph at all (e.g. a brand-new project) — 400 clears the toolbar panel's
    // rough footprint at a typical zoom level without depending on any connected node existing.
    const rowY = connectedYs.length > 0 ? Math.max(...connectedYs) + NODE_HEIGHT + 80 : 400;
    isolatedIds.forEach((id, index) => {
      positions.set(id, { x: 12 + index * (NODE_WIDTH + 40), y: rowY });
    });
  }

  return positions;
}
