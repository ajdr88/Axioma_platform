import { AxiomaEdge } from "./edges/AxiomaEdge";
import { AxiomaBlockNode } from "./nodes/AxiomaBlockNode";
import { SubsystemClusterNode } from "./nodes/SubsystemClusterNode";

export { computeClusteredNodes } from "./clustering";
export { NODE_HEIGHT, NODE_WIDTH } from "./dimensions";
export type { AxiomaEdgeData } from "./edges/AxiomaEdge";
export { AxiomaEdge } from "./edges/AxiomaEdge";
export { computeElkLayout } from "./layout";
export type {
  AxiomaBlockData,
  AxiomaBlockProperty,
  Origin,
  ValidationState,
} from "./nodes/AxiomaBlockNode";
export { AxiomaBlockNode } from "./nodes/AxiomaBlockNode";
export type { SubsystemClusterData } from "./nodes/SubsystemClusterNode";
export { SubsystemClusterNode } from "./nodes/SubsystemClusterNode";

/** React Flow `nodeTypes` map, ready to pass straight to `<ReactFlow nodeTypes={nodeTypes} />`. */
export const nodeTypes = {
  axiomaBlock: AxiomaBlockNode,
  subsystemCluster: SubsystemClusterNode,
};

/** React Flow `edgeTypes` map, ready to pass straight to `<ReactFlow edgeTypes={edgeTypes} />`. */
export const edgeTypes = {
  axiomaEdge: AxiomaEdge,
};
