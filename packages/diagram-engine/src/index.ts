import { AxiomaEdge } from "./edges/AxiomaEdge";
import { AxiomaBlockNode } from "./nodes/AxiomaBlockNode";

export type { AxiomaEdgeData } from "./edges/AxiomaEdge";
export { AxiomaEdge } from "./edges/AxiomaEdge";
export { computeGridLayout } from "./layout";
export type {
  AxiomaBlockData,
  AxiomaBlockProperty,
  Origin,
  ValidationState,
} from "./nodes/AxiomaBlockNode";
export { AxiomaBlockNode } from "./nodes/AxiomaBlockNode";

/** React Flow `nodeTypes` map, ready to pass straight to `<ReactFlow nodeTypes={nodeTypes} />`. */
export const nodeTypes = {
  axiomaBlock: AxiomaBlockNode,
};

/** React Flow `edgeTypes` map, ready to pass straight to `<ReactFlow edgeTypes={edgeTypes} />`. */
export const edgeTypes = {
  axiomaEdge: AxiomaEdge,
};
