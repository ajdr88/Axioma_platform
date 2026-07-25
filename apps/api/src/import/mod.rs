//! `POST /import/reqif` and `POST /import/sysml-v2` (FR-CORE-07, impl §1.1) — the first-hour
//! "bring an existing model in" path. Both importers are deliberately restricted subsets of the
//! real OMG formats (see each module's doc comment for exactly what's supported) — full ReqIF
//! attribute-definition resolution and full SysML v2 API compliance are separately-scoped, large
//! efforts. The AI-assisted "documents → draft model" path (also part of FR-CORE-07) needs
//! `llm-gateway`, which doesn't exist yet, and isn't covered here.

pub mod reqif;
pub mod sysml_v2;

/// A client-caused import failure (malformed input) — downgrades to 400 in [`crate::ApiError`],
/// same as [`sysml_core::ValidationError`], rather than the default 500.
#[derive(Debug)]
pub struct BadRequest(pub String);

impl std::fmt::Display for BadRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for BadRequest {}
