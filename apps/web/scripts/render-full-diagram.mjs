// docs/IMPLEMENTATION_KICKOFF.md scope-downs pass (FR-EXPORT-01, server-side half). Invoked by
// apps/web/src/app/api/projects/[projectId]/export/full-diagram/route.ts via `execFile` — never
// run directly by a user. Navigates a headless Chromium to this same Next.js server's own
// internal `/export/full-diagram/:projectId` route (see that page's own doc comment for why no
// auth guard is needed), waits for the page's own `data-diagram-ready="true"` readiness signal,
// screenshots just the canvas, and writes the PNG bytes to stdout.
//
// Usage: node render-full-diagram.mjs <projectId> <baseUrl>

import { chromium } from "playwright";

const [, , projectId, baseUrl] = process.argv;

if (!projectId || !baseUrl) {
  process.stderr.write("usage: render-full-diagram.mjs <projectId> <baseUrl>\n");
  process.exit(1);
}

const browser = await chromium.launch();
try {
  const page = await browser.newPage({ viewport: { width: 1920, height: 1080 } });
  await page.goto(`${baseUrl}/export/full-diagram/${encodeURIComponent(projectId)}`, {
    waitUntil: "networkidle",
  });
  await page.waitForSelector('[data-diagram-ready="true"]', { timeout: 30000 });
  const png = await page.screenshot({ fullPage: true });
  process.stdout.write(png);
} finally {
  await browser.close();
}
