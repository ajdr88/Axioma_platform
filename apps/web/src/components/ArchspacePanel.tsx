"use client";

import type { Element as ApiElement } from "@axioma/shared-types";
import { Button, Panel } from "@axioma/ui-components";
import { useState } from "react";

interface SkippedItem {
  elementId: string;
  reason: string;
}

interface DesignSpaceStats {
  nDesignVariables: number;
  nDeclared: number;
  nValid: number;
  imputationRatio: number;
  /** FR-ARCH-06's other three real metrics — absent (not 0) when this design space has no
   * objective, same "real absence, not a fake zero" precedent as the backend's own DTO. */
  correctionRatio?: number;
  discreteCorrectionRatio?: number;
  continuousCorrectionRatio?: number;
  correctionFraction?: number;
  maxRateDiversity?: number;
}

interface DefineResponse {
  handleId: string;
  stats: DesignSpaceStats;
  skipped: SkippedItem[];
}

interface DecodedChoice {
  choiceId: string;
  presentOption: string | null;
}

interface DecodeResponse {
  designVector: number[];
  isActive: boolean[];
  presentNodeNames: string[];
  choices: DecodedChoice[];
  otherPresentNodes: string[];
  refreshedHandleId?: string;
}

interface Viability {
  state: string;
  probabilityOfViability: number;
  objectiveValue: number | null;
  trainingSamplesUsed: number;
}

interface GeneratedInstance {
  designVector: number[];
  presentNodeNames: string[];
  choices: DecodedChoice[];
  viability: Viability;
}

/** Tier 1 pass (item 6) — `refreshedHandleId` is set only when the handle sent in the request had
 * gone stale (e.g. a sidecar restart) and was transparently recovered server-side; present on
 * every archspace response that takes a `handleId`, per its own backend DTO. */
interface GenerateInstancesResponse {
  instances: GeneratedInstance[];
  refreshedHandleId?: string;
}

interface ProposeResponse {
  proposalId: string;
  branchId: string;
  viability: Viability;
  refreshedHandleId?: string;
}

/** Tier 1 pass (item 7) — real multi-objective search. `bestObjectiveValues` has one real entry
 * per encoded design variable now (was a single value before this pass). */
interface OptimizeResponse {
  algorithm: string;
  bestObjectiveValues: number[];
  bestDesignVector: number[];
  refreshedHandleId?: string;
}

interface DerivedExistenceResponse {
  derivedElementIds: string[];
  withinCycle: string[];
}

/** FR-ARCH-08's typed states — color-coded the same way this app's other validation/status
 * badges already are (green = good, amber = needs a closer look, red = real problem), not a new
 * visual convention. */
function viabilityBadgeClass(state: string): string {
  if (state === "Converged") return "text-cobalt-glow";
  if (state === "Suspect-Numerical") return "text-yellow-400";
  return "text-alert";
}

interface ArchspacePanelProps {
  projectId: string;
  /** Filtered locally to `kind === "Structure"` — no dedicated subsystem-list endpoint exists,
   * the same "filter the already-loaded element list" pattern `ParametricsPanel`/
   * `ElementInspector`'s Collection-members section already use. */
  elements: ApiElement[];
  onClose: () => void;
}

/**
 * FR-ARCH-01…08's real build-out (reqs v5 §5.17) — defines a design space from a subsystem's real
 * graph content via `cem_core::archspace::encode_design_space` + the `cem-archspace` sidecar
 * (FR-ARCH-05/06), decodes a real instance from it, generates/compares a real browsable set of
 * instances each carrying a real FR-ARCH-08 typed viability signal (a real
 * `sb_arch_opt.algo.arch_sbo.hc_strategy.RandomForestClassifier`, not a heuristic stand-in), and
 * proposes one specific instance into the real `/cem/proposals/*` review-gate flow (FR-ARCH-07) —
 * the same accept/reject UI every other proposal origin already uses, not a separate mechanism.
 * Still deliberately minimal: no resolution UI here (`PATCH .../cem/archspace/choices/:id/resolve`
 * is exercised via curl/tests, not yet a dedicated control).
 */
export function ArchspacePanel({ projectId, elements, onClose }: ArchspacePanelProps) {
  const subsystems = elements.filter((e) => e.kind === "Structure");
  const [subsystemId, setSubsystemId] = useState(subsystems[0]?.id ?? "");
  const [defining, setDefining] = useState(false);
  const [defineError, setDefineError] = useState<string | null>(null);
  const [defineResult, setDefineResult] = useState<DefineResponse | null>(null);
  const [decoding, setDecoding] = useState(false);
  const [decodeError, setDecodeError] = useState<string | null>(null);
  const [decodeResult, setDecodeResult] = useState<DecodeResponse | null>(null);
  const [generating, setGenerating] = useState(false);
  const [generateError, setGenerateError] = useState<string | null>(null);
  const [generatedInstances, setGeneratedInstances] = useState<GeneratedInstance[] | null>(null);
  const [proposingIndex, setProposingIndex] = useState<number | null>(null);
  const [proposeError, setProposeError] = useState<string | null>(null);
  const [proposedByIndex, setProposedByIndex] = useState<Record<number, ProposeResponse>>({});
  const [derivedExistenceLoading, setDerivedExistenceLoading] = useState(false);
  const [derivedExistenceError, setDerivedExistenceError] = useState<string | null>(null);
  const [derivedExistenceResult, setDerivedExistenceResult] =
    useState<DerivedExistenceResponse | null>(null);
  const [optimizeAlgorithm, setOptimizeAlgorithm] = useState<"nsga2" | "hierarchical-bo">("nsga2");
  const [optimizing, setOptimizing] = useState(false);
  const [optimizeError, setOptimizeError] = useState<string | null>(null);
  const [optimizeResult, setOptimizeResult] = useState<OptimizeResponse | null>(null);

  async function handleDefine() {
    if (!subsystemId) return;
    setDefining(true);
    setDefineError(null);
    setDecodeResult(null);
    setDecodeError(null);
    try {
      const res = await fetch(`/api/projects/${projectId}/cem/archspace/${subsystemId}/define`, {
        method: "POST",
      });
      if (!res.ok) {
        const err = await res.json().catch(() => null);
        throw new Error(err?.error ?? `request failed with status ${res.status}`);
      }
      setDefineResult(await res.json());
    } catch (err) {
      setDefineError(err instanceof Error ? err.message : "failed to define design space");
    } finally {
      setDefining(false);
    }
  }

  /** Tier 1 pass (item 6) — a stale handle (e.g. a sidecar restart) is transparently recovered
   * server-side; this keeps the panel's own `defineResult.handleId` pointed at the fresh one so
   * the *next* call in this session doesn't have to pay the recovery cost again. */
  function updateHandleIfRefreshed(refreshedHandleId?: string) {
    if (!refreshedHandleId) return;
    setDefineResult((prev) => (prev ? { ...prev, handleId: refreshedHandleId } : prev));
  }

  async function handleDecode() {
    if (!defineResult) return;
    setDecoding(true);
    setDecodeError(null);
    try {
      const res = await fetch(
        `/api/projects/${projectId}/cem/archspace/${defineResult.handleId}/decode`,
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({}),
        },
      );
      if (!res.ok) {
        const err = await res.json().catch(() => null);
        throw new Error(err?.error ?? `request failed with status ${res.status}`);
      }
      const decoded: DecodeResponse = await res.json();
      updateHandleIfRefreshed(decoded.refreshedHandleId);
      setDecodeResult(decoded);
    } catch (err) {
      setDecodeError(err instanceof Error ? err.message : "failed to decode instance");
    } finally {
      setDecoding(false);
    }
  }

  async function handleGenerate() {
    if (!defineResult) return;
    setGenerating(true);
    setGenerateError(null);
    setGeneratedInstances(null);
    setProposedByIndex({});
    try {
      const res = await fetch(
        `/api/projects/${projectId}/cem/archspace/${defineResult.handleId}/generate-instances`,
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ count: 5 }),
        },
      );
      if (!res.ok) {
        const err = await res.json().catch(() => null);
        throw new Error(err?.error ?? `request failed with status ${res.status}`);
      }
      const generated: GenerateInstancesResponse = await res.json();
      updateHandleIfRefreshed(generated.refreshedHandleId);
      setGeneratedInstances(generated.instances);
    } catch (err) {
      setGenerateError(err instanceof Error ? err.message : "failed to generate instances");
    } finally {
      setGenerating(false);
    }
  }

  /** Tier 1 pass (item 7) — the first real, human-reachable way to trigger `RunOptimization`
   * (previously zero real HTTP callers). `hierarchical-bo` genuinely trains a real Gaussian-
   * Process surrogate server-side and can take significantly longer than `nsga2` — no client-side
   * timeout is set, matching the panel's other long-ish calls (Generate & Compare). */
  async function handleOptimize() {
    if (!defineResult) return;
    setOptimizing(true);
    setOptimizeError(null);
    setOptimizeResult(null);
    try {
      const res = await fetch(
        `/api/projects/${projectId}/cem/archspace/${defineResult.handleId}/optimize`,
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            algorithm: optimizeAlgorithm,
            populationSize: 10,
            nGenerations: 5,
          }),
        },
      );
      if (!res.ok) {
        const err = await res.json().catch(() => null);
        throw new Error(err?.error ?? `request failed with status ${res.status}`);
      }
      const optimized: OptimizeResponse = await res.json();
      updateHandleIfRefreshed(optimized.refreshedHandleId);
      setOptimizeResult(optimized);
    } catch (err) {
      setOptimizeError(err instanceof Error ? err.message : "failed to run optimization");
    } finally {
      setOptimizing(false);
    }
  }

  async function handlePropose(index: number, instance: GeneratedInstance) {
    if (!defineResult) return;
    setProposingIndex(index);
    setProposeError(null);
    try {
      const res = await fetch(
        `/api/projects/${projectId}/cem/archspace/${defineResult.handleId}/propose`,
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ designVector: instance.designVector, subsystemId }),
        },
      );
      if (!res.ok) {
        const err = await res.json().catch(() => null);
        throw new Error(err?.error ?? `request failed with status ${res.status}`);
      }
      const proposed: ProposeResponse = await res.json();
      updateHandleIfRefreshed(proposed.refreshedHandleId);
      setProposedByIndex((prev) => ({ ...prev, [index]: proposed }));
    } catch (err) {
      setProposeError(err instanceof Error ? err.message : "failed to propose instance");
    } finally {
      setProposingIndex(null);
    }
  }

  async function handleCheckDerivedExistence() {
    if (!subsystemId) return;
    setDerivedExistenceLoading(true);
    setDerivedExistenceError(null);
    setDerivedExistenceResult(null);
    try {
      const res = await fetch(
        `/api/projects/${projectId}/cem/archspace/${subsystemId}/derived-existence?seedIds=${encodeURIComponent(subsystemId)}`,
      );
      if (!res.ok) {
        const err = await res.json().catch(() => null);
        throw new Error(err?.error ?? `request failed with status ${res.status}`);
      }
      setDerivedExistenceResult(await res.json());
    } catch (err) {
      setDerivedExistenceError(
        err instanceof Error ? err.message : "failed to evaluate derived existence",
      );
    } finally {
      setDerivedExistenceLoading(false);
    }
  }

  return (
    <Panel className="absolute right-4 top-4 z-10 w-96 max-w-[calc(100vw-2rem)] max-h-[calc(100vh-2rem)] overflow-y-auto p-4">
      <div className="mb-3 flex items-start justify-between gap-2">
        <p className="text-sm font-semibold text-white/90">Architecture Design Space</p>
        <Button variant="ghost" onClick={onClose} className="!px-2 !py-1 text-xs">
          Close
        </Button>
      </div>

      {subsystems.length === 0 && (
        <p className="text-xs text-white/40">No Structure elements exist in this project.</p>
      )}

      {subsystems.length > 0 && (
        <>
          <label
            htmlFor="archspace-subsystem"
            className="mb-1 block text-[10px] uppercase tracking-widest text-white/40"
          >
            Subsystem
          </label>
          <select
            id="archspace-subsystem"
            value={subsystemId}
            onChange={(event) => {
              setSubsystemId(event.target.value);
              setDefineResult(null);
              setDecodeResult(null);
            }}
            className="mb-3 w-full rounded border border-white/10 bg-obsidian/60 px-1.5 py-1 text-xs text-white/80"
          >
            {subsystems.map((s) => (
              <option key={s.id} value={s.id}>
                {s.name}
              </option>
            ))}
          </select>

          <Button
            onClick={handleDefine}
            disabled={defining || !subsystemId}
            className="mb-3 w-full justify-center !py-1 text-xs"
          >
            {defining ? "Defining…" : "Define Design Space"}
          </Button>

          {/* FR-ARCH-02 — reads the real graph directly (no design space handle needed), unlike
           * everything below it, so it's available as soon as a subsystem is picked. */}
          <Button
            onClick={handleCheckDerivedExistence}
            disabled={derivedExistenceLoading || !subsystemId}
            className="mb-3 w-full justify-center !py-1 text-xs"
          >
            {derivedExistenceLoading ? "Checking…" : "Check Derived Existence"}
          </Button>

          {derivedExistenceError && (
            <p className="mb-2 text-xs text-alert">{derivedExistenceError}</p>
          )}

          {derivedExistenceResult && (
            <div
              className="mb-3 rounded border border-white/10 p-1.5"
              data-archspace-derived-existence
            >
              <p className="mb-1 text-[10px] uppercase tracking-widest text-white/40">
                Derived Existence (seed: {subsystemId})
              </p>
              {derivedExistenceResult.derivedElementIds.length === 0 ? (
                <p className="text-[11px] text-white/50">
                  nothing derived — no ArchDerives edges from this seed.
                </p>
              ) : (
                derivedExistenceResult.derivedElementIds.map((id) => {
                  const cyclic = derivedExistenceResult.withinCycle.includes(id);
                  return (
                    <p
                      key={id}
                      className={`font-mono text-[11px] ${cyclic ? "text-yellow-400" : "text-white/80"}`}
                    >
                      {id}
                      {cyclic && " (within a cycle)"}
                    </p>
                  );
                })
              )}
            </div>
          )}

          {defineError && <p className="mb-2 text-xs text-alert">{defineError}</p>}

          {defineResult && (
            <div className="mb-3 space-y-2" data-archspace-define-result>
              <div className="rounded border border-white/10 p-1.5">
                <p className="font-mono text-[11px] text-cobalt-glow">
                  handle: {defineResult.handleId}
                </p>
                <p className="text-[11px] text-white/70">
                  design variables: {defineResult.stats.nDesignVariables} · declared:{" "}
                  {defineResult.stats.nDeclared} · valid: {defineResult.stats.nValid} · imputation
                  ratio: {defineResult.stats.imputationRatio.toFixed(3)}
                </p>
                {defineResult.stats.correctionRatio !== undefined && (
                  <p className="text-[11px] text-white/70">
                    correction ratio: {defineResult.stats.correctionRatio.toFixed(3)} (discrete{" "}
                    {defineResult.stats.discreteCorrectionRatio?.toFixed(3)} · continuous{" "}
                    {defineResult.stats.continuousCorrectionRatio?.toFixed(3)}) · correction
                    fraction: {defineResult.stats.correctionFraction?.toFixed(3)} · max rate
                    diversity: {defineResult.stats.maxRateDiversity?.toFixed(3)}
                  </p>
                )}
              </div>

              {defineResult.skipped.length > 0 && (
                <div className="rounded border border-white/10 p-1.5" data-archspace-skipped>
                  <p className="mb-1 text-[10px] uppercase tracking-widest text-white/40">
                    Skipped ({defineResult.skipped.length})
                  </p>
                  {defineResult.skipped.map((s) => (
                    <p key={s.elementId} className="text-[11px] text-white/60">
                      <span className="font-mono text-white/80">{s.elementId}</span>: {s.reason}
                    </p>
                  ))}
                </div>
              )}

              <Button
                onClick={handleDecode}
                disabled={decoding}
                className="w-full justify-center !py-1 text-xs"
              >
                {decoding ? "Decoding…" : "Decode Random Instance"}
              </Button>
              <Button
                onClick={handleGenerate}
                disabled={generating}
                className="w-full justify-center !py-1 text-xs"
              >
                {generating ? "Generating…" : "Generate & Compare (5)"}
              </Button>

              <div className="mt-2 flex gap-1.5">
                <select
                  value={optimizeAlgorithm}
                  onChange={(event) =>
                    setOptimizeAlgorithm(event.target.value as "nsga2" | "hierarchical-bo")
                  }
                  className="flex-1 rounded border border-white/10 bg-obsidian/60 px-1.5 py-1 text-xs text-white/80"
                >
                  <option value="nsga2">NSGA-II</option>
                  <option value="hierarchical-bo">Hierarchical BO</option>
                </select>
                <Button
                  onClick={handleOptimize}
                  disabled={optimizing}
                  className="flex-1 justify-center !py-1 text-xs"
                >
                  {optimizing ? "Optimizing…" : "Run Optimization"}
                </Button>
              </div>
            </div>
          )}

          {optimizeError && <p className="mb-2 text-xs text-alert">{optimizeError}</p>}

          {optimizeResult && (
            <div
              className="mb-3 rounded border border-white/10 p-1.5"
              data-archspace-optimize-result
            >
              <p className="mb-1 text-[10px] uppercase tracking-widest text-white/40">
                Optimization Result ({optimizeResult.algorithm})
              </p>
              <p className="font-mono text-[11px] text-white/70">
                objectives: [
                {optimizeResult.bestObjectiveValues.map((v) => v.toFixed(3)).join(", ")}]
              </p>
              <p className="font-mono text-[11px] text-white/70">
                vector: [{optimizeResult.bestDesignVector.map((v) => v.toFixed(2)).join(", ")}]
              </p>
            </div>
          )}

          {decodeError && <p className="mb-2 text-xs text-alert">{decodeError}</p>}

          {decodeResult && (
            <div className="space-y-1.5" data-archspace-decode-result>
              <p className="font-mono text-[11px] text-white/70">
                vector: [{decodeResult.designVector.map((v) => v.toFixed(2)).join(", ")}]
              </p>
              {decodeResult.choices.map((c) => (
                <p key={c.choiceId} className="text-[11px] text-white/80">
                  {c.choiceId} = {c.presentOption ?? "(none)"}
                </p>
              ))}
              {decodeResult.otherPresentNodes.length > 0 && (
                <p className="text-[11px] text-white/50">
                  also present: {decodeResult.otherPresentNodes.join(", ")}
                </p>
              )}
            </div>
          )}

          {generateError && <p className="mb-2 text-xs text-alert">{generateError}</p>}
          {proposeError && <p className="mb-2 text-xs text-alert">{proposeError}</p>}

          {generatedInstances && (
            <div className="space-y-2" data-archspace-generated-instances>
              <p className="text-[10px] uppercase tracking-widest text-white/40">
                Generated Instances ({generatedInstances.length})
              </p>
              {generatedInstances.map((instance, index) => {
                const proposed = proposedByIndex[index];
                return (
                  // biome-ignore lint/suspicious/noArrayIndexKey: a fixed, never-reordered snapshot from one generate call.
                  <div key={index} className="rounded border border-white/10 p-1.5">
                    <p className="font-mono text-[11px] text-white/70">
                      [{instance.designVector.map((v) => v.toFixed(2)).join(", ")}]
                    </p>
                    <p className={`text-[11px] ${viabilityBadgeClass(instance.viability.state)}`}>
                      {instance.viability.state} · PoV{" "}
                      {instance.viability.probabilityOfViability.toFixed(2)}
                      {instance.viability.objectiveValue !== null &&
                        ` · f=${instance.viability.objectiveValue.toFixed(3)}`}
                    </p>
                    {instance.choices.length > 0 && (
                      <p className="text-[11px] text-white/60">
                        {instance.choices
                          .map((c) => `${c.choiceId}=${c.presentOption ?? "(none)"}`)
                          .join(", ")}
                      </p>
                    )}
                    {proposed ? (
                      <p className="text-[11px] text-cobalt-glow">
                        proposed: {proposed.proposalId}
                      </p>
                    ) : (
                      <Button
                        onClick={() => handlePropose(index, instance)}
                        disabled={proposingIndex === index}
                        className="mt-1 w-full justify-center !py-1 text-xs"
                      >
                        {proposingIndex === index ? "Proposing…" : "Propose"}
                      </Button>
                    )}
                  </div>
                );
              })}
            </div>
          )}
        </>
      )}
    </Panel>
  );
}
