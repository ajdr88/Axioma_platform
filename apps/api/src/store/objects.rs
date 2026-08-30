//! Blob store (ADR-003 / NFR-DATA-02): geometry, meshes, solver result files. Only a pointer
//! (`s3://bucket/key`) ever crosses into the graph or a body — raw bytes never do. Configured
//! against MinIO in dev via a custom endpoint + path-style addressing; same client works against
//! real S3 in other environments.

use anyhow::{Context, Result};
use aws_sdk_s3::{
    config::{Credentials, Region},
    error::SdkError,
    primitives::ByteStream,
    Client,
};

#[derive(Clone)]
pub struct ObjectStore {
    client: Client,
    bucket: String,
}

impl ObjectStore {
    pub async fn connect(
        endpoint: &str,
        access_key: &str,
        secret_key: &str,
        bucket: &str,
    ) -> Result<Self> {
        let credentials = Credentials::new(access_key, secret_key, None, None, "axioma-static");
        let config = aws_sdk_s3::Config::builder()
            .behavior_version_latest()
            .endpoint_url(endpoint)
            .region(Region::new("us-east-1"))
            .credentials_provider(credentials)
            .force_path_style(true)
            .build();
        let client = Client::from_conf(config);

        let store = Self {
            client,
            bucket: bucket.to_string(),
        };
        store.ensure_bucket().await?;
        Ok(store)
    }

    async fn ensure_bucket(&self) -> Result<()> {
        match self.client.head_bucket().bucket(&self.bucket).send().await {
            Ok(_) => Ok(()),
            Err(SdkError::ServiceError(_)) => {
                self.client
                    .create_bucket()
                    .bucket(&self.bucket)
                    .send()
                    .await
                    .with_context(|| format!("creating bucket {}", self.bucket))?;
                Ok(())
            }
            Err(err) => Err(err).with_context(|| format!("checking bucket {}", self.bucket)),
        }
    }

    pub async fn ping(&self) -> Result<()> {
        self.client
            .head_bucket()
            .bucket(&self.bucket)
            .send()
            .await
            .context("object store ping failed")?;
        Ok(())
    }

    /// Puts a blob and returns its pointer (`s3://bucket/key`) — never the bytes. Named to match
    /// `get_object` below (renamed from `put_placeholder` during Phase 5's FR-EXPORT-04 work,
    /// which needed the read half added — the "placeholder" name was accurate for its one
    /// original caller, `seed_turbofan_ref`'s fake casing blob, but not for a real user-attached
    /// file's real bytes).
    pub async fn put_object(&self, key: &str, bytes: Vec<u8>) -> Result<String> {
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .body(ByteStream::from(bytes))
            .send()
            .await
            .with_context(|| format!("putting object {key}"))?;
        Ok(format!("s3://{}/{key}", self.bucket))
    }

    /// docs/IMPLEMENTATION_KICKOFF.md Phase 5 (FR-EXPORT-04) — the missing read half of the
    /// pointer pattern. Nothing before this pass ever needed to read a blob back out.
    pub async fn get_object(&self, key: &str) -> Result<Vec<u8>> {
        let output = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .with_context(|| format!("getting object {key}"))?;
        let bytes = output
            .body
            .collect()
            .await
            .with_context(|| format!("reading object body {key}"))?
            .into_bytes();
        Ok(bytes.to_vec())
    }
}
