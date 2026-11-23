//! ページの置き場。担当が置いたページ（HTML）と、ページから参照する file を DB に持ち、絞って探せる形で返す。
//! 消すこともできる（戻せない）。
//! 種類と役割の名の一覧は持たない（cli が持つ）。ここでは字の形だけを見る。

pub mod api;
pub mod castlist;
pub mod db;
pub mod events;
pub mod files;
pub mod pages;
pub mod runlog;
pub mod text;
pub mod ui;

use axum::extract::{DefaultBodyLimit, Request};
use axum::http::{HeaderName, HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::Level;

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
    /// 送り手を聞く castlist（`PAGETRAY_CASTLIST_URL`）と、画面の link の頭（`PAGETRAY_CASTLIST_LINK_URL`）。
    /// 無ければ、付随する id だけを残す。
    pub castlist: Option<castlist::Castlist>,
    /// 記録の置き場（`GET /api/logs` が読む）。無ければ、その道は 404。
    pub runlog: Option<runlog::RunLog>,
}

impl AppState {
    pub fn new(db: db::Db) -> Self {
        let (events, _) = tokio::sync::broadcast::channel(64);
        Self {
            db,
            events,
            castlist: None,
            runlog: None,
        }
    }

    /// 記録を、この置き場から読む。
    pub fn with_runlog(self, runlog: runlog::RunLog) -> Self {
        Self {
            runlog: Some(runlog),
            ..self
        }
    }

    /// 送り手を、この castlist に聞く。
    pub fn with_castlist(self, castlist: castlist::Castlist) -> Self {
        Self {
            castlist: Some(castlist),
            ..self
        }
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(|| async { "ok\n" }))
        .route(
            "/api/pages",
            get(api::list).post(api::put).delete(api::remove),
        )
        .route("/api/pages/reply", post(api::reply))
        .route("/api/pages/review", post(api::review))
        .route(
            "/api/files",
            // file の上限は、置く道にだけ掛ける（下の MAX_BODY より内側なので、こちらが効く）。
            post(api::put_file)
                .layer(DefaultBodyLimit::max(MAX_FILE))
                .get(api::list_files)
                .delete(api::remove_files),
        )
        .route("/api/backup", get(api::backup))
        .route("/api/logs", get(api::logs))
        .route("/api/version", get(version))
        .route("/api/events", get(events::stream))
        .route("/", get(ui::index))
        .route("/{project}/", get(ui::project))
        .route("/{project}/{key}/", get(ui::key))
        .route("/{project}/{key}/{file}", get(pages::show))
        .route("/{project}/{key}/files/{file}", get(files::show))
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .layer(middleware::from_fn(guard))
        // 問い合わせごとの 1 行（記録の file にだけ書く。src/runlog.rs の init）。guard が断った物も残る。
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
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

/// 一覧の UI の form から来た要求の印。guard が付ける。付いた要求は、役割と workspace を置き場が決める。
#[derive(Clone, Copy, Debug)]
pub struct FromUi;

/// 一覧の UI から上げた file の役割。
pub const UI_ROLE: &str = "product-manager";

/// この Mac の中からの要求だけを受ける。
/// - `Host` が 127.0.0.1・localhost でなければ断る（外の site が名前を付け替えて届ける要求を防ぐ）。
/// - 書く要求に `Origin` が付いていれば断る（browser の中のページからは置かせない。`curl` は付けない）。
///   例外は 1 つだけ: 一覧の UI の form が file を上げる要求（[`from_ui`]）。印 [`FromUi`] を付けて通す。
async fn guard(mut request: Request, next: Next) -> Response {
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
        if !from_ui(&request) {
            return error(StatusCode::FORBIDDEN, "browser の中のページからは置けない");
        }
        request.extensions_mut().insert(FromUi);
    }
    next.run(request).await
}

/// 一覧の UI の form が file を上げる要求か: `POST /api/files` で、`Origin` が `http://<Host（port も）>` と同じで、
/// `Sec-Fetch-Site` が無いか `same-origin`。本文の枠の中のページ（`Origin: null`）や、ほかの site からは当たらない。
fn from_ui(request: &Request) -> bool {
    let headers = request.headers();
    let text = |name| {
        headers
            .get(name)
            .and_then(|v: &HeaderValue| v.to_str().ok())
    };
    let same_origin = match (text(header::ORIGIN), text(header::HOST)) {
        (Some(origin), Some(host)) => origin.strip_prefix("http://") == Some(host),
        _ => false,
    };
    request.method() == Method::POST
        && request.uri().path() == "/api/files"
        && same_origin
        && matches!(
            text(HeaderName::from_static("sec-fetch-site")),
            None | Some("same-origin")
        )
}

pub fn error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({ "error": message }))).into_response()
}

pub(crate) fn internal(e: impl std::fmt::Display) -> Response {
    tracing::error!("error: {e}");
    error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "サーバーの中で失敗した（log を見る）",
    )
}
