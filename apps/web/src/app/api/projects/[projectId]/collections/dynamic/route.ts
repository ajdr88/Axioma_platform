import { proxyRequest } from "@/lib/api-proxy";

/** FR-CORE-10 — saves a named, budgeted-traversal spec (root/depth/maxFanout/direction), the
 * live "Dynamic Query" definition. Freezing it (`.../collections/:id/freeze`) is a separate call. */
export async function POST(
  request: Request,
  { params }: { params: Promise<{ projectId: string }> },
) {
  const { projectId } = await params;
  const body = await request.json();
  return proxyRequest("POST", `/api/v0/projects/${projectId}/collections/dynamic`, body);
}
