# apps/docs

A [MkDocs](https://www.mkdocs.org/) + [Material](https://squidfunk.github.io/mkdocs-material/)
site that renders the repo's own source-of-truth Markdown (`docs/` at the repo root) directly —
`mkdocs.yml`'s `docs_dir` points at `../../docs`, so this is not a copy of anything and can never
drift out of sync with what's actually checked in. Same "standalone Python tool, not a pnpm
workspace member" convention as `packages/cem-archspace` — no `package.json`, not in Turborepo's
pipeline, driven purely via its own venv.

## Setup

```bash
cd apps/docs
python -m venv .venv          # or: uv venv .venv
.venv/Scripts/pip install -r requirements.txt   # Windows; use .venv/bin/pip on Linux/macOS
```

Confirmed installed/working (2026-09-01): `mkdocs==1.6.1`, `mkdocs-material==9.7.7`, pinned in
`requirements.txt`.

## Run locally

```bash
.venv/Scripts/python -m mkdocs serve   # → http://127.0.0.1:8000, live-reloads on doc edits
```

## Build (static output, for hosting)

```bash
.venv/Scripts/python -m mkdocs build --strict   # → apps/docs/site/, gitignored
```

`--strict` fails the build on broken internal links or a page referenced in `nav` that doesn't
exist — worth running before trusting a nav change.

## What's in `nav` vs. what's excluded, and why

`mkdocs.yml`'s `nav` curates the current, actively-maintained doc set: v5 requirements/
implementation (current), v3/v4 (kept, marked superseded), the test specification, design
philosophy, the Stage Tracking amendment, the implementation kickoff plan, and the latest pending-
items snapshot. Deliberately left out of `nav` (still present in `docs_dir`, just unlinked from
navigation — a normal, warning-free MkDocs state, confirmed via `--strict`):

- **`docs/CLAUDE.md`** — a stale, pre-v5 copy; the real, continuously-updated project-context file
  lives at the repo root, outside this site's `docs_dir` on purpose (see `docs/index.md`'s own
  "Not on this site" note — it's agent context, not versioned end-user documentation).
- **`docs/claude/*.md`** — the original amendment/analysis drafts Phase 0 merged into v5; kept for
  history per `CLAUDE.md`'s own note, not meant for day-to-day navigation.

If a new doc file needs a nav entry, add it to `mkdocs.yml`'s `nav:` list — nothing else to wire up,
since `docs_dir` already points at the right folder.
