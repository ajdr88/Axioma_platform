# Axioma MBSE Platform

Axioma is a cloud-native model-based systems engineering platform, built around a SysML v2 model
graph, shipped as two independently-shippable products:

- **Product 1 — MBSE Platform.** Requirements, architecture, safety, mission planning,
  traceability, simulation, collaboration.
- **Product 2 — Computational Engineering Model (CEM).** Sits on top of Product 1 — a grounded AI
  copilot (Mode A), a deterministic architecture/trade-study optimizer (Mode B), and manufacturable
  geometry synthesis (Mode C, research-risk).

This site renders the repo's own source-of-truth Markdown directly (`docs/` at the repo root) —
nothing here is a copy, so it can never drift out of sync with what's actually checked in.

## Where to start

- **[Requirements v5](Axioma_requirements_v5.md)** and **[Implementation v5](Axioma_implementation_v5.md)**
  are the current source of truth. Implementation v5's numbered sections (§9 onward) narrate every
  real build pass in order, including what was found, fixed, and deliberately scoped out along the
  way — read those before assuming a capability is or isn't built.
- **[Test Specification v4](Axioma_test_specification_v4.md)** has the full PASS/FAIL test suite,
  organized by test ID.
- **[Pending Items](pending_items_2026-09-01.md)** is a point-in-time, prioritized inventory of
  every genuinely open gap across the whole doc set — start there for "what's left."
- **[Design Philosophy](Axioma_design_philosophy.md)** covers the product's design system and UX
  principles.

## Not on this site

`CLAUDE.md` at the repo root is a separate, continuously-updated project-context file maintained
for AI coding agents (Claude Code) working in this repo — it carries the same architectural rules
and current status as the docs above, kept terser and rewritten in place rather than versioned by
revision letter. It lives outside this site's `docs_dir` on purpose; read it directly in the repo
if you're working with an agent, or as a fast-to-skim status summary either way.

Superseded v3/v4 requirements/implementation docs are kept under **Requirements**/**Implementation**
above for history, not deleted — v5/v4 (test spec) are current, the rest are historical record only.
