import { proxyRequest } from "@/lib/api-proxy";

export async function GET(
  _request: Request,
  { params }: { params: Promise<{ projectId: string }> },
) {
  const { projectId } = await params;
  return proxyRequest("GET", `/api/v0/projects/${projectId}/contains`);
}

export async function POST(
  request: Request,
  { params }: { params: Promise<{ projectId: string }> },
) {
  const { projectId } = await params;
  const body = await request.json();
  return proxyRequest("POST", `/api/v0/projects/${projectId}/contains`, body);
}

export async function DELETE(
  request: Request,
  { params }: { params: Promise<{ projectId: string }> },
) {
  const { projectId } = await params;
  const body = await request.json();
  return proxyRequest("DELETE", `/api/v0/projects/${projectId}/contains`, body);
}
