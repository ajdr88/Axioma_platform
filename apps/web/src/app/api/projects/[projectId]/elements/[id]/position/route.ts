import { proxyRequest } from "@/lib/api-proxy";

export async function PATCH(
  request: Request,
  { params }: { params: Promise<{ projectId: string; id: string }> },
) {
  const { projectId, id } = await params;
  const body = await request.json();
  return proxyRequest("PATCH", `/api/v0/projects/${projectId}/elements/${id}/position`, body);
}
