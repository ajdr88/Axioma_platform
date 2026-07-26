interface LayoutElement {
  id: string;
}

interface LayoutEdge {
  source: string;
  target: string;
}

const COLUMN_WIDTH = 220;
const ROW_HEIGHT = 180;

/**
 * A generic layered layout — root-finding + BFS depth — standing in for real ELK auto-layout
 * (separately-scoped P1.2 work, impl §4.3). Roots are elements with no incoming edge (including
 * elements with no edges at all, e.g. a flat ReqIF-imported requirement), so nothing overlaps at
 * the origin. Elements at the same depth are spaced horizontally; depths are spaced vertically.
 */
export function computeGridLayout(
  elements: LayoutElement[],
  edges: LayoutEdge[],
): Map<string, { x: number; y: number }> {
  const childrenByParent = new Map<string, string[]>();
  const hasIncomingEdge = new Set<string>();

  for (const edge of edges) {
    hasIncomingEdge.add(edge.target);
    const children = childrenByParent.get(edge.source) ?? [];
    children.push(edge.target);
    childrenByParent.set(edge.source, children);
  }

  const roots = elements.filter((el) => !hasIncomingEdge.has(el.id));

  const depthById = new Map<string, number>();
  const queue: { id: string; depth: number }[] = roots.map((el) => ({ id: el.id, depth: 0 }));

  while (queue.length > 0) {
    const next = queue.shift();
    if (!next || depthById.has(next.id)) {
      continue;
    }
    const { id, depth } = next;
    depthById.set(id, depth);
    for (const childId of childrenByParent.get(id) ?? []) {
      queue.push({ id: childId, depth: depth + 1 });
    }
  }

  // Anything unreachable from a root (shouldn't happen given roots = "no incoming edge", but
  // guards against a malformed edge list referencing an unknown source) still gets placed.
  for (const el of elements) {
    if (!depthById.has(el.id)) {
      depthById.set(el.id, 0);
    }
  }

  const countByDepth = new Map<number, number>();
  const positions = new Map<string, { x: number; y: number }>();

  for (const el of elements) {
    const depth = depthById.get(el.id) ?? 0;
    const indexInRow = countByDepth.get(depth) ?? 0;
    countByDepth.set(depth, indexInRow + 1);
    positions.set(el.id, { x: indexInRow * COLUMN_WIDTH, y: depth * ROW_HEIGHT });
  }

  return positions;
}
