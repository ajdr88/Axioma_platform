import { proxyRequest } from "@/lib/api-proxy";

/** FR-ARCH-02's own direct HTTP surface — `?seedIds=A,B,C` is forwarded as-is, same "required
 * query params, not defaulted here" precedent as elements/[id]/traceability's own proxy route. */
export async function GET(
  request: Request,
  { params }: { params: Promise<{ projectId: string; id: string }> },
) {
  const { projectId, id } = await params;
  const search = new URL(request.url).searchParams;
  return proxyRequest(
    "GET",
    `/api/v0/projects/${projectId}/cem/archspace/${id}/derived-existence?${search.toString()}`,
  );
}
