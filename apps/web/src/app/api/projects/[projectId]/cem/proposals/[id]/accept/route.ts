import { proxyRequest } from "@/lib/api-proxy";

/** `id` is a proposal id here — see the sibling `cem/proposals/[id]/route.ts` for why this
 * segment is named `id` rather than `proposalId` (Next.js requires one shared param name per
 * dynamic segment across every route under it). */
export async function POST(
  _request: Request,
  { params }: { params: Promise<{ projectId: string; id: string }> },
) {
  const { projectId, id: proposalId } = await params;
  return proxyRequest("POST", `/api/v0/projects/${projectId}/cem/proposals/${proposalId}/accept`);
}
