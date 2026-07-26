import { proxyRequest } from "@/lib/api-proxy";

export async function GET() {
  return proxyRequest("GET", "/api/v0/elements");
}

export async function POST(request: Request) {
  const body = await request.json();
  return proxyRequest("POST", "/api/v0/elements", body);
}
