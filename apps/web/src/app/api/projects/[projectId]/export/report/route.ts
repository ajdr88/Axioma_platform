import { proxyRequest } from "@/lib/api-proxy";

/** FR-EXPORT-03 — templated document generation. Only `"risk-register"` is a registered
 * `templateId` today; any other value is a precise 400 from the backend, not a silent fallback. */
export async function POST(
  request: Request,
  { params }: { params: Promise<{ projectId: string }> },
) {
  const { projectId } = await params;
  const body = await request.json();
  return proxyRequest("POST", `/api/v0/projects/${projectId}/export/report`, body);
}
