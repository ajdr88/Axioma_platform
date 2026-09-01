//! Git-backed model versioning (roadmap: P1.1, T-P1.1-05) — an abstract Postgres-backed
//! Commit/Branch/Project model, not a literal on-disk git repository: "Git-backed" here means
//! the vocabulary/semantics (branch, commit, diff, history) are borrowed, reusing the polyglot
//! stores already in place rather than adding a `git2`/`gix` dependency and a from-scratch
//! serialization format neither the docs nor any test specify.
//!
//! **Delta chain, not snapshot-per-commit** (revised after a real, measured T-P1.1-07 failure —
//! see `apps/api/src/traceability.rs`'s doc comment for the numbers). Each commit stores only its
//! own (usually single-entry) diff — no full graph copy. Reconstructing "the state as of commit
//! X" (needed for `main`-vs-branch diffing and for layering a branch's own edits) replays a
//! commit chain instead of comparing two stored copies:
//!
//! - `main`'s current state is *never* reconstructed from commits at all — it's always exactly
//!   the live Neo4j/Postgres graph (`apps/api/src/main.rs`'s `build_snapshot`), fetched lazily
//!   only when a diff is actually requested, never on every write. This is what makes an ordinary
//!   element create O(1) again: `record_commit` now inserts one small diff row and advances a
//!   pointer, nothing else.
//! - Every branch remembers the exact commit it forked from (`Branch::fork_commit_id`, set once
//!   at creation, never mutated). Reconstructing a branch's head = recursively resolve its fork
//!   point's snapshot, then replay that branch's own commits (and only that branch's — the walk
//!   stops the moment it crosses into a different `branch_id`) on top, oldest-first
//!   (`apps/api/src/main.rs`'s `resolve_snapshot`, `apply_diff` below).
//!
//! This trades a cheap, unbounded-scale write path for a reconstruction walk on read — the right
//! trade here, since only branch-scoped edits and cross-commit diffs (a deliberately rare,
//! reviewed, human-triggered path — there's no "browse history" feature) ever need
//! reconstruction; every ordinary create/rename/edge/body mutation never does. **Known, accepted
//! limit, not solved here**: reconstructing an *old, non-head* `main` commit (unreachable through
//! any current endpoint — nothing exposes a historical `main` commit id, only branch heads) would
//! replay `main`'s entire commit history from empty, same as walking any other branch's chain to
//! its root; fine for how few ordinary commits a project accumulates today, a real cost only if
//! that ever changes and something starts asking for it.
//!
//! Only a project's `main` branch is ever "live" in Neo4j/Postgres — every other branch exists
//! purely as a chain of stored diffs and never mutates the live graph, matching how impl §2.4
//! describes a CEM proposal: "lands as branch/Commit like human changes" (i.e. reviewable and
//! diffable before anything is live).

use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result};
use sqlx::{postgres::PgPoolOptions, PgPool};
use sysml_core::{EdgeKind, Element, ElementId, NodeKind, Origin};
use sysml_textual::GraphOp;

pub const MAIN_BRANCH: &str = "main";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    /// NFR-COMP-02 (data residency): the region this project's data is declared to live in.
    /// Recorded and returned as a first-class fact from creation on — this alone doesn't
    /// physically place the bytes in that region (this deployment has one Postgres/Neo4j/object
    /// store, not a real per-region topology); it's the hook a real multi-region deployment
    /// (`infrastructure/`'s `region` variable) wires actual placement to.
    pub region: String,
}

/// The default when a caller doesn't specify one — not a real region-pinning decision on its
/// own, just what a brand-new single-region dev/local deployment already is.
pub const DEFAULT_REGION: &str = "us-east";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Branch {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub head_commit_id: Option<String>,
    /// The commit this branch was forked from, fixed forever at creation time (`None` for
    /// `main`, which forks from nothing). Unlike `head_commit_id`, this never advances — it's
    /// the anchor `resolve_snapshot` recurses to once it walks this branch's own commits back to
    /// their root.
    pub fork_commit_id: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Commit {
    pub id: String,
    pub project_id: String,
    pub branch_id: String,
    pub parent_commit_id: Option<String>,
    pub message: String,
    pub actor: String,
    pub diff: Vec<DiffEntry>,
}

/// Full graph state as of some commit — elements, every edge kind (including `Contains`), and
/// element bodies (rationale + properties). Postgres canvas position is deliberately excluded
/// (UI metadata, not modeling content, same NFR-DATA-01 stance the rest of the app already
/// takes toward position). Never stored wholesale anymore (see the module doc comment) — always
/// either the live graph (`main`'s current state) or reconstructed by replaying a diff chain
/// onto `Snapshot::default()` via `apply_diff`.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Snapshot {
    pub elements: Vec<Element>,
    pub edges: Vec<SnapshotEdge>,
    pub bodies: HashMap<ElementId, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct SnapshotEdge {
    pub source: ElementId,
    pub target: ElementId,
    pub kind: EdgeKind,
}

/// Every mutation kind the existing write paths produce — covers more than T-P1.1-05's one
/// property-change scenario, since every existing mutating endpoint now commits one of these.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum DiffEntry {
    PropertyChanged {
        element_id: ElementId,
        property: String,
        old: serde_json::Value,
        new: serde_json::Value,
    },
    RationaleChanged {
        element_id: ElementId,
        old: Option<String>,
        new: Option<String>,
    },
    ElementRenamed {
        element_id: ElementId,
        old_name: String,
        new_name: String,
    },
    ElementCreated {
        element_id: ElementId,
        kind: NodeKind,
        name: String,
    },
    ElementDeleted {
        element_id: ElementId,
        kind: NodeKind,
        name: String,
    },
    ElementActiveChanged {
        element_id: ElementId,
        active: bool,
    },
    ElementOriginChanged {
        element_id: ElementId,
        origin: Origin,
    },
    EdgeCreated {
        source: ElementId,
        target: ElementId,
        kind: EdgeKind,
    },
    EdgeDeleted {
        source: ElementId,
        target: ElementId,
        kind: EdgeKind,
    },
    TextModelApplied {
        ops: Vec<GraphOp>,
    },
    /// NFR-CEM-06 (Autonomy Auditability) — recorded via `record_audit` directly, never through
    /// `record_commit`: an autonomy-level change isn't a graph mutation (nothing for `apply_diff`
    /// below to actually replay — see its own no-op arm), it's project-level config, so it never
    /// touches `main`'s commit history, only the audit log.
    AutonomyLevelChanged {
        scope: String,
        old_level: Option<String>,
        new_level: String,
    },
}

async fn create_versioning_tables(conn: &mut sqlx::PgConnection) -> Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS projects (\
            id TEXT PRIMARY KEY, \
            name TEXT NOT NULL, \
            created_at TIMESTAMPTZ NOT NULL DEFAULT now()\
        )",
    )
    .execute(&mut *conn)
    .await
    .context("creating projects table")?;
    // Migration for pre-existing dev databases (see `branches.fork_commit_id`'s identical
    // pattern above) — `NOT NULL DEFAULT` backfills every existing row in the same statement.
    // The literal below must match `DEFAULT_REGION` — sqlx's compile-time SQL-injection lint
    // requires a `&'static str`, so it can't be interpolated from the Rust constant directly.
    sqlx::query(
        "ALTER TABLE projects ADD COLUMN IF NOT EXISTS region TEXT NOT NULL DEFAULT 'us-east'",
    )
    .execute(&mut *conn)
    .await
    .context("adding projects.region column")?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS branches (\
            id TEXT PRIMARY KEY, \
            project_id TEXT NOT NULL, \
            name TEXT NOT NULL, \
            head_commit_id TEXT, \
            created_at TIMESTAMPTZ NOT NULL DEFAULT now(), \
            UNIQUE (project_id, name)\
        )",
    )
    .execute(&mut *conn)
    .await
    .context("creating branches table")?;
    // Migration for pre-existing dev databases (see `postgres.rs`'s identical pattern for
    // `project_id`) — fixed forever per branch once set, see `Branch::fork_commit_id`'s doc
    // comment.
    sqlx::query("ALTER TABLE branches ADD COLUMN IF NOT EXISTS fork_commit_id TEXT")
        .execute(&mut *conn)
        .await
        .context("adding branches.fork_commit_id column")?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS commits (\
            id TEXT PRIMARY KEY, \
            project_id TEXT NOT NULL, \
            branch_id TEXT NOT NULL, \
            parent_commit_id TEXT, \
            message TEXT NOT NULL, \
            actor TEXT NOT NULL, \
            created_at TIMESTAMPTZ NOT NULL DEFAULT now(), \
            diff JSONB NOT NULL\
        )",
    )
    .execute(&mut *conn)
    .await
    .context("creating commits table")?;
    // Migration for pre-existing dev databases: commits used to carry a full-graph `snapshot`
    // column (the T-P1.1-07 bottleneck this module's doc comment describes) — no longer written
    // or read anywhere, dropped rather than left as unused dead weight.
    sqlx::query("ALTER TABLE commits DROP COLUMN IF EXISTS snapshot")
        .execute(&mut *conn)
        .await
        .context("dropping commits.snapshot column")?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS audit_log (\
            id TEXT PRIMARY KEY, \
            project_id TEXT NOT NULL, \
            actor TEXT NOT NULL, \
            created_at TIMESTAMPTZ NOT NULL DEFAULT now(), \
            diff JSONB NOT NULL\
        )",
    )
    .execute(&mut *conn)
    .await
    .context("creating audit_log table")?;

    // P2.2 (FR-CEM-16/17) — `scope` is an opaque caller-assigned string ("project" for
    // project-wide, the only scope this pass's UI/tests exercise; finer-grained scopes per
    // FR-CEM-17 are a natural extension of the same column, not a schema change).
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS autonomy_config (\
            project_id TEXT NOT NULL, \
            scope TEXT NOT NULL, \
            level TEXT NOT NULL, \
            mass_deviation_threshold_percent DOUBLE PRECISION, \
            updated_by TEXT NOT NULL, \
            updated_at TIMESTAMPTZ NOT NULL DEFAULT now(), \
            PRIMARY KEY (project_id, scope)\
        )",
    )
    .execute(&mut *conn)
    .await
    .context("creating autonomy_config table")?;

    // P2.2 (T-P2.2-01) — one row per *subsystem*, not per `propose` call, so "each element
    // individually accept/reject-able" is real rather than all-or-nothing; see
    // `store::versioning`'s module doc comment and `mode_b.rs`'s `propose` for how the
    // containing `branch_id` and these rows relate.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS proposals (\
            id TEXT PRIMARY KEY, \
            project_id TEXT NOT NULL, \
            branch_id TEXT NOT NULL, \
            subsystem_id TEXT NOT NULL, \
            status TEXT NOT NULL, \
            candidate JSONB NOT NULL, \
            top_level_requirement_ids JSONB NOT NULL, \
            reason TEXT NOT NULL, \
            origin TEXT NOT NULL DEFAULT 'cem-generated', \
            created_at TIMESTAMPTZ NOT NULL DEFAULT now()\
        )",
    )
    .execute(&mut *conn)
    .await
    .context("creating proposals table")?;

    // docs/IMPLEMENTATION_KICKOFF.md Phase 1 — `origin` distinguishes `cem-generated` (P2.2's
    // only real caller today, `mode_b.rs::propose`) from the still-unbuilt `human-authored`
    // (FR-PM-05) and `document-import` (FR-CORE-16) origins the merged spec already names.
    // Migration for pre-existing dev databases (see `region`'s identical pattern above) — the
    // `proposals` table already existed on this machine from P2.2 work earlier this session.
    sqlx::query("ALTER TABLE proposals ADD COLUMN IF NOT EXISTS origin TEXT NOT NULL DEFAULT 'cem-generated'")
        .execute(&mut *conn)
        .await
        .context("adding proposals.origin column")?;

    // Tier 1 pass (item 6, FR-ARCH-05-adjacent) — real persistence for a Mode B design-space
    // *definition* (small, serializable — everything `cem_core::archspace::encode_design_space`
    // produced), keyed by the sidecar's own `handle_id`. This is deliberately NOT an attempt to
    // persist the sidecar's own live constructed `BasicDSG` graph object (which stays process-
    // lifetime/in-memory, per `cem-archspace/README.md`'s own recommendation) — the durable thing
    // here is "the input," which `archspace::resolve_or_redefine` uses to transparently rebuild a
    // fresh sidecar handle whenever a stale one 404s (e.g. after a sidecar restart). See
    // `apps/api/src/archspace.rs`'s own doc comment for the full recovery flow.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS archspace_design_spaces (\
            handle_id TEXT PRIMARY KEY, \
            project_id TEXT NOT NULL, \
            subsystem_id TEXT NOT NULL, \
            definition JSONB NOT NULL, \
            created_at TIMESTAMPTZ NOT NULL DEFAULT now()\
        )",
    )
    .execute(&mut *conn)
    .await
    .context("creating archspace_design_spaces table")?;

    Ok(())
}

#[derive(Clone)]
pub struct VersioningStore {
    pool: PgPool,
}

/// Arbitrary constant for `pg_advisory_lock` — serializes the one-time schema creation below
/// across concurrent callers (multiple integration tests connecting in parallel to a fresh
/// schema, or multiple API replicas starting simultaneously in a real deployment). Postgres's
/// own `CREATE TABLE IF NOT EXISTS` is *not* atomic under true concurrency — two connections
/// racing to create the same brand-new table can both pass the "not exists" check and then
/// collide on an internal catalog constraint (confirmed directly: running this crate's
/// integration tests in parallel against a fresh database surfaced exactly this as a "duplicate
/// key value violates unique constraint pg_type_typname_nsp_index" error).
const SCHEMA_LOCK_ID: i64 = 0x4158_494f_4d41; // "AXIOMA" in hex, arbitrary but stable

impl VersioningStore {
    pub async fn connect(database_url: &str) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await
            .context("connecting to Postgres (versioning)")?;

        // `pg_advisory_lock`/`_unlock` are session-scoped (held by one specific backend
        // connection, not "the pool") — acquiring and releasing must happen on the *same*
        // checked-out connection, not via `&pool` (which could hand the lock and unlock calls to
        // two different pooled connections, silently no-op-ing the unlock and deadlocking the
        // next caller).
        let mut conn = pool
            .acquire()
            .await
            .context("acquiring a connection for schema setup")?;
        sqlx::query("SELECT pg_advisory_lock($1)")
            .bind(SCHEMA_LOCK_ID)
            .execute(&mut *conn)
            .await
            .context("acquiring schema lock")?;

        let setup = create_versioning_tables(&mut conn).await;

        sqlx::query("SELECT pg_advisory_unlock($1)")
            .bind(SCHEMA_LOCK_ID)
            .execute(&mut *conn)
            .await
            .context("releasing schema lock")?;
        drop(conn);
        setup?;

        Ok(Self { pool })
    }

    pub async fn ping(&self) -> Result<()> {
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .context("Postgres (versioning) ping failed")?;
        Ok(())
    }

    pub async fn count_projects(&self) -> Result<i64> {
        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM projects")
            .fetch_one(&self.pool)
            .await
            .context("counting projects")?;
        Ok(count)
    }

    /// Creates a project and its `main` branch (no commits yet) in one call — every project
    /// always has a `main` branch, the same "every git repo starts with a default branch"
    /// invariant this borrows the vocabulary from. `region` is NFR-COMP-02's data-residency
    /// declaration (see `Project::region`'s doc comment) — pass `DEFAULT_REGION` if the caller
    /// doesn't care.
    pub async fn create_project(&self, name: &str, region: &str) -> Result<Project> {
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO projects (id, name, region) VALUES ($1, $2, $3)")
            .bind(&id)
            .bind(name)
            .bind(region)
            .execute(&self.pool)
            .await
            .context("inserting project")?;

        let branch_id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO branches (id, project_id, name, head_commit_id, fork_commit_id) \
             VALUES ($1, $2, $3, NULL, NULL)",
        )
        .bind(&branch_id)
        .bind(&id)
        .bind(MAIN_BRANCH)
        .execute(&self.pool)
        .await
        .context("creating main branch")?;

        Ok(Project {
            id,
            name: name.to_string(),
            region: region.to_string(),
        })
    }

    pub async fn list_projects(&self) -> Result<Vec<Project>> {
        let rows: Vec<(String, String, String)> =
            sqlx::query_as("SELECT id, name, region FROM projects ORDER BY created_at")
                .fetch_all(&self.pool)
                .await
                .context("listing projects")?;
        Ok(rows
            .into_iter()
            .map(|(id, name, region)| Project { id, name, region })
            .collect())
    }

    pub async fn get_project(&self, id: &str) -> Result<Option<Project>> {
        let row: Option<(String, String, String)> =
            sqlx::query_as("SELECT id, name, region FROM projects WHERE id = $1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .context("fetching project")?;
        Ok(row.map(|(id, name, region)| Project { id, name, region }))
    }

    pub async fn get_branch(&self, project_id: &str, name: &str) -> Result<Option<Branch>> {
        let row: Option<(String, String, Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT id, name, head_commit_id, fork_commit_id FROM branches \
             WHERE project_id = $1 AND name = $2",
        )
        .bind(project_id)
        .bind(name)
        .fetch_optional(&self.pool)
        .await
        .context("fetching branch")?;
        Ok(
            row.map(|(id, name, head_commit_id, fork_commit_id)| Branch {
                id,
                project_id: project_id.to_string(),
                name,
                head_commit_id,
                fork_commit_id,
            }),
        )
    }

    /// Looks up a branch by its own id rather than `(project_id, name)` — needed once a commit
    /// is in hand (`Commit::branch_id`) and the branch's name isn't known, e.g. mid-`resolve_snapshot`.
    pub async fn get_branch_by_id(&self, branch_id: &str) -> Result<Option<Branch>> {
        type BranchRow = (String, String, String, Option<String>, Option<String>);
        let row: Option<BranchRow> = sqlx::query_as(
            "SELECT id, project_id, name, head_commit_id, fork_commit_id FROM branches \
             WHERE id = $1",
        )
        .bind(branch_id)
        .fetch_optional(&self.pool)
        .await
        .context("fetching branch by id")?;
        Ok(row.map(
            |(id, project_id, name, head_commit_id, fork_commit_id)| Branch {
                id,
                project_id,
                name,
                head_commit_id,
                fork_commit_id,
            },
        ))
    }

    pub async fn list_branches(&self, project_id: &str) -> Result<Vec<Branch>> {
        let rows: Vec<(String, String, Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT id, name, head_commit_id, fork_commit_id FROM branches \
             WHERE project_id = $1 ORDER BY created_at",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await
        .context("listing branches")?;
        Ok(rows
            .into_iter()
            .map(|(id, name, head_commit_id, fork_commit_id)| Branch {
                id,
                project_id: project_id.to_string(),
                name,
                head_commit_id,
                fork_commit_id,
            })
            .collect())
    }

    /// Creates a branch pointing at `from_commit_id` (defaults to `main`'s current head) — a
    /// lightweight pointer with no commits of its own until the first one lands on it, same as a
    /// fresh git branch. `fork_commit_id` is set to that same starting point and never changes
    /// again — see its doc comment on `Branch`.
    pub async fn create_branch(
        &self,
        project_id: &str,
        name: &str,
        from_commit_id: Option<&str>,
    ) -> Result<Branch> {
        let head_commit_id = match from_commit_id {
            Some(id) => Some(id.to_string()),
            None => self
                .get_branch(project_id, MAIN_BRANCH)
                .await?
                .and_then(|b| b.head_commit_id),
        };
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO branches (id, project_id, name, head_commit_id, fork_commit_id) \
             VALUES ($1, $2, $3, $4, $4)",
        )
        .bind(&id)
        .bind(project_id)
        .bind(name)
        .bind(&head_commit_id)
        .execute(&self.pool)
        .await
        .with_context(|| format!("creating branch {name}"))?;
        Ok(Branch {
            id,
            project_id: project_id.to_string(),
            name: name.to_string(),
            fork_commit_id: head_commit_id.clone(),
            head_commit_id,
        })
    }

    /// Inserts a commit and advances its branch's head pointer — the one write path every
    /// mutation (existing endpoints on `main`, or a branch-scoped property edit) goes through.
    /// Stores only `diff` — no full-graph snapshot, see the module doc comment for why.
    pub async fn commit(
        &self,
        project_id: &str,
        branch: &Branch,
        actor: &str,
        message: &str,
        diff: &[DiffEntry],
    ) -> Result<Commit> {
        let id = uuid::Uuid::new_v4().to_string();
        let diff_json = serde_json::to_value(diff).context("serializing commit diff")?;

        sqlx::query(
            "INSERT INTO commits (id, project_id, branch_id, parent_commit_id, message, actor, diff) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(&id)
        .bind(project_id)
        .bind(&branch.id)
        .bind(&branch.head_commit_id)
        .bind(message)
        .bind(actor)
        .bind(&diff_json)
        .execute(&self.pool)
        .await
        .context("inserting commit")?;

        sqlx::query("UPDATE branches SET head_commit_id = $1 WHERE id = $2")
            .bind(&id)
            .bind(&branch.id)
            .execute(&self.pool)
            .await
            .context("advancing branch head")?;

        Ok(Commit {
            id,
            project_id: project_id.to_string(),
            branch_id: branch.id.clone(),
            parent_commit_id: branch.head_commit_id.clone(),
            message: message.to_string(),
            actor: actor.to_string(),
            diff: diff.to_vec(),
        })
    }

    pub async fn get_commit(&self, commit_id: &str) -> Result<Option<Commit>> {
        type CommitRow = (
            String,
            String,
            Option<String>,
            String,
            String,
            serde_json::Value,
        );
        let row: Option<CommitRow> = sqlx::query_as(
            "SELECT project_id, branch_id, parent_commit_id, message, actor, diff \
             FROM commits WHERE id = $1",
        )
        .bind(commit_id)
        .fetch_optional(&self.pool)
        .await
        .context("fetching commit")?;

        let Some((project_id, branch_id, parent_commit_id, message, actor, diff_json)) = row else {
            return Ok(None);
        };
        let diff: Vec<DiffEntry> =
            serde_json::from_value(diff_json).context("parsing stored diff")?;
        Ok(Some(Commit {
            id: commit_id.to_string(),
            project_id,
            branch_id,
            parent_commit_id,
            message,
            actor,
            diff,
        }))
    }

    pub async fn record_audit(
        &self,
        project_id: &str,
        actor: &str,
        diff: &DiffEntry,
    ) -> Result<()> {
        let id = uuid::Uuid::new_v4().to_string();
        let diff_json = serde_json::to_value(diff).context("serializing audit diff")?;
        sqlx::query("INSERT INTO audit_log (id, project_id, actor, diff) VALUES ($1, $2, $3, $4)")
            .bind(&id)
            .bind(project_id)
            .bind(actor)
            .bind(&diff_json)
            .execute(&self.pool)
            .await
            .context("inserting audit log entry")?;
        Ok(())
    }

    /// NFR-CEM-06 (Autonomy Auditability) — the read side of `record_audit`, oldest-first.
    /// `created_at` comes back as its `text` cast rather than a typed timestamp: nothing else in
    /// this store decodes `TIMESTAMPTZ` today, and a plain non-empty string is enough to prove the
    /// column's own `NOT NULL DEFAULT now()` did its job without pulling in a new date/time crate
    /// just for this.
    ///
    /// `#[allow(dead_code)]`: this pass's autonomy work verifies auditing directly against this
    /// accessor (see `main.rs`'s `autonomy_level_change_is_audited_with_actor_and_old_new_levels`)
    /// rather than through a dedicated HTTP endpoint — no `GET .../audit-log` was in scope, so
    /// nothing in the non-test build calls this yet.
    #[allow(dead_code)]
    pub async fn list_audit_log(&self, project_id: &str) -> Result<Vec<AuditLogEntry>> {
        let rows: Vec<(String, String, serde_json::Value)> = sqlx::query_as(
            "SELECT actor, created_at::text, diff FROM audit_log \
             WHERE project_id = $1 ORDER BY created_at",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await
        .context("listing audit log")?;
        rows.into_iter()
            .map(|(actor, created_at, diff_json)| {
                let diff: DiffEntry =
                    serde_json::from_value(diff_json).context("parsing audit diff")?;
                Ok(AuditLogEntry {
                    actor,
                    created_at,
                    diff,
                })
            })
            .collect()
    }

    /// P2.2 (FR-CEM-16/17) — `level` is stored as a plain string here (like `Commit.message`),
    /// not a Rust enum: `apps/api/src/autonomy.rs` owns the `Level` type and its parsing/decision
    /// logic, this store layer just persists whatever string it's given.
    pub async fn get_autonomy_config(
        &self,
        project_id: &str,
        scope: &str,
    ) -> Result<Option<AutonomyConfig>> {
        let row: Option<(String, Option<f64>)> = sqlx::query_as(
            "SELECT level, mass_deviation_threshold_percent FROM autonomy_config \
             WHERE project_id = $1 AND scope = $2",
        )
        .bind(project_id)
        .bind(scope)
        .fetch_optional(&self.pool)
        .await
        .context("fetching autonomy config")?;
        Ok(
            row.map(|(level, mass_deviation_threshold_percent)| AutonomyConfig {
                scope: scope.to_string(),
                level,
                mass_deviation_threshold_percent,
            }),
        )
    }

    pub async fn set_autonomy_config(
        &self,
        project_id: &str,
        scope: &str,
        level: &str,
        mass_deviation_threshold_percent: Option<f64>,
        updated_by: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO autonomy_config \
                (project_id, scope, level, mass_deviation_threshold_percent, updated_by) \
             VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT (project_id, scope) DO UPDATE SET \
                level = EXCLUDED.level, \
                mass_deviation_threshold_percent = EXCLUDED.mass_deviation_threshold_percent, \
                updated_by = EXCLUDED.updated_by, \
                updated_at = now()",
        )
        .bind(project_id)
        .bind(scope)
        .bind(level)
        .bind(mass_deviation_threshold_percent)
        .bind(updated_by)
        .execute(&self.pool)
        .await
        .context("upserting autonomy config")?;
        Ok(())
    }

    /// P2.2 (T-P2.2-01) — one row per subsystem; `reason` is the honest "why does this need
    /// review" string a UI would surface (see `apps/api/src/autonomy.rs::Decision`). `origin`
    /// (docs/IMPLEMENTATION_KICKOFF.md Phase 1) distinguishes `cem-generated` (the only real
    /// caller today, `mode_b.rs::propose`) from the still-unbuilt `human-authored`/
    /// `document-import` origins the merged spec already names (FR-PM-05/FR-CORE-16).
    #[allow(clippy::too_many_arguments)]
    pub async fn create_proposal(
        &self,
        project_id: &str,
        branch_id: &str,
        subsystem_id: &str,
        candidate: &serde_json::Value,
        top_level_requirement_ids: &[String],
        reason: &str,
        origin: &str,
    ) -> Result<Proposal> {
        let id = uuid::Uuid::new_v4().to_string();
        let requirement_ids_json = serde_json::to_value(top_level_requirement_ids)
            .context("serializing requirement ids")?;
        sqlx::query(
            "INSERT INTO proposals \
                (id, project_id, branch_id, subsystem_id, status, candidate, \
                 top_level_requirement_ids, reason, origin) \
             VALUES ($1, $2, $3, $4, 'pending', $5, $6, $7, $8)",
        )
        .bind(&id)
        .bind(project_id)
        .bind(branch_id)
        .bind(subsystem_id)
        .bind(candidate)
        .bind(&requirement_ids_json)
        .bind(reason)
        .bind(origin)
        .execute(&self.pool)
        .await
        .context("inserting proposal")?;
        Ok(Proposal {
            id,
            project_id: project_id.to_string(),
            branch_id: branch_id.to_string(),
            subsystem_id: subsystem_id.to_string(),
            status: "pending".to_string(),
            candidate: candidate.clone(),
            top_level_requirement_ids: top_level_requirement_ids.to_vec(),
            reason: reason.to_string(),
            origin: origin.to_string(),
        })
    }

    pub async fn list_proposals(&self, project_id: &str, branch_id: &str) -> Result<Vec<Proposal>> {
        type Row = (
            String,
            String,
            String,
            serde_json::Value,
            serde_json::Value,
            String,
            String,
        );
        let rows: Vec<Row> = sqlx::query_as(
            "SELECT id, subsystem_id, status, candidate, top_level_requirement_ids, reason, origin \
             FROM proposals WHERE project_id = $1 AND branch_id = $2 ORDER BY created_at",
        )
        .bind(project_id)
        .bind(branch_id)
        .fetch_all(&self.pool)
        .await
        .context("listing proposals")?;
        rows.into_iter()
            .map(
                |(id, subsystem_id, status, candidate, requirement_ids_json, reason, origin)| {
                    let top_level_requirement_ids: Vec<String> =
                        serde_json::from_value(requirement_ids_json)
                            .context("parsing proposal requirement ids")?;
                    Ok(Proposal {
                        id,
                        project_id: project_id.to_string(),
                        branch_id: branch_id.to_string(),
                        subsystem_id,
                        status,
                        candidate,
                        top_level_requirement_ids,
                        reason,
                        origin,
                    })
                },
            )
            .collect()
    }

    pub async fn get_proposal(
        &self,
        project_id: &str,
        proposal_id: &str,
    ) -> Result<Option<Proposal>> {
        type Row = (
            String,
            String,
            String,
            serde_json::Value,
            serde_json::Value,
            String,
            String,
        );
        let row: Option<Row> = sqlx::query_as(
            "SELECT branch_id, subsystem_id, status, candidate, top_level_requirement_ids, reason, origin \
             FROM proposals WHERE project_id = $1 AND id = $2",
        )
        .bind(project_id)
        .bind(proposal_id)
        .fetch_optional(&self.pool)
        .await
        .context("fetching proposal")?;
        row.map(
            |(branch_id, subsystem_id, status, candidate, requirement_ids_json, reason, origin)| {
                let top_level_requirement_ids: Vec<String> =
                    serde_json::from_value(requirement_ids_json)
                        .context("parsing proposal requirement ids")?;
                Ok(Proposal {
                    id: proposal_id.to_string(),
                    project_id: project_id.to_string(),
                    branch_id,
                    subsystem_id,
                    status,
                    candidate,
                    top_level_requirement_ids,
                    reason,
                    origin,
                })
            },
        )
        .transpose()
    }

    pub async fn set_proposal_status(
        &self,
        project_id: &str,
        proposal_id: &str,
        status: &str,
    ) -> Result<()> {
        sqlx::query("UPDATE proposals SET status = $1 WHERE project_id = $2 AND id = $3")
            .bind(status)
            .bind(project_id)
            .bind(proposal_id)
            .execute(&self.pool)
            .await
            .context("updating proposal status")?;
        Ok(())
    }

    /// Tier 1 pass (item 6) — persists a Mode B design-space definition, keyed by the sidecar's
    /// own `handle_id`. Called once, right after a successful `DefineDesignSpace` (`archspace.rs`
    /// `define`) and again whenever `resolve_or_redefine` recovers a stale handle (a fresh row for
    /// the fresh handle, the stale row left in place — harmless, unreachable dead history rather
    /// than something worth a delete path for).
    pub async fn persist_archspace_definition(
        &self,
        handle_id: &str,
        project_id: &str,
        subsystem_id: &str,
        definition: &cem_core::archspace::DesignSpaceDefinitionInput,
    ) -> Result<()> {
        let definition_json =
            serde_json::to_value(definition).context("serializing design-space definition")?;
        sqlx::query(
            "INSERT INTO archspace_design_spaces (handle_id, project_id, subsystem_id, definition) \
             VALUES ($1, $2, $3, $4) \
             ON CONFLICT (handle_id) DO UPDATE SET \
                project_id = EXCLUDED.project_id, subsystem_id = EXCLUDED.subsystem_id, \
                definition = EXCLUDED.definition",
        )
        .bind(handle_id)
        .bind(project_id)
        .bind(subsystem_id)
        .bind(&definition_json)
        .execute(&self.pool)
        .await
        .context("persisting archspace design-space definition")?;
        Ok(())
    }

    /// The read half of [`Self::persist_archspace_definition`] — `None` if this `handle_id` was
    /// never persisted (e.g. a handle from before this pass, or a genuinely unknown id), not an
    /// error; the caller (`resolve_or_redefine`) treats that as "nothing to recover from."
    pub async fn get_archspace_definition(
        &self,
        handle_id: &str,
    ) -> Result<
        Option<(
            String,
            String,
            cem_core::archspace::DesignSpaceDefinitionInput,
        )>,
    > {
        let row: Option<(String, String, serde_json::Value)> = sqlx::query_as(
            "SELECT project_id, subsystem_id, definition FROM archspace_design_spaces \
             WHERE handle_id = $1",
        )
        .bind(handle_id)
        .fetch_optional(&self.pool)
        .await
        .context("fetching archspace design-space definition")?;
        let Some((project_id, subsystem_id, definition_json)) = row else {
            return Ok(None);
        };
        let definition: cem_core::archspace::DesignSpaceDefinitionInput =
            serde_json::from_value(definition_json)
                .context("deserializing design-space definition")?;
        Ok(Some((project_id, subsystem_id, definition)))
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutonomyConfig {
    pub scope: String,
    pub level: String,
    pub mass_deviation_threshold_percent: Option<f64>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Proposal {
    pub id: String,
    pub project_id: String,
    pub branch_id: String,
    pub subsystem_id: String,
    pub status: String,
    pub candidate: serde_json::Value,
    pub top_level_requirement_ids: Vec<String>,
    pub reason: String,
    /// docs/IMPLEMENTATION_KICKOFF.md Phase 1 — `cem-generated` today (the only real caller);
    /// `human-authored`/`document-import` are real spec'd values (FR-PM-05/FR-CORE-16) neither of
    /// which has a caller yet.
    pub origin: String,
}

/// See `list_audit_log`'s own doc comment for why `#[allow(dead_code)]` is warranted here too.
#[allow(dead_code)]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditLogEntry {
    pub actor: String,
    pub created_at: String,
    pub diff: DiffEntry,
}

/// Compares `old` against `new`: property changes (bodies present in both), renames/active/
/// origin changes (elements present in both), created elements, and created/deleted edges. `pub`
/// since `apps/api/src/main.rs`'s `diff_commit` handler calls this directly on two snapshots it
/// resolved itself (`resolve_snapshot`) — this module no longer has enough context (no live
/// Neo4j/Postgres access) to resolve snapshots itself, only to diff two already-resolved ones.
pub fn compute_snapshot_diff(old: &Snapshot, new: &Snapshot) -> Vec<DiffEntry> {
    let mut out = Vec::new();

    let old_by_id: HashMap<&str, &Element> =
        old.elements.iter().map(|e| (e.id.as_str(), e)).collect();
    for element in &new.elements {
        match old_by_id.get(element.id.as_str()) {
            Some(prev) => {
                if prev.name != element.name {
                    out.push(DiffEntry::ElementRenamed {
                        element_id: element.id.clone(),
                        old_name: prev.name.clone(),
                        new_name: element.name.clone(),
                    });
                }
                if prev.active != element.active {
                    out.push(DiffEntry::ElementActiveChanged {
                        element_id: element.id.clone(),
                        active: element.active,
                    });
                }
                if prev.origin != element.origin {
                    out.push(DiffEntry::ElementOriginChanged {
                        element_id: element.id.clone(),
                        origin: element.origin,
                    });
                }
            }
            None => out.push(DiffEntry::ElementCreated {
                element_id: element.id.clone(),
                kind: element.kind,
                name: element.name.clone(),
            }),
        }
    }

    let new_by_id: HashMap<&str, &Element> =
        new.elements.iter().map(|e| (e.id.as_str(), e)).collect();
    for element in old
        .elements
        .iter()
        .filter(|e| !new_by_id.contains_key(e.id.as_str()))
    {
        out.push(DiffEntry::ElementDeleted {
            element_id: element.id.clone(),
            kind: element.kind,
            name: element.name.clone(),
        });
    }

    for (id, new_body) in &new.bodies {
        let Some(old_body) = old.bodies.get(id) else {
            continue;
        };
        // Bodies are stored as `{"rationale": ..., "properties": {...}}` (see
        // `PostgresStore::upsert_body`) — drill into `properties` rather than diffing the
        // wrapper's own two keys, or every changed property would misreport as a change to a
        // field literally named "properties"/"rationale".
        let new_properties = new_body.get("properties").and_then(|v| v.as_object());
        let old_properties = old_body.get("properties").and_then(|v| v.as_object());
        if let (Some(new_obj), Some(old_obj)) = (new_properties, old_properties) {
            for (key, new_val) in new_obj {
                let old_val = old_obj.get(key).cloned().unwrap_or(serde_json::Value::Null);
                if &old_val != new_val {
                    out.push(DiffEntry::PropertyChanged {
                        element_id: id.clone(),
                        property: key.clone(),
                        old: old_val,
                        new: new_val.clone(),
                    });
                }
            }
        }

        let new_rationale = new_body.get("rationale").and_then(|v| v.as_str());
        let old_rationale = old_body.get("rationale").and_then(|v| v.as_str());
        if new_rationale != old_rationale {
            out.push(DiffEntry::RationaleChanged {
                element_id: id.clone(),
                old: old_rationale.map(String::from),
                new: new_rationale.map(String::from),
            });
        }
    }

    let old_edges: HashSet<&SnapshotEdge> = old.edges.iter().collect();
    let new_edges: HashSet<&SnapshotEdge> = new.edges.iter().collect();
    for edge in new.edges.iter().filter(|e| !old_edges.contains(e)) {
        out.push(DiffEntry::EdgeCreated {
            source: edge.source.clone(),
            target: edge.target.clone(),
            kind: edge.kind,
        });
    }
    for edge in old.edges.iter().filter(|e| !new_edges.contains(e)) {
        out.push(DiffEntry::EdgeDeleted {
            source: edge.source.clone(),
            target: edge.target.clone(),
            kind: edge.kind,
        });
    }

    out
}

/// Applies one commit's diff onto a snapshot representing the state immediately *before* that
/// commit, mutating it into the state immediately *after* — the inverse of `compute_snapshot_diff`,
/// used by `apps/api/src/main.rs`'s `resolve_snapshot` to replay a commit chain instead of
/// storing/reading a full copy at every commit (see the module doc comment).
///
/// `GraphOp::Create`'s `temp_id` field is expected to already hold the server-assigned *real* id
/// by the time it reaches here, not the client's original temp id — `apps/api/src/main.rs`'s
/// `apply_text_model` handler remaps every op through `ApplyOpsOutcome::Applied`'s `id_map`
/// before constructing the `TextModelApplied` diff entry, specifically so replay has real,
/// resolvable ids to work with (the raw client ops are only ever meaningful within their own
/// single `apply_graph_ops` call).
pub fn apply_diff(snapshot: &mut Snapshot, diff: &DiffEntry) {
    match diff {
        DiffEntry::PropertyChanged {
            element_id,
            property,
            new,
            ..
        } => {
            let body = snapshot
                .bodies
                .entry(element_id.clone())
                .or_insert_with(|| serde_json::json!({"rationale": null, "properties": {}}));
            if body.get("properties").and_then(|v| v.as_object()).is_none() {
                if let Some(obj) = body.as_object_mut() {
                    obj.insert("properties".to_string(), serde_json::json!({}));
                }
            }
            if let Some(obj) = body.get_mut("properties").and_then(|v| v.as_object_mut()) {
                obj.insert(property.clone(), new.clone());
            }
        }
        DiffEntry::RationaleChanged {
            element_id, new, ..
        } => {
            let body = snapshot
                .bodies
                .entry(element_id.clone())
                .or_insert_with(|| serde_json::json!({"rationale": null, "properties": {}}));
            if let Some(obj) = body.as_object_mut() {
                obj.insert(
                    "rationale".to_string(),
                    new.clone()
                        .map(serde_json::Value::String)
                        .unwrap_or(serde_json::Value::Null),
                );
            }
        }
        DiffEntry::ElementRenamed {
            element_id,
            new_name,
            ..
        } => {
            if let Some(e) = snapshot.elements.iter_mut().find(|e| &e.id == element_id) {
                e.name = new_name.clone();
            }
        }
        DiffEntry::ElementCreated {
            element_id,
            kind,
            name,
        } => {
            if !snapshot.elements.iter().any(|e| &e.id == element_id) {
                snapshot.elements.push(Element {
                    id: element_id.clone(),
                    kind: *kind,
                    name: name.clone(),
                    active: true,
                    origin: Origin::Human,
                });
            }
        }
        DiffEntry::ElementDeleted { element_id, .. } => {
            snapshot.elements.retain(|e| &e.id != element_id);
            snapshot.bodies.remove(element_id);
            // The live delete only ever diffs `ElementDeleted` itself, not the cascade-deleted
            // edges Neo4j drops alongside it (a pre-existing gap, not introduced here) — replay
            // strips them explicitly instead, so a reconstructed snapshot never carries a
            // dangling edge a real delete would have removed.
            snapshot
                .edges
                .retain(|e| &e.source != element_id && &e.target != element_id);
        }
        DiffEntry::ElementActiveChanged { element_id, active } => {
            if let Some(e) = snapshot.elements.iter_mut().find(|e| &e.id == element_id) {
                e.active = *active;
            }
        }
        DiffEntry::ElementOriginChanged { element_id, origin } => {
            if let Some(e) = snapshot.elements.iter_mut().find(|e| &e.id == element_id) {
                e.origin = *origin;
            }
        }
        DiffEntry::EdgeCreated {
            source,
            target,
            kind,
        } => {
            let edge = SnapshotEdge {
                source: source.clone(),
                target: target.clone(),
                kind: *kind,
            };
            if !snapshot.edges.contains(&edge) {
                snapshot.edges.push(edge);
            }
        }
        DiffEntry::EdgeDeleted {
            source,
            target,
            kind,
        } => {
            snapshot
                .edges
                .retain(|e| !(&e.source == source && &e.target == target && &e.kind == kind));
        }
        DiffEntry::TextModelApplied { ops } => {
            for op in ops {
                apply_graph_op(snapshot, op);
            }
        }
        // Not a graph mutation — project-level config, nothing in a Snapshot to update. See the
        // variant's own doc comment.
        DiffEntry::AutonomyLevelChanged { .. } => {}
    }
}

fn apply_graph_op(snapshot: &mut Snapshot, op: &GraphOp) {
    match op {
        GraphOp::Rename { id, name } => {
            if let Some(e) = snapshot.elements.iter_mut().find(|e| &e.id == id) {
                e.name = name.clone();
            }
        }
        GraphOp::Create {
            temp_id: real_id,
            kind,
            name,
            parent_id,
        } => {
            if !snapshot.elements.iter().any(|e| &e.id == real_id) {
                snapshot.elements.push(Element {
                    id: real_id.clone(),
                    kind: *kind,
                    name: name.clone(),
                    active: true,
                    origin: Origin::Human,
                });
            }
            if let Some(parent) = parent_id {
                let edge = SnapshotEdge {
                    source: parent.clone(),
                    target: real_id.clone(),
                    kind: EdgeKind::Contains,
                };
                if !snapshot.edges.contains(&edge) {
                    snapshot.edges.push(edge);
                }
            }
        }
        GraphOp::Reparent { id, new_parent_id } => {
            snapshot
                .edges
                .retain(|e| !(&e.target == id && e.kind == EdgeKind::Contains));
            if let Some(parent) = new_parent_id {
                snapshot.edges.push(SnapshotEdge {
                    source: parent.clone(),
                    target: id.clone(),
                    kind: EdgeKind::Contains,
                });
            }
        }
    }
}
