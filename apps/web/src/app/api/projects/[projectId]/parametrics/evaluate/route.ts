import { proxyRequest } from "@/lib/api-proxy";

/** FR-PARAM-03 — synchronous, server-side Constraint evaluation (linear interpolation over a
 * Constraint's `sampledPointsAtDesignSpeed`). Never touches `cem-core`/`cem-connectors`/`scheduler`. */
export async function POST(
  request: Request,
  { params }: { params: Promise<{ projectId: string }> },
) {
  const { projectId } = await params;
  const body = await request.json();
  return proxyRequest("POST", `/api/v0/projects/${projectId}/parametrics/evaluate`, body);
}
