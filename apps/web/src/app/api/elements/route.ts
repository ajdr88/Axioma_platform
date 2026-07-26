import { proxyGet } from "@/lib/api-proxy";

export async function GET() {
  return proxyGet("/api/v0/elements");
}
