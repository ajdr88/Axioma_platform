import { proxyRequest } from "@/lib/api-proxy";

export async function GET() {
  return proxyRequest("GET", "/api/v0/contains");
}

export async function POST(request: Request) {
  const body = await request.json();
  return proxyRequest("POST", "/api/v0/contains", body);
}

export async function DELETE(request: Request) {
  const body = await request.json();
  return proxyRequest("DELETE", "/api/v0/contains", body);
}
