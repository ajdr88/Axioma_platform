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
}
