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
  | "SimulationRun"
  // docs/IMPLEMENTATION_KICKOFF.md Phase 1 — mirrors packages/sysml-core/src/lib.rs's NodeKind
  // exactly; see that enum's own doc comments for what each addition is for.
  | "Constraint"
  | "Parameter"
  | "InformationElement"
  | "Interaction"
  | "InteractionFragment"
  | "Collection"
  | "CandidateStructureSuggestion"
  | "Function"
  | "SelectionChoice"
  | "ConnectionChoice"
  // FR-CORE-13 real build-out — mirrors packages/sysml-core/src/lib.rs's NodeKind::Action exactly.
  | "Action";

export type EdgeKind =
  | "Contains"
  | "Satisfy"
  | "Verify"
  | "Refine"
  | "Causes"
  | "MitigatedBy"
  | "ValidatedBy"
  | "Suspect"
  | "Concerns"
  // Phase 1 — mirrors packages/sysml-core/src/lib.rs's EdgeKind exactly.
  | "Bound"
  | "Derive"
  | "Copy"
  | "Member"
  | "ArchDerives"
  | "IncompatibleWith"
  | "ChoiceConstraint"
  // Phase 5 (FR-CORE-12) — mirrors packages/sysml-core/src/lib.rs's EdgeKind::Allocate exactly.
  | "Allocate"
  // FR-CORE-13 real build-out — mirrors packages/sysml-core/src/lib.rs's EdgeKind::Flow exactly.
  | "Flow";

/** FR-CORE-08 provenance origin — who/what created an element. */
export type Origin = "Human" | "AiSuggested" | "AiAutoMerged";

export interface Element {
  id: string;
  kind: NodeKind;
  name: string;
  /** Excluded from *future* system-optimization loops (Mode B, not built yet) when false. Keeps
   * all its data either way — this is a visual/modeling marker, not a delete. */
  active: boolean;
  origin: Origin;
}

export interface Edge {
  source: string;
  target: string;
  kind: EdgeKind;
  /** A generic JSON tag for edge-kind-specific data too small to warrant a dedicated field per
   * kind — mirrors `packages/sysml-core/src/lib.rs::Edge::metadata` exactly. Only `ChoiceConstraint`
   * populates it today: `{ choiceConstraintType: "Linked" | "Permutation" | "Unordered" |
   * "UnorderedNorepl" }`, mirroring `adsg_core.ChoiceConstraintType`. Absent for every other kind. */
  metadata?: unknown;
}

/** Git-backed model versioning (roadmap: P1.1, T-P1.1-05) — mirrors `store::versioning::Branch`.
 * `headCommitId`/`forkCommitId` are `null` for a brand-new `main` branch with no commits yet. */
export interface Branch {
  id: string;
  projectId: string;
  name: string;
  headCommitId: string | null;
  forkCommitId: string | null;
}
