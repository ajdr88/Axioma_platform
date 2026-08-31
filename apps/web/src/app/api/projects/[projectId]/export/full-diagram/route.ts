import { execFile } from "node:child_process";
import path from "node:path";
import { promisify } from "node:util";

const execFileAsync = promisify(execFile);

/** FR-EXPORT-01 (the server-side half) — a headless-render path for the full diagram at any
 * size, distinct from the existing client-side viewport-only "Export PNG". Shells out to
 * `apps/web/scripts/render-full-diagram.mjs` (a real `playwright` dependency, not the dev-only
 * on-demand fetch earlier verification passes used) rather than driving Chromium in-process —
 * keeps the heavy Playwright/Chromium runtime isolated to a child process invoked only when this
 * route is actually hit, not loaded into the main Next.js server process on every request. */
export async function GET(
  request: Request,
  { params }: { params: Promise<{ projectId: string }> },
) {
  const { projectId } = await params;
  const baseUrl = new URL(request.url).origin;
  const scriptPath = path.join(process.cwd(), "scripts", "render-full-diagram.mjs");

  try {
    const { stdout } = await execFileAsync("node", [scriptPath, projectId, baseUrl], {
      encoding: "buffer",
      maxBuffer: 50 * 1024 * 1024, // 50MB — a full-diagram screenshot can be large
      timeout: 45000,
    });
    return new Response(stdout, {
      status: 200,
      headers: {
        "Content-Type": "image/png",
        "Content-Disposition": `attachment; filename="axioma-full-diagram-${projectId}.png"`,
      },
    });
  } catch (error) {
    return Response.json(
      {
        error: `full-diagram render failed: ${error instanceof Error ? error.message : String(error)}`,
      },
      { status: 502 },
    );
  }
}
