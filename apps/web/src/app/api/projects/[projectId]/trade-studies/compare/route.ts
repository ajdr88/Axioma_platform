import { proxyRequest } from "@/lib/api-proxy";

/** T-P1.4-05: compares a variant branch's Fan bypass ratio against `main`'s live value, runs the
 * pilot's Control-state-machine sim as a regression check, and returns the estimated thrust
 * delta — see `apps/api/src/trade_study.rs`'s doc comment for the formula/scope this is built on. */
export async function POST(
  request: Request,
  { params }: { params: Promise<{ projectId: string }> },
) {
  const { projectId } = await params;
  const body = await request.json();
  return proxyRequest("POST", `/api/v0/projects/${projectId}/trade-studies/compare`, body);
}
