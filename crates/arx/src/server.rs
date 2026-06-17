//! Built-in static HTTP server (axum + tower-http) exposing the repository
//! tree directly to `apt`/`dnf`, plus a Prometheus `/metrics` endpoint.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use metrics_exporter_prometheus::PrometheusHandle;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

/// Render the Prometheus exposition text for `GET /metrics`.
async fn metrics_handler(State(handle): State<PrometheusHandle>) -> String {
    handle.render()
}

/// Optional bearer-token gate. When a token is configured, every request must
/// carry `Authorization: Bearer <token>`; otherwise the server is public
/// (keeping the zero-config quickstart frictionless — TLS belongs at a proxy).
async fn require_auth(
    State(token): State<Option<Arc<str>>>,
    req: Request,
    next: Next,
) -> Response {
    if let Some(token) = token.as_deref() {
        let presented = req
            .headers()
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok());
        let expected = format!("Bearer {token}");
        if presented != Some(expected.as_str()) {
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

/// Count every served request.
async fn track_metrics(req: Request, next: Next) -> Response {
    let path = req.uri().path().to_string();
    let response = next.run(req).await;
    metrics::counter!("arx_http_requests_total").increment(1);
    metrics::counter!("arx_http_responses_total", "status" => response.status().as_u16().to_string())
        .increment(1);
    tracing::debug!(path, status = %response.status(), "served request");
    response.into_response()
}

/// Serve `root` over HTTP on `addr` until the process is signalled.
///
/// When `token` is `Some`, requests must present it as a bearer token.
/// `ServeDir` itself rejects path traversal (`..`), so static serving is safe.
pub async fn serve(
    root: PathBuf,
    addr: String,
    metrics: PrometheusHandle,
    token: Option<String>,
) -> Result<()> {
    let serve_dir = ServeDir::new(&root).append_index_html_on_directories(false);
    let token: Option<Arc<str>> = token.map(Arc::from);
    let authed = token.is_some();

    let app = Router::new()
        .route("/metrics", get(metrics_handler))
        .with_state(metrics)
        .fallback_service(serve_dir)
        .layer(middleware::from_fn(track_metrics))
        .layer(middleware::from_fn_with_state(token, require_auth))
        .layer(TraceLayer::new_for_http());

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
