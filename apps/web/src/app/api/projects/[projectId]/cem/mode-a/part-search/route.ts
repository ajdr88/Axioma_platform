import { proxyRequest } from "@/lib/api-proxy";

/** Mode A part search — in-context LLM ranking over every element in the project, see
 * `apps/api/src/mode_a.rs`'s doc comment for scope/limitations (not real embeddings, doesn't
 * scale past reference-fixture size). First UI surface Mode A has had at all — the copilot query
 * endpoint (`.../cem/mode-a/query`) has never had a proxy route or UI either. */
export async function POST(
  request: Request,
  { params }: { params: Promise<{ projectId: string }> },
) {
  const { projectId } = await params;
  const body = await request.json();
  return proxyRequest("POST", `/api/v0/projects/${projectId}/cem/mode-a/part-search`, body);
}
