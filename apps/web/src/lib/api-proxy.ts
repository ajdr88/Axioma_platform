const API_URL = process.env.API_URL ?? "http://localhost:8080";

/**
 * Proxies a GET request to `apps/api` (the Rust/Axum backend). Runs server-side in a Next.js
 * Route Handler — the browser only ever calls same-origin `/api/...` paths, so no CORS
 * configuration is needed on the Axum side.
 */
export async function proxyGet(path: string): Promise<Response> {
  try {
    const upstream = await fetch(`${API_URL}${path}`, { cache: "no-store" });
    const body = await upstream.text();
    return new Response(body, {
      status: upstream.status,
      headers: { "Content-Type": upstream.headers.get("Content-Type") ?? "application/json" },
    });
  } catch {
    return Response.json(
      {
        error: `Could not reach the API at ${API_URL} — is \`cargo run -p api\` running?`,
      },
      { status: 502 },
    );
  }
}
