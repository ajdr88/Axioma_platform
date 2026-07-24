# Axioma

A cloud-native model-based systems engineering platform built around a SysML v2 model graph.

Shipped as two independently-shippable products — see [`CLAUDE.md`](CLAUDE.md) for the full
architecture summary and [`docs/`](docs/) for the source-of-truth specs:

- **Product 1 — MBSE Platform** (this repo's current focus): requirements, architecture, safety,
  mission planning, traceability, simulation, collaboration.
- **Product 2 — Computational Engineering Model (CEM):** a generative, physics-validated design
  layer on top of Product 1. Not started yet — see ADR-001 in
  [`docs/Axioma_implementation_v3.md`](docs/Axioma_implementation_v3.md#25-architecture-decision-records-adr-log-rev-b-d2).

## Docs

| Doc | Contents |
| --- | --- |
| [`docs/Axioma_requirements_v3.md`](docs/Axioma_requirements_v3.md) | Functional & non-functional requirements |
| [`docs/Axioma_implementation_v3.md`](docs/Axioma_implementation_v3.md) | Architecture, tech stack, ADR log, roadmap |
| [`docs/Axioma_test_specification_v3.md`](docs/Axioma_test_specification_v3.md) | Turbofan pilot acceptance tests |
| [`docs/Axioma_design_philosophy.md`](docs/Axioma_design_philosophy.md) | "Structural Clarity" design system |

## Prerequisites

- [Docker Desktop](https://www.docker.com/products/docker-desktop/)
- [Node.js 24+](https://nodejs.org/) (ships `corepack`, which provides `pnpm`)
- [Rust](https://rustup.rs/) (stable toolchain)
- VS Code + the Dev Containers extension (optional, but `.devcontainer/` mirrors this setup)

## Quickstart

```sh
corepack enable
pnpm install

docker compose up -d      # Neo4j, Postgres (host port 5433), MinIO, Redis
cp .env.example .env      # apps/api's store connection settings (defaults already match compose)

pnpm dev                  # apps/web on http://localhost:3000
cargo run -p api          # apps/api on http://localhost:8080 — reads/writes Neo4j+Postgres+MinIO
```

`cargo test -p api -- --ignored` runs the store integration tests against the live stack above
(skipped by default so `cargo test --workspace` never needs a live stack, e.g. in CI).

## Monorepo layout

```
apps/
  web/          Next.js 19 + React Flow frontend
  api/          Rust (Axum) REST surface
  docs/         docs site (placeholder)
packages/
  sysml-core/       SysML v2 / KerML parsing + semantic-validation layer
  diagram-engine/   React Flow node/edge components
  shared-types/     TS types generated from Rust structs
  ui-components/    "Structural Clarity" design system (Tailwind + shadcn/ui)
  cem-*, llm-gateway/, scheduler/, fuml-runtime/, alf-lite/
                    Product 2 (CEM) — not started, see ADR-001
infrastructure/     Terraform/K8s (provider-parameterized) — not started
```

## Status

Early **P1.1 Core Graph** work. `apps/api` is wired to real Neo4j (topology) + Postgres (element
bodies) + MinIO (blob pointers) per the ADR-003 polyglot split, with one semantic-validation rule
(containment acyclicity) enforced before every write. Still ahead: the rest of the validation rule
set, Git-backed model versioning, auth, and import — see [`CLAUDE.md`](CLAUDE.md) for the full
roadmap and non-negotiable architectural rules.
