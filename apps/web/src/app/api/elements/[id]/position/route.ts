import { proxyRequest } from "@/lib/api-proxy";

export async function PATCH(request: Request, { params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const body = await request.json();
  return proxyRequest("PATCH", `/api/v0/elements/${id}/position`, body);
}
