import { BaseEdge, EdgeLabelRenderer, type EdgeProps, getSmoothStepPath } from "@xyflow/react";

export interface AxiomaEdgeData extends Record<string, unknown> {
  /** Whether Edit Mode is on — gates the hover disconnect button below. */
  editable?: boolean;
  onDisconnect?: () => void;
}

/**
 * `Contains` edge with a disconnect control at its midpoint — the standard React Flow
 * "edge with a button" pattern (`BaseEdge` + `EdgeLabelRenderer`). Visible only in Edit Mode,
 * matching every other canvas interaction. `EdgeLabelRenderer`'s container is
 * `pointer-events: none` by default (so it doesn't block panning elsewhere on the canvas) — the
 * button opts back in with an explicit `pointerEvents: "auto"`, otherwise clicks fall through to
 * the edge path underneath and the button is visually present but unclickable.
 */
export function AxiomaEdge({
  id,
  sourceX,
  sourceY,
  targetX,
  targetY,
  sourcePosition,
  targetPosition,
  style,
  data,
}: EdgeProps & { data?: AxiomaEdgeData }) {
  const [edgePath, labelX, labelY] = getSmoothStepPath({
    sourceX,
    sourceY,
    sourcePosition,
    targetX,
    targetY,
    targetPosition,
  });

  return (
    <>
      <BaseEdge id={id} path={edgePath} style={style} />
      {data?.editable && (
        <EdgeLabelRenderer>
          <button
            type="button"
            data-edge-id={id}
            style={{
              position: "absolute",
              pointerEvents: "auto",
              // Above nodes (`.react-flow__nodes` renders after `.react-flow__edgelabel-renderer`
              // in the DOM, so without this any node card overlapping the button's position —
              // common for an edge between two non-adjacent nodes — silently steals the click).
              zIndex: 1000,
              transform: `translate(-50%, -50%) translate(${labelX}px, ${labelY}px)`,
            }}
            className="nodrag nopan flex h-5 w-5 items-center justify-center rounded-full border border-white/10 bg-obsidian text-xs text-white/60 shadow transition-colors hover:border-alert hover:text-alert"
            onClick={(event) => {
              event.stopPropagation();
              data.onDisconnect?.();
            }}
            title="Disconnect"
          >
            &times;
          </button>
        </EdgeLabelRenderer>
      )}
    </>
  );
}
