import { proxyRequest } from "@/lib/api-proxy";

/** T-P1.1-05's branch-scoped property edit — applies a property change to a *copy* of a
 * branch's current snapshot, never the live graph (see `store::versioning`'s doc comment on the
 * Rust side). */
export async function PATCH(
  request: Request,
  { params }: { params: Promise<{ projectId: string; branch: string; elementId: string }> },
) {
  const { projectId, branch, elementId } = await params;
  const body = await request.json();
  return proxyRequest(
    "PATCH",
    `/api/v0/projects/${projectId}/branches/${branch}/elements/${elementId}/body`,
    body,
  );
}
