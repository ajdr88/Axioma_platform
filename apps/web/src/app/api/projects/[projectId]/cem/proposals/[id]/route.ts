import { proxyGet } from "@/lib/api-proxy";

/** T-P2.2-01 — the pending/accepted/rejected proposals `propose` filed on one branch. Next.js
 * requires every route sharing this `cem/proposals/[id]` segment to use the same param name, so
 * `id` here is a branch id (see the sibling `[id]/accept` and `[id]/reject` routes, where the same
 * segment name instead addresses a proposal id). */
export async function GET(
  _request: Request,
  { params }: { params: Promise<{ projectId: string; id: string }> },
) {
  const { projectId, id: branchId } = await params;
  return proxyGet(`/api/v0/projects/${projectId}/cem/proposals/${branchId}`);
}
