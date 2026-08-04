import { proxyRequest } from "@/lib/api-proxy";

/** Generic edge kinds (everything besides `Contains`, which keeps its own
 * `/api/projects/:projectId/contains` route) — e.g. the Hazard/Risk panel's
 * `Causes`/`MitigatedBy` edges. */
export async function GET(
  request: Request,
  { params }: { params: Promise<{ projectId: string }> },
) {
  const { projectId } = await params;
  const kind = new URL(request.url).searchParams.get("kind");
  return proxyRequest(
    "GET",
    `/api/v0/projects/${projectId}/edges?kind=${encodeURIComponent(kind ?? "")}`,
  );
}

export async function POST(
  request: Request,
  { params }: { params: Promise<{ projectId: string }> },
) {
  const { projectId } = await params;
  const body = await request.json();
  return proxyRequest("POST", `/api/v0/projects/${projectId}/edges`, body);
}

export async function DELETE(
  request: Request,
  { params }: { params: Promise<{ projectId: string }> },
) {
  const { projectId } = await params;
  const body = await request.json();
  return proxyRequest("DELETE", `/api/v0/projects/${projectId}/edges`, body);
}
