import { proxyRequest } from "@/lib/api-proxy";

/** FR-ARCH-07's "browsable, comparable set" half — decodes and evaluates several instances. */
export async function POST(
  request: Request,
  { params }: { params: Promise<{ projectId: string; id: string }> },
) {
  const { projectId, id } = await params;
  const body = await request.json().catch(() => ({}));
  return proxyRequest(
    "POST",
    `/api/v0/projects/${projectId}/cem/archspace/${id}/generate-instances`,
    body,
  );
}
