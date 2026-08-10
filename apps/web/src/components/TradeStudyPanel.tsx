"use client";

import type { Branch } from "@axioma/shared-types";
import { Button, Panel } from "@axioma/ui-components";
import { useCallback, useEffect, useState } from "react";

interface TradeStudyReport {
  branch: string;
  elementId: string;
  property: string;
  baseline: { bypassRatio: number; thrustLbf: number };
  variant: { bypassRatio: number; thrustLbf: number };
  delta: { thrustLbf: number; percent: number };
  simulation: { converged: boolean; finalRpm: string | null; note: string };
}

/** Matches `apps/api/src/trade_study.rs`'s request defaults — the pilot scenario T-P1.4-05
 * literally names ("swap a Fan variant"), not a generic pick-any-element trade-study tool. */
const TARGET_ELEMENT_ID = "FanLpCompression";
const TARGET_PROPERTY = "bypassRatio";

interface TradeStudyPanelProps {
  projectId: string;
  onClose: () => void;
}

/**
 * T-P1.4-05's pilot trade-study workflow: branch, swap `FanLpCompression`'s bypass ratio on the
 * branch (the existing branch-scoped property edit, T-P1.1-05), run the pilot's Control
 * state-machine sim, and show the estimated thrust delta against `main`'s current value — see
 * `apps/api/src/trade_study.rs`'s doc comment for the thrust formula/scope this report is built
 * on (a documented, deliberately simple stand-in, not a real performance model).
 */
export function TradeStudyPanel({ projectId, onClose }: TradeStudyPanelProps) {
  const [branches, setBranches] = useState<Branch[]>([]);
  const [selectedBranch, setSelectedBranch] = useState("");
  const [newBranchName, setNewBranchName] = useState("");
  const [bypassRatioInput, setBypassRatioInput] = useState("6.5");
  const [report, setReport] = useState<TradeStudyReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const loadBranches = useCallback(async () => {
    const res = await fetch(`/api/projects/${projectId}/branches`);
    if (!res.ok) {
      return;
    }
    const data: Branch[] = await res.json();
    setBranches(data.filter((b) => b.name !== "main"));
  }, [projectId]);

  useEffect(() => {
    loadBranches();
  }, [loadBranches]);

  async function readError(res: Response, fallback: string): Promise<string> {
    try {
      const body = await res.json();
      return typeof body?.error === "string" ? body.error : fallback;
    } catch {
      return fallback;
    }
  }

  async function handleCreateBranch() {
    const name = newBranchName.trim();
    if (!name) {
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const res = await fetch(`/api/projects/${projectId}/branches`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ name }),
      });
      if (!res.ok) {
        setError(await readError(res, `Could not create branch ${name}`));
        return;
      }
      setNewBranchName("");
      await loadBranches();
      setSelectedBranch(name);
    } finally {
      setBusy(false);
    }
  }

  async function handleApplyVariant() {
    const bypassRatio = Number.parseFloat(bypassRatioInput);
    if (!selectedBranch || Number.isNaN(bypassRatio)) {
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const res = await fetch(
        `/api/projects/${projectId}/branches/${selectedBranch}/elements/${TARGET_ELEMENT_ID}/body`,
        {
          method: "PATCH",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            properties: { [TARGET_PROPERTY]: bypassRatio },
            message: `Trade study: bypass ratio -> ${bypassRatio}`,
          }),
        },
      );
      if (!res.ok) {
        setError(await readError(res, "Could not apply the variant edit"));
      }
    } finally {
      setBusy(false);
    }
  }

  async function handleCompare() {
    if (!selectedBranch) {
      return;
    }
    setBusy(true);
    setError(null);
    setReport(null);
    try {
      const res = await fetch(`/api/projects/${projectId}/trade-studies/compare`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ branch: selectedBranch }),
      });
      if (!res.ok) {
        setError(await readError(res, "Could not run the comparison"));
        return;
      }
      setReport(await res.json());
    } finally {
      setBusy(false);
    }
  }

  return (
    <Panel className="absolute right-4 top-4 z-10 w-96 max-w-[calc(100vw-2rem)] max-h-[calc(100vh-2rem)] overflow-y-auto p-4">
      <div className="mb-3 flex items-start justify-between gap-2">
        <p className="text-sm font-semibold text-white/90">Trade Study</p>
        <Button variant="ghost" onClick={onClose} className="!px-2 !py-1 text-xs">
          Close
        </Button>
      </div>

      <p className="mb-3 text-[11px] text-white/60">
        Branch, swap {TARGET_ELEMENT_ID}&apos;s bypass ratio, run the pilot sim, and compare the
        estimated thrust against main&apos;s current value.
      </p>

      {error && <p className="mb-2 text-xs text-alert">{error}</p>}

      <div className="mb-3">
        <p className="mb-1 text-[10px] uppercase tracking-widest text-white/40">Variant branch</p>
        <select
          data-trade-study-branch-select
          value={selectedBranch}
          onChange={(event) => {
            setSelectedBranch(event.target.value);
            setReport(null);
          }}
          className="w-full rounded border border-white/10 bg-obsidian/60 px-1.5 py-1 text-xs text-white/80"
        >
          <option value="">Select a branch…</option>
          {branches.map((b) => (
            <option key={b.id} value={b.name}>
              {b.name}
            </option>
          ))}
        </select>
        <div className="mt-1.5 flex gap-1.5">
          <input
            value={newBranchName}
            onChange={(event) => setNewBranchName(event.target.value)}
            placeholder="New branch name"
            className="flex-1 rounded border border-white/10 bg-obsidian/60 px-1.5 py-1 text-xs text-white/80 outline-none"
          />
          <Button
            variant="ghost"
            className="!px-2 !py-1 text-xs"
            disabled={busy || !newBranchName.trim()}
            onClick={handleCreateBranch}
          >
            + Branch
          </Button>
        </div>
      </div>

      <div className="mb-3">
        <p className="mb-1 text-[10px] uppercase tracking-widest text-white/40">
          {TARGET_ELEMENT_ID} bypass ratio (variant)
        </p>
        <div className="flex gap-1.5">
          <input
            data-trade-study-bypass-ratio
            type="number"
            step="0.1"
            value={bypassRatioInput}
            onChange={(event) => setBypassRatioInput(event.target.value)}
            className="flex-1 rounded border border-white/10 bg-obsidian/60 px-1.5 py-1 text-xs text-white/80 outline-none"
          />
          <Button
            variant="ghost"
            className="!px-2 !py-1 text-xs"
            disabled={busy || !selectedBranch}
            onClick={handleApplyVariant}
          >
            Apply
          </Button>
        </div>
      </div>

      <Button
        variant="primary"
        className="w-full !py-1.5 text-xs"
        disabled={busy || !selectedBranch}
        onClick={handleCompare}
      >
        Run sim &amp; compare
      </Button>

      {report && (
        <div data-trade-study-report className="mt-3 rounded border border-white/10 p-2 text-xs">
          <div className="mb-1.5 grid grid-cols-2 gap-2">
            <div>
              <p className="text-[10px] uppercase tracking-widest text-white/40">Main</p>
              <p className="font-mono text-white/80">
                {report.baseline.bypassRatio.toFixed(2)} BPR
              </p>
              <p className="font-mono text-white/80">{report.baseline.thrustLbf.toFixed(0)} lbf</p>
            </div>
            <div>
              <p className="text-[10px] uppercase tracking-widest text-white/40">{report.branch}</p>
              <p className="font-mono text-white/80">{report.variant.bypassRatio.toFixed(2)} BPR</p>
              <p className="font-mono text-white/80">{report.variant.thrustLbf.toFixed(0)} lbf</p>
            </div>
          </div>
          <p
            data-thrust-delta
            className={`mb-1.5 font-mono ${
              report.delta.thrustLbf < 0 ? "text-alert" : "text-white/90"
            }`}
          >
            &Delta; {report.delta.thrustLbf >= 0 ? "+" : ""}
            {report.delta.thrustLbf.toFixed(0)} lbf ({report.delta.percent.toFixed(1)}%)
          </p>
          <p className="text-[10px] text-white/60">
            Sim: {report.simulation.converged ? "converged" : "did not converge"}
            {report.simulation.finalRpm && ` (final rpm ${report.simulation.finalRpm})`}
          </p>
          <p className="mt-1 text-[10px] text-graphite">{report.simulation.note}</p>
        </div>
      )}
    </Panel>
  );
}
