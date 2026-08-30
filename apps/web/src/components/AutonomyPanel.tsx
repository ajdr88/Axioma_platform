"use client";

import { Button, Panel } from "@axioma/ui-components";
import { useCallback, useEffect, useRef, useState } from "react";

type AutonomyLevel = "L0" | "L1" | "L2" | "L3" | "L4";
const LEVELS: AutonomyLevel[] = ["L0", "L1", "L2", "L3", "L4"];

/** Project-wide is the only scope this pass's UI exercises — see
 * `apps/api/src/store/versioning.rs`'s `autonomy_config` table doc comment. */
const PROJECT_SCOPE = "project";

/** Matches Trade Study's own pilot scenario — the reference requirement/subsystem this whole
 * fixture is built around, not a generic pick-any-requirement tool. */
const DEFAULT_REQUIREMENT_ID = "REQ-THRUST";

interface AutonomyConfig {
  scope: string;
  level: AutonomyLevel;
  massDeviationThresholdPercent: number | null;
}

interface SubsystemParams {
  bypassRatio: number;
  pressureRatio: number;
  turbineInletTempK: number;
  turbineStageCount: number;
}

interface Candidate {
  params: SubsystemParams;
  thrustLbf: number;
  sfc: number;
  totalMassKg: number;
}

interface ProposeOutcome {
  subsystemId: string;
  outcome: "merged" | "review";
  reason: string | null;
  proposalId: string | null;
}

interface ProposeResponse {
  outcomes: ProposeOutcome[];
  branchId: string | null;
}

/** Distinct from the FR-CORE-08 `Origin` type (`Human`/`AiSuggested`/`AiAutoMerged`, a per-Element
 * provenance field) — this is `apps/api/src/store/versioning.rs`'s `proposals.origin` column,
 * which the client previously declared nothing for at all and silently discarded on every fetch. */
type ProposalOrigin = "cem-generated" | "human-authored" | "document-import";
const PROPOSAL_ORIGINS: ProposalOrigin[] = ["cem-generated", "human-authored", "document-import"];

interface Proposal {
  id: string;
  subsystemId: string;
  status: string;
  reason: string;
  origin: ProposalOrigin;
}

interface AutonomyPanelProps {
  projectId: string;
  onClose: () => void;
}

async function readError(res: Response, fallback: string): Promise<string> {
  try {
    const body = await res.json();
    return typeof body?.error === "string" ? body.error : fallback;
  } catch {
    return fallback;
  }
}

/**
 * P2.2 (Contract + Autonomy + Review, FR-CEM-16/17/18) — configures the project's L0-L4 autonomy
 * level and L3 mass-deviation threshold, runs Mode B's `optimize` + autonomy-aware `propose`
 * (`apps/api/src/mode_b.rs`'s doc comment covers the merge-vs-review split), and lists/reviews
 * whatever `propose` filed as pending proposals on the branch it just created.
 */
export function AutonomyPanel({ projectId, onClose }: AutonomyPanelProps) {
  const [level, setLevel] = useState<AutonomyLevel>("L0");
  const [thresholdInput, setThresholdInput] = useState("5");
  const [configError, setConfigError] = useState<string | null>(null);
  const [configBusy, setConfigBusy] = useState(false);
  const [configSaved, setConfigSaved] = useState(false);

  const [requirementId, setRequirementId] = useState(DEFAULT_REQUIREMENT_ID);
  const [maxMassInput, setMaxMassInput] = useState("");
  const [candidate, setCandidate] = useState<Candidate | null>(null);
  const [proposeResponse, setProposeResponse] = useState<ProposeResponse | null>(null);
  const [runError, setRunError] = useState<string | null>(null);
  const [runBusy, setRunBusy] = useState(false);

  const [branchId, setBranchId] = useState<string | null>(null);
  const [proposals, setProposals] = useState<Proposal[]>([]);
  const [proposalsError, setProposalsError] = useState<string | null>(null);
  const [reviewBusyId, setReviewBusyId] = useState<string | null>(null);
  /** FR-CORE-16's "one mechanism, three origins" review gate — filters the already-loaded
   * proposals list client-side, same pattern as the canvas's own origin-filter dropdown
   * (`page.tsx`'s `originFilter`). `human-authored`/`document-import` have no real producer yet
   * (only `mode_b.rs` ever creates a `cem-generated` proposal), so those filters legitimately show
   * nothing until Phase 1.1a's document-import pipeline and FR-PM-05 exist — not a bug. */
  const [originFilter, setOriginFilter] = useState<ProposalOrigin | "all">("all");

  // Guards against the initial GET resolving after the user has already touched the level/
  // threshold fields — without this, a slow fetch can silently stomp an in-progress edit (or,
  // worse, land after Save and revert the dropdown to the pre-save value even though the save
  // itself succeeded).
  const configTouchedRef = useRef(false);

  const loadConfig = useCallback(async () => {
    const res = await fetch(`/api/projects/${projectId}/cem/autonomy-level/${PROJECT_SCOPE}`);
    if (!res.ok || configTouchedRef.current) {
      return;
    }
    const data: AutonomyConfig = await res.json();
    setLevel(data.level);
    if (data.massDeviationThresholdPercent !== null) {
      setThresholdInput(String(data.massDeviationThresholdPercent));
    }
  }, [projectId]);

  useEffect(() => {
    loadConfig();
  }, [loadConfig]);

  const loadProposals = useCallback(
    async (branch: string) => {
      const res = await fetch(`/api/projects/${projectId}/cem/proposals/${branch}`);
      if (!res.ok) {
        setProposalsError(await readError(res, "Could not load proposals"));
        return;
      }
      setProposals(await res.json());
    },
    [projectId],
  );

  async function handleSaveConfig() {
    const threshold = thresholdInput.trim() === "" ? null : Number.parseFloat(thresholdInput);
    if (threshold !== null && Number.isNaN(threshold)) {
      return;
    }
    setConfigBusy(true);
    setConfigError(null);
    setConfigSaved(false);
    try {
      const res = await fetch(`/api/projects/${projectId}/cem/autonomy-level`, {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          scope: PROJECT_SCOPE,
          level,
          massDeviationThresholdPercent: threshold,
        }),
      });
      if (!res.ok) {
        setConfigError(await readError(res, "Could not save the autonomy level"));
        return;
      }
      setConfigSaved(true);
    } finally {
      setConfigBusy(false);
    }
  }

  function currentConstraints() {
    const maxTotalMassKg = maxMassInput.trim() === "" ? undefined : Number.parseFloat(maxMassInput);
    return maxTotalMassKg !== undefined && !Number.isNaN(maxTotalMassKg) ? { maxTotalMassKg } : {};
  }

  async function handleOptimize() {
    if (!requirementId.trim()) {
      return;
    }
    setRunBusy(true);
    setRunError(null);
    setCandidate(null);
    setProposeResponse(null);
    try {
      const res = await fetch(`/api/projects/${projectId}/cem/mode-b/optimize`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          topLevelRequirementIds: [requirementId.trim()],
          constraints: currentConstraints(),
        }),
      });
      if (!res.ok) {
        setRunError(await readError(res, "Optimize failed"));
        return;
      }
      const data: { candidates: Candidate[] } = await res.json();
      if (data.candidates.length === 0) {
        setRunError("No feasible candidates for the given targets/constraints");
        return;
      }
      setCandidate(data.candidates[0]);
    } finally {
      setRunBusy(false);
    }
  }

  async function handlePropose() {
    if (!candidate || !requirementId.trim()) {
      return;
    }
    setRunBusy(true);
    setRunError(null);
    try {
      const res = await fetch(`/api/projects/${projectId}/cem/mode-b/propose`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          candidate,
          topLevelRequirementIds: [requirementId.trim()],
          constraints: currentConstraints(),
        }),
      });
      if (!res.ok) {
        setRunError(await readError(res, "Propose failed"));
        return;
      }
      const data: ProposeResponse = await res.json();
      setProposeResponse(data);
      if (data.branchId) {
        setBranchId(data.branchId);
        await loadProposals(data.branchId);
      }
    } finally {
      setRunBusy(false);
    }
  }

  async function handleReview(proposalId: string, action: "accept" | "reject") {
    setReviewBusyId(proposalId);
    setProposalsError(null);
    try {
      const res = await fetch(`/api/projects/${projectId}/cem/proposals/${proposalId}/${action}`, {
        method: "POST",
      });
      if (!res.ok) {
        setProposalsError(await readError(res, `Could not ${action} proposal`));
        return;
      }
      if (branchId) {
        await loadProposals(branchId);
      }
    } finally {
      setReviewBusyId(null);
    }
  }

  return (
    <Panel className="absolute right-4 top-4 z-10 w-96 max-w-[calc(100vw-2rem)] max-h-[calc(100vh-2rem)] overflow-y-auto p-4">
      <div className="mb-3 flex items-start justify-between gap-2">
        <p className="text-sm font-semibold text-white/90">Autonomy &amp; Proposals</p>
        <Button variant="ghost" onClick={onClose} className="!px-2 !py-1 text-xs">
          Close
        </Button>
      </div>

      <div className="mb-3 border-b border-white/10 pb-3">
        <p className="mb-1 text-[10px] uppercase tracking-widest text-white/40">
          Autonomy level (project-wide)
        </p>
        <div className="flex gap-1.5">
          <select
            data-autonomy-level-select
            value={level}
            onChange={(event) => {
              configTouchedRef.current = true;
              setLevel(event.target.value as AutonomyLevel);
              setConfigSaved(false);
            }}
            className="flex-1 rounded border border-white/10 bg-obsidian/60 px-1.5 py-1 text-xs text-white/80"
          >
            {LEVELS.map((l) => (
              <option key={l} value={l}>
                {l}
              </option>
            ))}
          </select>
          <input
            data-autonomy-threshold-input
            type="number"
            step="0.1"
            value={thresholdInput}
            onChange={(event) => {
              configTouchedRef.current = true;
              setThresholdInput(event.target.value);
              setConfigSaved(false);
            }}
            placeholder="L3 threshold %"
            className="w-24 rounded border border-white/10 bg-obsidian/60 px-1.5 py-1 text-xs text-white/80 outline-none"
          />
          <Button
            variant="primary"
            className="!px-2 !py-1 text-xs"
            disabled={configBusy}
            onClick={handleSaveConfig}
          >
            Save
          </Button>
        </div>
        {configError && <p className="mt-2 text-xs text-alert">{configError}</p>}
        {configSaved && !configError && <p className="mt-2 text-[11px] text-white/60">Saved.</p>}
      </div>

      <div className="mb-3 border-b border-white/10 pb-3">
        <p className="mb-1 text-[10px] uppercase tracking-widest text-white/40">
          Requirement / constraint
        </p>
        <input
          data-autonomy-requirement-input
          value={requirementId}
          onChange={(event) => setRequirementId(event.target.value)}
          placeholder="Top-level requirement id"
          className="mb-1.5 w-full rounded border border-white/10 bg-obsidian/60 px-1.5 py-1 text-xs text-white/80 outline-none"
        />
        <input
          data-autonomy-max-mass-input
          type="number"
          step="1"
          value={maxMassInput}
          onChange={(event) => setMaxMassInput(event.target.value)}
          placeholder="Max total mass (kg), optional"
          className="mb-1.5 w-full rounded border border-white/10 bg-obsidian/60 px-1.5 py-1 text-xs text-white/80 outline-none"
        />
        <div className="flex gap-1.5">
          <Button
            variant="ghost"
            className="flex-1 !py-1 text-xs"
            disabled={runBusy || !requirementId.trim()}
            onClick={handleOptimize}
          >
            Optimize
          </Button>
          <Button
            variant="primary"
            className="flex-1 !py-1 text-xs"
            disabled={runBusy || !candidate}
            onClick={handlePropose}
          >
            Propose
          </Button>
        </div>

        {runError && <p className="mt-2 text-xs text-alert">{runError}</p>}

        {candidate && (
          <div data-autonomy-candidate className="mt-2 rounded border border-white/10 p-2 text-xs">
            <p className="font-mono text-white/80">
              {candidate.totalMassKg.toFixed(0)} kg &middot; {candidate.thrustLbf.toFixed(0)} lbf
              &middot; SFC {candidate.sfc.toFixed(3)}
            </p>
          </div>
        )}

        {proposeResponse && (
          <div data-autonomy-outcomes className="mt-2 space-y-1">
            {proposeResponse.outcomes.map((o) => (
              <p key={o.subsystemId} className="text-[11px]">
                <span className="font-mono text-white/80">{o.subsystemId}</span>{" "}
                <span className={o.outcome === "merged" ? "text-white/60" : "text-alert"}>
                  {o.outcome}
                </span>
                {o.reason && <span className="text-graphite"> ({o.reason})</span>}
              </p>
            ))}
          </div>
        )}
      </div>

      <div>
        <div className="mb-1 flex items-center justify-between gap-2">
          <p className="text-[10px] uppercase tracking-widest text-white/40">
            Pending proposals{branchId ? ` (branch ${branchId.slice(0, 8)})` : ""}
          </p>
          <select
            data-autonomy-origin-filter
            value={originFilter}
            onChange={(event) => setOriginFilter(event.target.value as ProposalOrigin | "all")}
            className="rounded border border-white/10 bg-obsidian/60 px-1 py-0.5 text-[10px] text-white/70"
          >
            <option value="all">all origins</option>
            {PROPOSAL_ORIGINS.map((o) => (
              <option key={o} value={o}>
                {o}
              </option>
            ))}
          </select>
        </div>
        {proposalsError && <p className="mb-2 text-xs text-alert">{proposalsError}</p>}
        {!branchId && (
          <p className="text-[11px] text-white/40">
            Propose a candidate that needs review to see it here.
          </p>
        )}
        {branchId && proposals.length === 0 && !proposalsError && (
          <p className="text-[11px] text-white/40">No proposals on this branch.</p>
        )}
        <div data-autonomy-proposals className="space-y-1.5">
          {proposals
            .filter((p) => originFilter === "all" || p.origin === originFilter)
            .map((p) => (
              <div
                key={p.id}
                data-autonomy-proposal-id={p.id}
                className="flex items-center justify-between gap-2 rounded border border-white/10 p-2 text-xs"
              >
                <div>
                  <p className="font-mono text-white/80">
                    {p.subsystemId}{" "}
                    <span
                      className="rounded bg-white/5 px-1 py-0.5 text-[9px] uppercase tracking-wide text-white/50"
                      title="Proposal origin (FR-CORE-16)"
                    >
                      {p.origin}
                    </span>
                  </p>
                  <p className="text-[10px] text-graphite">
                    {p.status} &middot; {p.reason}
                  </p>
                </div>
                {p.status === "pending" && (
                  <div className="flex gap-1">
                    <Button
                      variant="primary"
                      className="!px-2 !py-1 text-[10px]"
                      disabled={reviewBusyId === p.id}
                      onClick={() => handleReview(p.id, "accept")}
                    >
                      Accept
                    </Button>
                    <Button
                      variant="ghost"
                      className="!px-2 !py-1 text-[10px]"
                      disabled={reviewBusyId === p.id}
                      onClick={() => handleReview(p.id, "reject")}
                    >
                      Reject
                    </Button>
                  </div>
                )}
              </div>
            ))}
        </div>
      </div>
    </Panel>
  );
}
