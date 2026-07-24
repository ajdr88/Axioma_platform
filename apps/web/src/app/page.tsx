"use client";

import { type AxiomaBlockData, nodeTypes } from "@axioma/diagram-engine";
import { Panel as GlassPanel } from "@axioma/ui-components";
import {
  Background,
  BackgroundVariant,
  Controls,
  type Edge,
  type Node,
  ReactFlow,
  ReactFlowProvider,
} from "@xyflow/react";

/**
 * `Turbofan-Ref` structural fixture (docs/Axioma_test_specification_v3.md §0, P1.1+): `Engine`
 * composed of the five reference subsystems. Hardcoded here to prove the render path — this will
 * be replaced by a real fetch against `GET /api/v0/elements` once the frontend talks to `apps/api`.
 */
const nodes: Node<AxiomaBlockData>[] = [
  {
    id: "Engine",
    type: "axiomaBlock",
    position: { x: 400, y: 0 },
    data: {
      label: "Engine",
      origin: "human",
      validation: "unverified",
      properties: [{ id: "p1", name: "thrust", type: "REQ-THRUST" }],
    },
  },
  {
    id: "FanLpCompression",
    type: "axiomaBlock",
    position: { x: 0, y: 180 },
    data: {
      label: "Fan & LP Compression",
      origin: "human",
      validation: "test-validated",
      properties: [{ id: "p1", name: "mass", type: "kg" }],
    },
  },
  {
    id: "CoreHpCompressor",
    type: "axiomaBlock",
    position: { x: 220, y: 180 },
    data: {
      label: "Core (HP) Compressor",
      origin: "ai-suggested",
      validation: "solver-validated",
      properties: [{ id: "p1", name: "pressureRatio", type: "f64" }],
    },
  },
  {
    id: "Combustor",
    type: "axiomaBlock",
    position: { x: 440, y: 180 },
    data: {
      label: "Combustor",
      origin: "human",
      validation: "unverified",
      properties: [{ id: "p1", name: "inletTemp", type: "K" }],
    },
  },
  {
    id: "TurbineHpLp",
    type: "axiomaBlock",
    position: { x: 660, y: 180 },
    data: {
      label: "Turbine (HP & LP)",
      origin: "ai-auto-merged",
      validation: "solver-validated",
      suspect: true,
      properties: [{ id: "p1", name: "rpm", type: "u32" }],
    },
  },
  {
    id: "ControlFadecEec",
    type: "axiomaBlock",
    position: { x: 880, y: 180 },
    data: {
      label: "Control (FADEC/EEC)",
      origin: "human",
      validation: "unverified",
      properties: [{ id: "p1", name: "state", type: "StateMachine" }],
    },
  },
];

const subsystemIds = [
  "FanLpCompression",
  "CoreHpCompressor",
  "Combustor",
  "TurbineHpLp",
  "ControlFadecEec",
];

const edges: Edge[] = subsystemIds.map((id) => ({
  id: `Engine-contains-${id}`,
  source: "Engine",
  target: id,
  type: "smoothstep",
  style: { stroke: "#7C7C86" },
}));

export default function Home() {
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
            <p className="mt-0.5 font-mono text-[10px] uppercase tracking-widest text-white/40">
              Turbofan-Ref &middot; P1.1 Core Graph
            </p>
          </GlassPanel>
        </ReactFlow>
      </ReactFlowProvider>
    </div>
  );
}
