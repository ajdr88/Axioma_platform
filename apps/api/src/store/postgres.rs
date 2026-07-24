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

        Ok(Self { pool })
    }

    pub async fn ping(&self) -> Result<()> {
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .context("Postgres ping failed")?;
        Ok(())
    }

    pub async fn upsert_body(&self, body: &ElementBody) -> Result<()> {
        let payload = serde_json::json!({
            "rationale": body.rationale,
            "properties": body.properties,
        });
        sqlx::query(
            "INSERT INTO element_bodies (element_id, body) VALUES ($1, $2) \
             ON CONFLICT (element_id) DO UPDATE SET body = EXCLUDED.body",
        )
        .bind(&body.element_id)
        .bind(payload)
        .execute(&self.pool)
        .await
        .with_context(|| format!("upserting body for {}", body.element_id))?;
        Ok(())
    }

    pub async fn get_body(&self, element_id: &str) -> Result<Option<serde_json::Value>> {
        let row: Option<(serde_json::Value,)> =
            sqlx::query_as("SELECT body FROM element_bodies WHERE element_id = $1")
                .bind(element_id)
                .fetch_optional(&self.pool)
                .await
                .with_context(|| format!("fetching body for {element_id}"))?;
        Ok(row.map(|(body,)| body))
    }
}
