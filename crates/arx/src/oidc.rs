//! OIDC JWT validation for GitHub Actions keyless push (ADR-0014).
//!
//! Validates a GitHub-issued OIDC JWT against the platform's JWKS (JSON Web Key
//! Set), with in-memory caching (key rotation is year-scale, so TTL is generous).
//! Falls through to the static-token path when the bearer token isn't a JWT.

use std::sync::RwLock;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use crate::config::OidcConfig;

/// GitHub OIDC issuer — the JWT `iss` claim must match this exactly.
const GITHUB_ISSUER: &str = "https://token.actions.githubusercontent.com";
const GITHUB_JWKS_URL: &str = "https://token.actions.githubusercontent.com/.well-known/jwks";
const JWKS_TTL: Duration = Duration::from_secs(3600);

/// Minimum set of claims we extract from the GitHub OIDC token.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct Claims {
    /// Issuer — must be the GitHub Actions OIDC endpoint.
    iss: Option<String>,
    /// Audience — must match the configured `audience`.
    aud: Option<String>,
    /// Repository in `owner/repo` form.
    repository: Option<String>,
    /// Standard JWT expiry (unix timestamp).
    exp: Option<u64>,
}

/// Cached JWKS and its fetch time.
struct JwksCache {
    keys: jsonwebtoken::jwk::JwkSet,
    fetched_at: Instant,
}

static JWKS_CACHE: RwLock<Option<JwksCache>> = RwLock::new(None);

/// Load (or re-use cached) GitHub's JWKS. Re-fetches when the cache is cold or
/// the TTL has expired.
async fn fetch_jwks() -> Result<jsonwebtoken::jwk::JwkSet> {
    {
        let cache = JWKS_CACHE.read().unwrap();
        if let Some(ref c) = *cache {
            if c.fetched_at.elapsed() < JWKS_TTL {
                return Ok(c.keys.clone());
            }
        }
    }
    // Re-fetch.
    let client = reqwest::Client::new();
    let resp = client
        .get(GITHUB_JWKS_URL)
        .send()
        .await
        .context("fetching GitHub JWKS")?
        .error_for_status()
        .context("GitHub JWKS returned error")?;
    let body = resp.bytes().await.context("reading JWKS body")?;
    let keys: jsonwebtoken::jwk::JwkSet = serde_json::from_slice(&body).context("parsing JWKS")?;
    let mut cache = JWKS_CACHE.write().unwrap();
    *cache = Some(JwksCache {
        keys: keys.clone(),
        fetched_at: Instant::now(),
    });
    Ok(keys)
}

/// Validate a GitHub OIDC JWT against the configured policy.
/// Returns `Ok(())` if the token is valid and the repository is allowed.
pub async fn validate_github_oidc(token: &str, cfg: &OidcConfig) -> Result<()> {
    if !cfg.enabled {
        bail!("OIDC is not enabled on this server");
    }
    if !is_jwt(token) {
        bail!("not a JWT");
    }

    // Find the key that matches the JWT header's `kid`.
    let header = jsonwebtoken::decode_header(token).context("decoding JWT header")?;
    let kid = header.kid.as_deref().unwrap_or("");
    let jwks = fetch_jwks()
        .await
        .context("fetching JWKS for OIDC validation")?;
    let jwk = jwks
        .find(kid)
        .ok_or_else(|| anyhow::anyhow!("JWT key id {kid:?} not found in GitHub JWKS"))?;

    let decoding_key =
        jsonwebtoken::DecodingKey::from_jwk(jwk).context("converting JWK to decoding key")?;

    let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::RS256);
    validation.set_issuer(&[GITHUB_ISSUER]);
    validation.set_audience(&[&cfg.audience]);
    // GitHub OIDC tokens are valid for ~5 minutes; allow 30s clock skew.
    validation.leeway = 30;

    let data = jsonwebtoken::decode::<Claims>(token, &decoding_key, &validation)
        .context("validating JWT")?;

    let repo = data.claims.repository.as_deref().unwrap_or("");
    if repo.is_empty() {
        bail!("OIDC token missing 'repository' claim");
    }

    if !cfg.allowed_repos.is_empty() && !repo_allowed(repo, &cfg.allowed_repos) {
        bail!(
            "repository {repo:?} is not in the OIDC push allowlist (allowed_repos: {:?})",
            cfg.allowed_repos
        );
    }

    tracing::info!(repo, "OIDC push authorised");
    Ok(())
}

/// Simple glob match: `myorg/*` matches `myorg/repo`, `myorg/repo` exact match.
fn repo_allowed(repo: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|p| {
        if let Some(prefix) = p.strip_suffix("/*") {
            repo.starts_with(prefix) && repo[prefix.len()..].starts_with('/')
        } else {
            p == repo
        }
    })
}

/// Heuristic: does this bearer token look like a JWT?
fn is_jwt(token: &str) -> bool {
    // JWT = header.payload.signature (three base64url segments).
    // Non-JWT static tokens are typically hex or random strings.
    token.matches('.').count() == 2 && !token.contains(' ')
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;

    #[derive(Debug, Deserialize, Serialize)]
    struct ProviderSmokeClaims {
        sub: String,
        exp: u64,
    }

    #[test]
    fn jsonwebtoken_crypto_provider_is_configured() {
        let claims = ProviderSmokeClaims {
            sub: "provider-smoke".to_string(),
            exp: 4_102_444_800, // 2100-01-01
        };
        let token = jsonwebtoken::encode(
            &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256),
            &claims,
            &jsonwebtoken::EncodingKey::from_secret(b"artifactx-test"),
        )
        .expect("HS256 token can be encoded");
        let decoded = jsonwebtoken::decode::<ProviderSmokeClaims>(
            &token,
            &jsonwebtoken::DecodingKey::from_secret(b"artifactx-test"),
            &jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::HS256),
        )
        .expect("HS256 token can be decoded with configured crypto provider");
        assert_eq!(decoded.claims.sub, "provider-smoke");
    }

    #[test]
    fn glob_prefix_matches() {
        let allowed = vec!["artifactx-rs/*".to_string()];
        assert!(repo_allowed("artifactx-rs/artifactx", &allowed));
        assert!(!repo_allowed("evil-org/artifactx", &allowed));
    }

    #[test]
    fn exact_match() {
        let allowed = vec!["myorg/myrepo".to_string()];
        assert!(repo_allowed("myorg/myrepo", &allowed));
        assert!(!repo_allowed("myorg/other", &allowed));
    }

    #[test]
    fn jwt_detection() {
        assert!(is_jwt("eyJhbG.eyJzdW.test"));
        assert!(!is_jwt("not-a-jwt-token"));
        assert!(!is_jwt(""));
    }
}
