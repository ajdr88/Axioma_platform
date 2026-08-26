import { proxyRequest } from "@/lib/api-proxy";

/** Read-only Mode B exploration — see `apps/api/src/mode_b.rs::optimize`'s doc comment. */
export async function POST(
  request: Request,
  { params }: { params: Promise<{ projectId: string }> },
) {
  const { projectId } = await params;
  const body = await request.json();
  return proxyRequest("POST", `/api/v0/projects/${projectId}/cem/mode-b/optimize`, body);
}
