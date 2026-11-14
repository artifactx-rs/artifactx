//! S3-compatible blob storage: key mapping, SigV4 signing, and uploads.
//!
//! Phase 1 of ADR-0031: the filesystem remains the publish input; this module
//! uploads pool blobs and presigns reads so `arx serve` can answer with a
//! redirect instead of streaming package bytes.

use std::fmt::Write as _;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use reqwest::Url;
use sha2::{Digest, Sha256};

const ALGORITHM: &str = "AWS4-HMAC-SHA256";
const SERVICE: &str = "s3";
const MAX_PRESIGN_TTL: u64 = 604_800;

/// Validated S3-compatible storage settings.
#[derive(Debug, Clone)]
pub struct S3Settings {
    endpoint: Url,
    region: String,
    bucket: String,
    prefix: String,
    path_style: bool,
    presign_ttl_secs: u64,
}

impl S3Settings {
    pub fn new(
        endpoint: &str,
        region: &str,
        bucket: &str,
        prefix: &str,
        path_style: bool,
        presign_ttl_secs: u64,
    ) -> Result<Self> {
        let endpoint = Url::parse(endpoint).context("parsing [storage] endpoint")?;
        if !matches!(endpoint.scheme(), "http" | "https") {
            bail!("[storage] endpoint must be http(s)");
        }
        if endpoint.host_str().is_none() {
            bail!("[storage] endpoint must include a host");
        }
        if !endpoint.username().is_empty() || endpoint.password().is_some() {
            bail!("[storage] endpoint must not include credentials; use ARX_S3_* env vars");
        }
        if endpoint.path() != "/" && !endpoint.path().is_empty() {
            bail!("[storage] endpoint must not include a path");
        }
        if endpoint.query().is_some() || endpoint.fragment().is_some() {
            bail!("[storage] endpoint must not include a query or fragment");
        }
        if bucket.is_empty() || bucket.contains('/') || bucket.chars().any(char::is_whitespace) {
            bail!("[storage] bucket must be a non-empty name without '/' or whitespace");
        }
        if !path_style
            && !bucket
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.')
        {
            bail!("[storage] bucket must be DNS-compatible when path_style = false");
        }
        if region.is_empty() {
            bail!("[storage] region must not be empty");
        }
        let prefix = prefix.trim_matches('/');
        if !prefix.is_empty()
            && prefix
                .split('/')
                .any(|part| part == ".." || part == "." || part.is_empty())
        {
            bail!("[storage] prefix must not contain empty, '.' or '..' segments");
        }
        if presign_ttl_secs == 0 || presign_ttl_secs > MAX_PRESIGN_TTL {
            bail!("[storage] presign_ttl_secs must be between 1 and {MAX_PRESIGN_TTL}");
        }
        Ok(Self {
            endpoint,
            region: region.to_string(),
            bucket: bucket.to_string(),
            prefix: prefix.to_string(),
            path_style,
            presign_ttl_secs,
        })
    }

    /// Object key for a repository-relative blob path, with the configured prefix.
    pub fn object_key(&self, relative: &str) -> String {
        let relative = relative.trim_start_matches('/');
        if self.prefix.is_empty() {
            relative.to_string()
        } else {
            format!("{}/{relative}", self.prefix)
        }
    }

    fn object_url(&self, key: &str) -> Result<(Url, String)> {
        let host = self.endpoint.host_str().context("endpoint host")?;
        let port = self
            .endpoint
            .port()
            .map(|port| format!(":{port}"))
            .unwrap_or_default();
        let key = key.trim_start_matches('/');
        if key.is_empty() {
            bail!("S3 object key must not be empty");
        }
        // Build the URL from SigV4-encoded segments so the path we sign is the
        // path S3 canonicalizes: `+`, `%`, `?`, `#` and spaces are all encoded.
        let encoded_key = uri_encode(key, false);
        let path = if self.path_style {
            format!("/{}/{encoded_key}", uri_encode(&self.bucket, true))
        } else {
            format!("/{encoded_key}")
        };
        let authority = if self.path_style {
            format!("{host}{port}")
        } else {
            format!("{}.{host}{port}", self.bucket)
        };
        let url = Url::parse(&format!("{}://{authority}{path}", self.endpoint.scheme()))
            .context("building S3 object URL")?;
        let host_header = if self.path_style {
            format!("{host}{port}")
        } else {
            format!("{}.{host}{port}", self.bucket)
        };
        Ok((url, host_header))
    }

    /// Presign a GET for `key` with the configured TTL.
    pub fn presign_get(&self, key: &str) -> Result<String> {
        self.presign_at("GET", key, Utc::now())
    }

    /// Presign a HEAD for `key` with the configured TTL.
    pub fn presign_head(&self, key: &str) -> Result<String> {
        self.presign_at("HEAD", key, Utc::now())
    }

    #[cfg(test)]
    fn presign_get_at(&self, key: &str, now: DateTime<Utc>) -> Result<String> {
        self.presign_at("GET", key, now)
    }

    fn presign_at(&self, method: &str, key: &str, now: DateTime<Utc>) -> Result<String> {
        let creds = Credentials::from_env()?;
        let (url, host) = self.object_url(key)?;
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
        let date = &amz_date[..8];
        let scope = format!("{date}/{}/{SERVICE}/aws4_request", self.region);
        let credential = format!("{}/{scope}", creds.access_key);
        let mut params = vec![
            ("X-Amz-Algorithm", ALGORITHM.to_string()),
            ("X-Amz-Credential", credential),
            ("X-Amz-Date", amz_date.clone()),
            ("X-Amz-Expires", self.presign_ttl_secs.to_string()),
            ("X-Amz-SignedHeaders", "host".to_string()),
        ];
        if let Some(token) = creds.session_token.as_deref() {
            params.push(("X-Amz-Security-Token", token.to_string()));
        }
        params.sort_by(|a, b| a.0.cmp(b.0));
        let canonical_query = params
            .iter()
            .map(|(name, value)| format!("{}={}", uri_encode(name, true), uri_encode(value, true)))
            .collect::<Vec<_>>()
            .join("&");

        let canonical_request = format!(
            "{method}\n{}\n{}\nhost:{}\n\nhost\nUNSIGNED-PAYLOAD",
            url.path(),
            canonical_query,
            host
        );
        let string_to_sign = format!(
            "{ALGORITHM}\n{amz_date}\n{scope}\n{}",
            sha256_hex(canonical_request.as_bytes())
        );
        let signature = hex::encode(hmac_sha256(
            &signing_key(&creds.secret_key, date, &self.region),
            string_to_sign.as_bytes(),
        ));
        Ok(format!(
            "{}?{canonical_query}&X-Amz-Signature={signature}",
            url
        ))
    }

    /// Upload a file without loading the whole package into memory.
    pub fn put_file(&self, key: &str, path: &Path) -> Result<()> {
        let mut file =
            std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
        let payload_hash =
            sha256_reader(&mut file).with_context(|| format!("hashing {}", path.display()))?;
        file.seek(SeekFrom::Start(0))
            .with_context(|| format!("rewinding {}", path.display()))?;
        self.put_signed(
            key,
            &payload_hash,
            reqwest::blocking::Body::new(file),
            Utc::now(),
        )
    }

    fn put_signed(
        &self,
        key: &str,
        payload_hash: &str,
        body: reqwest::blocking::Body,
        now: DateTime<Utc>,
    ) -> Result<()> {
        let signed = self.signed_put(key, payload_hash, now)?;
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .context("building S3 HTTP client")?;
        let mut request = client
            .put(signed.url)
            .header("x-amz-content-sha256", signed.payload_hash)
            .header("x-amz-date", signed.amz_date)
            .header("authorization", signed.authorization);
        if let Some(token) = signed.session_token.as_deref() {
            request = request.header("x-amz-security-token", token);
        }
        let response = request.body(body).send().context("uploading to S3")?;
        let status = response.status();
        if !status.is_success() {
            let text = response.text().unwrap_or_default();
            bail!("S3 PUT failed with {status}: {}", text.trim());
        }
        Ok(())
    }

    fn signed_put(&self, key: &str, payload_hash: &str, now: DateTime<Utc>) -> Result<SignedPut> {
        let creds = Credentials::from_env()?;
        let (url, host) = self.object_url(key)?;
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
        let date = &amz_date[..8];
        let scope = format!("{date}/{}/{SERVICE}/aws4_request", self.region);

        let mut headers = vec![
            ("host".to_string(), host),
            ("x-amz-content-sha256".to_string(), payload_hash.to_string()),
            ("x-amz-date".to_string(), amz_date.clone()),
        ];
        if let Some(token) = creds.session_token.as_deref() {
            headers.push(("x-amz-security-token".to_string(), token.to_string()));
        }
        headers.sort_by(|a, b| a.0.cmp(&b.0));
        let canonical_headers = headers
            .iter()
            .map(|(name, value)| format!("{name}:{}\n", value.trim()))
            .collect::<String>();
        let signed_headers = headers
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>()
            .join(";");
        let canonical_request = format!(
            "PUT\n{}\n\n{canonical_headers}\n{signed_headers}\n{payload_hash}",
            url.path()
        );
        let string_to_sign = format!(
            "{ALGORITHM}\n{amz_date}\n{scope}\n{}",
            sha256_hex(canonical_request.as_bytes())
        );
        let signature = hex::encode(hmac_sha256(
            &signing_key(&creds.secret_key, date, &self.region),
            string_to_sign.as_bytes(),
        ));
        let authorization = format!(
            "{ALGORITHM} Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={signature}",
            creds.access_key
        );
        Ok(SignedPut {
            url,
            payload_hash: payload_hash.to_string(),
            amz_date,
            authorization,
            session_token: creds.session_token,
        })
    }
}

struct SignedPut {
    url: Url,
    payload_hash: String,
    amz_date: String,
    authorization: String,
    session_token: Option<String>,
}

/// Environment-only S3 credentials.
#[derive(Clone)]
pub struct Credentials {
    access_key: String,
    secret_key: String,
    session_token: Option<String>,
}

impl Credentials {
    pub fn from_env() -> Result<Self> {
        let access_key = std::env::var("ARX_S3_ACCESS_KEY_ID").unwrap_or_default();
        let secret_key = std::env::var("ARX_S3_SECRET_ACCESS_KEY").unwrap_or_default();
        let session_token = std::env::var("ARX_S3_SESSION_TOKEN")
            .ok()
            .filter(|value| !value.is_empty());
        if access_key.is_empty() || secret_key.is_empty() {
            bail!("S3 credentials missing: set ARX_S3_ACCESS_KEY_ID and ARX_S3_SECRET_ACCESS_KEY");
        }
        Ok(Self {
            access_key,
            secret_key,
            session_token,
        })
    }
}

/// Repository-relative blob roots derived from `arx.toml`.
#[derive(Debug, Clone)]
pub struct BlobLayout {
    apt_prefix: String,
    yum_prefix: String,
}

impl BlobLayout {
    pub fn new(apt_base: &str, pool_dir: &str, yum_base: &str) -> Self {
        Self {
            apt_prefix: format!("{}/", apt_base.trim_matches('/')),
            yum_prefix: format!("{}/", yum_base.trim_matches('/')),
        }
        .with_pool(pool_dir)
    }

    fn with_pool(mut self, pool_dir: &str) -> Self {
        self.apt_prefix = format!("{}{}/", self.apt_prefix, pool_dir.trim_matches('/'));
        self
    }

    /// Key for a repository-relative path. Only apt pool files and yum RPMs
    /// outside `repodata/` are blobs.
    pub fn key_for_path(&self, relative: &str) -> Option<String> {
        let relative = relative.trim_start_matches('/');
        if unsafe_relative_path(relative) {
            return None;
        }
        // Internal metadata (e.g. `.arx-manifest.toml`) is not a package blob.
        if relative.split('/').any(|part| part.starts_with('.')) {
            return None;
        }
        if let Some(rest) = relative.strip_prefix(&self.apt_prefix) {
            if rest.is_empty() {
                return None;
            }
            return Some(relative.to_string());
        }
        if let Some(rest) = relative.strip_prefix(&self.yum_prefix) {
            if rest.is_empty() || rest.contains("/repodata/") || rest.starts_with("repodata/") {
                return None;
            }
            if !is_yum_blob(Path::new(rest)) {
                return None;
            }
            return Some(relative.to_string());
        }
        None
    }

    /// Key for an HTTP path (percent-encoded) below the served repository root.
    pub fn key_for_uri(&self, uri_path: &str) -> Option<String> {
        let decoded = percent_decode(uri_path.trim_start_matches('/'))?;
        self.key_for_path(&decoded)
    }
}

fn unsafe_relative_path(relative: &str) -> bool {
    relative.is_empty()
        || relative.split('/').any(|part| {
            part.is_empty()
                || part == "."
                || part == ".."
                || part.contains('\\')
                || part.chars().any(char::is_control)
        })
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return None;
            }
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn sha256_reader(reader: &mut impl Read) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn signing_key(secret: &str, date: &str, region: &str) -> Vec<u8> {
    let date_key = hmac_sha256(format!("AWS4{secret}").as_bytes(), date.as_bytes());
    let region_key = hmac_sha256(&date_key, region.as_bytes());
    let service_key = hmac_sha256(&region_key, SERVICE.as_bytes());
    hmac_sha256(&service_key, b"aws4_request")
}

fn uri_encode(input: &str, encode_slash: bool) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            b'/' if !encode_slash => out.push('/'),
            _ => {
                write!(out, "%{byte:02X}").expect("writing to String");
            }
        }
    }
    out
}

/// Extensions that are package blobs under the yum base.
pub fn is_yum_blob(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("rpm"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Credentials are process-global; serialize tests that mutate them.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn aws_presign_example_matches_published_signature() {
        let _guard = ENV_LOCK.lock().unwrap();
        // AWS S3 SigV4 query-parameter example (2013-05-24).
        let settings = S3Settings::new(
            "https://s3.amazonaws.com",
            "us-east-1",
            "examplebucket",
            "",
            false,
            86_400,
        )
        .unwrap();
        std::env::set_var("ARX_S3_ACCESS_KEY_ID", "AKIAIOSFODNN7EXAMPLE");
        std::env::set_var(
            "ARX_S3_SECRET_ACCESS_KEY",
            "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
        );
        std::env::remove_var("ARX_S3_SESSION_TOKEN");
        let now = DateTime::parse_from_rfc3339("2013-05-24T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let url = settings.presign_get_at("test.txt", now).unwrap();
        assert!(
            url.ends_with(
                "X-Amz-Signature=aeeed9bbccd4d02ee5c0109b86d86835f995330da4c265957d157751f604d404"
            ),
            "{url}"
        );
    }

    #[test]
    fn aws_put_authorization_matches_botocore() {
        let _guard = ENV_LOCK.lock().unwrap();
        // Cross-checked against botocore's SigV4 signer at the same timestamp.
        std::env::set_var("ARX_S3_ACCESS_KEY_ID", "S3RVER");
        std::env::set_var("ARX_S3_SECRET_ACCESS_KEY", "S3RVER");
        std::env::remove_var("ARX_S3_SESSION_TOKEN");
        let settings = S3Settings::new(
            "http://127.0.0.1:19000",
            "us-east-1",
            "packages",
            "",
            true,
            300,
        )
        .unwrap();
        let now = DateTime::parse_from_rfc3339("2013-05-24T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let signed = settings
            .signed_put(
                "apt/pool/main/hello_1.0.0_amd64.deb",
                &sha256_hex(b"hello"),
                now,
            )
            .unwrap();
        assert!(
            signed.authorization.ends_with(
                "Signature=dd66e59f01372bd89778343e8ceb7f9eed57f548cec66b512cd987dd77873670"
            ),
            "{}",
            signed.authorization
        );
    }

    #[test]
    fn encodes_reserved_path_characters() {
        let settings = S3Settings::new(
            "http://127.0.0.1:9000",
            "us-east-1",
            "packages",
            "",
            true,
            300,
        )
        .unwrap();
        let (url, _) = settings.object_url("apt/pool/main/hello+1%25.deb").unwrap();
        assert_eq!(url.path(), "/packages/apt/pool/main/hello%2B1%2525.deb");

        let layout = BlobLayout::new("apt", "pool", "yum");
        assert_eq!(
            layout
                .key_for_uri("/apt/pool/main/hello%2B1%2525.deb")
                .as_deref(),
            Some("apt/pool/main/hello+1%25.deb")
        );
    }

    #[test]
    fn rejects_endpoint_credentials() {
        let err = S3Settings::new(
            "http://user:pass@127.0.0.1:9000",
            "us-east-1",
            "packages",
            "",
            true,
            300,
        )
        .unwrap_err();
        assert!(err.to_string().contains("must not include credentials"));
    }

    #[test]
    fn head_presign_uses_head_method() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("ARX_S3_ACCESS_KEY_ID", "S3RVER");
        std::env::set_var("ARX_S3_SECRET_ACCESS_KEY", "S3RVER");
        std::env::remove_var("ARX_S3_SESSION_TOKEN");
        let settings = S3Settings::new(
            "http://127.0.0.1:9000",
            "us-east-1",
            "packages",
            "",
            true,
            300,
        )
        .unwrap();
        let now = DateTime::parse_from_rfc3339("2013-05-24T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let get = settings.presign_get_at("apt/pool/a.deb", now).unwrap();
        let head = settings.presign_at("HEAD", "apt/pool/a.deb", now).unwrap();
        assert!(get.contains("X-Amz-Signature="));
        assert!(head.contains("X-Amz-Signature="));
        assert_ne!(get, head, "HEAD must be signed as HEAD, not GET");
    }

    #[test]
    fn path_style_url_and_key_prefix() {
        let settings = S3Settings::new(
            "http://127.0.0.1:9000",
            "us-east-1",
            "packages",
            "prod",
            true,
            300,
        )
        .unwrap();
        assert_eq!(
            settings.object_key("apt/pool/main/x.deb"),
            "prod/apt/pool/main/x.deb"
        );
        let (url, host) = settings.object_url("apt/pool/main/x.deb").unwrap();
        assert_eq!(url.path(), "/packages/apt/pool/main/x.deb");
        assert_eq!(host, "127.0.0.1:9000");
    }

    #[test]
    fn blob_layout_excludes_metadata_and_non_rpms() {
        let layout = BlobLayout::new("apt", "pool", "yum");
        assert_eq!(
            layout
                .key_for_path("apt/pool/main/h/hello/hello_1_amd64.deb")
                .as_deref(),
            Some("apt/pool/main/h/hello/hello_1_amd64.deb")
        );
        assert_eq!(
            layout
                .key_for_path("yum/stable/x86_64/hello-1.0.rpm")
                .as_deref(),
            Some("yum/stable/x86_64/hello-1.0.rpm")
        );
        assert!(layout
            .key_for_path("yum/stable/x86_64/repodata/repomd.xml")
            .is_none());
        assert!(layout
            .key_for_path("yum/stable/x86_64/hello-1.0.rpm.asc")
            .is_none());
        assert!(layout.key_for_path("keys/public.asc").is_none());
        assert!(layout.key_for_path("apt/pool/../keys/public.asc").is_none());
    }

    #[test]
    fn uri_decoding_rejects_traversal() {
        let layout = BlobLayout::new("apt", "pool", "yum");
        assert_eq!(
            layout
                .key_for_uri("/apt/pool/main/hello%5F1.0_amd64.deb")
                .as_deref(),
            Some("apt/pool/main/hello_1.0_amd64.deb")
        );
        assert!(layout
            .key_for_uri("/apt/pool/%2e%2e/keys/public.asc")
            .is_none());
        assert!(layout.key_for_uri("/apt/pool/hello%2F..%2Fkeys").is_none());
    }
}
