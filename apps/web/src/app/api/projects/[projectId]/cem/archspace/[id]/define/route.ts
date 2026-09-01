import { proxyRequest } from "@/lib/api-proxy";

/** FR-ARCH-05/06's real build-out — defines a design space from a subsystem's real graph content
 * and returns its stats/skipped-item list in one round trip. */
export async function POST(
  _request: Request,
  { params }: { params: Promise<{ projectId: string; id: string }> },
) {
  const { projectId, id } = await params;
  return proxyRequest("POST", `/api/v0/projects/${projectId}/cem/archspace/${id}/define`);
}
