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
}

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
    /// invariant this borrows the vocabulary from.
    pub async fn create_project(&self, name: &str) -> Result<Project> {
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO projects (id, name) VALUES ($1, $2)")
            .bind(&id)
            .bind(name)
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
        })
    }

    pub async fn list_projects(&self) -> Result<Vec<Project>> {
        let rows: Vec<(String, String)> =
            sqlx::query_as("SELECT id, name FROM projects ORDER BY created_at")
                .fetch_all(&self.pool)
                .await
                .context("listing projects")?;
        Ok(rows
            .into_iter()
            .map(|(id, name)| Project { id, name })
            .collect())
    }

    pub async fn get_project(&self, id: &str) -> Result<Option<Project>> {
        let row: Option<(String, String)> =
            sqlx::query_as("SELECT id, name FROM projects WHERE id = $1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .context("fetching project")?;
        Ok(row.map(|(id, name)| Project { id, name }))
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
