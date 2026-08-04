"use client";

import {
  type AxiomaBlockData,
  type AxiomaEdgeData,
  computeGridLayout,
  edgeTypes,
  nodeTypes,
} from "@axioma/diagram-engine";
import type {
  Edge as ApiEdge,
  Element as ApiElement,
  NodeKind,
  Origin,
} from "@axioma/shared-types";
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
import { HazardRiskPanel } from "@/components/HazardRiskPanel";
import { MissionPlanningPanel } from "@/components/MissionPlanningPanel";

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
      kind: element.kind,
      origin: element.origin,
      // Placeholder — `validation`/`suspect` have no real trigger yet (solver/test runs,
      // staleness propagation); FR-CORE-08's `origin` signal is real, these two aren't yet.
      validation: "unverified",
      active: element.active,
      properties: [],
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
  const [showHazardPanel, setShowHazardPanel] = useState(false);
  const [showMissionPanel, setShowMissionPanel] = useState(false);
  /** FR-CORE-08 / T-P1.2-06's "AI-suggested only" filter — "all" shows every origin. */
  const [originFilter, setOriginFilter] = useState<Origin | "all">("all");

  const [nodes, setNodes, onNodesChange] = useNodesState<FlowNode<AxiomaBlockData>>([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState<FlowEdge<AxiomaEdgeData>>([]);
  /** source=Structure, target=Hazard (FR-SAFE-01) — read by the hazard-indicator badge and the
   * Hazard/Risk panel. */
  const [causesEdges, setCausesEdges] = useState<ApiEdge[]>([]);
  /** source=Hazard, target=Control (FR-SAFE-01/03) — read by the Hazard/Risk panel. */
  const [mitigatedByEdges, setMitigatedByEdges] = useState<ApiEdge[]>([]);
  /** source=Stakeholder, target=Mission or Requirement (FR-MSN-02) — read by the Mission
   * Planning panel. */
  const [concernsEdges, setConcernsEdges] = useState<ApiEdge[]>([]);

  useEffect(() => {
    let cancelled = false;

    async function load() {
      try {
        const [elementsRes, containsRes, positionsRes, causesRes, mitigatedByRes, concernsRes] =
          await Promise.all([
            fetch("/api/elements"),
            fetch("/api/contains"),
            fetch("/api/positions"),
            fetch("/api/edges?kind=Causes"),
            fetch("/api/edges?kind=MitigatedBy"),
            fetch("/api/edges?kind=Concerns"),
          ]);
        for (const res of [
          elementsRes,
          containsRes,
          positionsRes,
          causesRes,
          mitigatedByRes,
          concernsRes,
        ]) {
          if (!res.ok) {
            throw new Error(await readErrorMessage(res));
          }
        }
        const elements: ApiElement[] = await elementsRes.json();
        const contains: ApiEdge[] = await containsRes.json();
        const positionEntries: PositionEntry[] = await positionsRes.json();
        const causes: ApiEdge[] = await causesRes.json();
        const mitigatedBy: ApiEdge[] = await mitigatedByRes.json();
        const concerns: ApiEdge[] = await concernsRes.json();
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
        setCausesEdges(causes);
        setMitigatedByEdges(mitigatedBy);
        setConcernsEdges(concerns);
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

  /** Shared by "+ Add Node" and the Hazard/Risk panel's Hazard/Control creation — POSTs the
   * element, gives it a jittered starting position (so repeated adds don't stack exactly), and
   * adds it to canvas state. Returns `null` (after showing a notice) on failure. */
  async function createElement(
    name: string,
    kind: NodeKind,
  ): Promise<FlowNode<AxiomaBlockData> | null> {
    const res = await fetch("/api/elements", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name, kind }),
    });
    if (!res.ok) {
      showNotice(await readErrorMessage(res));
      return null;
    }
    const element: ApiElement = await res.json();
    const position = { x: 40 + Math.random() * 80, y: 40 + Math.random() * 80 };
    await fetch(`/api/elements/${element.id}/position`, {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(position),
    });
    const newNode = toFlowNode(element, position);
    setNodes((nds) => [...nds, newNode]);
    return newNode;
  }

  async function handleAddNode() {
    const newNode = await createElement("New Element", "Structure");
    if (newNode) {
      setNodes((nds) =>
        nds.map((n) =>
          n.id === newNode.id ? { ...n, data: { ...n.data, autoFocusRename: true } } : n,
        ),
      );
    }
  }

  /** Hazard/Risk panel "+ Add Hazard" — creates the Hazard, then links it to its causing
   * subsystem via a validated `Causes` edge (FR-SAFE-01). */
  async function handleCreateHazard(name: string, causingStructureId: string) {
    const newNode = await createElement(name, "Hazard");
    if (!newNode) {
      return;
    }
    const res = await fetch("/api/edges", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ source: causingStructureId, target: newNode.id, kind: "Causes" }),
    });
    if (!res.ok) {
      showNotice(await readErrorMessage(res));
      return;
    }
    setCausesEdges((eds) => [
      ...eds,
      { source: causingStructureId, target: newNode.id, kind: "Causes" },
    ]);
  }

  /** Hazard/Risk panel "+ Control" — creates the Control, then links it to the hazard it
   * mitigates via a validated `MitigatedBy` edge (FR-SAFE-03). */
  async function handleCreateControl(hazardId: string, name: string) {
    const newNode = await createElement(name, "Control");
    if (!newNode) {
      return;
    }
    const res = await fetch("/api/edges", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ source: hazardId, target: newNode.id, kind: "MitigatedBy" }),
    });
    if (!res.ok) {
      showNotice(await readErrorMessage(res));
      return;
    }
    setMitigatedByEdges((eds) => [
      ...eds,
      { source: hazardId, target: newNode.id, kind: "MitigatedBy" },
    ]);
  }

  /** Mission Planning panel "+ Mission" — no links needed at creation time (FR-MSN-01). */
  async function handleCreateMission(name: string) {
    await createElement(name, "Mission");
  }

  /** Mission Planning panel "+ Stakeholder" — creates the Stakeholder, saves its concern text,
   * then links it to the chosen Mission and Requirement via validated `Concerns` edges
   * (FR-MSN-02), traversable from either end. */
  async function handleCreateStakeholder(
    name: string,
    concern: string,
    missionId: string,
    requirementId: string,
  ) {
    const newNode = await createElement(name, "Stakeholder");
    if (!newNode) {
      return;
    }
    if (concern) {
      await fetch(`/api/elements/${newNode.id}/body`, {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ rationale: null, properties: { concern } }),
      });
    }
    for (const targetId of [missionId, requirementId]) {
      const res = await fetch("/api/edges", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ source: newNode.id, target: targetId, kind: "Concerns" }),
      });
      if (!res.ok) {
        showNotice(await readErrorMessage(res));
        continue;
      }
      setConcernsEdges((eds) => [
        ...eds,
        { source: newNode.id, target: targetId, kind: "Concerns" },
      ]);
    }
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

  /** FR-CORE-08 provenance scaffolding (T-P1.2-06) — the UI trigger for "mark as ai-suggested
   * via the API": a picker next to Deactivate/Reactivate on the selected node. */
  async function handleSetOrigin(node: FlowNode<AxiomaBlockData>, origin: Origin) {
    const res = await fetch(`/api/elements/${node.id}/origin`, {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ origin }),
    });
    if (!res.ok) {
      showNotice(await readErrorMessage(res));
      return;
    }
    setNodes((nds) =>
      nds.map((n) => (n.id === node.id ? { ...n, data: { ...n.data, origin } } : n)),
    );
  }

  const selectedNode = nodes.find((n) => n.id === selectedNodeId) ?? null;
  const hazardCauseIds = new Set(causesEdges.map((e) => e.source));
  const visibleNodes =
    originFilter === "all" ? nodes : nodes.filter((n) => n.data.origin === originFilter);

  const displayNodes = visibleNodes.map((node) => ({
    ...node,
    data: {
      ...node.data,
      editable: editMode,
      hasHazard: hazardCauseIds.has(node.id),
      onRename: (name: string) => handleRename(node.id, name),
      // Clears the one-shot autoFocusRename flag from persisted state right after it's consumed
      // — see AxiomaBlockNode's doc comment. Without this, a later remount of this same node
      // (e.g. it gets filtered out of the canvas by the origin filter and back in) would
      // re-enter rename mode from a stale `true` flag.
      onAutoFocusRenameConsumed: () => {
        setNodes((nds) =>
          nds.map((n) =>
            n.id === node.id ? { ...n, data: { ...n.data, autoFocusRename: false } } : n,
          ),
        );
      },
    },
  }));

  const visibleNodeIds = new Set(visibleNodes.map((n) => n.id));
  const displayEdges = edges
    .filter((edge) => visibleNodeIds.has(edge.source) && visibleNodeIds.has(edge.target))
    .map((edge) => ({
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
          onNodeClick={(_event, node) => {
            setSelectedNodeId(node.id);
            setShowHazardPanel(false);
            setShowMissionPanel(false);
          }}
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
              {editMode && selectedNode && (
                <select
                  id="origin-picker"
                  value={selectedNode.data.origin}
                  onChange={(event) => handleSetOrigin(selectedNode, event.target.value as Origin)}
                  className="rounded border border-white/10 bg-obsidian/60 px-1.5 py-1 text-xs text-white/80"
                >
                  <option value="Human">Human</option>
                  <option value="AiSuggested">AI-suggested</option>
                  <option value="AiAutoMerged">AI-auto-merged</option>
                </select>
              )}
              <select
                id="origin-filter"
                value={originFilter}
                onChange={(event) => setOriginFilter(event.target.value as Origin | "all")}
                className="rounded border border-white/10 bg-obsidian/60 px-1.5 py-1 text-xs text-white/80"
              >
                <option value="all">All origins</option>
                <option value="Human">Human only</option>
                <option value="AiSuggested">AI-suggested only</option>
                <option value="AiAutoMerged">AI-auto-merged only</option>
              </select>
              <Button
                variant={showHazardPanel ? "primary" : "ghost"}
                onClick={() => {
                  setShowHazardPanel((v) => !v);
                  setShowMissionPanel(false);
                  setSelectedNodeId(null);
                }}
                className="!px-2 !py-1 text-xs"
              >
                Hazard/Risk
              </Button>
              <Button
                variant={showMissionPanel ? "primary" : "ghost"}
                onClick={() => {
                  setShowMissionPanel((v) => !v);
                  setShowHazardPanel(false);
                  setSelectedNodeId(null);
                }}
                className="!px-2 !py-1 text-xs"
              >
                Mission Planning
              </Button>
            </div>
          </GlassPanel>

          {selectedNode && (
            <ElementInspector
              elementId={selectedNode.id}
              elementLabel={selectedNode.data.label}
              onClose={() => setSelectedNodeId(null)}
            />
          )}

          {showHazardPanel && (
            <HazardRiskPanel
              nodes={nodes}
              causesEdges={causesEdges}
              mitigatedByEdges={mitigatedByEdges}
              editMode={editMode}
              onClose={() => setShowHazardPanel(false)}
              onCreateHazard={handleCreateHazard}
              onCreateControl={handleCreateControl}
            />
          )}

          {showMissionPanel && (
            <MissionPlanningPanel
              nodes={nodes}
              concernsEdges={concernsEdges}
              editMode={editMode}
              onClose={() => setShowMissionPanel(false)}
              onCreateMission={handleCreateMission}
              onCreateStakeholder={handleCreateStakeholder}
            />
          )}
        </ReactFlow>
      </ReactFlowProvider>
    </div>
  );
}
