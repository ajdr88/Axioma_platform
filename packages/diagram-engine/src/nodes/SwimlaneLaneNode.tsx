import type { NodeProps } from "@xyflow/react";

export interface SwimlaneLaneData extends Record<string, unknown> {
  /** The lane owner's own name, or "Unallocated" for the catch-all lane (FR-CORE-12's own text
   * never says allocation is mandatory, so unallocated content is a real, visible lane, not
   * silently dropped from the swimlane view). */
  label: string;
}

/**
 * docs/IMPLEMENTATION_KICKOFF.md Phase 5 (FR-CORE-12) — a swimlane's own header/boundary box.
 * Serves as the real React Flow `parentId` target for its member `AxiomaBlockNode`s (the actual
 * nested-child primitive this feature reuses is `parentId`/`extent: "parent"`, confirmed real and
 * already shipped in the pinned `@xyflow/react` version — this component is just this feature's
 * own labeled header rendering for that parent node, not a reimplementation of the primitive
 * itself).
 */
export function SwimlaneLaneNode({ data }: NodeProps & { data: SwimlaneLaneData }) {
  return (
    <div className="h-full w-full rounded-lg border border-white/10 bg-white/[0.02]">
      <div className="sticky top-0 truncate rounded-t-lg border-b border-white/10 bg-white/5 px-3 py-2 text-xs font-semibold uppercase tracking-widest text-white/60">
        {data.label}
      </div>
    </div>
  );
}
