const API_URL = process.env.API_URL ?? "http://localhost:8080";

/**
 * Proxies a request to `apps/api` (the Rust/Axum backend). Runs server-side in a Next.js Route
 * Handler — the browser only ever calls same-origin `/api/...` paths, so no CORS configuration
 * is needed on the Axum side.
 */
export async function proxyRequest(
  method: "GET" | "POST" | "PATCH" | "PUT" | "DELETE",
  path: string,
  body?: unknown,
): Promise<Response> {
  try {
    const upstream = await fetch(`${API_URL}${path}`, {
      method,
      cache: "no-store",
      headers: body !== undefined ? { "Content-Type": "application/json" } : undefined,
      body: body !== undefined ? JSON.stringify(body) : undefined,
    });
    const responseBody = await upstream.text();
    // The Response constructor throws if given a body (even "") alongside a null-body status —
    // apps/api's PATCH/PUT endpoints return 204 with no content, so this isn't an edge case.
    const NULL_BODY_STATUSES = new Set([101, 204, 205, 304]);
    return new Response(NULL_BODY_STATUSES.has(upstream.status) ? null : responseBody, {
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

export function proxyGet(path: string): Promise<Response> {
  return proxyRequest("GET", path);
}
