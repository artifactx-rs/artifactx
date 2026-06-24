//! Built-in HTTP server (axum + tower-http): serves the repository tree to
//! `apt`/`dnf`, exposes Prometheus `/metrics`, and offers a small modern REST
//! API under `/api/v1` for managing packages — the same operations as the CLI:
//!
//! | Method & path | CLI equivalent | Notes |
//! |---|---|---|
//! | `GET /api/v1/health` | — | `{name, version}` |
//! | `GET /api/v1/packages` | `arx list` | JSON list of pooled packages |
//! | `POST /api/v1/packages` | `arx push` / `arx add` + `publish` | upload a `.deb`/`.rpm`, republish |
//! | `DELETE /api/v1/packages/:name` | `arx rm` + `publish` | `?version=&apt=&yum=` |
//! | `POST /api/v1/gc` | `arx gc` | `?keep=N&dry_run=&apt=&yum=` |
//! | `POST /api/v1/publish` | `arx publish` | trigger apt+yum publish |
//! | `POST /api/v1/rollback/*target` | `arx rollback` | atomic symlink flip |
//! | `GET /api/v1/history/*target` | `arx history` | JSON list of published states |
//! | `POST /api/v1/import` | `arx import` | pull from upstream repo |
//! | `POST /api/v1/promote` | `arx promote` | move packages between components |
//!
//! All write operations require a configured `ARX_SERVE_TOKEN` (bearer auth).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use walkdir::WalkDir;

use anyhow::{bail, Context, Result};
use axum::{
    body::Bytes,
    extract::{Path as AxPath, Query, Request, State},
    http::{header, HeaderMap, Method, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use metrics_exporter_prometheus::PrometheusHandle;
use pgp::composed::SignedSecretKey;
use serde::{Deserialize, Serialize};
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

const ROLLBACK_ROUTE: &str = "/api/v1/rollback/*target";
const HISTORY_ROUTE: &str = "/api/v1/history/*target";
const OPENAPI_YAML: &str = include_str!("openapi.yaml");

use crate::config::Config;
use crate::pool;

/// Shared server state. Cloneable (cheap — everything behind `Arc`/handle).
#[derive(Clone)]
struct AppState {
    metrics: PrometheusHandle,
    /// Bearer token; `None` = public reads, writes disabled (unless OIDC is on).
    token: Option<Arc<str>>,
    root: PathBuf,
    cfg: Arc<Config>,
    key: Option<Arc<SignedSecretKey>>,
    passphrase: Arc<str>,
}

#[derive(Debug)]
struct BadRequest(anyhow::Error);

impl std::fmt::Display for BadRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for BadRequest {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.0.source()
    }
}

fn bad_request<E>(error: E) -> anyhow::Error
where
    E: Into<anyhow::Error>,
{
    BadRequest(error.into()).into()
}

fn client_scope_name<'a>(name: &'a str, field: &str) -> Result<&'a str> {
    crate::scope::validate_scope_name(name, field).map_err(bad_request)
}

fn client_target_link(root: &Path, cfg: &Config, target: &str) -> Result<PathBuf> {
    match target.split_once('/') {
        Some((repo, arch)) if !arch.contains('/') => {
            let repo = client_scope_name(repo, "yum repo")?;
            let arch = client_scope_name(arch, "yum arch")?;
            Ok(cfg
                .checked_yum_base(root)?
                .join(repo)
                .join(arch)
                .join("repodata"))
        }
        Some(_) => Err(bad_request(anyhow::anyhow!(
            "invalid rollback target {target:?}: expected <repo>/<arch>"
        ))),
        None => {
            let dist = client_scope_name(target, "apt dist")?;
            Ok(root.join("apt/dists").join(dist))
        }
    }
}

impl AppState {
    /// Writes require authentication — either a stored token or OIDC.
    /// Returns `None` if writes are allowed (auth is configured), `Some(403)` if
    /// no auth is configured at all.
    fn write_forbidden(&self) -> Option<Response> {
        let has_auth = self.token.is_some() || self.cfg.oidc.enabled;
        if !has_auth {
            Some(
                (
                    StatusCode::FORBIDDEN,
                    "writes disabled: set ARX_SERVE_TOKEN or enable OIDC on the server\n",
                )
                    .into_response(),
            )
        } else {
            None
        }
    }
}

// --- middleware ---

async fn metrics_handler(State(st): State<AppState>) -> String {
    st.metrics.render()
}

async fn openapi_handler() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/yaml; charset=utf-8")],
        OPENAPI_YAML,
    )
}

async fn api_docs_handler() -> Html<&'static str> {
    Html(
        r##"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>ArtifactX API Docs</title>
  <link rel="stylesheet" href="https://unpkg.com/swagger-ui-dist@5/swagger-ui.css" />
  <style>
    body { margin: 0; background: #fafafa; }
    .topbar { display: none; }
  </style>
</head>
<body>
  <div id="swagger-ui"></div>
  <script src="https://unpkg.com/swagger-ui-dist@5/swagger-ui-bundle.js"></script>
  <script>
    window.ui = SwaggerUIBundle({
      url: "/api/openapi.yaml",
      dom_id: "#swagger-ui",
      deepLinking: true,
      persistAuthorization: true,
      tryItOutEnabled: true
    });
  </script>
</body>
</html>"##,
    )
}

/// Bearer-token gate: static `ARX_SERVE_TOKEN` OR OIDC JWT (ADR-0014).
/// Unset token AND OIDC disabled = public reads, writes disabled.
async fn require_auth(State(st): State<AppState>, req: Request, next: Next) -> Response {
    let auth_configured = st.token.is_some() || st.cfg.oidc.enabled;
    let safe_method = matches!(
        *req.method(),
        Method::GET | Method::HEAD | Method::OPTIONS | Method::TRACE
    );
    let presented = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::to_string);

    match presented {
        Some(token) => {
            // OIDC path: JWT-looking token + OIDC enabled.
            if st.cfg.oidc.enabled && token.contains('.') {
                match crate::oidc::validate_github_oidc(&token, &st.cfg.oidc).await {
                    Ok(()) => return next.run(req).await,
                    Err(e) => {
                        tracing::warn!(error = %e, "OIDC validation failed");
                        // Fall through to static-token check.
                    }
                }
            }
            // Static token path.
            if st.token.as_deref() == Some(&token) {
                return next.run(req).await;
            }
            (
                StatusCode::UNAUTHORIZED,
                [(axum::http::header::WWW_AUTHENTICATE, "Bearer")],
                "unauthorized\n",
            )
                .into_response()
        }
        None => {
            if auth_configured && !safe_method {
                return (
                    StatusCode::UNAUTHORIZED,
                    [(axum::http::header::WWW_AUTHENTICATE, "Bearer")],
                    "unauthorized\n",
                )
                    .into_response();
            }
            // No creds presented on public reads, or on a server with no auth
            // configured. Write handlers still return 403 when writes are
            // disabled by configuration.
            next.run(req).await
        }
    }
}

fn normalize_repo_relative_path(path: &str) -> String {
    path.trim_start_matches('/')
        .trim_start_matches("./")
        .replace('\\', "/")
}

fn percent_decode_path_lossy(path: &str) -> String {
    let mut out = Vec::with_capacity(path.len());
    let mut bytes = path.as_bytes().iter().copied();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let first = bytes.next();
            let second = bytes.next();
            if let (Some(first), Some(second)) = (first, second) {
                let hex = [first, second];
                if let Ok(hex) = std::str::from_utf8(&hex) {
                    if let Ok(decoded) = u8::from_str_radix(hex, 16) {
                        out.push(decoded);
                        continue;
                    }
                }
                out.push(byte);
                out.push(first);
                out.push(second);
                continue;
            }
            out.push(byte);
            if let Some(first) = first {
                out.push(first);
            }
            continue;
        }
        out.push(byte);
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn configured_repo_relative_path(root: &Path, path: &str) -> Option<String> {
    let path = Path::new(path);
    let relative = if path.is_absolute() {
        path.strip_prefix(root).ok()?
    } else {
        path
    };
    Some(normalize_repo_relative_path(
        relative.to_string_lossy().as_ref(),
    ))
}

fn is_sensitive_static_path(path: &str, root: &Path, cfg: &Config) -> bool {
    let path = normalize_repo_relative_path(&percent_decode_path_lossy(path));
    let Some(private_key) = configured_repo_relative_path(root, &cfg.signing.private_key) else {
        return false;
    };
    let sensitive = [
        private_key.clone(),
        format!("{private_key}.old"),
        format!("{private_key}.bak"),
    ];
    sensitive.iter().any(|candidate| path == candidate.as_str())
}

async fn block_sensitive_static_paths(
    State(st): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    if is_sensitive_static_path(req.uri().path(), &st.root, &st.cfg) {
        tracing::warn!(path = req.uri().path(), "blocked sensitive static path");
        return StatusCode::NOT_FOUND.into_response();
    }
    next.run(req).await
}

async fn track_metrics(req: Request, next: Next) -> Response {
    let path = req.uri().path().to_string();
    let response = next.run(req).await;
    metrics::counter!("arx_http_requests_total").increment(1);
    metrics::counter!("arx_http_responses_total", "status" => response.status().as_u16().to_string())
        .increment(1);
    tracing::debug!(path, status = %response.status(), "served request");
    response
}

// --- API handlers ---

#[derive(Serialize)]
struct Health {
    name: &'static str,
    version: &'static str,
}

async fn health_handler() -> Json<Health> {
    Json(Health {
        name: "arx",
        version: crate::VERSION,
    })
}

#[derive(Deserialize)]
struct PackageListQuery {
    q: Option<String>,
    name_prefix: Option<String>,
    version: Option<String>,
    arch: Option<String>,
    scope: Option<String>,
    #[serde(default)]
    apt: bool,
    #[serde(default)]
    yum: bool,
}

async fn list_handler(State(st): State<AppState>, Query(q): Query<PackageListQuery>) -> Response {
    let apt_pool_root = match st.cfg.checked_apt_pool_root(&st.root) {
        Ok(path) => path,
        Err(e) => return err_response(&e),
    };
    let yum_base = match st.cfg.checked_yum_base(&st.root) {
        Ok(path) => path,
        Err(e) => return err_response(&e),
    };
    match pool::search(
        &apt_pool_root,
        &yum_base,
        pool::SearchOptions {
            query: q.q.as_deref(),
            name_prefix: q.name_prefix.as_deref(),
            version: q.version.as_deref(),
            arch: q.arch.as_deref(),
            scope: q.scope.as_deref(),
            apt: q.apt,
            yum: q.yum,
        },
    ) {
        Ok(entries) => {
            let infos: Vec<pool::PackageInfo> = entries.iter().map(pool::Entry::info).collect();
            Json(infos).into_response()
        }
        Err(e) => err_response(&e),
    }
}

#[derive(Deserialize)]
struct ScopeQuery {
    version: Option<String>,
    #[serde(default)]
    apt: bool,
    #[serde(default)]
    yum: bool,
}

#[derive(Serialize)]
struct DeleteResult {
    removed: Vec<pool::PackageInfo>,
    published: String,
}

async fn delete_handler(
    State(st): State<AppState>,
    AxPath(name): AxPath<String>,
    Query(q): Query<ScopeQuery>,
) -> Response {
    if let Some(resp) = st.write_forbidden() {
        return resp;
    }
    let blocking = move || -> Result<DeleteResult> {
        let _lock = crate::PublishLock::acquire(&st.root)?;
        let apt_pool_root = st.cfg.checked_apt_pool_root(&st.root)?;
        let yum_base = st.cfg.checked_yum_base(&st.root)?;
        let removed = pool::remove(
            &apt_pool_root,
            &name,
            q.version.as_deref(),
            &yum_base,
            q.apt,
            q.yum,
        )?;
        let infos = removed.iter().map(pool::Entry::info).collect();
        let published = publish_both(&st)?;
        Ok(DeleteResult {
            removed: infos,
            published,
        })
    };
    run_blocking(blocking).await
}

fn default_keep() -> usize {
    3
}

#[derive(Deserialize)]
struct GcQuery {
    name: Option<String>,
    name_prefix: Option<String>,
    #[serde(default = "default_keep")]
    keep: usize,
    #[serde(default)]
    keep_within_days: u32,
    #[serde(default)]
    grace_days: u32,
    #[serde(default)]
    dry_run: bool,
    #[serde(default)]
    ignore_rollback_states: bool,
    #[serde(default)]
    apt: bool,
    #[serde(default)]
    yum: bool,
}

#[derive(Serialize)]
struct GcResult {
    pruned: Vec<pool::PackageInfo>,
    dry_run: bool,
    retained_for_rollback: usize,
    deferred: usize,
    bytes_freed: u64,
    published: Option<String>,
}

async fn gc_handler(State(st): State<AppState>, Query(q): Query<GcQuery>) -> Response {
    if let Some(resp) = st.write_forbidden() {
        return resp;
    }
    let blocking = move || -> Result<GcResult> {
        let _lock = crate::PublishLock::acquire(&st.root)?;
        let apt_pool_root = st.cfg.checked_apt_pool_root(&st.root)?;
        let yum_base = st.cfg.checked_yum_base(&st.root)?;
        let report = pool::gc(
            &st.root,
            pool::GcOptions {
                name: q.name.as_deref(),
                name_prefix: q.name_prefix.as_deref(),
                keep: q.keep,
                keep_within_days: q.keep_within_days,
                grace_days: q.grace_days,
                apt_pool_root: &apt_pool_root,
                yum_base: &yum_base,
                apt: q.apt,
                yum: q.yum,
                dry_run: q.dry_run,
                retain_rollback_states: !q.ignore_rollback_states,
            },
        )?;
        let pruned = report.pruned.iter().map(pool::Entry::info).collect();
        let published = if report.dry_run || report.pruned.is_empty() {
            None
        } else {
            Some(publish_both(&st)?)
        };
        Ok(GcResult {
            pruned,
            dry_run: report.dry_run,
            retained_for_rollback: report.retained_for_rollback,
            deferred: report.deferred,
            bytes_freed: report.bytes_freed,
            published,
        })
    };
    run_blocking(blocking).await
}

#[derive(Serialize)]
struct PushResult {
    stored: String,
    published: String,
    /// Reasons any packages were left out of the index (empty on a clean push).
    /// Non-empty here means the server is in forgiving mode (`[apt].strict` off);
    /// under strict the request fails with 422 instead.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    skipped: Vec<String>,
}

/// `POST /api/v1/packages` — upload a `.deb`/`.rpm`, store it, republish.
async fn upload_handler(State(st): State<AppState>, headers: HeaderMap, body: Bytes) -> Response {
    if let Some(resp) = st.write_forbidden() {
        return resp;
    }
    let filename = match header(&headers, "x-arx-filename").and_then(safe_filename) {
        Some(f) => f,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                "missing or unsafe X-Arx-Filename header\n",
            )
                .into_response()
        }
    };
    let component = header(&headers, "x-arx-component").map(str::to_string);
    let repo = header(&headers, "x-arx-repo").map(str::to_string);
    let body = body.to_vec();
    let blocking = move || ingest(&st, &filename, component, repo, body);
    run_blocking(blocking).await
}

// --- API parity handlers (ADR: REST API = CLI first-class citizen) ---

#[derive(Serialize)]
struct PublishResult {
    apt: String,
    yum: String,
}

async fn publish_handler(State(st): State<AppState>) -> Response {
    if let Some(resp) = st.write_forbidden() {
        return resp;
    }
    let root = st.root.clone();
    let cfg = Arc::clone(&st.cfg);
    let key = st.key.clone();
    let passphrase = Arc::clone(&st.passphrase);
    let blocking = move || -> Result<PublishResult> {
        let _lock = crate::PublishLock::acquire(&root)?;
        crate::hooks::run(
            &root,
            &cfg,
            crate::hooks::HookEvent::PrePublish,
            &crate::hooks::HookContext::new().with("ARX_FORMATS", "apt,yum"),
        )?;
        let k = key.as_deref();
        let apt = crate::publish_apt(&root, &cfg, k, &passphrase, cfg.apt.strict, true)?;
        let yum = crate::publish_yum(&root, &cfg, k, &passphrase, true)?;
        let summary = format!("{}; {yum}", apt.summary);
        crate::hooks::run(
            &root,
            &cfg,
            crate::hooks::HookEvent::PostPublish,
            &crate::hooks::HookContext::new()
                .with("ARX_FORMATS", "apt,yum")
                .with("ARX_SUMMARY", summary),
        )?;
        Ok(PublishResult {
            apt: apt.summary,
            yum,
        })
    };
    run_blocking(blocking).await
}

#[derive(Serialize)]
struct HistoryItem {
    id: String,
    current: bool,
}

async fn history_handler(State(st): State<AppState>, AxPath(target): AxPath<String>) -> Response {
    let root = st.root.clone();
    let cfg = Arc::clone(&st.cfg);
    let blocking = move || -> Result<Vec<HistoryItem>> {
        let link = client_target_link(&root, &cfg, &target)?;
        let entries = arx_debrepo::statedir::list(&link).context("listing states")?;
        Ok(entries
            .into_iter()
            .map(|s| HistoryItem {
                id: s.id,
                current: s.current,
            })
            .collect())
    };
    run_blocking(blocking).await
}

#[derive(Deserialize)]
struct RollbackQuery {
    to: Option<String>,
}

#[derive(Serialize)]
struct RollbackResult {
    previous: String,
    current: String,
}

async fn rollback_handler(
    State(st): State<AppState>,
    AxPath(target): AxPath<String>,
    Query(q): Query<RollbackQuery>,
) -> Response {
    if let Some(resp) = st.write_forbidden() {
        return resp;
    }
    let root = st.root.clone();
    let cfg = Arc::clone(&st.cfg);
    let blocking = move || -> Result<RollbackResult> {
        crate::hooks::run(
            &root,
            &cfg,
            crate::hooks::HookEvent::PreRollback,
            &crate::hooks::HookContext::new().with("ARX_TARGET", target.clone()),
        )?;
        let link = client_target_link(&root, &cfg, &target)?;
        let id =
            arx_debrepo::statedir::rollback(&link, q.to.as_deref()).context("rollback failed")?;
        crate::hooks::run(
            &root,
            &cfg,
            crate::hooks::HookEvent::PostRollback,
            &crate::hooks::HookContext::new()
                .with("ARX_TARGET", target.clone())
                .with("ARX_STATE", id.clone()),
        )?;
        Ok(RollbackResult {
            previous: target,
            current: id,
        })
    };
    run_blocking(blocking).await
}

#[derive(Deserialize)]
struct ApiImportQuery {
    url: String,
    #[serde(default)]
    apt: bool,
    #[serde(default)]
    yum: bool,
    #[serde(default)]
    dist: Option<String>,
    #[serde(default)]
    component: Option<String>,
    #[serde(default = "default_arch_str")]
    arch: String,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    match_name: Option<String>,
    #[serde(default)]
    publish: bool,
}
fn default_arch_str() -> String {
    "amd64".into()
}

#[derive(Serialize)]
struct ImportResult {
    imported: usize,
    published: Option<String>,
}

async fn import_handler(State(st): State<AppState>, Query(q): Query<ApiImportQuery>) -> Response {
    if let Some(resp) = st.write_forbidden() {
        return resp;
    }
    let root = st.root.clone();
    let cfg = Arc::clone(&st.cfg);
    let key = st.key.clone();
    let passphrase = Arc::clone(&st.passphrase);
    let blocking = move || -> Result<ImportResult> {
        let do_apt = q.apt || !q.yum;
        let do_yum = q.yum || !q.apt;
        let mut imported = 0usize;
        if do_apt {
            let dist = match q.dist.as_deref() {
                Some(dist) => client_scope_name(dist, "apt dist")?,
                None => &cfg.apt.dist,
            };
            let comp = match q.component.as_deref() {
                Some(component) => client_scope_name(component, "apt component")?,
                None => &cfg.apt.component,
            };
            let arch = client_scope_name(&q.arch, "apt arch")?;
            imported += crate::import::import_apt(&crate::import::ImportOpts {
                root: &root,
                cfg: &cfg,
                base_url: &q.url,
                dist,
                component: comp,
                arch,
                match_name: q.match_name.as_deref(),
                limit: q.limit,
            })?;
        }
        if do_yum {
            let repo = match q.component.as_deref() {
                Some(repo) => client_scope_name(repo, "yum repo")?,
                None => &cfg.yum.repo,
            };
            imported += crate::import::import_yum(&root, &cfg, &q.url, repo, q.limit, false)?;
        }
        let published = if q.publish {
            let _lock = crate::PublishLock::acquire(&root)?;
            let cfg = Config::load(&root).unwrap_or_else(|_| (*cfg).clone());
            Some(publish_selected(
                &root,
                &cfg,
                key.as_deref(),
                &passphrase,
                do_apt,
                do_yum,
            )?)
        } else {
            None
        };
        Ok(ImportResult {
            imported,
            published,
        })
    };
    run_blocking(blocking).await
}

#[derive(Deserialize)]
struct ApiPromoteQuery {
    name: String,
    from: String,
    to: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    apt: bool,
    #[serde(default)]
    yum: bool,
}

#[derive(Serialize)]
struct PromoteResult {
    moved: usize,
}

async fn promote_handler(State(st): State<AppState>, Query(q): Query<ApiPromoteQuery>) -> Response {
    if let Some(resp) = st.write_forbidden() {
        return resp;
    }
    let root = st.root.clone();
    let cfg = Arc::clone(&st.cfg);
    let blocking = move || -> Result<PromoteResult> {
        let do_apt = q.apt || !q.yum;
        let do_yum = q.yum || !q.apt;
        let mut moved = 0usize;
        if do_apt {
            moved += promote_files(
                &cfg.checked_apt_pool_root(&root)?,
                &q.from,
                &q.to,
                &q.name,
                q.version.as_deref(),
                "deb",
            )?;
        }
        if do_yum {
            moved += promote_files(
                &cfg.checked_yum_base(&root)?,
                &q.from,
                &q.to,
                &q.name,
                q.version.as_deref(),
                "rpm",
            )?;
        }
        Ok(PromoteResult { moved })
    };
    run_blocking(blocking).await
}

fn promote_files(
    base: &Path,
    from: &str,
    to: &str,
    name: &str,
    version: Option<&str>,
    ext: &str,
) -> Result<usize> {
    let from = client_scope_name(from, "source scope")?;
    let to = client_scope_name(to, "destination scope")?;
    let src = base.join(from);
    let dst = base.join(to);
    let mut moved = 0usize;
    if !src.is_dir() {
        return Ok(0);
    }
    for entry in WalkDir::new(&src).into_iter().filter_map(|e| e.ok()) {
        let p = entry.path();
        if !p.is_file() || p.extension().map(|e| e != ext).unwrap_or(true) {
            continue;
        }
        let dest_dir = if ext == "deb" {
            let ctrl = arx_debrepo::deb::read_control(p)
                .with_context(|| format!("reading {}", p.display()))?;
            let matches = (ctrl.package().ok() == Some(name))
                && version.is_none_or(|v| ctrl.version().ok() == Some(v));
            matches.then(|| dst.clone())
        } else {
            let mut r = crate::createrepo_rs::rpm::RpmReader::open(p)
                .with_context(|| format!("opening {}", p.display()))?;
            let pkg = r.read_package().context("reading rpm")?;
            if pkg.name == name && version.is_none_or(|v| pkg.version == v) {
                let arch = client_scope_name(&pkg.arch, "yum arch")?;
                Some(dst.join(arch))
            } else {
                None
            }
        };
        if let Some(dest_dir) = dest_dir {
            let name = p
                .file_name()
                .and_then(|n| n.to_str())
                .context("invalid filename")?;
            std::fs::create_dir_all(&dest_dir)?;
            std::fs::rename(p, dest_dir.join(name))?;
            moved += 1;
        }
    }
    Ok(moved)
}

// --- shared helpers ---

async fn run_blocking<T, F>(f: F) -> Response
where
    T: Serialize + Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    match tokio::task::spawn_blocking(f).await {
        Ok(Ok(value)) => (StatusCode::OK, Json(value)).into_response(),
        Ok(Err(e)) => err_response(&e),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "task panicked\n").into_response(),
    }
}

fn err_response(e: &anyhow::Error) -> Response {
    tracing::warn!(error = %e, "api error");
    // A package rejected under strict mode is the client's fault, not ours.
    if let Some(skip) = e.downcast_ref::<crate::StrictSkip>() {
        return (StatusCode::UNPROCESSABLE_ENTITY, format!("{skip}\n")).into_response();
    }
    if e.downcast_ref::<BadRequest>().is_some() {
        return (StatusCode::BAD_REQUEST, format!("{e:#}\n")).into_response();
    }
    (StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}\n")).into_response()
}

fn header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

/// Restrict an uploaded filename to a single safe path component.
fn safe_filename(name: &str) -> Option<String> {
    let p = Path::new(name);
    if p.components().count() != 1 {
        return None;
    }
    let f = p.file_name()?.to_str()?;
    if f.is_empty() || f == ".." {
        return None;
    }
    Some(f.to_string())
}

fn stored_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

/// Republish selected formats (caller already holds the publish lock). Strict
/// mode (a skipped package → error) is governed by the server's `[apt].strict`.
fn publish_selected(
    root: &Path,
    cfg: &Config,
    key: Option<&SignedSecretKey>,
    passphrase: &str,
    do_apt: bool,
    do_yum: bool,
) -> Result<String> {
    let formats = crate::publish_formats(do_apt, do_yum);
    crate::hooks::run(
        root,
        cfg,
        crate::hooks::HookEvent::PrePublish,
        &crate::hooks::HookContext::new().with("ARX_FORMATS", formats.clone()),
    )?;
    let mut published = Vec::new();
    if do_apt {
        let apt = crate::publish_apt(root, cfg, key, passphrase, cfg.apt.strict, true)?;
        published.push(apt.summary);
    }
    if do_yum {
        published.push(crate::publish_yum(root, cfg, key, passphrase, true)?);
    }
    let summary = published.join("; ");
    crate::hooks::run(
        root,
        cfg,
        crate::hooks::HookEvent::PostPublish,
        &crate::hooks::HookContext::new()
            .with("ARX_FORMATS", formats)
            .with("ARX_SUMMARY", summary.clone()),
    )?;
    Ok(summary)
}

/// Republish both formats (caller already holds the publish lock).
fn publish_both(st: &AppState) -> Result<String> {
    crate::hooks::run(
        &st.root,
        &st.cfg,
        crate::hooks::HookEvent::PrePublish,
        &crate::hooks::HookContext::new().with("ARX_FORMATS", "apt,yum"),
    )?;
    let apt = crate::publish_apt(
        &st.root,
        &st.cfg,
        st.key.as_deref(),
        &st.passphrase,
        st.cfg.apt.strict,
        true,
    )?;
    let yum = crate::publish_yum(&st.root, &st.cfg, st.key.as_deref(), &st.passphrase, true)?;
    let summary = format!("{}; {yum}", apt.summary);
    crate::hooks::run(
        &st.root,
        &st.cfg,
        crate::hooks::HookEvent::PostPublish,
        &crate::hooks::HookContext::new()
            .with("ARX_FORMATS", "apt,yum")
            .with("ARX_SUMMARY", summary.clone()),
    )?;
    Ok(summary)
}

/// Store an uploaded package in the pool and republish its format.
fn ingest(
    st: &AppState,
    filename: &str,
    component: Option<String>,
    repo: Option<String>,
    body: Vec<u8>,
) -> Result<PushResult> {
    let _lock = crate::PublishLock::acquire(&st.root)?;
    let key = st.key.as_deref();
    let ext = Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    match ext {
        "deb" => {
            let comp = component.unwrap_or_else(|| st.cfg.apt.component.clone());
            let comp = client_scope_name(&comp, "apt component")?;
            let dir = st.cfg.checked_apt_pool_root(&st.root)?.join(comp);
            let dest = dir.join(filename);
            std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
            std::fs::write(&dest, &body).context("writing uploaded .deb")?;
            crate::hooks::run(
                &st.root,
                &st.cfg,
                crate::hooks::HookEvent::PrePublish,
                &crate::hooks::HookContext::new().with("ARX_FORMATS", "apt"),
            )?;
            let published = crate::publish_apt(
                &st.root,
                &st.cfg,
                key,
                &st.passphrase,
                st.cfg.apt.strict,
                true,
            )?;
            crate::hooks::run(
                &st.root,
                &st.cfg,
                crate::hooks::HookEvent::PostPublish,
                &crate::hooks::HookContext::new()
                    .with("ARX_FORMATS", "apt")
                    .with("ARX_SUMMARY", published.summary.clone()),
            )?;
            Ok(PushResult {
                stored: stored_path(&st.root, &dest),
                published: published.summary,
                skipped: published.skipped,
            })
        }
        "rpm" => {
            let repo = repo.unwrap_or_else(|| st.cfg.yum.repo.clone());
            let repo = client_scope_name(&repo, "yum repo")?;
            let yum = st.cfg.checked_yum_base(&st.root)?;
            std::fs::create_dir_all(&yum).with_context(|| format!("creating {}", yum.display()))?;
            let tmp = yum.join(format!(".incoming-{filename}"));
            std::fs::write(&tmp, &body).context("writing uploaded .rpm")?;
            let arch = {
                let mut r = crate::createrepo_rs::rpm::RpmReader::open(&tmp)
                    .context("opening uploaded .rpm")?;
                r.read_package().context("reading uploaded .rpm")?.arch
            };
            let arch = client_scope_name(&arch, "yum arch")?;
            let dir = yum.join(repo).join(arch);
            let dest = dir.join(filename);
            std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
            std::fs::rename(&tmp, &dest).context("moving uploaded .rpm")?;
            crate::hooks::run(
                &st.root,
                &st.cfg,
                crate::hooks::HookEvent::PrePublish,
                &crate::hooks::HookContext::new().with("ARX_FORMATS", "yum"),
            )?;
            let published = crate::publish_yum(&st.root, &st.cfg, key, &st.passphrase, true)?;
            crate::hooks::run(
                &st.root,
                &st.cfg,
                crate::hooks::HookEvent::PostPublish,
                &crate::hooks::HookContext::new()
                    .with("ARX_FORMATS", "yum")
                    .with("ARX_SUMMARY", published.clone()),
            )?;
            Ok(PushResult {
                stored: stored_path(&st.root, &dest),
                published,
                skipped: Vec::new(),
            })
        }
        other => bail!("unsupported package type .{other} (expected .deb or .rpm)"),
    }
}

/// Context the server needs to accept and publish writes.
pub struct PushContext {
    pub cfg: Config,
    pub key: Option<SignedSecretKey>,
    pub passphrase: String,
}

/// Optional exported public layouts mounted alongside the canonical repo root.
#[derive(Debug, Clone, Default)]
pub struct StaticMounts {
    /// Serve this exported apt layout at `/deb/*`.
    pub apt_live: Option<PathBuf>,
    /// Serve this exported flat yum layout at `/repo/*`.
    pub yum_flat_live: Option<PathBuf>,
}

/// Serve `root` over HTTP on `addr` until the process is signalled.
pub async fn serve(
    root: PathBuf,
    addr: String,
    metrics: PrometheusHandle,
    token: Option<String>,
    push: PushContext,
    mounts: StaticMounts,
) -> Result<()> {
    let authed = token.is_some();
    let state = AppState {
        metrics,
        token: token.map(Arc::from),
        root: root.clone(),
        cfg: Arc::new(push.cfg),
        key: push.key.map(Arc::new),
        passphrase: Arc::from(push.passphrase),
    };

    let serve_dir = ServeDir::new(&root).append_index_html_on_directories(false);
    let mut app = Router::new()
        .route("/metrics", get(metrics_handler))
        .route("/api/docs", get(api_docs_handler))
        .route("/api/openapi.yaml", get(openapi_handler))
        .route("/api/v1/health", get(health_handler))
        .route("/api/v1/packages", get(list_handler).post(upload_handler))
        .route("/api/v1/packages/:name", delete(delete_handler))
        .route("/api/v1/gc", post(gc_handler))
        .route("/api/v1/publish", post(publish_handler))
        .route(ROLLBACK_ROUTE, post(rollback_handler))
        .route(HISTORY_ROUTE, get(history_handler))
        .route("/api/v1/import", post(import_handler))
        .route("/api/v1/promote", post(promote_handler));

    if let Some(apt_live) = mounts.apt_live.as_ref() {
        app = app.nest_service(
            "/deb",
            ServeDir::new(apt_live).append_index_html_on_directories(false),
        );
    }
    if let Some(yum_flat_live) = mounts.yum_flat_live.as_ref() {
        app = app.nest_service(
            "/repo",
            ServeDir::new(yum_flat_live).append_index_html_on_directories(false),
        );
    }

    let app = app
        .fallback_service(serve_dir)
        .layer(middleware::from_fn(track_metrics))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            block_sensitive_static_paths,
        ))
        .layer(middleware::from_fn_with_state(state.clone(), require_auth))
        .layer(TraceLayer::new_for_http())
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods([
                    Method::GET,
                    Method::HEAD,
                    Method::POST,
                    Method::DELETE,
                    Method::OPTIONS,
                ])
                .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE]),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("binding {addr}"))?;
    tracing::info!(
        %addr,
        root = %root.display(),
        apt_live = mounts.apt_live.as_ref().map(|p| p.display().to_string()),
        yum_flat_live = mounts.yum_flat_live.as_ref().map(|p| p.display().to_string()),
        auth = authed,
        "arx serving repository"
    );

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("http server error")?;
    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutdown signal received");
}

#[cfg(test)]
mod tests {
    use super::{
        bad_request, client_target_link, err_response, promote_files, HISTORY_ROUTE, ROLLBACK_ROUTE,
    };
    use axum::{
        http::StatusCode,
        routing::{get, post},
        Router,
    };

    #[test]
    fn embedded_openapi_matches_reference_doc() {
        assert_eq!(
            super::OPENAPI_YAML,
            include_str!("../../../docs/reference/openapi.yaml")
        );
    }

    #[test]
    fn rollback_and_history_routes_use_axum_0_7_wildcards() {
        let _ = Router::<super::AppState>::new()
            .route(ROLLBACK_ROUTE, post(super::rollback_handler))
            .route(HISTORY_ROUTE, get(super::history_handler));
    }

    #[test]
    fn client_wrapped_scope_errors_are_bad_requests() {
        let err = anyhow::Error::from(crate::scope::InvalidScopeName::new_for_test(
            "apt component",
            "../escape",
        ));
        let err = bad_request(err);
        let response = err_response(&err);
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn raw_scope_errors_are_server_errors() {
        let err = anyhow::Error::from(crate::scope::InvalidScopeName::new_for_test(
            "apt pool dir",
            "../escape",
        ));
        let response = err_response(&err);
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn client_target_errors_are_bad_requests_but_bad_config_is_server_error() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let mut cfg = crate::config::Config::default();

        let err = client_target_link(root, &cfg, "../escape")
            .expect_err("path-like target should be client bad request");
        assert_eq!(err_response(&err).status(), StatusCode::BAD_REQUEST);

        cfg.yum.base_dir = "../escape".into();
        let err = client_target_link(root, &cfg, "repo/x86_64")
            .expect_err("bad server config should surface as internal error");
        assert_eq!(
            err_response(&err).status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[test]
    fn promote_files_preserves_yum_arch_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let payload = tmp.path().join("payload.sh");
        std::fs::write(
            &payload,
            b"#!/bin/sh
echo yumapi
",
        )
        .unwrap();
        let manifest = arx_pack::Manifest::from_toml_str(&format!(
            "name = \"yumapi\"
\
             version = \"1.0.0\"
\
             arch = \"x86_64\"
\
             maintainer = \"T <t@localhost>\"
\
             description = \"server promote fixture\"
\
             license = \"MIT\"
\
             [[files]]
\
             source = \"{}\"
\
             dest = \"/usr/bin/yumapi\"
\
             mode = \"0755\"
",
            payload.display()
        ))
        .unwrap();
        let built = arx_pack::build_rpm(&manifest, &tmp.path().join("dist")).unwrap();
        let base = tmp.path().join("yum");
        let src = base.join("staging/x86_64");
        std::fs::create_dir_all(&src).unwrap();
        let rpm_name = built.file_name().unwrap();
        std::fs::copy(&built, src.join(rpm_name)).unwrap();

        let moved = promote_files(&base, "staging", "prod", "yumapi", Some("1.0.0"), "rpm")
            .expect("yum promote should succeed");

        assert_eq!(moved, 1);
        assert!(base.join("prod/x86_64").join(rpm_name).exists());
        assert!(
            !base.join("prod").join(rpm_name).exists(),
            "server yum promote must not drop the arch directory"
        );
    }

    #[test]
    fn promote_files_rejects_path_like_scopes_before_walking() {
        let tmp = tempfile::tempdir().unwrap();
        let err = promote_files(tmp.path(), "../escape", "stable", "hello", None, "deb")
            .expect_err("unsafe source scope should fail");
        assert_eq!(err_response(&err).status(), StatusCode::BAD_REQUEST);
    }
}
