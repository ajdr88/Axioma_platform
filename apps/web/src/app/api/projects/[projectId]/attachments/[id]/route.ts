import { proxyRequest } from "@/lib/api-proxy";

/** FR-EXPORT-04 — streams an attachment's raw bytes back with its stored `Content-Type` and a
 * `Content-Disposition: attachment` (relies on `proxyRequest`'s binary-safe `arrayBuffer` relay,
 * not a text round-trip, so non-UTF-8 files download intact). */
export async function GET(
  _request: Request,
  { params }: { params: Promise<{ projectId: string; id: string }> },
) {
  const { projectId, id } = await params;
  return proxyRequest("GET", `/api/v0/projects/${projectId}/attachments/${id}`);
}
