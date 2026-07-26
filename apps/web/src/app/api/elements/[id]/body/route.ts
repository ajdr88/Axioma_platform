import { proxyRequest } from "@/lib/api-proxy";

export async function GET(_request: Request, { params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  return proxyRequest("GET", `/api/v0/elements/${id}/body`);
}

export async function PUT(request: Request, { params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const body = await request.json();
  return proxyRequest("PUT", `/api/v0/elements/${id}/body`, body);
}
