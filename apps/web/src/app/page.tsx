"use client";

import { type AxiomaBlockData, computeGridLayout, nodeTypes } from "@axioma/diagram-engine";
import type { Edge as ApiEdge, Element as ApiElement } from "@axioma/shared-types";
import { Panel as GlassPanel } from "@axioma/ui-components";
import {
  Background,
  BackgroundVariant,
  Controls,
  type Edge as FlowEdge,
  type Node as FlowNode,
  ReactFlow,
  ReactFlowProvider,
} from "@xyflow/react";
import { useEffect, useState } from "react";

type LoadState =
  | { status: "loading" }
  | { status: "error"; message: string }
  | { status: "ready"; nodes: FlowNode<AxiomaBlockData>[]; edges: FlowEdge[] };

/**
 * Maps the real API response into React Flow data. `origin`/`validation` are placeholders —
 * `sysml_core::Element` carries no provenance yet (FR-CORE-08 is unimplemented) — not real data.
 */
function toFlowGraph(elements: ApiElement[], contains: ApiEdge[]): LoadState {
  const positions = computeGridLayout(elements, contains);

  const nodes: FlowNode<AxiomaBlockData>[] = elements.map((element) => {
    const position = positions.get(element.id) ?? { x: 0, y: 0 };
    return {
      id: element.id,
      type: "axiomaBlock",
      position,
      data: {
        label: element.name,
        origin: "human",
        validation: "unverified",
        properties: [{ id: "kind", name: "kind", type: element.kind }],
      },
    };
  });

  const edges: FlowEdge[] = contains.map((edge) => ({
    id: `${edge.source}-contains-${edge.target}`,
    source: edge.source,
    target: edge.target,
    type: "smoothstep",
    style: { stroke: "#7C7C86" },
  }));

  return { status: "ready", nodes, edges };
}

export default function Home() {
  const [state, setState] = useState<LoadState>({ status: "loading" });

  useEffect(() => {
    let cancelled = false;

    async function load() {
      try {
        const [elementsRes, containsRes] = await Promise.all([
          fetch("/api/elements"),
          fetch("/api/contains"),
        ]);
        if (!elementsRes.ok || !containsRes.ok) {
          const failed = !elementsRes.ok ? elementsRes : containsRes;
          const body = await failed.json().catch(() => null);
          throw new Error(body?.error ?? `request failed with status ${failed.status}`);
        }
        const elements: ApiElement[] = await elementsRes.json();
        const contains: ApiEdge[] = await containsRes.json();
        if (!cancelled) {
          setState(toFlowGraph(elements, contains));
        }
      } catch (error) {
        if (!cancelled) {
          setState({
            status: "error",
            message: error instanceof Error ? error.message : "failed to load the model",
          });
        }
      }
    }

    load();
    return () => {
      cancelled = true;
    };
  }, []);

  const nodes = state.status === "ready" ? state.nodes : [];
  const edges = state.status === "ready" ? state.edges : [];

  return (
    <div className="h-screen w-screen">
      <ReactFlowProvider>
        <ReactFlow
          nodes={nodes}
          edges={edges}
          nodeTypes={nodeTypes}
          fitView
          proOptions={{ hideAttribution: true }}
        >
          <Background variant={BackgroundVariant.Dots} color="#7C7C86" gap={24} size={1} />
          <Controls className="!bg-obsidian/80 !border-white/10 [&_button]:!bg-transparent [&_button]:!fill-white/70 [&_button]:!border-white/10" />

          <GlassPanel className="absolute left-4 top-4 z-10 px-4 py-3">
            <p className="text-sm font-semibold tracking-[0.2em] text-white/90">AXIOMA</p>
            {state.status === "loading" && (
              <p className="mt-0.5 font-mono text-[10px] uppercase tracking-widest text-white/40">
                Loading model…
              </p>
            )}
            {state.status === "error" && (
              <p className="mt-0.5 max-w-xs font-mono text-[10px] uppercase tracking-widest text-alert">
                {state.message}
              </p>
            )}
            {state.status === "ready" && (
              <p className="mt-0.5 font-mono text-[10px] uppercase tracking-widest text-white/40">
                {state.nodes.length} elements &middot; {state.edges.length} containment edges
              </p>
            )}
          </GlassPanel>
        </ReactFlow>
      </ReactFlowProvider>
    </div>
  );
}
