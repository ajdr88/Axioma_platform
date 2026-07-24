import { Handle, type NodeProps, Position } from "@xyflow/react";

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
 * component example in docs/Axioma_implementation_v3.md §6.4.
 */
export function AxiomaBlockNode({ data }: NodeProps & { data: AxiomaBlockData }) {
  return (
    <div
      className={`rounded-xl border bg-obsidian/80 p-0 shadow-2xl backdrop-blur-md ${originBorder[data.origin]}`}
    >
      <Handle type="target" position={Position.Top} className="!bg-graphite" />

      <div className="flex items-center gap-2 rounded-t-xl border-b border-white/5 bg-cobalt-glow/10 p-3">
        <span className="h-2 w-2 rounded-full bg-cobalt-glow" aria-hidden />
        <span className="text-sm font-semibold text-white/90">{data.label}</span>
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

      <Handle type="source" position={Position.Bottom} className="!bg-graphite" />
    </div>
  );
}
