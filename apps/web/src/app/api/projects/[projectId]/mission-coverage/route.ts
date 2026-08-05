import { proxyRequest } from "@/lib/api-proxy";

/** P1.3 mission-coverage orphan check (FR-MSN-04, T-P1.3-05). */
export async function GET(
  _request: Request,
  { params }: { params: Promise<{ projectId: string }> },
) {
  const { projectId } = await params;
  return proxyRequest("GET", `/api/v0/projects/${projectId}/mission-coverage`);
}
