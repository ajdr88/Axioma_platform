import { proxyRequest } from "@/lib/api-proxy";

/** P1.3 budgeted traceability (FR-CORE-03) — depth/maxFanout are required by apps/api, not
 * defaulted here; a request missing them is forwarded as-is so the backend's own 400 surfaces. */
export async function GET(
  request: Request,
  { params }: { params: Promise<{ projectId: string; id: string }> },
) {
  const { projectId, id } = await params;
  const search = new URL(request.url).searchParams;
  return proxyRequest(
    "GET",
    `/api/v0/projects/${projectId}/elements/${id}/traceability?${search.toString()}`,
  );
}
