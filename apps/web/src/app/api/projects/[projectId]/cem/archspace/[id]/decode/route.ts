import { proxyRequest } from "@/lib/api-proxy";

/** FR-ARCH-05's decode half — an empty body asks the sidecar to sample a random valid vector. */
export async function POST(
  request: Request,
  { params }: { params: Promise<{ projectId: string; id: string }> },
) {
  const { projectId, id } = await params;
  const body = await request.json().catch(() => ({}));
  return proxyRequest("POST", `/api/v0/projects/${projectId}/cem/archspace/${id}/decode`, body);
}
