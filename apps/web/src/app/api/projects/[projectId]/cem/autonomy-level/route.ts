import { proxyRequest } from "@/lib/api-proxy";

/** FR-CEM-16/17 — sets the L0-L4 autonomy level (and optional L3 threshold) for a scope; every
 * change is audited server-side (NFR-CEM-06). */
export async function PUT(
  request: Request,
  { params }: { params: Promise<{ projectId: string }> },
) {
  const { projectId } = await params;
  const body = await request.json();
  return proxyRequest("PUT", `/api/v0/projects/${projectId}/cem/autonomy-level`, body);
}
