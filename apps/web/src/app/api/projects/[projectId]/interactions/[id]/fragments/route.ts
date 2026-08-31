import { proxyRequest } from "@/lib/api-proxy";

/** docs/IMPLEMENTATION_KICKOFF.md Phase 5 (FR-INTX-01..04, ADR-009) — creates a real
 * `:InteractionFragment` element, `Contains`-edged from its parent Interaction
 * (`apps/api/src/interactions.rs::add_fragment`), called by `InteractionPanel`'s "Add fragment"
 * form. */
export async function POST(
  request: Request,
  { params }: { params: Promise<{ projectId: string; id: string }> },
) {
  const { projectId, id } = await params;
  const body = await request.json();
  return proxyRequest("POST", `/api/v0/projects/${projectId}/interactions/${id}/fragments`, body);
}
