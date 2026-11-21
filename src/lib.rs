//! ページの置き場。担当が置いたページ（HTML）と、ページから参照する file を DB に持ち、絞って探せる形で返す。
//! 消すこともできる（戻せない）。
//! 種類と役割の名の一覧は持たない（cli が持つ）。ここでは字の形だけを見る。

pub mod api;
pub mod db;
pub mod events;
pub mod files;
pub mod pages;
pub mod text;
pub mod ui;

use axum::extract::{DefaultBodyLimit, Request};
use axum::http::{HeaderName, HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;

/// API の版（決まりは docs/src/api.md。値は、CLI と共に使う crates/api にある）。
pub use pagetray_api::{API_VERSION, MAX_FILE};
/// どの答えにも付く、API の版の header。
pub const API_HEADER: HeaderName = HeaderName::from_static(pagetray_api::API_HEADER);

/// 1 枚のページの上限。
pub const MAX_BODY: usize = 16 * 1024 * 1024;

#[derive(Clone)]
pub struct AppState {
    pub db: db::Db,
    /// 置かれたページと file の知らせ。聞いている一覧の UI に push する。
    pub events: tokio::sync::broadcast::Sender<events::Note>,
}

impl AppState {
    pub fn new(db: db::Db) -> Self {
        let (events, _) = tokio::sync::broadcast::channel(64);
        Self { db, events }
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(|| async { "ok\n" }))
        .route(
            "/api/pages",
            get(api::list).post(api::put).delete(api::remove),
        )
        .route(
            "/api/files",
            // file の上限は、置く道にだけ掛ける（下の MAX_BODY より内側なので、こちらが効く）。
            post(api::put_file)
                .layer(DefaultBodyLimit::max(MAX_FILE))
                .get(api::list_files)
                .delete(api::remove_files),
        )
        .route("/api/backup", get(api::backup))
        .route("/api/version", get(version))
        .route("/api/events", get(events::stream))
        .route("/", get(ui::index))
        .route("/{project}/", get(ui::project))
        .route("/{project}/{key}/", get(ui::key))
        .route("/{project}/{key}/{file}", get(pages::show))
        .route("/{project}/{key}/files/{file}", get(files::show))
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .layer(middleware::from_fn(guard))
        // 一番外に置く。guard が断った答えにも付く。
        .layer(middleware::from_fn(stamp))
        .with_state(state)
}

/// サーバーの版と API の版（`{"version":"<版>","api":1}`）。
async fn version() -> Json<serde_json::Value> {
    Json(json!({ "version": env!("CARGO_PKG_VERSION"), "api": API_VERSION }))
}

/// どの答えにも API の版を付ける。呼ぶ側は、頼んだその答えで、合っているかを見られる。
async fn stamp(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(API_HEADER, HeaderValue::from(API_VERSION));
    response
}

/// この Mac の中からの要求だけを受ける。
/// - `Host` が 127.0.0.1・localhost でなければ断る（外の site が名前を付け替えて届ける要求を防ぐ）。
/// - 書く要求に `Origin` が付いていれば断る（browser の中のページからは置かせない。`curl` は付けない）。
async fn guard(request: Request, next: Next) -> Response {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.rsplit_once(':').map_or(v, |(name, _port)| name));
    if !matches!(host, Some("127.0.0.1" | "localhost")) {
        return error(
            StatusCode::FORBIDDEN,
            "Host は 127.0.0.1 か localhost だけを受ける",
        );
    }
    if request.method() != Method::GET
        && request.method() != Method::HEAD
        && request.headers().contains_key(header::ORIGIN)
    {
        return error(StatusCode::FORBIDDEN, "browser の中のページからは置けない");
    }
    next.run(request).await
}

pub fn error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({ "error": message }))).into_response()
}

pub(crate) fn internal(e: impl std::fmt::Display) -> Response {
    eprintln!("error: {e}");
    error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "サーバーの中で失敗した（log を見る）",
    )
}
