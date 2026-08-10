import { proxyRequest } from "@/lib/api-proxy";

/** Git-backed model versioning (roadmap: P1.1, T-P1.1-05) — lists/creates branches for a
 * project. First UI consumer: `TradeStudyPanel` (T-P1.4-05). */
export async function GET(
  _request: Request,
  { params }: { params: Promise<{ projectId: string }> },
) {
  const { projectId } = await params;
  return proxyRequest("GET", `/api/v0/projects/${projectId}/branches`);
}

export async function POST(
  request: Request,
  { params }: { params: Promise<{ projectId: string }> },
) {
  const { projectId } = await params;
  const body = await request.json();
  return proxyRequest("POST", `/api/v0/projects/${projectId}/branches`, body);
}
