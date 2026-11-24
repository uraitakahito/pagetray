//! 試験の足場: 試験ごとに一時の DB を作り、Router に要求を通す。DB の決まりは、書く接続と読む接続を直に使って見る。
#![allow(dead_code)]

pub mod castlist;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode, header};
use http_body_util::BodyExt;
use pagetray_next::db::Db;
use serde_json::Value;
use tower::ServiceExt;

/// 試験の要求の Host。
pub const HOST: &str = "127.0.0.1:8801";

/// 一時の DB の path（試験ごとに別の、空の folder の中）。
pub fn temp_db(tag: &str) -> PathBuf {
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "pagetray-next-test-{tag}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("試験の folder");
    dir.join("pages.db")
}

/// DB の file と WAL の file を、folder ごと消す（temp_db が作った folder だけ）。
pub fn remove_db(path: &Path) {
    let Some(dir) = path.parent() else { return };
    let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
    assert!(
        name.starts_with("pagetray-next-test-"),
        "temp_db の folder ではない: {}",
        dir.display()
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// 一時の DB と、それを使う Router。
pub struct App {
    pub app: Router,
    pub db: Arc<Db>,
    pub path: PathBuf,
}

impl Drop for App {
    fn drop(&mut self) {
        remove_db(&self.path);
    }
}

impl App {
    pub async fn start() -> Self {
        Self::start_with(pagetray_next::Config::default()).await
    }

    /// 守りの層の設定（時間切れの長さ）を決めて起こす。
    pub async fn start_with(config: pagetray_next::Config) -> Self {
        let path = temp_db("app");
        let db = Arc::new(Db::open(&path).await.expect("Db::open"));
        let app = pagetray_next::app_with(db.clone(), config);
        Self { app, db, path }
    }

    /// 送り手を `url`（`PAGETRAY_CASTLIST_URL` の値）の castlist に聞いて起こす。
    pub async fn with_castlist(url: &str) -> Self {
        let castlist = pagetray_next::castlist::Castlist::configure(Some(url), None)
            .expect("castlist の URL")
            .expect("castlist がある");
        Self::start_with(pagetray_next::Config {
            castlist: Some(castlist),
            ..Default::default()
        })
        .await
    }

    /// 送り手の行（id の順。付随する id を出した物・付随する id・castlist のアクターの id・名・種類・答えた時刻）。
    pub async fn senders(&self) -> Vec<Sender> {
        sqlx::query_as(
            "select actor_issuer, actor_ref, actor_id, actor_name, actor_kind, resolved_at
             from senders order by id",
        )
        .fetch_all(self.db.read())
        .await
        .expect("senders")
    }

    /// 要求を 1 つ通し、状態と header と本文の byte を返す。
    pub async fn send(&self, request: Request<Body>) -> (StatusCode, HeaderMap, Vec<u8>) {
        let response = self.app.clone().oneshot(request).await.expect("oneshot");
        let (parts, body) = response.into_parts();
        let bytes = body.collect().await.expect("本文").to_bytes();
        (parts.status, parts.headers, bytes.to_vec())
    }

    /// `GET`。
    pub async fn get(&self, uri: &str) -> (StatusCode, HeaderMap, Vec<u8>) {
        self.send(get(uri)).await
    }

    /// ページを置く（`POST /api/projects/{project}/keys/{key}/kinds/{kind}/pages?{query}`。本文は HTML）。
    /// 答えの本文は JSON に読む（読めなければ Null）。
    pub async fn put(
        &self,
        project: &str,
        key: &str,
        kind: &str,
        query: &str,
        body: impl Into<Body>,
    ) -> (StatusCode, HeaderMap, Value) {
        let (status, headers, body) = self
            .send(put_page(&pages_uri(project, key, kind, query), body))
            .await;
        (
            status,
            headers,
            serde_json::from_slice(&body).unwrap_or(Value::Null),
        )
    }

    /// ページを置き、201 を見て、番号（数の字）を返す。
    pub async fn place(
        &self,
        project: &str,
        key: &str,
        kind: &str,
        query: &str,
        html: &str,
    ) -> String {
        let (status, _, json) = self.put(project, key, kind, query, html.to_owned()).await;
        assert_eq!(status, StatusCode::CREATED, "{json}");
        json["n"]
            .as_str()
            .unwrap_or_else(|| panic!("{json}"))
            .to_owned()
    }

    /// 表の行の数（読む接続で数える）。
    pub async fn count(&self, table: &str) -> i64 {
        scalar(self, &format!("select count(*) from {table}")).await
    }

    /// 書く接続で SQL を 1 つ流す（試験の見本を直に置く）。
    pub async fn execute(&self, sql: &str) {
        sqlx::query(sqlx::AssertSqlSafe(sql.to_owned()))
            .execute(self.db.write())
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e}"));
    }
}

/// 送り手の行（[`App::senders`]）。
pub type Sender = (
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// 1 つの値を読む SQL（読む接続）。
pub async fn scalar<T>(app: &App, sql: &str) -> T
where
    T: for<'r> sqlx::Decode<'r, sqlx::Sqlite> + sqlx::Type<sqlx::Sqlite> + Send + Unpin,
{
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
        .fetch_one(app.db.read())
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
}

/// ページを置く道（引数の `query` は `&` でつないだ物。空でもよい）。
pub fn pages_uri(project: &str, key: &str, kind: &str, query: &str) -> String {
    let uri = format!("/api/projects/{project}/keys/{key}/kinds/{kind}/pages");
    if query.is_empty() {
        uri
    } else {
        format!("{uri}?{query}")
    }
}

pub fn get(path: &str) -> Request<Body> {
    Request::get(path)
        .header(header::HOST, HOST)
        .body(Body::empty())
        .unwrap()
}

/// ページを置く要求（`Content-Type: text/html`）。
pub fn put_page(uri: &str, body: impl Into<Body>) -> Request<Body> {
    Request::post(uri)
        .header(header::HOST, HOST)
        .header(header::CONTENT_TYPE, "text/html")
        .body(body.into())
        .unwrap()
}

/// 題と本文の HTML。
pub fn html(title: &str, body: &str) -> String {
    format!("<!doctype html><html><head><title>{title}</title></head><body>{body}</body></html>")
}

/// 誤りの種類（header `X-Amzn-Errortype`）。
pub fn error_type(headers: &HeaderMap) -> &str {
    headers
        .get("x-amzn-errortype")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
}

/// 生成した層の外が返す誤りの形（種類の header・JSON の本文の `message`）を見て、`message` を返す。
pub fn refused(headers: &HeaderMap, body: &[u8], kind: &str) -> String {
    let text = String::from_utf8_lossy(body);
    assert_eq!(error_type(headers), kind, "{text}");
    assert_eq!(
        headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("application/json"),
        "{text}"
    );
    let json: Value = serde_json::from_slice(body).unwrap_or_else(|e| panic!("{e}: {text}"));
    let message = json["message"].as_str().unwrap_or_else(|| panic!("{text}"));
    assert!(!message.is_empty());
    message.to_owned()
}
