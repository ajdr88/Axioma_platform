//! Document store (ADR-003 / NFR-DATA-02): element bodies, long text, and large metadata that
//! must never land in the (deliberately lean) topology store. See `super::neo4j`.

use anyhow::{Context, Result};
use sqlx::{postgres::PgPoolOptions, PgPool};
use sysml_core::ElementBody;

#[derive(Clone)]
pub struct PostgresStore {
    pool: PgPool,
}

impl PostgresStore {
    pub async fn connect(database_url: &str) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await
            .context("connecting to Postgres")?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS element_bodies (\
                element_id TEXT PRIMARY KEY, \
                body JSONB NOT NULL\
            )",
        )
        .execute(&pool)
        .await
        .context("creating element_bodies table")?;

        // Canvas position (added after the table itself — ALTER ... IF NOT EXISTS so an
        // environment that already has the table from before this feature upgrades in place,
        // no manual migration needed. No migration framework exists yet (NFR-REL-05 is separate,
        // deferred work); this is the ad hoc equivalent for one small addition.
        sqlx::query(
            "ALTER TABLE element_bodies ADD COLUMN IF NOT EXISTS canvas_x DOUBLE PRECISION",
        )
        .execute(&pool)
        .await
        .context("adding canvas_x column")?;
        sqlx::query(
            "ALTER TABLE element_bodies ADD COLUMN IF NOT EXISTS canvas_y DOUBLE PRECISION",
        )
        .execute(&pool)
        .await
        .context("adding canvas_y column")?;

        // Multi-project support (roadmap versioning work) — same ad hoc `ALTER ... IF NOT
        // EXISTS` upgrade path as canvas_x/canvas_y above. `element_id` stays the primary key
        // (ids are either fixture-seeded-once-per-project or freshly minted UUIDs, so a
        // cross-project collision is not a real risk in practice) — `project_id` is an indexed
        // filter column, not part of a composite key.
        sqlx::query("ALTER TABLE element_bodies ADD COLUMN IF NOT EXISTS project_id TEXT")
            .execute(&pool)
            .await
            .context("adding project_id column")?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS element_bodies_project_id_idx \
             ON element_bodies (project_id)",
        )
        .execute(&pool)
        .await
        .context("indexing project_id column")?;

        // docs/IMPLEMENTATION_KICKOFF.md Phase 5 (FR-CORE-10/11) — a Dynamic Query's definition
        // (root/depth/maxFanout/direction, the exact shape `traceability::run_traversal` already
        // takes). `definition` is JSONB, same "structured body, no schema-per-field" convention as
        // `element_bodies.body` -- avoids a column per query parameter for a shape likely to grow.
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS dynamic_collections (\
                id TEXT PRIMARY KEY, \
                project_id TEXT NOT NULL, \
                name TEXT NOT NULL, \
                definition JSONB NOT NULL\
            )",
        )
        .execute(&pool)
        .await
        .context("creating dynamic_collections table")?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS dynamic_collections_project_id_idx \
             ON dynamic_collections (project_id)",
        )
        .execute(&pool)
        .await
        .context("indexing dynamic_collections project_id column")?;

        // docs/IMPLEMENTATION_KICKOFF.md Phase 5 (FR-EXPORT-04) — attachment metadata; the bytes
        // themselves live in the object store (`object_key`), never here (NFR-DATA-02).
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS attachments (\
                id TEXT PRIMARY KEY, \
                project_id TEXT NOT NULL, \
                element_id TEXT NOT NULL, \
                file_name TEXT NOT NULL, \
                content_type TEXT NOT NULL, \
                object_key TEXT NOT NULL, \
                size_bytes BIGINT NOT NULL\
            )",
        )
        .execute(&pool)
        .await
        .context("creating attachments table")?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS attachments_element_id_idx \
             ON attachments (project_id, element_id)",
        )
        .execute(&pool)
        .await
        .context("indexing attachments element_id column")?;

        // docs/IMPLEMENTATION_KICKOFF.md Phase 5 (FR-CORE-14..18) — status row for the async
        // documents-to-draft-model pipeline. `status` is a plain TEXT enum stored as its exact
        // spec string (Extracting/Segmenting/Drafting/Validating/AwaitingReview/Failed), same
        // "enum-as-string column" convention already used for autonomy level (L0..L4).
        // `candidates`/`suggestions` are JSONB, same "structured body, no schema-per-field"
        // convention as every other body/definition column in this store.
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS document_import_jobs (\
                id TEXT PRIMARY KEY, \
                project_id TEXT NOT NULL, \
                file_name TEXT NOT NULL, \
                status TEXT NOT NULL, \
                candidates JSONB, \
                suggestions JSONB, \
                error TEXT\
            )",
        )
        .execute(&pool)
        .await
        .context("creating document_import_jobs table")?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS document_import_jobs_project_id_idx \
             ON document_import_jobs (project_id)",
        )
        .execute(&pool)
        .await
        .context("indexing document_import_jobs project_id column")?;

        Ok(Self { pool })
    }

    pub async fn ping(&self) -> Result<()> {
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .context("Postgres ping failed")?;
        Ok(())
    }

    pub async fn upsert_body(&self, project_id: &str, body: &ElementBody) -> Result<()> {
        let payload = serde_json::json!({
            "rationale": body.rationale,
            "properties": body.properties,
        });
        sqlx::query(
            "INSERT INTO element_bodies (element_id, project_id, body) VALUES ($1, $2, $3) \
             ON CONFLICT (element_id) DO UPDATE SET project_id = EXCLUDED.project_id, \
             body = EXCLUDED.body",
        )
        .bind(&body.element_id)
        .bind(project_id)
        .bind(payload)
        .execute(&self.pool)
        .await
        .with_context(|| format!("upserting body for {}", body.element_id))?;
        Ok(())
    }

    pub async fn get_body(
        &self,
        project_id: &str,
        element_id: &str,
    ) -> Result<Option<serde_json::Value>> {
        let row: Option<(serde_json::Value,)> = sqlx::query_as(
            "SELECT body FROM element_bodies WHERE element_id = $1 AND project_id = $2",
        )
        .bind(element_id)
        .bind(project_id)
        .fetch_optional(&self.pool)
        .await
        .with_context(|| format!("fetching body for {element_id}"))?;
        Ok(row.map(|(body,)| body))
    }

    /// Every element body in a project, keyed by element id — used to build a versioning
    /// snapshot (roadmap: Git-backed model versioning).
    pub async fn list_bodies(
        &self,
        project_id: &str,
    ) -> Result<std::collections::HashMap<String, serde_json::Value>> {
        let rows: Vec<(String, serde_json::Value)> =
            sqlx::query_as("SELECT element_id, body FROM element_bodies WHERE project_id = $1")
                .bind(project_id)
                .fetch_all(&self.pool)
                .await
                .context("listing bodies")?;
        Ok(rows.into_iter().collect())
    }

    /// Sets just the canvas position (drag persistence) — never touches `body`, so a drag can't
    /// race with a properties/rationale edit landing at the same time. Deliberately excluded
    /// from versioning history (UI metadata, not modeling content — NFR-DATA-01).
    pub async fn upsert_position(
        &self,
        project_id: &str,
        element_id: &str,
        x: f64,
        y: f64,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO element_bodies (element_id, project_id, body, canvas_x, canvas_y) \
             VALUES ($1, $2, '{}'::jsonb, $3, $4) \
             ON CONFLICT (element_id) DO UPDATE SET project_id = EXCLUDED.project_id, \
             canvas_x = EXCLUDED.canvas_x, canvas_y = EXCLUDED.canvas_y",
        )
        .bind(element_id)
        .bind(project_id)
        .bind(x)
        .bind(y)
        .execute(&self.pool)
        .await
        .with_context(|| format!("upserting position for {element_id}"))?;
        Ok(())
    }

    pub async fn list_positions(&self, project_id: &str) -> Result<Vec<(String, f64, f64)>> {
        let rows: Vec<(String, f64, f64)> = sqlx::query_as(
            "SELECT element_id, canvas_x, canvas_y FROM element_bodies \
             WHERE project_id = $1 AND canvas_x IS NOT NULL AND canvas_y IS NOT NULL",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await
        .context("listing positions")?;
        Ok(rows)
    }

    /// Removes an element's body+position row (P1.3 element delete, T-P1.3-03) — a no-op if the
    /// element never had one (e.g. a freshly-created node with no properties/rationale/position
    /// set yet). Body and position share one row in this table, so one delete clears both.
    pub async fn delete_body_and_position(&self, project_id: &str, element_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM element_bodies WHERE element_id = $1 AND project_id = $2")
            .bind(element_id)
            .bind(project_id)
            .execute(&self.pool)
            .await
            .with_context(|| format!("deleting body/position for {element_id}"))?;
        Ok(())
    }

    /// docs/IMPLEMENTATION_KICKOFF.md Phase 5 (FR-CORE-10) — stores a Dynamic Query's definition
    /// (not its result, which is only ever computed on demand — see `get_dynamic_collection`'s
    /// caller, `collections::freeze_collection`). `id` is server-minted, matching every other
    /// element/entity creation convention in this codebase.
    pub async fn save_dynamic_collection(
        &self,
        project_id: &str,
        id: &str,
        name: &str,
        definition: &serde_json::Value,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO dynamic_collections (id, project_id, name, definition) \
             VALUES ($1, $2, $3, $4)",
        )
        .bind(id)
        .bind(project_id)
        .bind(name)
        .bind(definition)
        .execute(&self.pool)
        .await
        .with_context(|| format!("saving dynamic collection {id}"))?;
        Ok(())
    }

    pub async fn get_dynamic_collection(
        &self,
        project_id: &str,
        id: &str,
    ) -> Result<Option<(String, serde_json::Value)>> {
        let row: Option<(String, serde_json::Value)> = sqlx::query_as(
            "SELECT name, definition FROM dynamic_collections WHERE id = $1 AND project_id = $2",
        )
        .bind(id)
        .bind(project_id)
        .fetch_optional(&self.pool)
        .await
        .with_context(|| format!("fetching dynamic collection {id}"))?;
        Ok(row)
    }

    /// docs/IMPLEMENTATION_KICKOFF.md Phase 5 (FR-EXPORT-04) — attachment metadata row. `id` is
    /// server-minted, matching every other entity-creation convention in this codebase.
    #[allow(clippy::too_many_arguments)]
    pub async fn save_attachment(
        &self,
        project_id: &str,
        id: &str,
        element_id: &str,
        file_name: &str,
        content_type: &str,
        object_key: &str,
        size_bytes: i64,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO attachments \
             (id, project_id, element_id, file_name, content_type, object_key, size_bytes) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(id)
        .bind(project_id)
        .bind(element_id)
        .bind(file_name)
        .bind(content_type)
        .bind(object_key)
        .bind(size_bytes)
        .execute(&self.pool)
        .await
        .with_context(|| format!("saving attachment {id}"))?;
        Ok(())
    }

    /// Tuple-typed, not a derived struct — matches every other query in this file (no
    /// `#[derive(sqlx::FromRow)]` anywhere in this codebase yet, and the `sqlx` dependency
    /// doesn't enable the `macros` feature that derive needs; a plain tuple avoids adding one).
    pub async fn list_attachments(
        &self,
        project_id: &str,
        element_id: &str,
    ) -> Result<Vec<AttachmentMeta>> {
        let rows: Vec<(String, String, String, i64)> = sqlx::query_as(
            "SELECT id, file_name, content_type, size_bytes FROM attachments \
             WHERE project_id = $1 AND element_id = $2",
        )
        .bind(project_id)
        .bind(element_id)
        .fetch_all(&self.pool)
        .await
        .with_context(|| format!("listing attachments for {element_id}"))?;
        Ok(rows
            .into_iter()
            .map(|(id, file_name, content_type, size_bytes)| AttachmentMeta {
                id,
                file_name,
                content_type,
                size_bytes,
            })
            .collect())
    }

    /// Enough to actually stream the bytes back — `object_key` is deliberately never part of
    /// `AttachmentMeta` (an internal object-store detail no client needs to see).
    pub async fn get_attachment(
        &self,
        project_id: &str,
        id: &str,
    ) -> Result<Option<AttachmentRecord>> {
        let row: Option<(String, String, String)> = sqlx::query_as(
            "SELECT file_name, content_type, object_key FROM attachments \
             WHERE id = $1 AND project_id = $2",
        )
        .bind(id)
        .bind(project_id)
        .fetch_optional(&self.pool)
        .await
        .with_context(|| format!("fetching attachment {id}"))?;
        Ok(
            row.map(|(file_name, content_type, object_key)| AttachmentRecord {
                file_name,
                content_type,
                object_key,
            }),
        )
    }

    /// docs/IMPLEMENTATION_KICKOFF.md Phase 5 (FR-CORE-14..18) — creates the job row in its
    /// initial `Extracting` status. `id` is server-minted, matching every other entity-creation
    /// convention in this codebase.
    pub async fn create_import_job(
        &self,
        project_id: &str,
        id: &str,
        file_name: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO document_import_jobs (id, project_id, file_name, status) \
             VALUES ($1, $2, $3, 'Extracting')",
        )
        .bind(id)
        .bind(project_id)
        .bind(file_name)
        .execute(&self.pool)
        .await
        .with_context(|| format!("creating import job {id}"))?;
        Ok(())
    }

    /// Advances (or fails) a job's status — `error` is only ever set alongside `status = Failed`,
    /// left `NULL` otherwise (a prior failure reason must not linger once retried, though this
    /// pass has no retry path yet).
    pub async fn update_import_job_status(
        &self,
        project_id: &str,
        id: &str,
        status: &str,
        error: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE document_import_jobs SET status = $1, error = $2 \
             WHERE id = $3 AND project_id = $4",
        )
        .bind(status)
        .bind(error)
        .bind(id)
        .bind(project_id)
        .execute(&self.pool)
        .await
        .with_context(|| format!("updating import job {id} status to {status}"))?;
        Ok(())
    }

    pub async fn set_import_job_candidates(
        &self,
        project_id: &str,
        id: &str,
        candidates: &serde_json::Value,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE document_import_jobs SET candidates = $1 WHERE id = $2 AND project_id = $3",
        )
        .bind(candidates)
        .bind(id)
        .bind(project_id)
        .execute(&self.pool)
        .await
        .with_context(|| format!("setting candidates for import job {id}"))?;
        Ok(())
    }

    pub async fn set_import_job_suggestions(
        &self,
        project_id: &str,
        id: &str,
        suggestions: &serde_json::Value,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE document_import_jobs SET suggestions = $1 WHERE id = $2 AND project_id = $3",
        )
        .bind(suggestions)
        .bind(id)
        .bind(project_id)
        .execute(&self.pool)
        .await
        .with_context(|| format!("setting suggestions for import job {id}"))?;
        Ok(())
    }

    pub async fn get_import_job(
        &self,
        project_id: &str,
        id: &str,
    ) -> Result<Option<ImportJobRecord>> {
        type Row = (
            String,
            String,
            String,
            Option<serde_json::Value>,
            Option<serde_json::Value>,
            Option<String>,
        );
        let row: Option<Row> = sqlx::query_as(
            "SELECT id, file_name, status, candidates, suggestions, error \
             FROM document_import_jobs WHERE id = $1 AND project_id = $2",
        )
        .bind(id)
        .bind(project_id)
        .fetch_optional(&self.pool)
        .await
        .with_context(|| format!("fetching import job {id}"))?;
        Ok(row.map(
            |(id, file_name, status, candidates, suggestions, error)| ImportJobRecord {
                id,
                file_name,
                status,
                candidates,
                suggestions,
                error,
            },
        ))
    }
}

/// Attachment metadata as returned to a client listing an element's attachments — never includes
/// `object_key` (an internal object-store detail, not something a caller needs to see or use
/// directly; downloading goes through `get_attachment` + `ObjectStore::get_object` instead).
#[derive(Debug, Clone, serde::Serialize)]
pub struct AttachmentMeta {
    pub id: String,
    #[serde(rename = "fileName")]
    pub file_name: String,
    #[serde(rename = "contentType")]
    pub content_type: String,
    #[serde(rename = "sizeBytes")]
    pub size_bytes: i64,
}

/// Enough to actually stream the bytes back — used only by the download handler, never returned
/// to a client as JSON (that's `AttachmentMeta`'s job).
#[derive(Debug, Clone)]
pub struct AttachmentRecord {
    pub file_name: String,
    pub content_type: String,
    pub object_key: String,
}

/// A document-import job's full stored state — `document_import.rs` reads/writes this directly;
/// never returned to a client as-is (each of the four `GET`/status endpoints surfaces only the
/// slice it needs).
#[derive(Debug, Clone)]
pub struct ImportJobRecord {
    /// Mirrors the row's own primary key for completeness — no current caller needs it (the id is
    /// already known from the request path that fetched this record), kept rather than dropped
    /// from the `SELECT` since a future caller (e.g. a "list jobs" endpoint) naturally would.
    #[allow(dead_code)]
    pub id: String,
    pub file_name: String,
    pub status: String,
    pub candidates: Option<serde_json::Value>,
    pub suggestions: Option<serde_json::Value>,
    pub error: Option<String>,
}
