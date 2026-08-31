import { proxyRequest } from "@/lib/api-proxy";

/** FR-EXPORT-02 — CSV export scoped by `?kind=<NodeKind>` or `?collectionId=<id>` (mutually
 * exclusive on the backend). The query string is forwarded verbatim, not re-parsed. */
export async function GET(
  request: Request,
  { params }: { params: Promise<{ projectId: string }> },
) {
  const { projectId } = await params;
  const search = new URL(request.url).search;
  return proxyRequest("GET", `/api/v0/projects/${projectId}/export/table${search}`);
}
