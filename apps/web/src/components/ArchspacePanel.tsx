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

interface ProposeResponse {
  proposalId: string;
  branchId: string;
  viability: Viability;
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
      setDecodeResult(await res.json());
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
      setGeneratedInstances(await res.json());
    } catch (err) {
      setGenerateError(err instanceof Error ? err.message : "failed to generate instances");
    } finally {
      setGenerating(false);
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
      setProposedByIndex((prev) => ({ ...prev, [index]: proposed }));
    } catch (err) {
      setProposeError(err instanceof Error ? err.message : "failed to propose instance");
    } finally {
      setProposingIndex(null);
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
