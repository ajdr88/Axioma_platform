import { proxyRequest } from "@/lib/api-proxy";

/** FR-INFO-01/03 — creates a real `:InformationElement` with its Conceptual/Logical/Physical
 * abstraction tier set atomically in the same call. */
export async function POST(
  request: Request,
  { params }: { params: Promise<{ projectId: string }> },
) {
  const { projectId } = await params;
  const body = await request.json();
  return proxyRequest("POST", `/api/v0/projects/${projectId}/information/elements`, body);
}
