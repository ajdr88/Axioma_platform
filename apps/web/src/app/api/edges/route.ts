import { proxyRequest } from "@/lib/api-proxy";

/** Generic edge kinds (everything besides `Contains`, which keeps its own `/api/contains` route) —
 * e.g. the Hazard/Risk panel's `Causes`/`MitigatedBy` edges. */
export async function GET(request: Request) {
  const kind = new URL(request.url).searchParams.get("kind");
  return proxyRequest("GET", `/api/v0/edges?kind=${encodeURIComponent(kind ?? "")}`);
}

export async function POST(request: Request) {
  const body = await request.json();
  return proxyRequest("POST", "/api/v0/edges", body);
}

export async function DELETE(request: Request) {
  const body = await request.json();
  return proxyRequest("DELETE", "/api/v0/edges", body);
}
