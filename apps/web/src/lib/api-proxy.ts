const API_URL = process.env.API_URL ?? "http://localhost:8080";

// Buffered (arrayBuffer), not streamed — matches this whole proxy layer's existing style, and
// consistent with axum having no request-streaming story here either. `arrayBuffer()` (not
// `text()`) is required for correctness once a route can return arbitrary binary content (file
// attachment downloads) — a UTF-8 text round-trip would corrupt non-UTF-8 bytes. Safe for every
// existing JSON/CSV/HTML caller too, since a text body round-trips byte-for-byte through an
// ArrayBuffer.
async function relay(upstream: Response): Promise<Response> {
  const responseBody = await upstream.arrayBuffer();
  // The Response constructor throws if given a body (even empty) alongside a null-body status —
  // apps/api's PATCH/PUT endpoints return 204 with no content, so this isn't an edge case.
  const NULL_BODY_STATUSES = new Set([101, 204, 205, 304]);
  const headers: HeadersInit = {
    "Content-Type": upstream.headers.get("Content-Type") ?? "application/json",
  };
  // Forwarded so a download's "attachment" disposition survives the proxy hop — a browser only
  // triggers a native download if this header reaches the response it actually sees, not just the
  // upstream response the proxy fetched.
  const contentDisposition = upstream.headers.get("Content-Disposition");
  if (contentDisposition) {
    headers["Content-Disposition"] = contentDisposition;
  }
  return new Response(NULL_BODY_STATUSES.has(upstream.status) ? null : responseBody, {
    status: upstream.status,
    headers,
  });
}

function unreachable(): Response {
  return Response.json(
    { error: `Could not reach the API at ${API_URL} — is \`cargo run -p api\` running?` },
    { status: 502 },
  );
}

/**
 * Proxies a JSON-body request to `apps/api` (the Rust/Axum backend). Runs server-side in a
 * Next.js Route Handler — the browser only ever calls same-origin `/api/...` paths, so no CORS
 * configuration is needed on the Axum side.
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
    return await relay(upstream);
  } catch {
    return unreachable();
  }
}

/**
 * Proxies a raw (typically multipart/form-data) POST body straight through to `apps/api` — used
 * for file uploads (FR-EXPORT-04 attachments). Forwards the incoming request's exact bytes and
 * original `Content-Type` (boundary included) unchanged. Passing a raw `ArrayBuffer` as `fetch`'s
 * body — not a reconstructed `FormData` — matters: `fetch` only auto-generates a Content-Type
 * (with a *new*, mismatched boundary) when the body is `FormData` itself, so a manually-copied
 * header on an `ArrayBuffer` body is honored rather than silently overwritten.
 */
export async function proxyMultipart(path: string, request: Request): Promise<Response> {
  try {
    const contentType = request.headers.get("Content-Type");
    const body = await request.arrayBuffer();
    const upstream = await fetch(`${API_URL}${path}`, {
      method: "POST",
      cache: "no-store",
      headers: contentType ? { "Content-Type": contentType } : undefined,
      body,
    });
    return await relay(upstream);
  } catch {
    return unreachable();
  }
}

export function proxyGet(path: string): Promise<Response> {
  return proxyRequest("GET", path);
}
