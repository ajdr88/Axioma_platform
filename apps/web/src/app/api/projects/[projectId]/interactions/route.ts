import { proxyRequest } from "@/lib/api-proxy";

/** docs/IMPLEMENTATION_KICKOFF.md Phase 5 (FR-INTX-01, ADR-009) — creates a real `:Interaction`
 * element (`apps/api/src/interactions.rs::create_interaction`). */
export async function POST(
  request: Request,
  { params }: { params: Promise<{ projectId: string }> },
) {
  const { projectId } = await params;
  const body = await request.json();
  return proxyRequest("POST", `/api/v0/projects/${projectId}/interactions`, body);
}
