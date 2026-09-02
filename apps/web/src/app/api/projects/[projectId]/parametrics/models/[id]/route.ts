import { proxyRequest } from "@/lib/api-proxy";

/** Pending-items Tier 1 item 10 — a Model's declared inputs/outputs/formulas, for the
 * ParametricsPanel "Models" section to pre-fill an evaluation form. */
export async function GET(
  _request: Request,
  { params }: { params: Promise<{ projectId: string; id: string }> },
) {
  const { projectId, id } = await params;
  return proxyRequest("GET", `/api/v0/projects/${projectId}/parametrics/models/${id}`);
}
