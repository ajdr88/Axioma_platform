import type { NodeKind, Origin } from "@axioma/shared-types";
import { Handle, type NodeProps, Position } from "@xyflow/react";
import { useEffect, useState } from "react";

export type { Origin };
export type ValidationState = "unverified" | "solver-validated" | "test-validated";

export interface AxiomaBlockProperty {
  id: string;
  name: string;
  type: string;
}

export interface AxiomaBlockData extends Record<string, unknown> {
  label: string;
  /** Discriminates Hazard/Control accent styling below from every other kind (rendered plain). */
  kind: NodeKind;
  origin: Origin;
  validation: ValidationState;
  suspect?: boolean;
  /** Excluded from *future* system-optimization loops when false — keeps all its data, just
   * visually marked. Defaults to `true` (most callers, e.g. imported data, don't set it). */
  active?: boolean;
  /** True when a `Hazard` `causes`-edge targets this (Structure) node (FR-SAFE-01,
   * T-P1.2-04's "the Turbine block shows a hazard indicator"). */
  hasHazard?: boolean;
  /** Whether Edit Mode is on — gates the double-click-to-rename interaction below. */
  editable?: boolean;
  onRename?: (name: string) => void;
  /** Starts this node straight in rename mode (canvas "Add Node") — consumed once, on mount (see
   * the effect below), which immediately reports back via `onAutoFocusRenameConsumed` so the
   * caller can clear it from this node's persisted data. Without that, a *later* remount of this
   * same node (e.g. it gets filtered out of the canvas and back in) would re-enter rename mode
   * from the still-`true` flag — a real bug this project hit once the provenance-origin filter
   * could cause exactly that remount. */
  autoFocusRename?: boolean;
  onAutoFocusRenameConsumed?: () => void;
  properties: AxiomaBlockProperty[];
}

const originBorder: Record<Origin, string> = {
  Human: "border-white/10",
  AiSuggested: "border-dashed border-cobalt-glow/40",
  AiAutoMerged: "border-dashed border-cobalt-glow/70 shadow-cobalt-glow/20",
};

/** Header-dot/header-band accent per `kind` — every other kind (Structure, Requirement, etc.)
 * falls back to the default cobalt-glow treatment already in place before kind-awareness existed. */
const kindAccent: Partial<Record<NodeKind, { dot: string; band: string }>> = {
  Hazard: { dot: "bg-alert", band: "bg-alert/10" },
  Control: { dot: "bg-cobalt-glow", band: "bg-cobalt-glow/20" },
  // docs/IMPLEMENTATION_KICKOFF.md Phase 5 (turbofan amendment §3.5's ADSG canvas gap) — these
  // three kinds (Phase 4's first-ever real content) previously rendered identically to a plain
  // Structure. Deliberately reusing this existing accent extensibility point rather than three
  // new bespoke node components (see this feature's plan) — a distinct color plus `kindGlyph`
  // below is enough to make them recognizable at a glance without duplicating this whole card's
  // provenance/validation/rename chrome three times over.
  Function: { dot: "bg-[#B98CE8]", band: "bg-[#B98CE8]/10" },
  SelectionChoice: { dot: "bg-[#E8A93A]", band: "bg-[#E8A93A]/10" },
  ConnectionChoice: { dot: "bg-[#3AC7E8]", band: "bg-[#3AC7E8]/10" },
};

/** A single-character shape-cue next to the header dot, rendered in the same slot `hasHazard`'s
 * badge already uses — so these three kinds are distinguishable by more than color alone (color
 * alone isn't accessible/distinct enough at small canvas zoom levels). */
const kindGlyph: Partial<Record<NodeKind, string>> = {
  Function: "ƒ",
  SelectionChoice: "◈",
  ConnectionChoice: "⇄",
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

  // Intentionally mount-only: consumes the one-shot `autoFocusRename` flag exactly once per
  // mount, not on every data change.
  // biome-ignore lint/correctness/useExhaustiveDependencies: see comment above
  useEffect(() => {
    if (data.autoFocusRename) {
      data.onAutoFocusRenameConsumed?.();
    }
  }, []);

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
  const accent = kindAccent[data.kind];
  const glyph = kindGlyph[data.kind];

  return (
    <div
      className={`h-full w-full overflow-hidden rounded-xl border bg-obsidian/80 p-0 shadow-2xl backdrop-blur-md ${originBorder[data.origin]} ${isActive ? "" : "opacity-50"}`}
    >
      <Handle type="target" position={Position.Top} className={handleClassName} />

      <div
        className={`flex items-center gap-2 rounded-t-xl border-b border-white/5 p-3 ${accent?.band ?? "bg-cobalt-glow/10"}`}
      >
        <span
          className={`h-2 w-2 flex-shrink-0 rounded-full ${accent?.dot ?? "bg-cobalt-glow"}`}
          aria-hidden
        />
        {data.hasHazard && (
          <span
            className="h-2 w-2 rounded-full bg-alert"
            title="Linked Hazard — see the Hazard/Risk panel"
          />
        )}
        {glyph && (
          <span
            className="flex-shrink-0 text-xs leading-none text-white/60"
            title={`${data.kind} (FR-ARCH)`}
            aria-hidden
          >
            {glyph}
          </span>
        )}
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
            className="min-w-0 flex-1 rounded border border-cobalt-glow/40 bg-obsidian px-1 text-sm font-semibold text-white/90 outline-none"
          />
        ) : (
          <button
            type="button"
            title={data.label}
            className="min-w-0 flex-1 truncate cursor-default bg-transparent p-0 text-left text-sm font-semibold text-white/90"
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
