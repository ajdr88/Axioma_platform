import { proxyRequest } from "@/lib/api-proxy";

/** Tier 1 pass (item 7) — real multi-objective/hierarchical-BO search, the first real HTTP
 * surface for `RunOptimization`. */
export async function POST(
  request: Request,
  { params }: { params: Promise<{ projectId: string; id: string }> },
) {
  const { projectId, id } = await params;
  const body = await request.json().catch(() => ({}));
  return proxyRequest("POST", `/api/v0/projects/${projectId}/cem/archspace/${id}/optimize`, body);
}
