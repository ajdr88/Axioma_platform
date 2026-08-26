import { proxyRequest } from "@/lib/api-proxy";

/** P2.2's autonomy-aware alternative to `mode-b/accept` — see `apps/api/src/mode_b.rs::propose`'s
 * doc comment for the merge-vs-review split this runs per subsystem. */
export async function POST(
  request: Request,
  { params }: { params: Promise<{ projectId: string }> },
) {
  const { projectId } = await params;
  const body = await request.json();
  return proxyRequest("POST", `/api/v0/projects/${projectId}/cem/mode-b/propose`, body);
}
