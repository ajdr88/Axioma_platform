"use client";

import {
  type AxiomaBlockData,
  type AxiomaEdgeData,
  computeElkLayout,
  edgeTypes,
  NODE_HEIGHT,
  NODE_WIDTH,
  nodeTypes,
} from "@axioma/diagram-engine";
import type { Edge as ApiEdge, Element as ApiElement } from "@axioma/shared-types";
import {
  Background,
  BackgroundVariant,
  type Edge as FlowEdge,
  type Node as FlowNode,
  ReactFlow,
  ReactFlowProvider,
  useReactFlow,
} from "@xyflow/react";
import { useEffect, useState } from "react";

function toFlowNode(
  element: ApiElement,
  position: { x: number; y: number },
): FlowNode<AxiomaBlockData> {
  return {
    id: element.id,
    type: "axiomaBlock",
    position,
    width: NODE_WIDTH,
    height: NODE_HEIGHT,
    data: {
      label: element.name,
      kind: element.kind,
      origin: element.origin,
      validation: "unverified",
      active: element.active,
      properties: [],
    },
  };
}

function toFlowEdge(edge: ApiEdge): FlowEdge<AxiomaEdgeData> {
  return {
    id: `${edge.source}-contains-${edge.target}`,
    source: edge.source,
    target: edge.target,
    type: "axiomaEdge",
    style: { stroke: "#7C7C86" },
    data: {},
  };
}

/**
 * Scope-downs pass (FR-EXPORT-01, the server-side half) — the internal route a headless Chromium
 * driver (`apps/web/scripts/render-full-diagram.mjs`) navigates to. Deliberately minimal: no
 * toolbar/panels, no clustering (every element renders regardless of count — the whole point of
 * this route versus the existing client-side viewport-only "Export PNG"), no auth guard. This app
 * has no auth system anywhere else either (confirmed throughout this whole session) — a real,
 * pre-existing property of the app, not a gap introduced here; this route exposes nothing a direct
 * `/api/projects/:id/elements` call doesn't already.
 *
 * Split into an outer `FullDiagramExportPage` (owns state) + inner `DiagramCanvas` (calls
 * `useReactFlow`) for the same reason `page.tsx`'s `Home`/`Canvas` split exists: `useReactFlow`
 * requires being a descendant of `ReactFlowProvider`. **A real bug found via live verification,
 * not assumed away**: the declarative `<ReactFlow fitView>` prop only fits once, on ReactFlow's
 * own initial mount — which happens immediately with an empty `nodes` array (the fetch/layout
 * data hasn't loaded yet), so it fit an empty view and never re-fit once real nodes arrived,
 * clipping node edges out of the screenshot. Fixed the same way Swimlane View's own `fitView`
 * timing bug was fixed earlier this session: an imperative `reactFlowInstance.fitView()` call in
 * a `useEffect` keyed on the real node count, not the declarative prop.
 */
export default function FullDiagramExportPage({
  params,
}: {
  params: Promise<{ projectId: string }>;
}) {
  const [projectId, setProjectId] = useState<string | null>(null);
  const [nodes, setNodes] = useState<FlowNode<AxiomaBlockData>[]>([]);
  const [edges, setEdges] = useState<FlowEdge<AxiomaEdgeData>[]>([]);

  useEffect(() => {
    params.then((p) => setProjectId(p.projectId));
  }, [params]);

  useEffect(() => {
    if (!projectId) {
      return;
    }
    let cancelled = false;
    async function load() {
      const [elementsRes, containsRes] = await Promise.all([
        fetch(`/api/projects/${projectId}/elements`),
        fetch(`/api/projects/${projectId}/contains`),
      ]);
      if (!elementsRes.ok || !containsRes.ok || cancelled) {
        return;
      }
      const elements: ApiElement[] = await elementsRes.json();
      const contains: ApiEdge[] = await containsRes.json();
      const positions = await computeElkLayout(elements, contains);
      if (cancelled) {
        return;
      }
      setNodes(elements.map((el) => toFlowNode(el, positions.get(el.id) ?? { x: 0, y: 0 })));
      setEdges(contains.map(toFlowEdge));
    }
    load();
    return () => {
      cancelled = true;
    };
  }, [projectId]);

  return (
    <ReactFlowProvider>
      <DiagramCanvas nodes={nodes} edges={edges} />
    </ReactFlowProvider>
  );
}

function DiagramCanvas({
  nodes,
  edges,
}: {
  nodes: FlowNode<AxiomaBlockData>[];
  edges: FlowEdge<AxiomaEdgeData>[];
}) {
  const reactFlowInstance = useReactFlow<FlowNode<AxiomaBlockData>, FlowEdge<AxiomaEdgeData>>();
  const [ready, setReady] = useState(false);

  useEffect(() => {
    if (nodes.length === 0) {
      return;
    }
    // A `setTimeout(0)` still queues after React's own commit + ReactFlow's internal node
    // measurement pass, giving `fitView` real dimensions to fit against — the exact fix already
    // proven for Swimlane View's identical timing bug.
    const id = setTimeout(() => {
      reactFlowInstance.fitView({ padding: 0.1, duration: 0 });
      setReady(true);
    }, 50);
    return () => clearTimeout(id);
  }, [nodes, reactFlowInstance]);

  return (
    <div className="h-screen w-screen" data-diagram-ready={ready ? "true" : "false"}>
      <ReactFlow
        nodes={nodes}
        edges={edges}
        nodeTypes={nodeTypes}
        edgeTypes={edgeTypes}
        nodesDraggable={false}
        nodesConnectable={false}
        edgesReconnectable={false}
        minZoom={0.01}
      >
        <Background variant={BackgroundVariant.Dots} color="#7C7C86" gap={24} />
      </ReactFlow>
    </div>
  );
}
