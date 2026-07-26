"use client";

import {
  type AxiomaBlockData,
  type AxiomaEdgeData,
  computeGridLayout,
  edgeTypes,
  nodeTypes,
} from "@axioma/diagram-engine";
import type { Edge as ApiEdge, Element as ApiElement } from "@axioma/shared-types";
import { Button, Panel as GlassPanel } from "@axioma/ui-components";
import {
  addEdge,
  Background,
  BackgroundVariant,
  type Connection,
  Controls,
  type Edge as FlowEdge,
  type Node as FlowNode,
  ReactFlow,
  ReactFlowProvider,
  reconnectEdge,
  useEdgesState,
  useNodesState,
} from "@xyflow/react";
import { useEffect, useState } from "react";
import { ElementInspector } from "@/components/ElementInspector";

type LoadStatus = "loading" | "error" | "ready";

interface PositionEntry {
  elementId: string;
  x: number;
  y: number;
}

function toFlowNode(
  element: ApiElement,
  position: { x: number; y: number },
): FlowNode<AxiomaBlockData> {
  return {
    id: element.id,
    type: "axiomaBlock",
    position,
    data: {
      label: element.name,
      // Placeholder — sysml_core::Element carries no provenance yet (FR-CORE-08 unimplemented).
      origin: "human",
      validation: "unverified",
      active: element.active,
      properties: [{ id: "kind", name: "kind", type: element.kind }],
    },
  };
}

/** Single source of truth for edge ids — used both for edges loaded from the API and ones
 * created live via `onConnect`/`onReconnect`, so an edge's id doesn't change across a reload. */
function getEdgeId({ source, target }: { source: string; target: string }): string {
  return `${source}-contains-${target}`;
}

function toFlowEdge(edge: ApiEdge): FlowEdge<AxiomaEdgeData> {
  return {
    id: getEdgeId(edge),
    source: edge.source,
    target: edge.target,
    type: "axiomaEdge",
    style: { stroke: "#7C7C86" },
    data: {},
  };
}

/** Reads an upstream error out of a proxy response — JSON `{error}` (the "API unreachable" 502)
 * or plain text (a rejected `ValidationError`/`BadRequest`, which apps/api returns as text). */
async function readErrorMessage(res: Response): Promise<string> {
  const text = await res.text();
  try {
    const parsed = JSON.parse(text);
    return typeof parsed?.error === "string" ? parsed.error : text;
  } catch {
    return text || `request failed with status ${res.status}`;
  }
}

export default function Home() {
  const [status, setStatus] = useState<LoadStatus>("loading");
  const [errorMessage, setErrorMessage] = useState("");
  const [notice, setNotice] = useState<string | null>(null);
  const [editMode, setEditMode] = useState(false);
  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null);

  const [nodes, setNodes, onNodesChange] = useNodesState<FlowNode<AxiomaBlockData>>([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState<FlowEdge<AxiomaEdgeData>>([]);

  useEffect(() => {
    let cancelled = false;

    async function load() {
      try {
        const [elementsRes, containsRes, positionsRes] = await Promise.all([
          fetch("/api/elements"),
          fetch("/api/contains"),
          fetch("/api/positions"),
        ]);
        for (const res of [elementsRes, containsRes, positionsRes]) {
          if (!res.ok) {
            throw new Error(await readErrorMessage(res));
          }
        }
        const elements: ApiElement[] = await elementsRes.json();
        const contains: ApiEdge[] = await containsRes.json();
        const positionEntries: PositionEntry[] = await positionsRes.json();
        if (cancelled) {
          return;
        }

        const storedPositions = new Map(
          positionEntries.map((p) => [p.elementId, { x: p.x, y: p.y }]),
        );
        const gridPositions = computeGridLayout(elements, contains);

        setNodes(
          elements.map((element) =>
            toFlowNode(
              element,
              storedPositions.get(element.id) ?? gridPositions.get(element.id) ?? { x: 0, y: 0 },
            ),
          ),
        );
        setEdges(contains.map(toFlowEdge));
        setStatus("ready");
      } catch (error) {
        if (!cancelled) {
          setErrorMessage(error instanceof Error ? error.message : "failed to load the model");
          setStatus("error");
        }
      }
    }

    load();
    return () => {
      cancelled = true;
    };
    // setNodes/setEdges (from useNodesState/useEdgesState) are stable across renders — safe to
    // list without turning this into a run-on-every-render effect.
  }, [setNodes, setEdges]);

  function showNotice(message: string) {
    setNotice(message);
    setTimeout(() => setNotice(null), 4000);
  }

  async function handleRename(nodeId: string, name: string) {
    const res = await fetch(`/api/elements/${nodeId}`, {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name }),
    });
    if (!res.ok) {
      showNotice(await readErrorMessage(res));
      return;
    }
    setNodes((nds) =>
      nds.map((n) => (n.id === nodeId ? { ...n, data: { ...n.data, label: name } } : n)),
    );
  }

  async function handleAddNode() {
    const res = await fetch("/api/elements", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name: "New Element" }),
    });
    if (!res.ok) {
      showNotice(await readErrorMessage(res));
      return;
    }
    const element: ApiElement = await res.json();
    // Placed near the origin with a little jitter so repeated adds don't stack exactly.
    const position = { x: 40 + Math.random() * 80, y: 40 + Math.random() * 80 };
    await fetch(`/api/elements/${element.id}/position`, {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(position),
    });
    const newNode = toFlowNode(element, position);
    newNode.data.autoFocusRename = true;
    setNodes((nds) => [...nds, newNode]);
  }

  async function handleDisconnect(source: string, target: string) {
    const res = await fetch("/api/contains", {
      method: "DELETE",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ parent: source, child: target }),
    });
    if (!res.ok) {
      showNotice(await readErrorMessage(res));
      return;
    }
    setEdges((eds) => eds.filter((e) => !(e.source === source && e.target === target)));
  }

  async function handleToggleActive(node: FlowNode<AxiomaBlockData>) {
    const nextActive = !(node.data.active ?? true);
    const res = await fetch(`/api/elements/${node.id}/active`, {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ active: nextActive }),
    });
    if (!res.ok) {
      showNotice(await readErrorMessage(res));
      return;
    }
    setNodes((nds) =>
      nds.map((n) => (n.id === node.id ? { ...n, data: { ...n.data, active: nextActive } } : n)),
    );
  }

  const selectedNode = nodes.find((n) => n.id === selectedNodeId) ?? null;

  const displayNodes = nodes.map((node) => ({
    ...node,
    data: {
      ...node.data,
      editable: editMode,
      onRename: (name: string) => handleRename(node.id, name),
    },
  }));

  const displayEdges = edges.map((edge) => ({
    ...edge,
    data: {
      ...edge.data,
      editable: editMode,
      onDisconnect: () => handleDisconnect(edge.source, edge.target),
    },
  }));

  return (
    <div className="h-screen w-screen">
      <ReactFlowProvider>
        <ReactFlow
          nodes={displayNodes}
          edges={displayEdges}
          nodeTypes={nodeTypes}
          edgeTypes={edgeTypes}
          onNodesChange={onNodesChange}
          onEdgesChange={onEdgesChange}
          nodesDraggable={editMode}
          nodesConnectable={editMode}
          edgesReconnectable={editMode}
          connectionRadius={40} // more forgiving than the 20px default — see the plan's Context.
          deleteKeyCode={null} // Node delete is out of scope — disconnect uses the edge's own button.
          onNodeDragStop={(_event, node) => {
            fetch(`/api/elements/${node.id}/position`, {
              method: "PATCH",
              headers: { "Content-Type": "application/json" },
              body: JSON.stringify({ x: node.position.x, y: node.position.y }),
            }).catch((error) => console.error("failed to save node position", error));
          }}
          onNodeClick={(_event, node) => setSelectedNodeId(node.id)}
          onPaneClick={() => setSelectedNodeId(null)}
          onConnect={async (connection: Connection) => {
            if (!connection.source || !connection.target) {
              return;
            }
            const res = await fetch("/api/contains", {
              method: "POST",
              headers: { "Content-Type": "application/json" },
              body: JSON.stringify({ parent: connection.source, child: connection.target }),
            });
            if (!res.ok) {
              showNotice(await readErrorMessage(res));
              return;
            }
            setEdges((eds) =>
              addEdge(
                { ...connection, type: "axiomaEdge", style: { stroke: "#7C7C86" } },
                eds,
                // Match toFlowEdge's id convention — addEdge defaults to its own "xy-edge__..."
                // format, which would otherwise make a freshly-connected edge's id inconsistent
                // with the same edge's id after a reload.
                { getEdgeId },
              ),
            );
          }}
          onReconnect={async (oldEdge, newConnection) => {
            if (!newConnection.source || !newConnection.target) {
              return;
            }
            // Create the new edge first, through the same validated path a fresh connect uses —
            // a reconnect that would cycle is rejected exactly like one, leaving the old edge
            // untouched rather than half-changed.
            const createRes = await fetch("/api/contains", {
              method: "POST",
              headers: { "Content-Type": "application/json" },
              body: JSON.stringify({ parent: newConnection.source, child: newConnection.target }),
            });
            if (!createRes.ok) {
              showNotice(await readErrorMessage(createRes));
              return;
            }
            await fetch("/api/contains", {
              method: "DELETE",
              headers: { "Content-Type": "application/json" },
              body: JSON.stringify({ parent: oldEdge.source, child: oldEdge.target }),
            });
            setEdges((eds) => reconnectEdge(oldEdge, newConnection, eds, { getEdgeId }));
          }}
          fitView
          proOptions={{ hideAttribution: true }}
        >
          <Background variant={BackgroundVariant.Dots} color="#7C7C86" gap={24} size={1} />
          <Controls className="!bg-obsidian/80 !border-white/10 [&_button]:!bg-transparent [&_button]:!fill-white/70 [&_button]:!border-white/10" />

          <GlassPanel className="absolute left-4 top-4 z-10 w-64 px-4 py-3">
            <p className="text-sm font-semibold tracking-[0.2em] text-white/90">AXIOMA</p>
            {status === "loading" && (
              <p className="mt-0.5 font-mono text-[10px] uppercase tracking-widest text-white/40">
                Loading model…
              </p>
            )}
            {status === "error" && (
              <p className="mt-0.5 font-mono text-[10px] uppercase tracking-widest text-alert">
                {errorMessage}
              </p>
            )}
            {status === "ready" && (
              <p className="mt-0.5 font-mono text-[10px] uppercase tracking-widest text-white/40">
                {nodes.length} elements &middot; {edges.length} containment edges
              </p>
            )}
            {notice && <p className="mt-2 text-xs text-alert">{notice}</p>}

            <div className="mt-3 flex flex-wrap gap-1.5">
              <Button
                variant={editMode ? "primary" : "ghost"}
                onClick={() => setEditMode((v) => !v)}
                className="!px-2 !py-1 text-xs"
              >
                Edit Mode: {editMode ? "On" : "Off"}
              </Button>
              {editMode && (
                <Button variant="ghost" onClick={handleAddNode} className="!px-2 !py-1 text-xs">
                  + Add Node
                </Button>
              )}
              {editMode && selectedNode && (
                <Button
                  variant="ghost"
                  onClick={() => handleToggleActive(selectedNode)}
                  className="!px-2 !py-1 text-xs"
                >
                  {(selectedNode.data.active ?? true) ? "Deactivate" : "Reactivate"}
                </Button>
              )}
            </div>
          </GlassPanel>

          {selectedNode && (
            <ElementInspector
              elementId={selectedNode.id}
              elementLabel={selectedNode.data.label}
              onClose={() => setSelectedNodeId(null)}
            />
          )}
        </ReactFlow>
      </ReactFlowProvider>
    </div>
  );
}
