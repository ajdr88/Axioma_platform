import type { NodeProps } from "@xyflow/react";

export interface SubsystemClusterData extends Record<string, unknown> {
  /** The subsystem's own label (its Contains-parent's name). */
  label: string;
  /** Descendant count collapsed into this one node (NFR-PERF-01, T-P1.2-02). */
  count: number;
  /** Pans/zooms the viewport to this subsystem's bounding box — bringing it on-screen makes the
   * next viewport-intersection check in `computeClusteredNodes` un-cluster it back into its real
   * descendant nodes. */
  onExpand?: () => void;
}

/**
 * Stands in for an entire off-screen subsystem subtree (NFR-PERF-01, T-P1.2-02) — one of these
 * replaces however many real `AxiomaBlockNode`s that subtree contains in the array actually
 * passed to `<ReactFlow>`, which is what keeps the render-node count far below the total model
 * size. No `Handle`s: edges to/from a clustered-away node are hidden entirely (`page.tsx`'s
 * existing endpoint-visibility filter already does this), not rerouted to the cluster.
 */
export function SubsystemClusterNode({ data }: NodeProps & { data: SubsystemClusterData }) {
  return (
    <button
      type="button"
      onClick={() => data.onExpand?.()}
      title={`${data.label} — ${data.count} elements collapsed, off-screen. Click to expand.`}
      className="flex h-full w-full flex-col items-center justify-center gap-1 rounded-xl border-2 border-dashed border-graphite/60 bg-obsidian/50 p-3 text-center backdrop-blur-md transition-colors hover:border-cobalt-glow/60"
    >
      <p className="truncate text-xs font-semibold text-white/70">{data.label}</p>
      <p className="font-mono text-[10px] uppercase tracking-widest text-graphite">
        {data.count.toLocaleString()} elements
      </p>
    </button>
  );
}
