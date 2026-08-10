//! NFR-COMP-03 (Auth Abstraction) — a provider-agnostic identity-resolution interface, so "local
//! dev, no real IdP" vs. "a real OIDC issuer" is a config choice (`AUTH_PROVIDER` env var in
//! `apps/api/src/main.rs`'s `main()`), not a business-logic edit. That's T-X-07's literal PASS
//! criterion: "swapping the identity provider... is a config change with no business-logic edit."
//!
//! **Deliberately not an authorization/enforcement layer.** Nothing here rejects a request for
//! lacking credentials, and no route becomes newly protected by this module's existence — every
//! endpoint stays exactly as reachable as it is today. `main.rs` has had an explicit, long-
//! standing "no auth system yet, single-user assumption is intentional" stance since before this
//! module existed; building real authorization (rejecting unauthenticated requests, a login flow,
//! per-route permissions) is a separate, much larger feature this module doesn't attempt.
//!
//! `OidcAuthProvider` does real, working Bearer-JWT signature/expiry validation (via
//! `jsonwebtoken`) — but it is **not** a full OIDC client: no issuer discovery, no JWKS
//! fetch/rotation, a single configured HMAC secret instead of a live IdP's rotating RSA keys.
//! Those are exactly the kind of thing a real deployment adds once actually pointed at a live
//! IdP ("architected now, activated later" — impl §3.4); the abstraction and the validation
//! mechanics are real today, automatic key management against a live IdP is deferred, not faked.

use std::fmt;

use axum::http::HeaderMap;

/// Resolves "who is making this request" for commit/audit attribution
/// (`apps/api/src/main.rs`'s `record_commit`, `store::versioning`'s `audit_log`) — the one thing
/// every existing mutating endpoint already needed an actor string for, previously always the
/// hardcoded `DEFAULT_ACTOR` constant.
pub trait AuthProvider: Send + Sync {
    fn resolve_actor(&self, headers: &HeaderMap) -> Result<String, AuthError>;
}

#[derive(Debug)]
pub struct AuthError(pub String);

impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "authentication error: {}", self.0)
    }
}

impl std::error::Error for AuthError {}

/// The default, and the only behavior that existed before this module: an `X-Actor` header
/// override if present (the same override concept `branch_update_element_body`'s own `actor`
/// request field already had — this just generalizes it to every endpoint), else
/// `crate::DEFAULT_ACTOR`. Byte-for-byte the same value every call site already resolved to, so
/// this being the default provider changes nothing about today's behavior.
pub struct LocalAuthProvider;

impl AuthProvider for LocalAuthProvider {
    fn resolve_actor(&self, headers: &HeaderMap) -> Result<String, AuthError> {
        Ok(headers
            .get("x-actor")
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(crate::DEFAULT_ACTOR)
            .to_string())
    }
}

/// See the module doc comment for exactly what is/isn't implemented. A missing `Authorization`
/// header resolves to `"anonymous"` rather than an error (this provider validates credentials
/// when presented, it doesn't require them — see the trait's own doc comment on why); a
/// *present but invalid* token (bad signature, expired, wrong/missing claim) is a real error,
/// since presenting garbage credentials is meaningfully different from presenting none.
pub struct OidcAuthProvider {
    decoding_key: jsonwebtoken::DecodingKey,
    actor_claim: String,
}

impl OidcAuthProvider {
    pub fn new(hmac_secret: &str, actor_claim: impl Into<String>) -> Self {
        Self {
            decoding_key: jsonwebtoken::DecodingKey::from_secret(hmac_secret.as_bytes()),
            actor_claim: actor_claim.into(),
        }
    }
}

impl AuthProvider for OidcAuthProvider {
    fn resolve_actor(&self, headers: &HeaderMap) -> Result<String, AuthError> {
        let Some(auth_header) = headers.get(axum::http::header::AUTHORIZATION) else {
            return Ok("anonymous".to_string());
        };
        let auth_str = auth_header
            .to_str()
            .map_err(|_| AuthError("Authorization header is not valid UTF-8".to_string()))?;
        let token = auth_str
            .strip_prefix("Bearer ")
            .ok_or_else(|| AuthError("Authorization header must be a Bearer token".to_string()))?;

        let validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::HS256);
        let claims: std::collections::HashMap<String, serde_json::Value> =
            jsonwebtoken::decode(token, &self.decoding_key, &validation)
                .map_err(|err| AuthError(format!("invalid token: {err}")))?
                .claims;

        claims
            .get(&self.actor_claim)
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .ok_or_else(|| {
                AuthError(format!(
                    "token missing required claim {:?}",
                    self.actor_claim
                ))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers_with(name: &str, value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
            value.parse().unwrap(),
        );
        headers
    }

    #[test]
    fn local_provider_defaults_to_default_actor() {
        let resolved = LocalAuthProvider.resolve_actor(&HeaderMap::new()).unwrap();
        assert_eq!(resolved, crate::DEFAULT_ACTOR);
    }

    #[test]
    fn local_provider_honors_x_actor_override() {
        let headers = headers_with("x-actor", "alice");
        let resolved = LocalAuthProvider.resolve_actor(&headers).unwrap();
        assert_eq!(resolved, "alice");
    }

    /// Proves the abstraction is real, not a no-op: swapping the provider changes the resolved
    /// identity for the exact same kind of request, purely via which provider is configured —
    /// no hand-constructed IdP network call needed to prove the validation logic itself works.
    #[test]
    fn oidc_provider_validates_and_extracts_the_actor_claim() {
        let secret = "test-secret";
        let provider = OidcAuthProvider::new(secret, "sub");

        let mut claims = std::collections::HashMap::new();
        claims.insert("sub".to_string(), serde_json::json!("bob"));
        claims.insert(
            "exp".to_string(),
            serde_json::json!(jsonwebtoken::get_current_timestamp() + 3600),
        );
        let token = jsonwebtoken::encode(
            &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256),
            &claims,
            &jsonwebtoken::EncodingKey::from_secret(secret.as_bytes()),
        )
        .unwrap();

        let headers = headers_with("authorization", &format!("Bearer {token}"));
        let resolved = provider.resolve_actor(&headers).unwrap();
        assert_eq!(resolved, "bob");
    }

    #[test]
    fn oidc_provider_rejects_a_tampered_token() {
        let provider = OidcAuthProvider::new("test-secret", "sub");
        let headers = headers_with("authorization", "Bearer not.a.valid.jwt");
        assert!(provider.resolve_actor(&headers).is_err());
    }

    #[test]
    fn oidc_provider_treats_no_header_as_anonymous_not_an_error() {
        let provider = OidcAuthProvider::new("test-secret", "sub");
        let resolved = provider.resolve_actor(&HeaderMap::new()).unwrap();
        assert_eq!(resolved, "anonymous");
    }
}
