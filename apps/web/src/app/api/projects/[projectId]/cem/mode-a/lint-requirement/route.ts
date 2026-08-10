import { proxyRequest } from "@/lib/api-proxy";

/** Mode A requirement linting — LLM-based INCOSE-style wording review of one Requirement, see
 * `apps/api/src/mode_a.rs`'s doc comment for a real, measured limitation on output quality with
 * the small local models available in this environment. */
export async function POST(
  request: Request,
  { params }: { params: Promise<{ projectId: string }> },
) {
  const { projectId } = await params;
  const body = await request.json();
  return proxyRequest("POST", `/api/v0/projects/${projectId}/cem/mode-a/lint-requirement`, body);
}
