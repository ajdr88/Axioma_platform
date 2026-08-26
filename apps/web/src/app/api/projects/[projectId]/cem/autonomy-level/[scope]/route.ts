import { proxyGet } from "@/lib/api-proxy";

/** Reports the implicit `L0`/no-threshold default when nothing has been configured yet for this
 * scope — see `apps/api/src/mode_b.rs::get_autonomy_level`'s doc comment. */
export async function GET(
  _request: Request,
  { params }: { params: Promise<{ projectId: string; scope: string }> },
) {
  const { projectId, scope } = await params;
  return proxyGet(`/api/v0/projects/${projectId}/cem/autonomy-level/${scope}`);
}
