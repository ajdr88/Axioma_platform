"use client";

import {
  type AxiomaBlockData,
  type AxiomaEdgeData,
  computeClusteredNodes,
  computeElkLayout,
  edgeTypes,
  NODE_HEIGHT,
  NODE_WIDTH,
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
  type OnEdgesChange,
  type OnNodesChange,
  ReactFlow,
  ReactFlowProvider,
  type Rect,
  reconnectEdge,
  useEdgesState,
  useNodesState,
  useOnViewportChange,
  useReactFlow,
} from "@xyflow/react";
import dynamic from "next/dynamic";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ElementInspector } from "@/components/ElementInspector";
import { HazardRiskPanel } from "@/components/HazardRiskPanel";
import { MissionPlanningPanel } from "@/components/MissionPlanningPanel";
import type { TextualEditorPanelHandle } from "@/components/TextualEditorPanel";

// `monaco-editor` touches `window` at module-evaluation time — a plain static import would pull
// it into the SSR module graph (and crash prerendering) even while `showTextPanel` is false and
// nothing actually renders it. `next/dynamic` with `ssr: false` defers the import (and the
// code-split chunk) to the client, only once the panel is actually opened.
const TextualEditorPanel = dynamic(
  () => import("@/components/TextualEditorPanel").then((m) => m.TextualEditorPanel),
  { ssr: false },
);

type LoadStatus = "loading" | "error" | "ready";

interface PositionEntry {
  elementId: string;
  x: number;
  y: number;
}

interface Project {
  id: string;
  name: string;
}

/** Every read/write in this file is scoped to the current project (roadmap: Git-backed model
 * versioning) — `/api/projects/:projectId/...` mirrors `apps/api`'s own route restructuring. */
function apiPath(projectId: string, path: string): string {
  return `/api/projects/${projectId}${path}`;
}

function toFlowNode(
  element: ApiElement,
  position: { x: number; y: number },
): FlowNode<AxiomaBlockData> {
  return {
    id: element.id,
    type: "axiomaBlock",
    position,
    // Fixed footprint (matches AxiomaBlockNode's actual rendered size, see dimensions.ts) — ELK
    // auto-layout computes overlap-avoidance against this same box, so it must stay equal to
    // what's really on screen.
    width: NODE_WIDTH,
    height: NODE_HEIGHT,
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

  /** Roadmap: Git-backed model versioning — every read/write is scoped to one project. Defaults
   * to the first project returned by `/api/projects` (the seeded "Turbofan Reference" one on a
   * fresh install); switching projects re-runs the load effect below from scratch. */
  const [projects, setProjects] = useState<Project[]>([]);
  const [projectId, setProjectId] = useState<string | null>(null);

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

    async function loadProjects() {
      try {
        const res = await fetch("/api/projects");
        if (!res.ok) {
          throw new Error(await readErrorMessage(res));
        }
        const list: Project[] = await res.json();
        if (cancelled) {
          return;
        }
        setProjects(list);
        if (list.length > 0) {
          setProjectId(list[0].id);
        } else {
          setErrorMessage("No projects exist yet.");
          setStatus("error");
        }
      } catch (error) {
        if (!cancelled) {
          setErrorMessage(error instanceof Error ? error.message : "failed to load projects");
          setStatus("error");
        }
      }
    }

    loadProjects();
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (!projectId) {
      return;
    }
    // Captured as its own const so the closure below keeps TypeScript's non-null narrowing —
    // `projectId` itself is still `string | null` from the effect's own perspective.
    const currentProjectId = projectId;
    let cancelled = false;

    async function load() {
      setStatus("loading");
      try {
        const [elementsRes, containsRes, positionsRes, causesRes, mitigatedByRes, concernsRes] =
          await Promise.all([
            fetch(apiPath(currentProjectId, "/elements")),
            fetch(apiPath(currentProjectId, "/contains")),
            fetch(apiPath(currentProjectId, "/positions")),
            fetch(apiPath(currentProjectId, "/edges?kind=Causes")),
            fetch(apiPath(currentProjectId, "/edges?kind=MitigatedBy")),
            fetch(apiPath(currentProjectId, "/edges?kind=Concerns")),
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
        const allEdgePairs = [...contains, ...causes, ...mitigatedBy, ...concerns];
        const elkPositions = await computeElkLayout(elements, allEdgePairs);

        setNodes(
          elements.map((element) =>
            toFlowNode(
              element,
              storedPositions.get(element.id) ?? elkPositions.get(element.id) ?? { x: 0, y: 0 },
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
    // list without turning this into a run-on-every-render effect. `projectId` is the real
    // trigger — switching projects re-runs this from scratch.
  }, [projectId, setNodes, setEdges]);

  function showNotice(message: string) {
    setNotice(message);
    setTimeout(() => setNotice(null), 4000);
  }

  async function handleRename(nodeId: string, name: string) {
    if (!projectId) {
      return;
    }
    const res = await fetch(apiPath(projectId, `/elements/${nodeId}`), {
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
    if (!projectId) {
      return null;
    }
    const res = await fetch(apiPath(projectId, "/elements"), {
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
    await fetch(apiPath(projectId, `/elements/${element.id}/position`), {
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
    if (!newNode || !projectId) {
      return;
    }
    const res = await fetch(apiPath(projectId, "/edges"), {
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
    if (!newNode || !projectId) {
      return;
    }
    const res = await fetch(apiPath(projectId, "/edges"), {
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
    if (!newNode || !projectId) {
      return;
    }
    if (concern) {
      await fetch(apiPath(projectId, `/elements/${newNode.id}/body`), {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ rationale: null, properties: { concern } }),
      });
    }
    for (const targetId of [missionId, requirementId]) {
      const res = await fetch(apiPath(projectId, "/edges"), {
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
    if (!projectId) {
      return;
    }
    const res = await fetch(apiPath(projectId, "/contains"), {
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
    if (!projectId) {
      return;
    }
    const nextActive = !(node.data.active ?? true);
    const res = await fetch(apiPath(projectId, `/elements/${node.id}/active`), {
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
    if (!projectId) {
      return;
    }
    const res = await fetch(apiPath(projectId, `/elements/${node.id}/origin`), {
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

  /** "Auto-Layout" toolbar button (T-P1.2-03) — recomputes ELK layout over every currently
   * loaded node and edge (any kind), applies the new positions, and persists each one through
   * the same `PATCH .../position` path drag-to-move already uses. */
  async function handleAutoLayout() {
    if (!projectId) {
      return;
    }
    const allEdgePairs = [
      ...edges.map((e) => ({ source: e.source, target: e.target })),
      ...causesEdges,
      ...mitigatedByEdges,
      ...concernsEdges,
    ];
    const positions = await computeElkLayout(
      nodes.map((n) => ({ id: n.id })),
      allEdgePairs,
    );
    setNodes((nds) =>
      nds.map((n) => {
        const position = positions.get(n.id);
        return position ? { ...n, position } : n;
      }),
    );
    await Promise.all(
      nodes.map((n) => {
        const position = positions.get(n.id);
        if (!position) {
          return Promise.resolve();
        }
        return fetch(apiPath(projectId, `/elements/${n.id}/position`), {
          method: "PATCH",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(position),
        });
      }),
    );
  }

  /** Project switcher "+ New Project" — creates an empty project and switches to it. No inline
   * rename UI for the project itself yet (out of scope for this pass, same trim as the
   * branch/commit/diff UI below); a default name is enough to get a second, genuinely separate
   * project to switch between. */
  async function handleCreateProject() {
    const res = await fetch("/api/projects", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name: `New Project ${projects.length + 1}` }),
    });
    if (!res.ok) {
      showNotice(await readErrorMessage(res));
      return;
    }
    const project: Project = await res.json();
    setProjects((ps) => [...ps, project]);
    setProjectId(project.id);
  }

  const selectedNode = nodes.find((n) => n.id === selectedNodeId) ?? null;

  return (
    // `Canvas` needs `useOnViewportChange`/`useReactFlow` (NFR-PERF-01 canvas virtualization —
    // `fitView`/`fitBounds` move the viewport WITHOUT firing `onMove`/`onMoveEnd`, so those
    // props alone can't track the real transform; a real bug hit and fixed during this work),
    // both of which require being a descendant of `ReactFlowProvider`, hence this split.
    <ReactFlowProvider>
      <Canvas
        status={status}
        errorMessage={errorMessage}
        notice={notice}
        projects={projects}
        projectId={projectId}
        setProjectId={setProjectId}
        handleCreateProject={handleCreateProject}
        editMode={editMode}
        setEditMode={setEditMode}
        selectedNode={selectedNode}
        setSelectedNodeId={setSelectedNodeId}
        showHazardPanel={showHazardPanel}
        setShowHazardPanel={setShowHazardPanel}
        showMissionPanel={showMissionPanel}
        setShowMissionPanel={setShowMissionPanel}
        originFilter={originFilter}
        setOriginFilter={setOriginFilter}
        nodes={nodes}
        setNodes={setNodes}
        onNodesChange={onNodesChange}
        edges={edges}
        setEdges={setEdges}
        onEdgesChange={onEdgesChange}
        showNotice={showNotice}
        causesEdges={causesEdges}
        mitigatedByEdges={mitigatedByEdges}
        concernsEdges={concernsEdges}
        handleRename={handleRename}
        handleAddNode={handleAddNode}
        handleAutoLayout={handleAutoLayout}
        handleToggleActive={handleToggleActive}
        handleSetOrigin={handleSetOrigin}
        handleDisconnect={handleDisconnect}
        handleCreateHazard={handleCreateHazard}
        handleCreateControl={handleCreateControl}
        handleCreateMission={handleCreateMission}
        handleCreateStakeholder={handleCreateStakeholder}
      />
    </ReactFlowProvider>
  );
}

interface CanvasProps {
  status: LoadStatus;
  errorMessage: string;
  notice: string | null;
  projects: Project[];
  projectId: string | null;
  setProjectId: (id: string) => void;
  handleCreateProject: () => Promise<void>;
  editMode: boolean;
  setEditMode: React.Dispatch<React.SetStateAction<boolean>>;
  selectedNode: FlowNode<AxiomaBlockData> | null;
  setSelectedNodeId: (id: string | null) => void;
  showHazardPanel: boolean;
  setShowHazardPanel: React.Dispatch<React.SetStateAction<boolean>>;
  showMissionPanel: boolean;
  setShowMissionPanel: React.Dispatch<React.SetStateAction<boolean>>;
  originFilter: Origin | "all";
  setOriginFilter: (filter: Origin | "all") => void;
  nodes: FlowNode<AxiomaBlockData>[];
  setNodes: (updater: (nds: FlowNode<AxiomaBlockData>[]) => FlowNode<AxiomaBlockData>[]) => void;
  onNodesChange: OnNodesChange<FlowNode<AxiomaBlockData>>;
  edges: FlowEdge<AxiomaEdgeData>[];
  setEdges: (updater: (eds: FlowEdge<AxiomaEdgeData>[]) => FlowEdge<AxiomaEdgeData>[]) => void;
  onEdgesChange: OnEdgesChange<FlowEdge<AxiomaEdgeData>>;
  showNotice: (message: string) => void;
  causesEdges: ApiEdge[];
  mitigatedByEdges: ApiEdge[];
  concernsEdges: ApiEdge[];
  handleRename: (nodeId: string, name: string) => Promise<void>;
  handleAddNode: () => Promise<void>;
  handleAutoLayout: () => Promise<void>;
  handleToggleActive: (node: FlowNode<AxiomaBlockData>) => Promise<void>;
  handleSetOrigin: (node: FlowNode<AxiomaBlockData>, origin: Origin) => Promise<void>;
  handleDisconnect: (source: string, target: string) => Promise<void>;
  handleCreateHazard: (name: string, causingStructureId: string) => Promise<void>;
  handleCreateControl: (hazardId: string, name: string) => Promise<void>;
  handleCreateMission: (name: string) => Promise<void>;
  handleCreateStakeholder: (
    name: string,
    concern: string,
    missionId: string,
    requirementId: string,
  ) => Promise<void>;
}

function Canvas({
  status,
  errorMessage,
  notice,
  projects,
  projectId,
  setProjectId,
  handleCreateProject,
  editMode,
  setEditMode,
  selectedNode,
  setSelectedNodeId,
  showHazardPanel,
  setShowHazardPanel,
  showMissionPanel,
  setShowMissionPanel,
  originFilter,
  setOriginFilter,
  nodes,
  setNodes,
  onNodesChange,
  edges,
  setEdges,
  onEdgesChange,
  showNotice,
  causesEdges,
  mitigatedByEdges,
  concernsEdges,
  handleRename,
  handleAddNode,
  handleAutoLayout,
  handleToggleActive,
  handleSetOrigin,
  handleDisconnect,
  handleCreateHazard,
  handleCreateControl,
  handleCreateMission,
  handleCreateStakeholder,
}: CanvasProps) {
  const canvasWrapperRef = useRef<HTMLDivElement>(null);
  const reactFlowInstance = useReactFlow<FlowNode<AxiomaBlockData>, FlowEdge<AxiomaEdgeData>>();
  const [showTextPanel, setShowTextPanel] = useState(false);
  const textualHandleRef = useRef<TextualEditorPanelHandle | null>(null);
  const setTextualHandle = useCallback((handle: TextualEditorPanelHandle) => {
    textualHandleRef.current = handle;
  }, []);
  /** NFR-PERF-01 / T-P1.2-02 canvas virtualization — tracks the real current pan/zoom, including
   * programmatic changes (`fitView` on mount, `fitBounds` on cluster-expand), so
   * `computeClusteredNodes` always sees the transform actually on screen. */
  const [viewport, setViewport] = useState({ x: 0, y: 0, zoom: 1 });
  useOnViewportChange({
    onChange: (v) => setViewport(v),
  });

  /** Pans/zooms to a clicked cluster's bounding box, bringing it on-screen so the next
   * viewport-intersection check un-clusters it back into real nodes. */
  const handleExpandSubsystem = useCallback(
    (bbox: Rect) => {
      reactFlowInstance.fitBounds(bbox, { padding: 0.2, duration: 300 });
    },
    [reactFlowInstance],
  );

  const hazardCauseIds = new Set(causesEdges.map((e) => e.source));
  const realNodeIds = useMemo(() => new Set(nodes.map((n) => n.id)), [nodes]);
  const visibleNodes = useMemo(
    () => (originFilter === "all" ? nodes : nodes.filter((n) => n.data.origin === originFilter)),
    [nodes, originFilter],
  );

  const viewportBounds: Rect = useMemo(
    () => ({
      x: -viewport.x / viewport.zoom,
      y: -viewport.y / viewport.zoom,
      width: (canvasWrapperRef.current?.clientWidth ?? 1) / viewport.zoom,
      height: (canvasWrapperRef.current?.clientHeight ?? 1) / viewport.zoom,
    }),
    [viewport],
  );

  // Real dependencies only — `computeClusteredNodes` builds fresh node/data objects for every
  // off-screen subsystem's cluster, so without memoization those objects get new identity on
  // every render even when nothing relevant changed. React Flow treats that as new nodes and
  // remounts them, which re-triggers dimension measurement, which (via `onNodesChange`) sets
  // state again and forces another render — an infinite loop discovered via a real DOM
  // mutation-observer check during this work (a genuine bug, not just a perf nice-to-have).
  const containsEdgesForClustering = useMemo(
    () => edges.map((e) => ({ source: e.source, target: e.target })),
    [edges],
  );
  const clusteredNodes = useMemo(
    () =>
      computeClusteredNodes(
        visibleNodes,
        containsEdgesForClustering,
        viewportBounds,
        handleExpandSubsystem,
      ),
    [visibleNodes, containsEdgesForClustering, viewportBounds, handleExpandSubsystem],
  );

  const displayNodes = clusteredNodes.map((node) => {
    if (node.type === "subsystemCluster") {
      return node;
    }
    return {
      ...node,
      data: {
        ...node.data,
        editable: editMode,
        hasHazard: hazardCauseIds.has(node.id),
        onRename: async (name: string) => {
          await handleRename(node.id, name);
          // Diagram→text bridge (FR-CORE-02 / T-P1.2-01): the LSP server already knows this
          // rename landed (the PATCH above just resolved), so push the new name to it directly
          // instead of waiting on a round trip it doesn't need — a no-op if the text panel
          // isn't open/connected.
          textualHandleRef.current?.notifyElementRenamed(node.id, name);
        },
        // Clears the one-shot autoFocusRename flag from persisted state right after it's
        // consumed — see AxiomaBlockNode's doc comment. Without this, a later remount of this
        // same node (e.g. it gets filtered out of the canvas by the origin filter, or clustered
        // away and back, and back in) would re-enter rename mode from a stale `true` flag.
        onAutoFocusRenameConsumed: () => {
          setNodes((nds) =>
            nds.map((n) =>
              n.id === node.id ? { ...n, data: { ...n.data, autoFocusRename: false } } : n,
            ),
          );
        },
      },
    };
  });

  // Edges to/from a clustered-away node are hidden entirely, not rerouted to its cluster — the
  // cluster is a summary, not a real graph endpoint.
  const visibleNodeIds = new Set(
    clusteredNodes.filter((n) => n.type !== "subsystemCluster").map((n) => n.id),
  );
  const displayEdges: FlowEdge<AxiomaEdgeData>[] = edges
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
    <div className="flex h-screen w-screen">
      <div className="h-full min-w-0 flex-1" ref={canvasWrapperRef}>
        <ReactFlow
          // `displayNodes` is a runtime union of AxiomaBlockData and SubsystemClusterData nodes
          // (each correctly rendered per `nodeTypes`' `type` discriminant) — React Flow's own
          // generics assume one node-data type per instance, so the two are reconciled here
          // rather than trying to force a single generic across genuinely different node shapes.
          nodes={displayNodes as FlowNode<AxiomaBlockData>[]}
          edges={displayEdges}
          nodeTypes={nodeTypes}
          edgeTypes={edgeTypes}
          // Synthetic `subsystemCluster` nodes aren't in `nodes` state — React Flow still measures
          // and reports dimension-change events for them (they're real rendered nodes from its
          // point of view). `useNodesState`'s `onNodesChange` calls `setNodes` unconditionally,
          // even for a changes array that ends up empty after filtering to known ids — a new (if
          // content-equal) array reference still re-renders `Canvas`, which rebuilds the cluster
          // node objects fresh, which get remeasured, which fires the change again: an infinite
          // loop, confirmed via a DOM mutation-observer check (~40 remounts/sec) during this work.
          // Filtering to real-node ids AND skipping the call entirely when nothing real remains
          // breaks the cycle at its source.
          onNodesChange={(changes) => {
            const filtered = changes.filter((c) => !("id" in c) || realNodeIds.has(c.id));
            if (filtered.length > 0) {
              onNodesChange(filtered);
            }
          }}
          onEdgesChange={onEdgesChange}
          // Deliberately NOT using React Flow's `onlyRenderVisibleElements` here: it DOM-culls
          // strictly by exact pixel-viewport intersection with no margin (confirmed in
          // `@xyflow/system`'s `getNodesInside`), and applies uniformly to every node — including
          // `subsystemCluster` placeholders. Since a cluster's whole purpose is representing a
          // subsystem that's off-screen, its own position is (by construction) usually outside the
          // exact viewport too, so this prop would silently hide the very placeholder a user needs
          // to click to navigate back to it, breaking `SubsystemClusterNode`'s click-to-expand.
          // `computeClusteredNodes` is the real render-count lever for NFR-PERF-01 (shrinks the
          // array itself, not just what's painted from it) — sufficient on its own.
          nodesDraggable={editMode}
          nodesConnectable={editMode}
          edgesReconnectable={editMode}
          connectionRadius={40} // more forgiving than the 20px default — see the plan's Context.
          deleteKeyCode={null} // Node delete is out of scope — disconnect uses the edge's own button.
          onNodeDragStop={(_event, node) => {
            if (!projectId) {
              return;
            }
            fetch(apiPath(projectId, `/elements/${node.id}/position`), {
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
            if (!connection.source || !connection.target || !projectId) {
              return;
            }
            const res = await fetch(apiPath(projectId, "/contains"), {
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
            if (!newConnection.source || !newConnection.target || !projectId) {
              return;
            }
            // Create the new edge first, through the same validated path a fresh connect uses —
            // a reconnect that would cycle is rejected exactly like one, leaving the old edge
            // untouched rather than half-changed.
            const createRes = await fetch(apiPath(projectId, "/contains"), {
              method: "POST",
              headers: { "Content-Type": "application/json" },
              body: JSON.stringify({ parent: newConnection.source, child: newConnection.target }),
            });
            if (!createRes.ok) {
              showNotice(await readErrorMessage(createRes));
              return;
            }
            await fetch(apiPath(projectId, "/contains"), {
              method: "DELETE",
              headers: { "Content-Type": "application/json" },
              body: JSON.stringify({ parent: oldEdge.source, child: oldEdge.target }),
            });
            setEdges((eds) => reconnectEdge(oldEdge, newConnection, eds, { getEdgeId }));
          }}
          fitView
          // React Flow's default `minZoom` (0.5) caps the maximum flow-area a viewport can ever
          // show to 2x its pixel size — nowhere near enough to fit the NFR-PERF-01 target of
          // 10,000+ simultaneously-visible elements (confirmed via the large-fixture test during
          // this work: even after 30 zoom-out ticks, the transform never moved past scale 0.5).
          // Lower enough to comfortably fit a dense 10,000-element region at a real ELK-scale
          // layout density.
          minZoom={0.01}
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

            {/* Roadmap: Git-backed model versioning — every project is its own independent
             * graph. No branch/commit/diff UI here yet (that's the CEM proposal-review
             * surface's job later, P2.2); this is just create/switch. */}
            <div className="mt-2 flex items-center gap-1.5">
              <select
                id="project-switcher"
                value={projectId ?? ""}
                onChange={(event) => setProjectId(event.target.value)}
                className="min-w-0 flex-1 rounded border border-white/10 bg-obsidian/60 px-1.5 py-1 text-xs text-white/80"
              >
                {projects.map((project) => (
                  <option key={project.id} value={project.id}>
                    {project.name}
                  </option>
                ))}
              </select>
              <Button variant="ghost" onClick={handleCreateProject} className="!px-2 !py-1 text-xs">
                + New Project
              </Button>
            </div>

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
              {editMode && (
                <Button variant="ghost" onClick={handleAutoLayout} className="!px-2 !py-1 text-xs">
                  Auto-Layout
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
              <Button
                variant={showTextPanel ? "primary" : "ghost"}
                onClick={() => setShowTextPanel((v) => !v)}
                className="!px-2 !py-1 text-xs"
              >
                Text View
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
      </div>
      {showTextPanel && (
        <TextualEditorPanel
          onClose={() => setShowTextPanel(false)}
          onHandleReady={setTextualHandle}
        />
      )}
    </div>
  );
}
