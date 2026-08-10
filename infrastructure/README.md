# infrastructure

Provider-parameterized Terraform for the deployment-mode readiness work (NFR-COMP-01…05 in
`docs/Axioma_requirements_v3.md` §3.4). **Not required for local development** — see the root
`docker-compose.yml` and `.devcontainer/` for that; this directory is about what a real deployment
provisions, not the dev stack.

## Structure

Three genuinely **independent root Terraform configurations** — `aws/`, `gcp/`, `on_prem/` — not
one root switching between conditional modules. (An earlier draft tried the conditional-module
approach; `terraform init` rejected it: a module that declares its own provider blocks — needed
here, since the Kubernetes/Helm providers in `aws/`/`gcp/` depend on that same module's own
EKS/GKE cluster outputs — can't be called with `count`/`for_each`. Separate root configs per
target is the standard real-world pattern for exactly this shape, and is what NFR-COMP-01
("no hard dependency on a single vendor") actually cashes out to operationally: apply whichever
one target you're deploying to.)

```
infrastructure/
├── aws/       # EKS + RDS Postgres + S3 + Neo4j via Helm
├── gcp/       # GKE + Cloud SQL Postgres + MinIO-on-K8s (GCS isn't S3-API-compatible) + Neo4j via Helm
└── on_prem/   # Postgres + Neo4j + MinIO all via Helm onto a supplied kubeconfig, zero cloud API calls
```

Each exposes the **same output contract**, so the app's config layer never needs to know which
one ran: `kubeconfig`, `postgres_connection_string`, `neo4j_bolt_url`, `s3_endpoint`/`s3_bucket`/
`s3_access_key`/`s3_secret_key` (the last four match `apps/api/src/store/objects.rs`'s
`ObjectStore::connect` signature exactly — that client is already provider-agnostic, this just
gives it something equally portable to point at). Neo4j has no managed offering from any cloud
vendor, so it's deployed identically (a Helm chart onto whichever cluster resulted) in all three —
already-inherent portability, not something built per cloud.

Each config takes the same three inputs: `project_name`, `region` (NFR-COMP-02 — a real cloud
region code for `aws`/`gcp`; a label only for `on_prem`, where the operator's own cluster choice
*is* the residency decision), `deployment_mode` (`"multi_tenant"` default | `"single_tenant"` —
NFR-COMP-05: `single_tenant` provisions a wholly separate, dedicated cluster/DB/bucket, never a
shared pool). `on_prem` additionally takes `kubeconfig_path` — it never provisions the cluster
itself, only what runs on it.

## What this is *not*

- **Not applied.** Nothing here has been run against a real cloud account — no credentials exist
  in this repo or its CI, and provisioning real infrastructure is a deliberate, separate,
  explicitly-authorized action, not something that happens as a side effect of writing the config.
  `terraform fmt`/`validate`/`init -backend=false` (no credentials needed for any of those) are
  the extent of what's been verified.
- **Not app delivery.** `api`/`web`/`lsp` don't have Dockerfiles yet (only `fuml-runtime` does) —
  this provisions the *platform* (cluster + data stores), not a path to deploy Axioma's own
  services onto it. A separate, later undertaking (impl §3.3 tier 4).
- **Not physical multi-region proof.** The `region` input is a real, wired variable, and
  `apps/api`'s `Project.region` (NFR-COMP-02) is a real, queryable fact from creation on — but
  nothing has actually applied two live regions to prove bytes are physically pinned. That would
  require the step above (a real `apply`) that this pass deliberately doesn't take.
- **Not auth enforcement.** NFR-COMP-03's abstraction lives in `apps/api/src/auth.rs`
  (`AuthProvider`/`LocalAuthProvider`/`OidcAuthProvider`, selected via the `AUTH_PROVIDER` env
  var) — real, working, swappable, but nothing rejects an unauthenticated request. See that
  module's doc comment.

## Verifying without applying

```sh
cd infrastructure/aws   # or gcp, or on_prem
terraform init -backend=false   # downloads providers from the public registry; no cloud creds needed
terraform validate
terraform fmt -check -recursive ..
```

_____
run project: pnpm dev:all