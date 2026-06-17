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
//!
//! All write operations require a configured `ARX_SERVE_TOKEN` (bearer auth).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use axum::{
    body::Bytes,
    extract::{Path as AxPath, Query, Request, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use metrics_exporter_prometheus::PrometheusHandle;
use pgp::composed::SignedSecretKey;
use serde::{Deserialize, Serialize};
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

use crate::config::Config;
use crate::pool;

/// Shared server state. Cloneable (cheap — everything behind `Arc`/handle).
#[derive(Clone)]
struct AppState {
    metrics: PrometheusHandle,
    /// Bearer token; `None` = public reads, writes disabled.
    token: Option<Arc<str>>,
    root: PathBuf,
    cfg: Arc<Config>,
    key: Option<Arc<SignedSecretKey>>,
    passphrase: Arc<str>,
}

impl AppState {
    /// Writes require a configured token; returns a 403 response otherwise.
    fn write_forbidden(&self) -> Option<Response> {
        if self.token.is_none() {
            Some(
                (
                    StatusCode::FORBIDDEN,
                    "writes disabled: set ARX_SERVE_TOKEN on the server\n",
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

/// Optional bearer-token gate over every route. Unset token = public reads.
async fn require_auth(State(st): State<AppState>, req: Request, next: Next) -> Response {
    if let Some(token) = st.token.as_deref() {
        let presented = req
            .headers()
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok());
        if presented != Some(format!("Bearer {token}").as_str()) {
            return (
                StatusCode::UNAUTHORIZED,
                [(axum::http::header::WWW_AUTHENTICATE, "Bearer")],
                "unauthorized\n",
            )
                .into_response();
        }
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

async fn list_handler(State(st): State<AppState>) -> Response {
    match pool::list(&st.root, false, false) {
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
        let removed = pool::remove(&st.root, &name, q.version.as_deref(), q.apt, q.yum)?;
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
    #[serde(default = "default_keep")]
    keep: usize,
    #[serde(default)]
    dry_run: bool,
    #[serde(default)]
    apt: bool,
    #[serde(default)]
    yum: bool,
}

#[derive(Serialize)]
struct GcResult {
    pruned: Vec<pool::PackageInfo>,
    dry_run: bool,
    published: Option<String>,
}

async fn gc_handler(State(st): State<AppState>, Query(q): Query<GcQuery>) -> Response {
    if let Some(resp) = st.write_forbidden() {
        return resp;
    }
    let blocking = move || -> Result<GcResult> {
        let _lock = crate::PublishLock::acquire(&st.root)?;
        let report = pool::gc(&st.root, q.keep, q.apt, q.yum, q.dry_run)?;
        let pruned = report.pruned.iter().map(pool::Entry::info).collect();
        let published = if report.dry_run || report.pruned.is_empty() {
            None
        } else {
            Some(publish_both(&st)?)
        };
        Ok(GcResult {
            pruned,
            dry_run: report.dry_run,
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
            return (StatusCode::BAD_REQUEST, "missing or unsafe X-Arx-Filename header\n")
                .into_response()
        }
    };
    let component = header(&headers, "x-arx-component").map(str::to_string);
    let repo = header(&headers, "x-arx-repo").map(str::to_string);
    let body = body.to_vec();
    let blocking = move || ingest(&st, &filename, component, repo, body);
    run_blocking(blocking).await
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

/// Republish both formats (caller already holds the publish lock). Strict mode
/// (a skipped package → error) is governed by the server's `[apt].strict`.
fn publish_both(st: &AppState) -> Result<String> {
    let key = st.key.as_deref();
    let apt = crate::publish_apt(&st.root, &st.cfg, key, &st.passphrase, st.cfg.apt.strict, true)?;
    let yum = crate::publish_yum(&st.root, key, &st.passphrase, true)?;
    Ok(format!("{}; {yum}", apt.summary))
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
            let dir = st.root.join("apt/pool").join(&comp);
            std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
            std::fs::write(dir.join(filename), &body).context("writing uploaded .deb")?;
            let published =
                crate::publish_apt(&st.root, &st.cfg, key, &st.passphrase, st.cfg.apt.strict, true)?;
            Ok(PushResult {
                stored: format!("apt/{comp}/{filename}"),
                published: published.summary,
                skipped: published.skipped,
            })
        }
        "rpm" => {
            let repo = repo.unwrap_or_else(|| st.cfg.yum.repo.clone());
            let yum = st.root.join("yum");
            std::fs::create_dir_all(&yum).with_context(|| format!("creating {}", yum.display()))?;
            let tmp = yum.join(format!(".incoming-{filename}"));
            std::fs::write(&tmp, &body).context("writing uploaded .rpm")?;
            let arch = {
                let mut r = createrepo_rs::rpm::RpmReader::open(&tmp)
                    .context("opening uploaded .rpm")?;
                r.read_package().context("reading uploaded .rpm")?.arch
            };
            let dir = yum.join(&repo).join(&arch);
            std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
            std::fs::rename(&tmp, dir.join(filename)).context("moving uploaded .rpm")?;
            let published = crate::publish_yum(&st.root, key, &st.passphrase, true)?;
            Ok(PushResult {
                stored: format!("yum/{repo}/{arch}/{filename}"),
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

/// Serve `root` over HTTP on `addr` until the process is signalled.
pub async fn serve(
    root: PathBuf,
    addr: String,
    metrics: PrometheusHandle,
    token: Option<String>,
    push: PushContext,
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
    let app = Router::new()
        .route("/metrics", get(metrics_handler))
        .route("/api/v1/health", get(health_handler))
        .route("/api/v1/packages", get(list_handler).post(upload_handler))
        .route("/api/v1/packages/:name", delete(delete_handler))
        .route("/api/v1/gc", post(gc_handler))
        .fallback_service(serve_dir)
        .layer(middleware::from_fn(track_metrics))
        .layer(middleware::from_fn_with_state(state.clone(), require_auth))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("binding {addr}"))?;
    tracing::info!(%addr, root = %root.display(), auth = authed, "arx serving repository");

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
