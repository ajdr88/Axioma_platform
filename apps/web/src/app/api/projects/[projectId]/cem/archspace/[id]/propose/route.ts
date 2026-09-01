import { proxyRequest } from "@/lib/api-proxy";

/** FR-ARCH-07's "enterable into the existing proposal/review-gate flow" half. */
export async function POST(
  request: Request,
  { params }: { params: Promise<{ projectId: string; id: string }> },
) {
  const { projectId, id } = await params;
  const body = await request.json();
  return proxyRequest("POST", `/api/v0/projects/${projectId}/cem/archspace/${id}/propose`, body);
}
