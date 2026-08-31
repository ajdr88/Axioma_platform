"use client";

import type { Element as ApiElement } from "@axioma/shared-types";
import { Button, Panel } from "@axioma/ui-components";
import { useCallback, useEffect, useState } from "react";

interface TimingConstraint {
  minMs?: number | null;
  maxMs?: number | null;
}

interface Message {
  order: number;
  from: string;
  to: string;
  text: string;
  kind: string;
  fragmentId?: string | null;
  refInteractionId?: string | null;
  timingConstraint?: TimingConstraint | null;
}

interface InteractionBody {
  participantIds: string[];
  messages: Message[];
}

interface Fragment {
  id: string;
  fragmentKind: string;
  guard: string | null;
}

interface InteractionPanelProps {
  interactionId: string;
  projectId: string;
  /** Already loaded by `page.tsx` — reused here for participant/ref-interaction name lookups
   * rather than a second element fetch. */
  elements: ApiElement[];
  onClose: () => void;
}

const LIFELINE_SPACING = 170;
const PADDING_X = 24;
const HEADER_HEIGHT = 44;
const ROW_HEIGHT = 44;
const BOTTOM_PADDING = 24;

/**
 * docs/IMPLEMENTATION_KICKOFF.md Phase 5 (FR-INTX-01..04, ADR-009 ratification) — a real, if
 * visually simple, SVG Lifeline/Message diagram over an `:Interaction` element's stored content.
 * "Looks like a Sequence Diagram" is entirely this component's own rendering concern; nothing
 * about the underlying storage (a plain JSON `messages` array on the element's body) knows or
 * cares that it's drawn this way — the actual ADR-009 decoupling this phase ratifies.
 */
export function InteractionPanel({
  interactionId,
  projectId,
  elements,
  onClose,
}: InteractionPanelProps) {
  const [body, setBody] = useState<InteractionBody | null>(null);
  const [fragments, setFragments] = useState<Fragment[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  const [messageFrom, setMessageFrom] = useState("");
  const [messageTo, setMessageTo] = useState("");
  const [messageText, setMessageText] = useState("");
  const [messageKind, setMessageKind] = useState("sync");
  const [savingMessage, setSavingMessage] = useState(false);

  const [fragmentKind, setFragmentKind] = useState("alt");
  const [fragmentGuard, setFragmentGuard] = useState("");
  const [savingFragment, setSavingFragment] = useState(false);

  const nameOf = useCallback(
    (id: string) => elements.find((e) => e.id === id)?.name ?? id,
    [elements],
  );

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const res = await fetch(`/api/projects/${projectId}/elements/${interactionId}/body`);
      if (!res.ok) {
        throw new Error(`request failed with status ${res.status}`);
      }
      const data = await res.json();
      setBody({
        participantIds: data.properties?.participantIds ?? [],
        messages: data.properties?.messages ?? [],
      });

      const containsRes = await fetch(`/api/projects/${projectId}/contains`);
      const containsEdges: { source: string; target: string }[] = containsRes.ok
        ? await containsRes.json()
        : [];
      const fragmentIds = containsEdges
        .filter((e) => e.source === interactionId)
        .map((e) => e.target)
        .filter((id) => elements.find((el) => el.id === id)?.kind === "InteractionFragment");
      const loadedFragments: Fragment[] = [];
      for (const fragmentId of fragmentIds) {
        const fragmentRes = await fetch(`/api/projects/${projectId}/elements/${fragmentId}/body`);
        if (fragmentRes.ok) {
          const fragmentBody = await fragmentRes.json();
          loadedFragments.push({
            id: fragmentId,
            fragmentKind: fragmentBody.properties?.fragmentKind ?? "alt",
            guard: fragmentBody.properties?.guard ?? null,
          });
        }
      }
      setFragments(loadedFragments);
    } catch (err) {
      setError(err instanceof Error ? err.message : "failed to load interaction");
    } finally {
      setLoading(false);
    }
  }, [projectId, interactionId, elements]);

  useEffect(() => {
    load();
  }, [load]);

  async function handleAddMessage() {
    if (!messageFrom || !messageTo || !messageText.trim()) {
      return;
    }
    setSavingMessage(true);
    try {
      const res = await fetch(`/api/projects/${projectId}/interactions/${interactionId}/messages`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          from: messageFrom,
          to: messageTo,
          text: messageText.trim(),
          kind: messageKind,
        }),
      });
      if (!res.ok) {
        const err = await res.json().catch(() => null);
        throw new Error(err?.error ?? `request failed with status ${res.status}`);
      }
      setMessageText("");
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "failed to add message");
    } finally {
      setSavingMessage(false);
    }
  }

  async function handleAddFragment() {
    setSavingFragment(true);
    try {
      const res = await fetch(
        `/api/projects/${projectId}/interactions/${interactionId}/fragments`,
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            fragmentKind,
            guard: fragmentGuard.trim() || null,
          }),
        },
      );
      if (!res.ok) {
        const err = await res.json().catch(() => null);
        throw new Error(err?.error ?? `request failed with status ${res.status}`);
      }
      setFragmentGuard("");
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "failed to add fragment");
    } finally {
      setSavingFragment(false);
    }
  }

  const participantIds = body?.participantIds ?? [];
  const messages = [...(body?.messages ?? [])].sort((a, b) => a.order - b.order);
  const xOf = (id: string) => {
    const index = participantIds.indexOf(id);
    return PADDING_X + Math.max(index, 0) * LIFELINE_SPACING + LIFELINE_SPACING / 2;
  };
  const svgWidth = PADDING_X * 2 + participantIds.length * LIFELINE_SPACING;
  const svgHeight = HEADER_HEIGHT + Math.max(messages.length, 1) * ROW_HEIGHT + BOTTOM_PADDING;

  // Fragment bounding boxes: min/max x across every participant (a fragment's guard scopes the
  // whole interaction visually, not just the two lifelines its own messages touch), min/max y
  // across just the messages actually nested in it.
  const fragmentBoxes = fragments
    .map((fragment) => {
      const rows = messages
        .map((m, i) => ({ m, i }))
        .filter(({ m }) => m.fragmentId === fragment.id);
      if (rows.length === 0) {
        return null;
      }
      const yStart = HEADER_HEIGHT + rows[0].i * ROW_HEIGHT + 6;
      const yEnd = HEADER_HEIGHT + rows[rows.length - 1].i * ROW_HEIGHT + ROW_HEIGHT - 6;
      return { fragment, yStart, yEnd };
    })
    .filter((box): box is { fragment: Fragment; yStart: number; yEnd: number } => box !== null);

  return (
    <Panel className="absolute right-4 top-4 z-10 w-[520px] max-w-[calc(100vw-2rem)] max-h-[calc(100vh-2rem)] overflow-y-auto p-4">
      <div className="mb-3 flex items-start justify-between gap-2">
        <p className="text-sm font-semibold text-white/90">Interaction</p>
        <Button variant="ghost" onClick={onClose} className="!px-2 !py-1 text-xs">
          Close
        </Button>
      </div>

      {loading && <p className="text-xs text-white/40">Loading…</p>}
      {error && <p className="text-xs text-alert">{error}</p>}

      {!loading && body && (
        <>
          <div className="mb-3 overflow-x-auto rounded border border-white/10 bg-obsidian/40 p-2">
            <svg
              width={svgWidth}
              height={svgHeight}
              role="img"
              aria-label="Lifeline diagram"
              data-interaction-svg
            >
              {fragmentBoxes.map(({ fragment, yStart, yEnd }) => (
                <g key={fragment.id}>
                  <rect
                    x={PADDING_X / 2}
                    y={yStart}
                    width={svgWidth - PADDING_X}
                    height={yEnd - yStart}
                    fill="none"
                    stroke="#7C7C86"
                    strokeDasharray="3 2"
                    rx={4}
                  />
                  <text x={PADDING_X / 2 + 4} y={yStart + 12} fontSize={9} fill="#7C7C86">
                    {fragment.fragmentKind}
                    {fragment.guard ? ` [${fragment.guard}]` : ""}
                  </text>
                </g>
              ))}

              {participantIds.map((id) => (
                <g key={id}>
                  <rect
                    x={xOf(id) - 60}
                    y={4}
                    width={120}
                    height={HEADER_HEIGHT - 12}
                    fill="#3A5BFF22"
                    stroke="#3A5BFF"
                    rx={4}
                  />
                  <text
                    x={xOf(id)}
                    y={4 + (HEADER_HEIGHT - 12) / 2 + 4}
                    fontSize={10}
                    textAnchor="middle"
                    fill="#F3F4F8"
                  >
                    {nameOf(id).length > 16 ? `${nameOf(id).slice(0, 15)}…` : nameOf(id)}
                  </text>
                  <line
                    x1={xOf(id)}
                    y1={HEADER_HEIGHT}
                    x2={xOf(id)}
                    y2={svgHeight - BOTTOM_PADDING}
                    stroke="#7C7C86"
                    strokeDasharray="2 3"
                  />
                </g>
              ))}

              {messages.map((message, index) => {
                const y = HEADER_HEIGHT + index * ROW_HEIGHT + ROW_HEIGHT / 2;
                const fromX = xOf(message.from);
                const toX = xOf(message.to);
                const isReply = message.kind === "reply";
                const isRef = Boolean(message.refInteractionId);
                const label = isRef
                  ? `ref: ${nameOf(message.refInteractionId ?? "")}`
                  : message.text;
                return (
                  <g key={`${message.order}-${message.from}-${message.to}`}>
                    <text
                      x={(fromX + toX) / 2}
                      y={y - 6}
                      fontSize={9}
                      textAnchor="middle"
                      fill={isRef ? "#E8A93A" : "#F3F4F8"}
                    >
                      {label.length > 40 ? `${label.slice(0, 39)}…` : label}
                    </text>
                    <line
                      x1={fromX}
                      y1={y}
                      x2={toX}
                      y2={y}
                      stroke={isRef ? "#E8A93A" : "#3A5BFF"}
                      strokeDasharray={isReply ? "4 3" : undefined}
                      markerEnd="url(#interaction-arrow)"
                    />
                  </g>
                );
              })}

              <defs>
                <marker
                  id="interaction-arrow"
                  markerWidth={8}
                  markerHeight={8}
                  refX={7}
                  refY={4}
                  orient="auto"
                >
                  <path d="M0,0 L8,4 L0,8 Z" fill="#3A5BFF" />
                </marker>
              </defs>
            </svg>
          </div>

          <div className="mb-3 space-y-1.5 border-t border-white/10 pt-3">
            <p className="text-[10px] uppercase tracking-widest text-white/40">Add message</p>
            <div className="flex gap-1.5">
              <select
                value={messageFrom}
                onChange={(event) => setMessageFrom(event.target.value)}
                className="flex-1 rounded border border-white/10 bg-obsidian/60 px-1.5 py-1 text-xs text-white/80"
              >
                <option value="">from…</option>
                {participantIds.map((id) => (
                  <option key={id} value={id}>
                    {nameOf(id)}
                  </option>
                ))}
              </select>
              <select
                value={messageTo}
                onChange={(event) => setMessageTo(event.target.value)}
                className="flex-1 rounded border border-white/10 bg-obsidian/60 px-1.5 py-1 text-xs text-white/80"
              >
                <option value="">to…</option>
                {participantIds.map((id) => (
                  <option key={id} value={id}>
                    {nameOf(id)}
                  </option>
                ))}
              </select>
              <select
                value={messageKind}
                onChange={(event) => setMessageKind(event.target.value)}
                className="rounded border border-white/10 bg-obsidian/60 px-1.5 py-1 text-xs text-white/80"
              >
                <option value="sync">sync</option>
                <option value="async">async</option>
                <option value="reply">reply</option>
              </select>
            </div>
            <div className="flex gap-1.5">
              <input
                value={messageText}
                onChange={(event) => setMessageText(event.target.value)}
                placeholder="message text"
                className="flex-1 rounded border border-white/10 bg-obsidian/60 px-1.5 py-1 text-xs text-white/80 outline-none"
              />
              <Button
                variant="primary"
                className="!px-2 !py-1 text-xs"
                disabled={savingMessage || !messageFrom || !messageTo || !messageText.trim()}
                onClick={handleAddMessage}
              >
                Add
              </Button>
            </div>
          </div>

          <div className="space-y-1.5 border-t border-white/10 pt-3">
            <p className="text-[10px] uppercase tracking-widest text-white/40">Add fragment</p>
            <div className="flex gap-1.5">
              <select
                value={fragmentKind}
                onChange={(event) => setFragmentKind(event.target.value)}
                className="rounded border border-white/10 bg-obsidian/60 px-1.5 py-1 text-xs text-white/80"
              >
                <option value="alt">alt</option>
                <option value="opt">opt</option>
                <option value="par">par</option>
                <option value="loop">loop</option>
              </select>
              <input
                value={fragmentGuard}
                onChange={(event) => setFragmentGuard(event.target.value)}
                placeholder="guard (optional)"
                className="flex-1 rounded border border-white/10 bg-obsidian/60 px-1.5 py-1 text-xs text-white/80 outline-none"
              />
              <Button
                variant="primary"
                className="!px-2 !py-1 text-xs"
                disabled={savingFragment}
                onClick={handleAddFragment}
              >
                Add
              </Button>
            </div>
          </div>
        </>
      )}
    </Panel>
  );
}
