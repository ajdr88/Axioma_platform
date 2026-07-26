import { Handle, type NodeProps, Position } from "@xyflow/react";
import { useState } from "react";

export type Origin = "human" | "ai-suggested" | "ai-auto-merged";
export type ValidationState = "unverified" | "solver-validated" | "test-validated";

export interface AxiomaBlockProperty {
  id: string;
  name: string;
  type: string;
}

export interface AxiomaBlockData extends Record<string, unknown> {
  label: string;
  origin: Origin;
  validation: ValidationState;
  suspect?: boolean;
  /** Excluded from *future* system-optimization loops when false — keeps all its data, just
   * visually marked. Defaults to `true` (most callers, e.g. imported data, don't set it). */
  active?: boolean;
  /** Whether Edit Mode is on — gates the double-click-to-rename interaction below. */
  editable?: boolean;
  onRename?: (name: string) => void;
  /** Starts this node straight in rename mode (canvas "Add Node") — read once, as the initial
   * state below; toggling it after mount has no further effect. */
  autoFocusRename?: boolean;
  properties: AxiomaBlockProperty[];
}

const originBorder: Record<Origin, string> = {
  human: "border-white/10",
  "ai-suggested": "border-dashed border-cobalt-glow/40",
  "ai-auto-merged": "border-dashed border-cobalt-glow/70 shadow-cobalt-glow/20",
};

/**
 * Custom React Flow node rendering the three orthogonal provenance signals from impl §6.3:
 * Origin (border style), Validation (corner badge), Staleness (Suspect pulse). Ported from the
 * component example in docs/Axioma_implementation_v3.md §6.4. Also renders the `active` flag
 * (dimmed + tagged when deactivated) and an inline rename input, gated behind `data.editable`.
 */
export function AxiomaBlockNode({ data }: NodeProps & { data: AxiomaBlockData }) {
  const [isRenaming, setIsRenaming] = useState(data.autoFocusRename ?? false);
  const [draftName, setDraftName] = useState(data.label);
  const isActive = data.active ?? true;

  function startRename() {
    if (!data.editable) {
      return;
    }
    setDraftName(data.label);
    setIsRenaming(true);
  }

  function commitRename() {
    setIsRenaming(false);
    const trimmed = draftName.trim();
    if (trimmed && trimmed !== data.label) {
      data.onRename?.(trimmed);
    }
  }

  function cancelRename() {
    setIsRenaming(false);
    setDraftName(data.label);
  }

  // Enlarged past React Flow's tiny default hit target, with an explicit hover/cursor affordance
  // — only when Edit Mode is on, matching every other interaction this node gates. Otherwise
  // connecting isn't possible at all (`nodesConnectable={editMode}` on the canvas), so the handle
  // reads as inert rather than inviting a drag that won't do anything.
  const handleClassName = data.editable
    ? "!h-3.5 !w-3.5 !border-2 !border-obsidian !bg-cobalt-glow !cursor-crosshair transition-transform hover:!scale-125 hover:!bg-white"
    : "!h-2 !w-2 !border-2 !border-obsidian !bg-graphite !cursor-default";

  return (
    <div
      className={`rounded-xl border bg-obsidian/80 p-0 shadow-2xl backdrop-blur-md ${originBorder[data.origin]} ${isActive ? "" : "opacity-50"}`}
    >
      <Handle type="target" position={Position.Top} className={handleClassName} />

      <div className="flex items-center gap-2 rounded-t-xl border-b border-white/5 bg-cobalt-glow/10 p-3">
        <span className="h-2 w-2 rounded-full bg-cobalt-glow" aria-hidden />
        {isRenaming ? (
          <input
            // biome-ignore lint/a11y/noAutofocus: focuses an input a user action just revealed, not page-load autofocus
            autoFocus
            value={draftName}
            onChange={(event) => setDraftName(event.target.value)}
            onFocus={(event) => event.target.select()}
            onBlur={commitRename}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                commitRename();
              } else if (event.key === "Escape") {
                cancelRename();
              }
            }}
            onMouseDown={(event) => event.stopPropagation()}
            onClick={(event) => event.stopPropagation()}
            className="w-full rounded border border-cobalt-glow/40 bg-obsidian px-1 text-sm font-semibold text-white/90 outline-none"
          />
        ) : (
          <button
            type="button"
            className="cursor-default bg-transparent p-0 text-left text-sm font-semibold text-white/90"
            onDoubleClick={(event) => {
              event.stopPropagation();
              startRename();
            }}
          >
            {data.label}
          </button>
        )}
        {!isActive && (
          <span
            className="text-[9px] uppercase tracking-widest text-graphite"
            title="Deactivated — excluded from future optimization loops, data preserved"
          >
            Deactivated
          </span>
        )}
        {data.validation !== "unverified" && (
          <span className="ml-auto text-xs text-cobalt-glow" title={data.validation}>
            &#10003;
          </span>
        )}
        {data.suspect && (
          <span className="ml-auto h-2 w-2 animate-pulse rounded-full bg-alert" title="Suspect" />
        )}
      </div>

      <div className="space-y-1 p-3">
        <p className="mb-1 text-[10px] uppercase tracking-widest text-white/40">Properties</p>
        {data.properties.map((p) => (
          <div key={p.id} className="font-mono text-xs text-white/70">
            {p.name}: <span className="text-graphite">{p.type}</span>
          </div>
        ))}
      </div>

      <Handle type="source" position={Position.Bottom} className={handleClassName} />
    </div>
  );
}
