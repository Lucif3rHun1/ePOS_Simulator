//! HTTP server: routes, CORS/PNA handling, request logging middleware, and
//! bulk-request hardening (body size limit, per-request timeout, bounded
//! spooler concurrency, idempotency dedup, transient-error retry).

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    body::{Body, Bytes},
    extract::{Request, State},
    http::{HeaderMap, Method, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, options, post},
    Json, Router,
};
use parking_lot::Mutex;
use serde_json::json;
use tokio::sync::Semaphore;
use tracing::{debug, info, warn};

use crate::soap::{self, Format, Kind};
use crate::translate::{self, Options as TranslateOptions};
use crate::winspool;

pub const DEFAULT_MAX_BODY_BYTES: usize = 1024 * 1024;
pub const DEFAULT_REQUEST_TIMEOUT_MS: u64 = 30_000;
pub const DEFAULT_MAX_INFLIGHT_PRINTS: usize = 16;
pub const DEFAULT_IDEMPOTENCY_WINDOW: usize = 1024;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub printer_name: String,
    pub verbose: bool,
    pub allow_drawer: bool,
    pub strict_xml: bool,
    pub max_body_bytes: usize,
    pub request_timeout: Duration,
    pub max_inflight_prints: usize,
    pub idempotency_window: usize,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            printer_name: String::new(),
            verbose: false,
            allow_drawer: false,
            strict_xml: false,
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
            request_timeout: Duration::from_millis(DEFAULT_REQUEST_TIMEOUT_MS),
            max_inflight_prints: DEFAULT_MAX_INFLIGHT_PRINTS,
            idempotency_window: DEFAULT_IDEMPOTENCY_WINDOW,
        }
    }
}

#[derive(Clone)]
struct SharedState {
    cfg: AppConfig,
    print_slots: Arc<Semaphore>,
    idempotency: Arc<Mutex<IdempotencyCache>>,
}

struct IdempotencyCache {
    window: usize,
    entries: VecDeque<(u64, Vec<u8>)>,
}

impl IdempotencyCache {
    fn new(window: usize) -> Self {
        Self { window, entries: VecDeque::with_capacity(window) }
    }
    fn lookup(&mut self, fp: &u64) -> Option<Vec<u8>> {
        let resp = self.entries.iter().find(|(k, _)| k == fp).map(|(_, v)| v.clone());
        if resp.is_some() {
            // bubble the matched entry to MRU back without disturbing its payload
            if let Some(pos) = self.entries.iter().position(|(k, _)| k == fp) {
                if let Some(item) = self.entries.remove(pos) {
                    self.entries.push_back(item);
                }
            }
        }
        resp
    }
    fn store(&mut self, fp: u64, resp: Vec<u8>) {
        if self.entries.len() >= self.window {
            self.entries.pop_front();
        }
        self.entries.push_back((fp, resp));
    }
}

pub fn router(cfg: AppConfig) -> Router {
    let state = SharedState {
        cfg: cfg.clone(),
        print_slots: Arc::new(Semaphore::new(cfg.max_inflight_prints.max(1))),
        idempotency: Arc::new(Mutex::new(IdempotencyCache::new(cfg.idempotency_window.max(1)))),
    };
    let shared = Arc::new(state);
    Router::new()
        .route("/", get(health).options(preflight_health))
        .route(
            "/cgi-bin/epos/service.cgi",
            options(preflight).post(handle_epos),
        )
        .layer(middleware::from_fn_with_state(shared.clone(), log_request))
        .with_state(shared)
}

async fn health(State(shared): State<Arc<SharedState>>) -> impl IntoResponse {
    let mut resp = Json(json!({ "status": "ok" })).into_response();
    apply_cors_headers(resp.headers_mut(), None);
    resp
}

async fn preflight_health(State(_shared): State<Arc<SharedState>>, headers: HeaderMap) -> Response {
    let mut resp = (StatusCode::NO_CONTENT, "").into_response();
    apply_cors(resp.headers_mut(), &headers);
    resp
}

async fn preflight(State(_shared): State<Arc<SharedState>>, headers: HeaderMap) -> Response {
    let mut resp = (StatusCode::NO_CONTENT, "").into_response();
    apply_cors(resp.headers_mut(), &headers);
    resp
}

async fn handle_epos(
    State(shared): State<Arc<SharedState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let cfg = shared.cfg.clone();
    let _ = headers;

    if body.len() > cfg.max_body_bytes {
        warn!(target: "epos", "body too large | size={} max={}", body.len(), cfg.max_body_bytes);
        return make_response_with_cors(
            Format::detect(&body),
            Kind::Error,
            &format!("Request body too large ({} bytes > {})", body.len(), cfg.max_body_bytes),
        );
    }

    let fp = fnv1a_64(&body);
    if let Some(cached) = shared.idempotency.lock().lookup(&fp) {
        info!(target: "epos", "dedup hit | fp={:016x} size={}", fp, body.len());
        return cached_response(cached);
    }

    let work = do_print(shared.clone(), body.clone(), fp);
    let format = Format::detect(&body);
    let resp_bytes = match tokio::time::timeout(cfg.request_timeout, work).await {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(e)) => {
            tracing::error!(target: "epos", "print failed | err={}", e);
            soap::render(format, Kind::Error, &format!("Printer error: {e}")).into()
        }
        Err(_) => {
            tracing::error!(target: "epos", "request timed out | timeout_ms={}", cfg.request_timeout.as_millis());
            soap::render(format, Kind::Error, "Request timed out waiting for printer").into()
        }
    };

    // Cache the response so retries (browser auto-retry, idempotency-key header)
    // don't double-print and don't re-hit the spooler.
    shared.idempotency.lock().store(fp, resp_bytes.clone());

    let mut resp = Response::new(Body::from(resp_bytes));
    resp.headers_mut().insert(
        "Content-Type",
        axum::http::HeaderValue::from_static("text/xml; charset=utf-8"),
    );
    apply_cors_headers(resp.headers_mut(), None);
    resp
}

async fn do_print(shared: Arc<SharedState>, body: Bytes, fp: u64) -> anyhow::Result<Vec<u8>> {
    let cfg = shared.cfg.clone();
    let format = Format::detect(&body);
    let opts = TranslateOptions {
        verbose: cfg.verbose,
        allow_drawer: cfg.allow_drawer,
        strict_xml: cfg.strict_xml,
    };
    if cfg.verbose {
        debug!(target: "epos", "xml rx | size={}", body.len());
    }

    let bytes = translate::translate(&body, opts)
        .map_err(|e| anyhow::anyhow!("translate: {e}"))?;

    if !bytes.is_empty() {
        let permit = shared.print_slots.clone().acquire_owned().await
            .map_err(|e| anyhow::anyhow!("semaphore closed: {e}"))?;
        let name = cfg.printer_name.clone();
        let data = bytes.to_vec();
        let _ = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            print_with_retry(&name, &data, 2)
        })
        .await
        .map_err(|e| anyhow::anyhow!("join: {e}"))??;
        info!(target: "epos", "printed | printer={} bytes={} format={} fp={:016x}", cfg.printer_name, bytes.len(), format, fp);
    }

    Ok(soap::render(format, Kind::Success, "").into())
}

fn print_with_retry(name: &str, data: &[u8], max_retries: u32) -> anyhow::Result<()> {
    let mut attempt = 0;
    loop {
        match winspool::print_raw(name, "ePOS Emulator", data) {
            Ok(()) => return Ok(()),
            Err(e) if attempt < max_retries => {
                let backoff = Duration::from_millis(50 * (1u64 << attempt));
                warn!(target: "epos", "spooler retry | attempt={} err={} backoff_ms={}", attempt + 1, e, backoff.as_millis());
                std::thread::sleep(backoff);
                attempt += 1;
            }
            Err(e) => return Err(e),
        }
    }
}

fn cached_response(cached: Vec<u8>) -> Response {
    let mut resp = Response::new(Body::from(cached));
    resp.headers_mut().insert(
        "Content-Type",
        axum::http::HeaderValue::from_static("text/xml; charset=utf-8"),
    );
    resp.headers_mut().insert("X-Idempotency-Replay", axum::http::HeaderValue::from_static("true"));
    apply_cors_headers(resp.headers_mut(), None);
    resp
}

fn make_response_with_cors(format: Format, kind: Kind, code: &str) -> Response {
    let body = soap::render(format, kind, code);
    let mut resp = Response::new(Body::from(body));
    resp.headers_mut().insert(
        "Content-Type",
        axum::http::HeaderValue::from_static("text/xml; charset=utf-8"),
    );
    apply_cors_headers(resp.headers_mut(), None);
    resp
}

fn apply_cors(out: &mut HeaderMap, headers: &HeaderMap) {
    apply_cors_headers(out, Some(headers));
}

fn apply_cors_headers(out: &mut HeaderMap, in_req: Option<&HeaderMap>) {
    out.insert("Access-Control-Allow-Origin", axum::http::HeaderValue::from_static("*"));
    out.insert(
        "Access-Control-Allow-Methods",
        axum::http::HeaderValue::from_static("GET, POST, OPTIONS"),
    );
    out.insert(
        "Access-Control-Allow-Headers",
        axum::http::HeaderValue::from_static("Content-Type, SOAPAction, X-Request-ID, Idempotency-Key"),
    );
    out.insert(
        "Access-Control-Max-Age",
        axum::http::HeaderValue::from_static("86400"),
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
    State(shared): State<Arc<SharedState>>,
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
    let ua = req
        .headers()
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let idem = req
        .headers()
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let mut resp = next.run(req).await;
    let dur_ms = start.elapsed().as_millis() as u64;
    let status = resp.status().as_u16();
    resp.headers_mut().insert(
        "X-Request-ID",
        axum::http::HeaderValue::from_str(&id).unwrap_or(axum::http::HeaderValue::from_static("0")),
    );
    info!(
        target: "http",
        "request | id={} method={} path={} query={} remote={} status={} dur_ms={} ua={} idem={}",
        id, method, path, query, remote, status, dur_ms, ua, idem,
    );
    let _ = shared;
    resp
}

fn random_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("{:x}", nanos & 0xFFFF_FFFFFFFF)
}

fn fnv1a_64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

trait FormatDetect {
    fn detect(body: &[u8]) -> Self;
}
impl FormatDetect for Format {
    fn detect(body: &[u8]) -> Self {
        soap::detect_format(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::http::{header, Request as HttpRequest};
    use tower::ServiceExt;

    fn cfg() -> AppConfig {
        AppConfig::default()
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
    async fn health_responds_to_options_with_cors() {
        let app = router(cfg());
        let req = HttpRequest::builder()
            .method(Method::OPTIONS)
            .uri("/")
            .header(header::ORIGIN, "https://example.com")
            .header("Access-Control-Request-Private-Network", "true")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        assert!(resp.headers().contains_key("Access-Control-Allow-Origin"));
        assert!(resp.headers().contains_key("Access-Control-Allow-Private-Network"));
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
        assert!(resp.headers().contains_key("Access-Control-Max-Age"));
    }

    #[tokio::test]
    async fn oversized_body_is_rejected() {
        let mut big = vec![b'a'; DEFAULT_MAX_BODY_BYTES + 1];
        big.extend_from_slice(b"</epos-print>");
        let app = router(cfg());
        let req = HttpRequest::builder()
            .method(Method::POST)
            .uri("/cgi-bin/epos/service.cgi")
            .body(Body::from(big))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), 4096).await.unwrap();
        let s = std::str::from_utf8(&body).unwrap();
        assert!(s.contains("too large") || s.contains("Request body"));
    }

    #[tokio::test]
    async fn duplicate_body_is_deduped() {
        let app = router(cfg());
        let bad = b"<<<garbage>>>";
        let req1 = HttpRequest::builder()
            .method(Method::POST)
            .uri("/cgi-bin/epos/service.cgi")
            .body(Body::from(bad.to_vec()))
            .unwrap();
        let resp1 = app.clone().oneshot(req1).await.unwrap();
        let body1 = to_bytes(resp1.into_body(), 4096).await.unwrap().to_vec();
        let req2 = HttpRequest::builder()
            .method(Method::POST)
            .uri("/cgi-bin/epos/service.cgi")
            .body(Body::from(bad.to_vec()))
            .unwrap();
        let resp2 = app.oneshot(req2).await.unwrap();
        assert_eq!(
            resp2.headers().get("X-Idempotency-Replay").map(|v| v.to_str().unwrap()),
            Some("true")
        );
        let body2 = to_bytes(resp2.into_body(), 4096).await.unwrap().to_vec();
        assert_eq!(body1, body2);
    }

    #[test]
    fn fnv1a_is_deterministic() {
        let a = fnv1a_64(b"hello world");
        let b = fnv1a_64(b"hello world");
        let c = fnv1a_64(b"hello worlD");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn idempotency_cache_evicts_oldest() {
        let mut c = IdempotencyCache::new(3);
        c.store(1, b"a".to_vec());
        c.store(2, b"b".to_vec());
        c.store(3, b"c".to_vec());
        c.store(4, b"d".to_vec());
        assert!(c.lookup(&1).is_none());
        assert!(c.lookup(&4).is_some());
        assert!(c.lookup(&2).is_some());
        assert!(c.lookup(&3).is_some());
    }
}
