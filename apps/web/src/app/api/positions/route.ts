import { proxyRequest } from "@/lib/api-proxy";

export async function GET() {
  return proxyRequest("GET", "/api/v0/positions");
}
