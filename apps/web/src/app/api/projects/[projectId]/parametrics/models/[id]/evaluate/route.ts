import { proxyRequest } from "@/lib/api-proxy";

/** Pending-items Tier 1 item 10 — evaluates a Model's real rhai-formula Constraint chain against
 * caller-supplied input values. */
export async function POST(
  request: Request,
  { params }: { params: Promise<{ projectId: string; id: string }> },
) {
  const { projectId, id } = await params;
  const body = await request.json().catch(() => ({}));
  return proxyRequest(
    "POST",
    `/api/v0/projects/${projectId}/parametrics/models/${id}/evaluate`,
    body,
  );
}
