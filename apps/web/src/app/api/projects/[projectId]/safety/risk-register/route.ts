import { proxyRequest } from "@/lib/api-proxy";

/** P1.3 safety risk-register export (FR-SAFE-05, T-P1.3-04) — an ARP4761-shaped JSON download. */
export async function GET(
  _request: Request,
  { params }: { params: Promise<{ projectId: string }> },
) {
  const { projectId } = await params;
  return proxyRequest("GET", `/api/v0/projects/${projectId}/safety/risk-register`);
}
