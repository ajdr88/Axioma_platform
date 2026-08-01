"use client";

import { useEffect, useRef, useState } from "react";

export interface ElementBody {
  rationale: string | null;
  properties: Record<string, string>;
}

const EMPTY_BODY: ElementBody = { rationale: null, properties: {} };

/**
 * Hydrates and edits a set of elements' Postgres bodies (`GET`/`PUT /api/elements/:id/body`) —
 * the same endpoint `ElementInspector` uses, just driven by a purpose-built editor (dropdowns,
 * pickers) instead of the generic key/value rows. Originally written for `HazardRiskPanel`'s
 * severity/likelihood/status scoring; extracted here once a second panel needed the exact same
 * race-safe update logic.
 *
 * Both the lazy hydration effect and `updateProperty` read/write a `useRef` mirror of `bodies`,
 * not just the state itself. Two `updateProperty` calls fired back-to-back (e.g. scoring severity
 * then likelihood) would otherwise each close over the same pre-update state snapshot — and since
 * the PUT replaces the whole property bag, the second write would silently clobber the first. The
 * hydration effect additionally re-checks the ref *at fetch-resolution time*, not just at
 * fetch-issue time: a local write can land for an id while that id's very first hydration GET is
 * still in flight, and that GET's response (a snapshot from before the write existed) must not be
 * allowed to overwrite it once it resolves.
 */
export function useElementBodies(trackedIds: string[]) {
  const [bodies, setBodies] = useState<Record<string, ElementBody>>({});
  const [error, setError] = useState<string | null>(null);
  const bodiesRef = useRef<Record<string, ElementBody>>({});
  const idsKey = trackedIds.join(",");

  useEffect(() => {
    let cancelled = false;
    async function loadBodies() {
      const ids = (idsKey ? idsKey.split(",") : []).filter((id) => !(id in bodiesRef.current));
      if (ids.length === 0) {
        return;
      }
      const entries = await Promise.all(
        ids.map(async (id): Promise<[string, ElementBody]> => {
          const res = await fetch(`/api/elements/${id}/body`);
          if (!res.ok) {
            return [id, EMPTY_BODY];
          }
          const body = await res.json();
          return [id, { rationale: body.rationale ?? null, properties: body.properties ?? {} }];
        }),
      );
      if (!cancelled) {
        const freshEntries = entries.filter(([id]) => !(id in bodiesRef.current));
        if (freshEntries.length > 0) {
          bodiesRef.current = { ...bodiesRef.current, ...Object.fromEntries(freshEntries) };
          setBodies((b) => ({ ...b, ...Object.fromEntries(freshEntries) }));
        }
      }
    }
    loadBodies();
    return () => {
      cancelled = true;
    };
  }, [idsKey]);

  async function updateProperty(elementId: string, patch: Record<string, string>) {
    const previous = bodiesRef.current[elementId] ?? EMPTY_BODY;
    const nextBody: ElementBody = {
      rationale: previous.rationale,
      properties: { ...previous.properties, ...patch },
    };
    bodiesRef.current = { ...bodiesRef.current, [elementId]: nextBody };
    setBodies((b) => ({ ...b, [elementId]: nextBody }));

    const res = await fetch(`/api/elements/${elementId}/body`, {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(nextBody),
    });
    if (!res.ok) {
      const err = await res.json().catch(() => null);
      setError(err?.error ?? `save failed with status ${res.status}`);
      // Roll back the optimistic write — only if nothing newer has landed on top of it since.
      if (bodiesRef.current[elementId] === nextBody) {
        bodiesRef.current = { ...bodiesRef.current, [elementId]: previous };
        setBodies((b) => ({ ...b, [elementId]: previous }));
      }
    }
  }

  return { bodies, updateProperty, error, setError };
}
