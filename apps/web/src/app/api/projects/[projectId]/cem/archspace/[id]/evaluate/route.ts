import { proxyRequest } from "@/lib/api-proxy";

/** FR-ARCH-08's own direct HTTP surface — evaluate one specific candidate's typed viability. */
export async function POST(
  request: Request,
  { params }: { params: Promise<{ projectId: string; id: string }> },
) {
  const { projectId, id } = await params;
  const body = await request.json().catch(() => ({}));
  return proxyRequest("POST", `/api/v0/projects/${projectId}/cem/archspace/${id}/evaluate`, body);
}
