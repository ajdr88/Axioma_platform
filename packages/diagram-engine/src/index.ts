import { AxiomaBlockNode } from "./nodes/AxiomaBlockNode";

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
