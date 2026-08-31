import { proxyRequest } from "@/lib/api-proxy";

/** FR-CORE-11 — re-runs a saved Dynamic Collection's traversal and freezes the result into a
 * real `:Collection` element + `Member` edges (Cameo's "Freeze Contents" equivalent). */
export async function POST(
  _request: Request,
  { params }: { params: Promise<{ projectId: string; id: string }> },
) {
  const { projectId, id } = await params;
  return proxyRequest("POST", `/api/v0/projects/${projectId}/collections/${id}/freeze`);
}
