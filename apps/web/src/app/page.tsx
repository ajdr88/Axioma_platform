"use client";

import {
  type AxiomaBlockData,
  type AxiomaEdgeData,
  computeClusteredNodes,
  computeElkLayout,
  computeSwimlaneLayout,
  edgeTypes,
  NODE_HEIGHT,
  NODE_WIDTH,
  nodeTypes,
  UNALLOCATED_LANE_ID,
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
import { ArchspacePanel } from "@/components/ArchspacePanel";
import { AutonomyPanel } from "@/components/AutonomyPanel";
import { ElementInspector } from "@/components/ElementInspector";
import { HazardRiskPanel } from "@/components/HazardRiskPanel";
import { InteractionPanel } from "@/components/InteractionPanel";
import { MissionPlanningPanel } from "@/components/MissionPlanningPanel";
import { ParametricsPanel } from "@/components/ParametricsPanel";
import { PartSearchPanel } from "@/components/PartSearchPanel";
import { StageTrackingPanel } from "@/components/StageTrackingPanel";
import type { TextualEditorPanelHandle } from "@/components/TextualEditorPanel";
import { TraceabilityPanel } from "@/components/TraceabilityPanel";
import { TradeStudyPanel } from "@/components/TradeStudyPanel";

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
  /** NFR-COMP-02 (data residency) — declared, not (yet) physically enforced; see
   * `store::versioning::Project::region`'s doc comment on the Rust side. */
  region: string;
}

/** A representative sample, not an exhaustive list — matches the regions a real
 * `infrastructure/` deployment would plausibly parameterize (roadmap: NFR-COMP-01/02). */
const REGIONS = ["us-east", "eu-west", "ap-south"] as const;

/** FR-EXPORT-02's toolbar "Export Table" kind scope — every real `NodeKind` except `"Element"`
 * (too generic a base kind to export meaningfully) and `"CandidateStructureSuggestion"`
 * (proposal-scoped only, never present on Main). */
const EXPORTABLE_NODE_KINDS: NodeKind[] = [
  "Structure",
  "Requirement",
  "Port",
  "Hazard",
  "Control",
  "Mission",
  "Stakeholder",
  "SimulationRun",
  "Constraint",
  "Parameter",
  "InformationElement",
  "Interaction",
  "InteractionFragment",
  "Collection",
  "Function",
  "SelectionChoice",
  "ConnectionChoice",
];

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
    data: { kind: "Contains" },
  };
}

/** docs/IMPLEMENTATION_KICKOFF.md Phase 5 (turbofan amendment §3.5's ADSG canvas gap) — renders
 * `ArchDerives`/`IncompatibleWith`/`ChoiceConstraint` on the main canvas for the first time (they
 * previously weren't fetched into the canvas's own edge state at all, only `Contains` was).
 * Read-only: `reconnectable: false` so a user dragging one of these can never trigger
 * `onReconnect`'s Contains-only mutation, and the id prefix can't collide with
 * `getEdgeId`'s `-contains-` convention. */
const ARCH_EDGE_STYLES: Record<
  string,
  { slug: string; style: { stroke: string; strokeDasharray: string } }
> = {
  ArchDerives: { slug: "archderives", style: { stroke: "#B98CE8", strokeDasharray: "4 3" } },
  IncompatibleWith: {
    slug: "incompatiblewith",
    style: { stroke: "#FF5C5C", strokeDasharray: "2 3" },
  },
  ChoiceConstraint: {
    slug: "choiceconstraint",
    style: { stroke: "#E8A93A", strokeDasharray: "6 3" },
  },
  // FR-CORE-13 real build-out — same "read-only on this canvas, edited via ElementInspector's
  // dropdown" treatment as the three above (no drag-to-connect creation UI for this kind either).
  Flow: { slug: "flow", style: { stroke: "#6EE89A", strokeDasharray: "" } },
};

function toFlowArchEdge(
  edge: ApiEdge,
  kind: keyof typeof ARCH_EDGE_STYLES,
): FlowEdge<AxiomaEdgeData> {
  const { slug, style } = ARCH_EDGE_STYLES[kind];
  return {
    id: `${edge.source}-${slug}-${edge.target}`,
    source: edge.source,
    target: edge.target,
    type: "axiomaEdge",
    style,
    reconnectable: false,
    data: { kind },
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
  const [showStagePanel, setShowStagePanel] = useState(false);
  const [showTradeStudyPanel, setShowTradeStudyPanel] = useState(false);
  const [showPartSearchPanel, setShowPartSearchPanel] = useState(false);
  const [showAutonomyPanel, setShowAutonomyPanel] = useState(false);
  const [showTraceabilityPanel, setShowTraceabilityPanel] = useState(false);
  /** FR-PARAM-03 — panel toggle, same convention as every other panel below. */
  const [showParametricsPanel, setShowParametricsPanel] = useState(false);
  const [showArchspacePanel, setShowArchspacePanel] = useState(false);
  /** docs/IMPLEMENTATION_KICKOFF.md Phase 5 (FR-CORE-12) — replaces the normal ELK/clustering
   * layout with a lane-partitioned one while active; mutually exclusive with clustering (not
   * combined this pass — see `swimlane.ts`'s own doc comment). */
  const [showSwimlaneView, setShowSwimlaneView] = useState(false);
  /** FR-CORE-08 / T-P1.2-06's "AI-suggested only" filter — "all" shows every origin. */
  const [originFilter, setOriginFilter] = useState<Origin | "all">("all");

  /** Roadmap: Git-backed model versioning — every read/write is scoped to one project. Defaults
   * to the first project returned by `/api/projects` (the seeded "Turbofan Reference" one on a
   * fresh install); switching projects re-runs the load effect below from scratch. */
  const [projects, setProjects] = useState<Project[]>([]);
  const [projectId, setProjectId] = useState<string | null>(null);
  const [newProjectRegion, setNewProjectRegion] = useState<string>(REGIONS[0]);
  /** FR-INFO-01/03 — "+ Add Node" toolbar kind picker; `InformationElement` needs a second,
   * dedicated create call (`/information/elements`) to set `abstractionLevel` atomically, unlike
   * every other kind which goes through the generic `createElement`. */
  const [newElementKind, setNewElementKind] = useState<
    "Structure" | "InformationElement" | "Action"
  >("Structure");
  const [newInfoAbstractionLevel, setNewInfoAbstractionLevel] = useState<
    "Conceptual" | "Logical" | "Physical"
  >("Conceptual");
  /** FR-EXPORT-02 — the `NodeKind` scope for the toolbar's "Export Table" link. */
  const [exportTableKind, setExportTableKind] = useState<NodeKind>("Structure");
  /** Scope-downs pass — CSV or XLSX for the same "Export Table" link. */
  const [exportTableFormat, setExportTableFormat] = useState<"csv" | "xlsx">("csv");
  /** FR-CORE-10/11 — lifted here (not local to `TraceabilityPanel`) since that panel unmounts on
   * close; see `TraceabilityPanel`'s own prop doc comment for why. No LIST endpoint exists, so
   * this is still lost on a full page reload — a real, accepted gap. */
  const [savedCollections, setSavedCollections] = useState<{ id: string; name: string }[]>([]);

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
  /** docs/IMPLEMENTATION_KICKOFF.md Phase 5 (FR-CORE-12) — every element's swimlane assignment;
   * an element with none lands in the `UNALLOCATED_LANE_ID` catch-all lane. */
  const [allocateEdges, setAllocateEdges] = useState<ApiEdge[]>([]);
  /** The raw element list `toFlowNode` builds `nodes` from — kept around (not just discarded
   * after building nodes) for name/kind lookups the Interaction panel and Swimlane lane-option
   * dropdown both need, without a second fetch. */
  const [elements, setElements] = useState<ApiElement[]>([]);

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

  // A monotonic token, not a plain boolean flag, since this is now called both by the
  // project-switch effect below AND externally (the text panel's `onModelChanged` — a
  // text-driven edit landing on the backend while a project switch is also mid-flight must not
  // let the stale one win).
  const reloadTokenRef = useRef(0);

  const reloadModel = useCallback(async () => {
    if (!projectId) {
      return;
    }
    const currentProjectId = projectId;
    const token = ++reloadTokenRef.current;
    setStatus("loading");
    try {
      const [
        elementsRes,
        containsRes,
        positionsRes,
        causesRes,
        mitigatedByRes,
        concernsRes,
        archDerivesRes,
        incompatibleWithRes,
        choiceConstraintRes,
        allocateRes,
        flowRes,
      ] = await Promise.all([
        fetch(apiPath(currentProjectId, "/elements")),
        fetch(apiPath(currentProjectId, "/contains")),
        fetch(apiPath(currentProjectId, "/positions")),
        fetch(apiPath(currentProjectId, "/edges?kind=Causes")),
        fetch(apiPath(currentProjectId, "/edges?kind=MitigatedBy")),
        fetch(apiPath(currentProjectId, "/edges?kind=Concerns")),
        fetch(apiPath(currentProjectId, "/edges?kind=ArchDerives")),
        fetch(apiPath(currentProjectId, "/edges?kind=IncompatibleWith")),
        fetch(apiPath(currentProjectId, "/edges?kind=ChoiceConstraint")),
        fetch(apiPath(currentProjectId, "/edges?kind=Allocate")),
        fetch(apiPath(currentProjectId, "/edges?kind=Flow")),
      ]);
      for (const res of [
        elementsRes,
        containsRes,
        positionsRes,
        causesRes,
        mitigatedByRes,
        concernsRes,
        archDerivesRes,
        incompatibleWithRes,
        choiceConstraintRes,
        allocateRes,
        flowRes,
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
      const archDerives: ApiEdge[] = await archDerivesRes.json();
      const incompatibleWith: ApiEdge[] = await incompatibleWithRes.json();
      const choiceConstraint: ApiEdge[] = await choiceConstraintRes.json();
      const allocate: ApiEdge[] = await allocateRes.json();
      const flow: ApiEdge[] = await flowRes.json();
      if (reloadTokenRef.current !== token) {
        return;
      }

      const storedPositions = new Map(
        positionEntries.map((p) => [p.elementId, { x: p.x, y: p.y }]),
      );
      const allEdgePairs = [
        ...contains,
        ...causes,
        ...mitigatedBy,
        ...concerns,
        ...archDerives,
        ...incompatibleWith,
        ...choiceConstraint,
      ];
      const elkPositions = await computeElkLayout(elements, allEdgePairs);
      if (reloadTokenRef.current !== token) {
        return;
      }

      setNodes(
        elements.map((element) =>
          toFlowNode(
            element,
            storedPositions.get(element.id) ?? elkPositions.get(element.id) ?? { x: 0, y: 0 },
          ),
        ),
      );
      setEdges([
        ...contains.map(toFlowEdge),
        ...archDerives.map((e) => toFlowArchEdge(e, "ArchDerives")),
        ...incompatibleWith.map((e) => toFlowArchEdge(e, "IncompatibleWith")),
        ...choiceConstraint.map((e) => toFlowArchEdge(e, "ChoiceConstraint")),
        ...flow.map((e) => toFlowArchEdge(e, "Flow")),
      ]);
      setCausesEdges(causes);
      setMitigatedByEdges(mitigatedBy);
      setConcernsEdges(concerns);
      setAllocateEdges(allocate);
      setElements(elements);
      setStatus("ready");
    } catch (error) {
      if (reloadTokenRef.current === token) {
        setErrorMessage(error instanceof Error ? error.message : "failed to load the model");
        setStatus("error");
      }
    }
    // setNodes/setEdges (from useNodesState/useEdgesState) are stable across renders — safe to
    // list without turning this into a run-on-every-render callback. `projectId` is the real
    // trigger — switching projects gives this a new identity, which the effect below picks up.
  }, [projectId, setNodes, setEdges]);

  useEffect(() => {
    reloadModel();
  }, [reloadModel]);

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
  // Same reasoning as computeElkLayout's isolated-node placement (packages/diagram-engine/src
  // /layout.ts): the flow-space origin sits directly under the fixed top-left toolbar panel once
  // the canvas fits content into view, so a fresh node must not jitter-spawn near (0, 0). Shared
  // by every "create one new element" path (generic Structure/etc. and the dedicated Information
  // Element create call below) — the placement/PATCH/setNodes tail is identical either way.
  async function placeNewElement(element: ApiElement): Promise<FlowNode<AxiomaBlockData> | null> {
    if (!projectId) {
      return null;
    }
    const position = { x: 400 + Math.random() * 200, y: 400 + Math.random() * 200 };
    await fetch(apiPath(projectId, `/elements/${element.id}/position`), {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(position),
    });
    const newNode = toFlowNode(element, position);
    setNodes((nds) => [...nds, newNode]);
    // A real bug, found live: `elements` (the separate id/name/kind list `ElementInspector`'s
    // Collection-members and Flow dropdowns both filter from) previously only updated inside
    // `reloadModel`, so a just-created element wasn't selectable in either dropdown until an
    // unrelated action forced a full reload.
    setElements((els) => [...els, element]);
    return newNode;
  }

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
    return placeNewElement(element);
  }

  /** FR-INFO-01/03 — the dedicated create call, not the generic `/elements` endpoint, so
   * `abstractionLevel` lands atomically in the same request. */
  async function createInformationElement(
    name: string,
    abstractionLevel: "Conceptual" | "Logical" | "Physical",
  ): Promise<FlowNode<AxiomaBlockData> | null> {
    if (!projectId) {
      return null;
    }
    const res = await fetch(apiPath(projectId, "/information/elements"), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name, abstractionLevel }),
    });
    if (!res.ok) {
      showNotice(await readErrorMessage(res));
      return null;
    }
    const element: ApiElement = await res.json();
    return placeNewElement(element);
  }

  async function handleAddNode() {
    const newNode =
      newElementKind === "InformationElement"
        ? await createInformationElement("New Information Element", newInfoAbstractionLevel)
        : newElementKind === "Action"
          ? await createElement("New Action", "Action")
          : await createElement("New Element", "Structure");
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

  /** docs/IMPLEMENTATION_KICKOFF.md Phase 5 (FR-CORE-12) — the click-to-allocate dropdown's real
   * action (a deliberate scope-down from "drag-to-allocate headers," flagged in the plan/docs, not
   * silently substituted). Removes any prior `Allocate` edge from this element first — "each
   * partition allocated to exactly one structural element" (§5.11's own text) — then creates the
   * new one via the existing generic edge endpoints (no dedicated Allocate endpoint needed).
   */
  async function handleAllocate(elementId: string, laneId: string) {
    if (!projectId) {
      return;
    }
    const priorEdge = allocateEdges.find((e) => e.source === elementId);
    if (priorEdge) {
      await fetch(apiPath(projectId, "/edges"), {
        method: "DELETE",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          source: priorEdge.source,
          target: priorEdge.target,
          kind: "Allocate",
        }),
      });
    }
    if (laneId !== UNALLOCATED_LANE_ID) {
      const res = await fetch(apiPath(projectId, "/edges"), {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ source: elementId, target: laneId, kind: "Allocate" }),
      });
      if (!res.ok) {
        showNotice(await readErrorMessage(res));
        return;
      }
    }
    await reloadModel();
  }

  /** FR-CORE-10 — saves a Dynamic Collection definition; passed to `TraceabilityPanel`. */
  async function handleSaveDynamicCollection(params: {
    name: string;
    rootId: string;
    depth: number;
    maxFanout: number;
    direction: "both" | "incoming" | "outgoing";
  }) {
    if (!projectId) {
      return;
    }
    const res = await fetch(apiPath(projectId, "/collections/dynamic"), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(params),
    });
    if (!res.ok) {
      showNotice(await readErrorMessage(res));
      return;
    }
    const { id }: { id: string } = await res.json();
    setSavedCollections((prev) => [...prev, { id, name: params.name }]);
  }

  /** FR-CORE-11 — freezes a saved Dynamic Collection into a real `:Collection` element; the
   * resulting element is picked up by the following `reloadModel()`, same as every other
   * mutation that creates a new graph element outside `createElement`'s own local `setNodes`. */
  async function handleFreezeCollection(id: string) {
    if (!projectId) {
      return;
    }
    const res = await fetch(apiPath(projectId, `/collections/${id}/freeze`), { method: "POST" });
    if (!res.ok) {
      showNotice(await readErrorMessage(res));
      return;
    }
    await reloadModel();
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
      body: JSON.stringify({
        name: `New Project ${projects.length + 1}`,
        region: newProjectRegion,
      }),
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
        newProjectRegion={newProjectRegion}
        setNewProjectRegion={setNewProjectRegion}
        handleCreateProject={handleCreateProject}
        reloadModel={reloadModel}
        editMode={editMode}
        setEditMode={setEditMode}
        selectedNode={selectedNode}
        setSelectedNodeId={setSelectedNodeId}
        showHazardPanel={showHazardPanel}
        setShowHazardPanel={setShowHazardPanel}
        showMissionPanel={showMissionPanel}
        setShowMissionPanel={setShowMissionPanel}
        showStagePanel={showStagePanel}
        setShowStagePanel={setShowStagePanel}
        showTradeStudyPanel={showTradeStudyPanel}
        setShowTradeStudyPanel={setShowTradeStudyPanel}
        showPartSearchPanel={showPartSearchPanel}
        setShowPartSearchPanel={setShowPartSearchPanel}
        showAutonomyPanel={showAutonomyPanel}
        setShowAutonomyPanel={setShowAutonomyPanel}
        showTraceabilityPanel={showTraceabilityPanel}
        setShowTraceabilityPanel={setShowTraceabilityPanel}
        showParametricsPanel={showParametricsPanel}
        setShowParametricsPanel={setShowParametricsPanel}
        showArchspacePanel={showArchspacePanel}
        setShowArchspacePanel={setShowArchspacePanel}
        showSwimlaneView={showSwimlaneView}
        setShowSwimlaneView={setShowSwimlaneView}
        elements={elements}
        allocateEdges={allocateEdges}
        handleAllocate={handleAllocate}
        newElementKind={newElementKind}
        setNewElementKind={setNewElementKind}
        newInfoAbstractionLevel={newInfoAbstractionLevel}
        setNewInfoAbstractionLevel={setNewInfoAbstractionLevel}
        exportTableKind={exportTableKind}
        setExportTableKind={setExportTableKind}
        exportTableFormat={exportTableFormat}
        setExportTableFormat={setExportTableFormat}
        savedCollections={savedCollections}
        handleSaveDynamicCollection={handleSaveDynamicCollection}
        handleFreezeCollection={handleFreezeCollection}
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
  newProjectRegion: string;
  setNewProjectRegion: (region: string) => void;
  handleCreateProject: () => Promise<void>;
  reloadModel: () => Promise<void>;
  editMode: boolean;
  setEditMode: React.Dispatch<React.SetStateAction<boolean>>;
  selectedNode: FlowNode<AxiomaBlockData> | null;
  setSelectedNodeId: (id: string | null) => void;
  showHazardPanel: boolean;
  setShowHazardPanel: React.Dispatch<React.SetStateAction<boolean>>;
  showMissionPanel: boolean;
  setShowMissionPanel: React.Dispatch<React.SetStateAction<boolean>>;
  showStagePanel: boolean;
  setShowStagePanel: React.Dispatch<React.SetStateAction<boolean>>;
  showTradeStudyPanel: boolean;
  setShowTradeStudyPanel: React.Dispatch<React.SetStateAction<boolean>>;
  showPartSearchPanel: boolean;
  setShowPartSearchPanel: React.Dispatch<React.SetStateAction<boolean>>;
  showAutonomyPanel: boolean;
  setShowAutonomyPanel: React.Dispatch<React.SetStateAction<boolean>>;
  showTraceabilityPanel: boolean;
  setShowTraceabilityPanel: React.Dispatch<React.SetStateAction<boolean>>;
  showParametricsPanel: boolean;
  setShowParametricsPanel: React.Dispatch<React.SetStateAction<boolean>>;
  showArchspacePanel: boolean;
  setShowArchspacePanel: React.Dispatch<React.SetStateAction<boolean>>;
  showSwimlaneView: boolean;
  setShowSwimlaneView: React.Dispatch<React.SetStateAction<boolean>>;
  elements: ApiElement[];
  allocateEdges: ApiEdge[];
  handleAllocate: (elementId: string, laneId: string) => Promise<void>;
  newElementKind: "Structure" | "InformationElement" | "Action";
  setNewElementKind: React.Dispatch<
    React.SetStateAction<"Structure" | "InformationElement" | "Action">
  >;
  newInfoAbstractionLevel: "Conceptual" | "Logical" | "Physical";
  setNewInfoAbstractionLevel: React.Dispatch<
    React.SetStateAction<"Conceptual" | "Logical" | "Physical">
  >;
  exportTableKind: NodeKind;
  setExportTableKind: React.Dispatch<React.SetStateAction<NodeKind>>;
  exportTableFormat: "csv" | "xlsx";
  setExportTableFormat: React.Dispatch<React.SetStateAction<"csv" | "xlsx">>;
  savedCollections: { id: string; name: string }[];
  handleSaveDynamicCollection: (params: {
    name: string;
    rootId: string;
    depth: number;
    maxFanout: number;
    direction: "both" | "incoming" | "outgoing";
  }) => Promise<void>;
  handleFreezeCollection: (id: string) => Promise<void>;
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
  newProjectRegion,
  setNewProjectRegion,
  handleCreateProject,
  reloadModel,
  editMode,
  setEditMode,
  selectedNode,
  setSelectedNodeId,
  showHazardPanel,
  setShowHazardPanel,
  showMissionPanel,
  setShowMissionPanel,
  showStagePanel,
  setShowStagePanel,
  showTradeStudyPanel,
  setShowTradeStudyPanel,
  showPartSearchPanel,
  setShowPartSearchPanel,
  showAutonomyPanel,
  setShowAutonomyPanel,
  showTraceabilityPanel,
  setShowTraceabilityPanel,
  showParametricsPanel,
  setShowParametricsPanel,
  showArchspacePanel,
  setShowArchspacePanel,
  showSwimlaneView,
  setShowSwimlaneView,
  elements,
  allocateEdges,
  handleAllocate,
  newElementKind,
  setNewElementKind,
  newInfoAbstractionLevel,
  setNewInfoAbstractionLevel,
  exportTableKind,
  setExportTableKind,
  exportTableFormat,
  setExportTableFormat,
  savedCollections,
  handleSaveDynamicCollection,
  handleFreezeCollection,
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
  const [exportingPng, setExportingPng] = useState(false);

  /** docs/IMPLEMENTATION_KICKOFF.md Phase 5 (FR-EXPORT-01) — client-side, current-viewport only.
   * Captures `canvasWrapperRef` (the same node the clustering margin math already measures) via
   * `html-to-image`'s `toPng`, since it's just whatever's currently rendered in the DOM -- React
   * Flow doesn't use `onlyRenderVisibleElements`, so this is the real on-screen canvas, not a
   * partial capture. The server-side headless-render path for a full-diagram export "at any
   * size" (reqs v5 §5.12's other named half) is a separate, larger capability, not attempted
   * here. */
  async function handleExportPng() {
    if (!canvasWrapperRef.current || exportingPng) {
      return;
    }
    setExportingPng(true);
    try {
      const { toPng } = await import("html-to-image");
      const dataUrl = await toPng(canvasWrapperRef.current, { backgroundColor: "#07070C" });
      const link = document.createElement("a");
      link.download = `axioma-canvas-${projectId ?? "export"}.png`;
      link.href = dataUrl;
      link.click();
    } finally {
      setExportingPng(false);
    }
  }
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

  // Entering Swimlane View leaves whatever pan/zoom the normal ELK/clustering canvas was at,
  // which usually shows nothing useful of the lane grid (a real gap found via live browser
  // verification during this work — lanes rendered fully off-screen behind the toolbar panel
  // with no fit-to-view). `setTimeout` gives the lane/member nodes one render pass to actually
  // mount (and be measured) before `fitView` computes their bounding box.
  useEffect(() => {
    if (!showSwimlaneView) {
      return;
    }
    const id = setTimeout(() => reactFlowInstance.fitView({ padding: 0.2, duration: 300 }), 50);
    return () => clearTimeout(id);
  }, [showSwimlaneView, reactFlowInstance]);

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
  // `edges` also carries ArchDerives/IncompatibleWith/ChoiceConstraint (rendered on the main
  // canvas since docs/IMPLEMENTATION_KICKOFF.md Phase 5) — filtered to real `Contains` edges
  // only. A real bug, found live: passing every edge unfiltered meant a genuinely cyclic
  // ArchDerives edge (FR-ARCH-02's own seeded example) got walked by `computeClusteredNodes`'s
  // containment traversal as if it were parent/child, crashing the whole canvas
  // (`RangeError: Invalid array length` from its unbounded stack). `Contains` is the only kind
  // this clustering may ever treat as containment.
  const containsEdgesForClustering = useMemo(
    () =>
      edges
        .filter((e) => e.data?.kind === "Contains")
        .map((e) => ({ source: e.source, target: e.target })),
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

  // Scope-downs pass — drag-and-drop reallocation bug found via live browser verification: without
  // this, `swimlaneNodes` below recomputes every member's position from `computeSwimlaneLayout` on
  // every render (including the renders `onNodesChange` fires on every drag mousemove), which
  // always has an entry for every element (unallocated elements land in a catch-all lane) and so
  // always overrides React Flow's own live drag position — the node visually snaps back to its grid
  // slot mid-drag, and `onNodeDragStop` then reads that frozen, never-moved position, so a drop
  // never intersects any lane but the one the node was already in. Tracking the actively-dragged
  // node id and falling back to its real (drag-tracked) `node.position` while dragging fixes it.
  const [draggingNodeId, setDraggingNodeId] = useState<string | null>(null);

  // docs/IMPLEMENTATION_KICKOFF.md Phase 5 (FR-CORE-12) — replaces the normal ELK/clustering
  // layout entirely while active (see `showSwimlaneView`'s own doc comment for why the two
  // aren't combined this pass). Built from `visibleNodes` (already origin-filtered), not the
  // viewport-clustered set — lane position is computed structurally, so viewport-based
  // clustering doesn't apply here.
  const swimlaneLayout = useMemo(
    () =>
      computeSwimlaneLayout(
        elements.map((e) => ({ id: e.id, name: e.name })),
        allocateEdges,
      ),
    [elements, allocateEdges],
  );
  const swimlaneNodes = showSwimlaneView
    ? [
        ...swimlaneLayout.laneNodes,
        ...visibleNodes.map((node) => {
          // While this node is actively being dragged, skip the layout-computed position entirely
          // (see `draggingNodeId`'s own comment above) so React Flow's live drag position — already
          // tracked into `node.position` via `onNodesChange`/`setNodes` — is what actually renders.
          const position =
            node.id === draggingNodeId ? undefined : swimlaneLayout.memberPositions.get(node.id);
          return {
            ...node,
            parentId: position?.parentId,
            extent: position ? ("parent" as const) : undefined,
            position: position ? { x: position.x, y: position.y } : node.position,
            // Scope-downs pass — real drag-and-drop-to-reallocate (`onNodeDragStop` below). A
            // successful drop calls `handleAllocate`, whose `reloadModel()` re-render then snaps
            // the node to `computeSwimlaneLayout`'s freshly-computed position anyway, so free
            // dragging here doesn't fight the grid the way it would without that reallocate step.
            draggable: true,
            data: {
              ...node.data,
              editable: false,
              hasHazard: hazardCauseIds.has(node.id),
            },
          };
        }),
      ]
    : [];

  return (
    <div className="flex h-screen w-screen">
      <div className="h-full min-w-0 flex-1" ref={canvasWrapperRef}>
        <ReactFlow
          // `displayNodes` is a runtime union of AxiomaBlockData and SubsystemClusterData nodes
          // (each correctly rendered per `nodeTypes`' `type` discriminant) — React Flow's own
          // generics assume one node-data type per instance, so the two are reconciled here
          // rather than trying to force a single generic across genuinely different node shapes.
          // Swimlane View swaps in `swimlaneNodes` (a union of SwimlaneLaneData/AxiomaBlockData)
          // wholesale instead — see its own comment above for why it replaces rather than layers
          // onto the ELK/clustering path. Edges are deliberately empty in Swimlane View: structural
          // Contains edges drawn across lane boundaries would clash with the lane partitioning that
          // is the whole point of this view, and no other edge kind is swimlane-relevant yet.
          nodes={(showSwimlaneView ? swimlaneNodes : displayNodes) as FlowNode<AxiomaBlockData>[]}
          edges={showSwimlaneView ? [] : displayEdges}
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
          // Swimlane View's own drag-to-allocate (below) needs to work independent of Edit
          // Mode — the click-to-allocate dropdown in `ElementInspector` already does, and the
          // two should behave consistently. `showSwimlaneView` being true already means `nodes`
          // is `swimlaneNodes`, not the normal canvas's `displayNodes`, so this doesn't loosen
          // dragging on the normal canvas at all.
          nodesDraggable={editMode || showSwimlaneView}
          nodesConnectable={editMode}
          edgesReconnectable={editMode}
          connectionRadius={40} // more forgiving than the 20px default — see the plan's Context.
          deleteKeyCode={null} // Node delete is out of scope — disconnect uses the edge's own button.
          onNodeDragStart={(_event, node) => setDraggingNodeId(node.id)}
          onNodeDragStop={(_event, node) => {
            setDraggingNodeId(null);
            if (!projectId) {
              return;
            }
            // Scope-downs pass — real drag-and-drop Swimlane reallocation (FR-CORE-12). Real
            // React Flow instance methods, not guessed:`getIntersectingNodes` reports every node
            // whose rect overlaps the dragged node's final rect; filtered to lane nodes so a drop
            // anywhere over a lane (not just its header) reallocates. No intersecting lane (e.g.
            // dropped in the gap between lanes) is a no-op, not a silent reallocation to
            // Unallocated — the dropdown remains the precise way to explicitly unallocate.
            if (showSwimlaneView) {
              const intersectingLane = reactFlowInstance
                .getIntersectingNodes(node)
                .find((n) => n.type === "swimlaneLane");
              if (intersectingLane) {
                const laneId = intersectingLane.id.replace(/^swimlane-lane-/, "");
                handleAllocate(node.id, laneId);
              }
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
                {
                  ...connection,
                  type: "axiomaEdge",
                  style: { stroke: "#7C7C86" },
                  // Matches toFlowEdge's own data shape — without this, a freshly drag-connected
                  // edge would be silently excluded from clustering's containment walk until the
                  // next full reload (a real, related bug found alongside the crash this same
                  // fix addresses).
                  data: { kind: "Contains" },
                },
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
              <select
                id="new-project-region"
                title="Region for the next new project (NFR-COMP-02)"
                value={newProjectRegion}
                onChange={(event) => setNewProjectRegion(event.target.value)}
                className="rounded border border-white/10 bg-obsidian/60 px-1 py-1 text-xs text-white/80"
              >
                {REGIONS.map((region) => (
                  <option key={region} value={region}>
                    {region}
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
                <select
                  value={newElementKind}
                  onChange={(event) =>
                    setNewElementKind(
                      event.target.value as "Structure" | "InformationElement" | "Action",
                    )
                  }
                  className="rounded border border-white/10 bg-obsidian/60 px-1 py-1 text-xs text-white/80"
                >
                  <option value="Structure">Structure</option>
                  <option value="InformationElement">Information Element</option>
                  <option value="Action">Action</option>
                </select>
              )}
              {editMode && newElementKind === "InformationElement" && (
                <select
                  value={newInfoAbstractionLevel}
                  onChange={(event) =>
                    setNewInfoAbstractionLevel(
                      event.target.value as "Conceptual" | "Logical" | "Physical",
                    )
                  }
                  className="rounded border border-white/10 bg-obsidian/60 px-1 py-1 text-xs text-white/80"
                >
                  <option value="Conceptual">Conceptual</option>
                  <option value="Logical">Logical</option>
                  <option value="Physical">Physical</option>
                </select>
              )}
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
                  setShowStagePanel(false);
                  setShowTradeStudyPanel(false);
                  setShowPartSearchPanel(false);
                  setShowAutonomyPanel(false);
                  setShowTraceabilityPanel(false);
                  setShowParametricsPanel(false);
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
                  setShowStagePanel(false);
                  setShowTradeStudyPanel(false);
                  setShowPartSearchPanel(false);
                  setShowAutonomyPanel(false);
                  setShowTraceabilityPanel(false);
                  setShowParametricsPanel(false);
                  setSelectedNodeId(null);
                }}
                className="!px-2 !py-1 text-xs"
              >
                Mission Planning
              </Button>
              <Button
                variant={showStagePanel ? "primary" : "ghost"}
                onClick={() => {
                  setShowStagePanel((v) => !v);
                  setShowHazardPanel(false);
                  setShowMissionPanel(false);
                  setShowTradeStudyPanel(false);
                  setShowPartSearchPanel(false);
                  setShowAutonomyPanel(false);
                  setShowTraceabilityPanel(false);
                  setShowParametricsPanel(false);
                  setSelectedNodeId(null);
                }}
                className="!px-2 !py-1 text-xs"
              >
                Stage Tracking
              </Button>
              <Button
                variant={showTradeStudyPanel ? "primary" : "ghost"}
                onClick={() => {
                  setShowTradeStudyPanel((v) => !v);
                  setShowHazardPanel(false);
                  setShowMissionPanel(false);
                  setShowStagePanel(false);
                  setShowPartSearchPanel(false);
                  setShowAutonomyPanel(false);
                  setShowTraceabilityPanel(false);
                  setShowParametricsPanel(false);
                  setSelectedNodeId(null);
                }}
                className="!px-2 !py-1 text-xs"
              >
                Trade Study
              </Button>
              <Button
                variant={showPartSearchPanel ? "primary" : "ghost"}
                onClick={() => {
                  setShowPartSearchPanel((v) => !v);
                  setShowHazardPanel(false);
                  setShowMissionPanel(false);
                  setShowStagePanel(false);
                  setShowTradeStudyPanel(false);
                  setShowAutonomyPanel(false);
                  setShowTraceabilityPanel(false);
                  setShowParametricsPanel(false);
                  setSelectedNodeId(null);
                }}
                className="!px-2 !py-1 text-xs"
              >
                Part Search
              </Button>
              <Button
                variant={showAutonomyPanel ? "primary" : "ghost"}
                onClick={() => {
                  setShowAutonomyPanel((v) => !v);
                  setShowHazardPanel(false);
                  setShowMissionPanel(false);
                  setShowStagePanel(false);
                  setShowTradeStudyPanel(false);
                  setShowPartSearchPanel(false);
                  setShowTraceabilityPanel(false);
                  setShowParametricsPanel(false);
                  setSelectedNodeId(null);
                }}
                className="!px-2 !py-1 text-xs"
              >
                Autonomy
              </Button>
              <Button
                variant={showTraceabilityPanel ? "primary" : "ghost"}
                onClick={() => {
                  setShowTraceabilityPanel((v) => !v);
                  setShowHazardPanel(false);
                  setShowMissionPanel(false);
                  setShowStagePanel(false);
                  setShowTradeStudyPanel(false);
                  setShowPartSearchPanel(false);
                  setShowAutonomyPanel(false);
                  setShowParametricsPanel(false);
                }}
                className="!px-2 !py-1 text-xs"
              >
                Traceability
              </Button>
              <Button
                variant={showParametricsPanel ? "primary" : "ghost"}
                onClick={() => {
                  setShowParametricsPanel((v) => !v);
                  setShowHazardPanel(false);
                  setShowMissionPanel(false);
                  setShowStagePanel(false);
                  setShowTradeStudyPanel(false);
                  setShowPartSearchPanel(false);
                  setShowAutonomyPanel(false);
                  setShowTraceabilityPanel(false);
                  setSelectedNodeId(null);
                }}
                className="!px-2 !py-1 text-xs"
              >
                Parametrics
              </Button>
              <Button
                variant={showSwimlaneView ? "primary" : "ghost"}
                onClick={() => {
                  setShowSwimlaneView((v) => !v);
                  setShowTraceabilityPanel(false);
                  setShowPartSearchPanel(false);
                }}
                className="!px-2 !py-1 text-xs"
              >
                Swimlane View
              </Button>
              <Button
                variant={showArchspacePanel ? "primary" : "ghost"}
                onClick={() => setShowArchspacePanel((v) => !v)}
                className="!px-2 !py-1 text-xs"
              >
                Architecture Design Space
              </Button>
              <Button
                variant={showTextPanel ? "primary" : "ghost"}
                onClick={() => setShowTextPanel((v) => !v)}
                className="!px-2 !py-1 text-xs"
              >
                Text View
              </Button>
              <Button
                variant="ghost"
                disabled={exportingPng}
                onClick={handleExportPng}
                className="!px-2 !py-1 text-xs"
              >
                {exportingPng ? "Exporting…" : "Export PNG"}
              </Button>
              <a
                href={projectId ? `/api/projects/${projectId}/export/full-diagram` : "#"}
                className="inline-flex items-center rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-xs text-white/80 hover:bg-white/10"
              >
                Export Full Diagram (PNG)
              </a>
              <select
                value={exportTableKind}
                onChange={(event) => setExportTableKind(event.target.value as NodeKind)}
                className="rounded border border-white/10 bg-obsidian/60 px-1 py-1 text-xs text-white/80"
              >
                {EXPORTABLE_NODE_KINDS.map((kind) => (
                  <option key={kind} value={kind}>
                    {kind}
                  </option>
                ))}
              </select>
              <select
                value={exportTableFormat}
                onChange={(event) => setExportTableFormat(event.target.value as "csv" | "xlsx")}
                className="rounded border border-white/10 bg-obsidian/60 px-1 py-1 text-xs text-white/80"
              >
                <option value="csv">CSV</option>
                <option value="xlsx">XLSX</option>
              </select>
              <a
                href={
                  projectId
                    ? `${apiPath(projectId, "/export/table")}?kind=${encodeURIComponent(exportTableKind)}&format=${exportTableFormat}`
                    : "#"
                }
                className="inline-flex items-center rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-xs text-white/80 hover:bg-white/10"
              >
                Export Table
              </a>
            </div>
          </GlassPanel>

          {selectedNode &&
            projectId &&
            !showTraceabilityPanel &&
            !showPartSearchPanel &&
            (selectedNode.data.kind === "Interaction" ? (
              <InteractionPanel
                interactionId={selectedNode.id}
                projectId={projectId}
                elements={elements}
                onClose={() => setSelectedNodeId(null)}
              />
            ) : (
              <ElementInspector
                elementId={selectedNode.id}
                elementLabel={selectedNode.data.label}
                elementKind={selectedNode.data.kind}
                projectId={projectId}
                editMode={editMode}
                onClose={() => setSelectedNodeId(null)}
                reloadModel={reloadModel}
                swimlaneView={showSwimlaneView}
                laneOptions={elements
                  .filter((e) => e.kind === "Structure" && e.id !== selectedNode.id)
                  .map((e) => ({ id: e.id, name: e.name }))}
                currentLaneId={
                  allocateEdges.find((edge) => edge.source === selectedNode.id)?.target ?? null
                }
                onAllocate={(laneId) => handleAllocate(selectedNode.id, laneId)}
                elements={elements}
              />
            ))}

          {showTraceabilityPanel && projectId && (
            <TraceabilityPanel
              selectedNode={selectedNode}
              projectId={projectId}
              onClose={() => setShowTraceabilityPanel(false)}
              savedCollections={savedCollections}
              onSaveDynamicCollection={handleSaveDynamicCollection}
              onFreezeCollection={handleFreezeCollection}
            />
          )}

          {showParametricsPanel && projectId && (
            <ParametricsPanel
              projectId={projectId}
              elements={elements}
              onClose={() => setShowParametricsPanel(false)}
            />
          )}

          {showArchspacePanel && projectId && (
            <ArchspacePanel
              projectId={projectId}
              elements={elements}
              onClose={() => setShowArchspacePanel(false)}
            />
          )}

          {showHazardPanel && projectId && (
            <HazardRiskPanel
              nodes={nodes}
              causesEdges={causesEdges}
              mitigatedByEdges={mitigatedByEdges}
              editMode={editMode}
              projectId={projectId}
              onClose={() => setShowHazardPanel(false)}
              onCreateHazard={handleCreateHazard}
              onCreateControl={handleCreateControl}
            />
          )}

          {showMissionPanel && projectId && (
            <MissionPlanningPanel
              nodes={nodes}
              concernsEdges={concernsEdges}
              editMode={editMode}
              projectId={projectId}
              onClose={() => setShowMissionPanel(false)}
              onCreateMission={handleCreateMission}
              onCreateStakeholder={handleCreateStakeholder}
            />
          )}

          {showStagePanel && projectId && (
            <StageTrackingPanel
              nodes={nodes}
              containsEdges={containsEdgesForClustering}
              projectId={projectId}
              onClose={() => setShowStagePanel(false)}
            />
          )}

          {showTradeStudyPanel && projectId && (
            <TradeStudyPanel projectId={projectId} onClose={() => setShowTradeStudyPanel(false)} />
          )}

          {showPartSearchPanel && projectId && (
            <PartSearchPanel
              projectId={projectId}
              onClose={() => setShowPartSearchPanel(false)}
              onSelectElement={(elementId) => {
                setSelectedNodeId(elementId);
                setShowPartSearchPanel(false);
              }}
            />
          )}

          {showAutonomyPanel && projectId && (
            <AutonomyPanel projectId={projectId} onClose={() => setShowAutonomyPanel(false)} />
          )}
        </ReactFlow>
      </div>
      {showTextPanel && (
        <TextualEditorPanel
          onClose={() => setShowTextPanel(false)}
          onHandleReady={setTextualHandle}
          onModelChanged={reloadModel}
        />
      )}
    </div>
  );
}
