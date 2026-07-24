/**
 * Hand-written for now. impl §3.1 ("Working conventions"): "Keep TypeScript types generated from
 * Rust structs (shared-types) in sync — don't hand-edit." The generator doesn't exist yet
 * (see scripts/generate.js) — until it does, these types are kept in sync by hand with
 * packages/sysml-core/src/lib.rs.
 */

export type NodeKind =
  | "Element"
  | "Structure"
  | "Requirement"
  | "Port"
  | "Hazard"
  | "Control"
  | "Mission"
  | "Stakeholder"
  | "SimulationRun";

export type EdgeKind =
  | "Contains"
  | "Satisfy"
  | "Verify"
  | "Refine"
  | "Causes"
  | "MitigatedBy"
  | "ValidatedBy"
  | "Suspect";

export interface Element {
  id: string;
  kind: NodeKind;
  name: string;
}

export interface Edge {
  source: string;
  target: string;
  kind: EdgeKind;
}
