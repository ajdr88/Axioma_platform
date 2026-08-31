import { proxyMultipart, proxyRequest } from "@/lib/api-proxy";

/** FR-EXPORT-04 — element file attachments. GET lists metadata; POST is a real multipart file
 * upload, forwarded byte-for-byte (see `proxyMultipart`'s doc comment for why). */
export async function GET(
  _request: Request,
  { params }: { params: Promise<{ projectId: string; id: string }> },
) {
  const { projectId, id } = await params;
  return proxyRequest("GET", `/api/v0/projects/${projectId}/elements/${id}/attachments`);
}

export async function POST(
  request: Request,
  { params }: { params: Promise<{ projectId: string; id: string }> },
) {
  const { projectId, id } = await params;
  return proxyMultipart(`/api/v0/projects/${projectId}/elements/${id}/attachments`, request);
}
