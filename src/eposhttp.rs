//! HTTP server: routes, CORS/PNA handling, request logging middleware.
//!
//! Routes:
//! - `GET /` -> `{"status":"ok"}`
//! - `OPTIONS /cgi-bin/epos/service.cgi` -> CORS preflight (204)
//! - `POST /cgi-bin/epos/service.cgi` -> translate body to ESC/POS,
//!   forward to winspool, return SOAP or bare XML success/error response.

use std::sync::Arc;
use std::time::Instant;

use axum::{
    body::{Body, Bytes},
    extract::{Request, State},
    http::{HeaderMap, Method, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, options, post},
    Json, Router,
};
use serde_json::json;
use tracing::{debug, info};

use crate::soap::{self, Format, Kind};
use crate::translate::{self, Options as TranslateOptions};
use crate::winspool;

/// Application configuration shared with the router.
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub printer_name: String,
    pub verbose: bool,
    pub allow_drawer: bool,
    pub strict_xml: bool,
}

/// Build the axum router with logging middleware.
pub fn router(cfg: AppConfig) -> Router {
    let state = Arc::new(cfg);
    Router::new()
        .route("/", get(health))
        .route(
            "/cgi-bin/epos/service.cgi",
            options(preflight).post(handle_epos),
        )
        .layer(middleware::from_fn_with_state(state.clone(), log_request))
        .with_state(state)
}

async fn health() -> impl IntoResponse {
    Json(json!({ "status": "ok" }))
}

async fn preflight(State(cfg): State<Arc<AppConfig>>, headers: HeaderMap) -> Response {
    let mut resp = (StatusCode::NO_CONTENT, "").into_response();
    apply_cors(&mut resp, &headers);
    resp
}

async fn handle_epos(
    State(cfg): State<Arc<AppConfig>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let format = soap::detect_format(&body);
    let opts = TranslateOptions {
        verbose: cfg.verbose,
        allow_drawer: cfg.allow_drawer,
        strict_xml: cfg.strict_xml,
    };

    if cfg.verbose {
        debug!(target: "epos", "xml rx | size={}", body.len());
    }

    let bytes = match translate::translate(&body, opts) {
        Ok(b) => b,
        Err(e) => {
            tracing::error!(target: "epos", "translate failed | err={} body_size={}", e, body.len());
            return make_response(format, Kind::Error, &e.to_string());
        }
    };

    if !bytes.is_empty() {
        if let Err(e) = print_bytes(&cfg.printer_name, &bytes).await {
            tracing::error!(target: "epos", "print failed | err={}", e);
            return make_response(format, Kind::Error, &format!("Printer error: {e}"));
        }
        info!(target: "epos", "printed | printer={} bytes={} format={}", cfg.printer_name, bytes.len(), format);
    }

    make_response(format, Kind::Success, "")
}

async fn print_bytes(printer_name: &str, bytes: &[u8]) -> anyhow::Result<()> {
    let name = printer_name.to_string();
    let data = bytes.to_vec();
    // Run the synchronous winspool call on a blocking thread.
    tokio::task::spawn_blocking(move || winspool::print_raw(&name, "ePOS Emulator", &data))
        .await
        .map_err(|e| anyhow::anyhow!("join error: {e}"))??;
    Ok(())
}

fn make_response(format: Format, kind: Kind, code: &str) -> Response {
    let body = soap::render(format, kind, code);
    let mut resp = Response::new(Body::from(body));
    resp.headers_mut().insert(
        "Content-Type",
        axum::http::HeaderValue::from_static("text/xml; charset=utf-8"),
    );
    apply_cors_headers(resp.headers_mut(), None);
    resp
}

fn apply_cors(resp: &mut Response, headers: &HeaderMap) {
    apply_cors_headers(resp.headers_mut(), Some(headers));
}

fn apply_cors_headers(out: &mut HeaderMap, in_req: Option<&HeaderMap>) {
    out.insert("Access-Control-Allow-Origin", axum::http::HeaderValue::from_static("*"));
    out.insert(
        "Access-Control-Allow-Methods",
        axum::http::HeaderValue::from_static("POST, OPTIONS"),
    );
    out.insert(
        "Access-Control-Allow-Headers",
        axum::http::HeaderValue::from_static("Content-Type, SOAPAction"),
    );
    if let Some(req) = in_req {
        if req.get("Access-Control-Request-Private-Network").map(|v| v == "true").unwrap_or(false) {
            out.insert(
                "Access-Control-Allow-Private-Network",
                axum::http::HeaderValue::from_static("true"),
            );
        }
    }
}

async fn log_request(
    State(cfg): State<Arc<AppConfig>>,
    req: Request,
    next: Next,
) -> Response {
    let start = Instant::now();
    let id = random_id();
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let query = req.uri().query().unwrap_or("").to_string();
    let remote = req
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .or_else(|| req.extensions().get::<std::net::SocketAddr>().map(|s| s.to_string()))
        .unwrap_or_default();

    let mut resp = next.run(req).await;
    let dur_ms = start.elapsed().as_millis() as u64;
    let status = resp.status().as_u16();
    let ua = resp
        .headers()
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    resp.headers_mut().insert(
        "X-Request-ID",
        axum::http::HeaderValue::from_str(&id).unwrap_or(axum::http::HeaderValue::from_static("0")),
    );
    info!(
        target: "http",
        "request | id={} method={} path={} query={} remote={} status={} bytes=? dur_ms={} ua={}",
        id, method, path, query, remote, status, dur_ms, ua,
    );
    let _ = cfg; // keep state alive for the duration of the request
    resp
}

fn random_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("{:x}", nanos & 0xFFFF_FFFF)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::http::{header, Request as HttpRequest};
    use tower::ServiceExt;

    fn cfg() -> AppConfig {
        AppConfig {
            printer_name: String::new(),
            verbose: false,
            allow_drawer: false,
            strict_xml: false,
        }
    }

    #[tokio::test]
    async fn health_responds_json() {
        let app = router(cfg());
        let req = HttpRequest::builder().method(Method::GET).uri("/").body(Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), 1024).await.unwrap();
        let s = std::str::from_utf8(&bytes).unwrap();
        assert!(s.contains("\"status\""));
        assert!(s.contains("\"ok\""));
    }

    #[tokio::test]
    async fn preflight_sets_cors_headers() {
        let app = router(cfg());
        let req = HttpRequest::builder()
            .method(Method::OPTIONS)
            .uri("/cgi-bin/epos/service.cgi")
            .header(header::ORIGIN, "https://example.com")
            .header("Access-Control-Request-Private-Network", "true")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        assert!(resp.headers().contains_key("Access-Control-Allow-Origin"));
        assert!(resp.headers().contains_key("Access-Control-Allow-Private-Network"));
    }
}
