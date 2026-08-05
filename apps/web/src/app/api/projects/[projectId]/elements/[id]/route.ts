import { proxyRequest } from "@/lib/api-proxy";

export async function PATCH(
  request: Request,
  { params }: { params: Promise<{ projectId: string; id: string }> },
) {
  const { projectId, id } = await params;
  const body = await request.json();
  return proxyRequest("PATCH", `/api/v0/projects/${projectId}/elements/${id}`, body);
}

/** P1.3 element delete (T-P1.3-03) — `?acknowledge=true` bypasses the Traceability Breach gate. */
export async function DELETE(
  request: Request,
  { params }: { params: Promise<{ projectId: string; id: string }> },
) {
  const { projectId, id } = await params;
  const acknowledge = new URL(request.url).searchParams.get("acknowledge");
  const query = acknowledge ? `?acknowledge=${encodeURIComponent(acknowledge)}` : "";
  return proxyRequest("DELETE", `/api/v0/projects/${projectId}/elements/${id}${query}`);
}
