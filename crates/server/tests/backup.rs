//! 手の道 `GET /api/backup`（DB の揃ったバックアップ）と `GET /api/logs`（残した記録）。今の server の同じ道と同じ振る舞い
//! （今の server の tests/api.rs の backup の試験と tests/logs.rs を移した物）。
mod common;

use std::path::Path;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use common::{App, PNG, error_type, get, html, refused, temp_db};
use http_body_util::BodyExt;
use pagetray_next::runlog::RunLog;
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use tower::ServiceExt;

const SE: &str = "role=software-engineer&workspace=workspace:1";

/// DB の folder の中の、バックアップの一時の file の名（`pages.db.backup-…`）。
fn leftovers(db: &Path) -> Vec<String> {
    std::fs::read_dir(db.parent().unwrap())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(".backup-"))
        .collect()
}

/// 受けたバックアップを別の folder に置き、読むだけで開く（壊れていない・外のキーの崩れが無いことも見る）。
async fn open_copy(bytes: &[u8]) -> (SqlitePool, std::path::PathBuf) {
    let copy = temp_db("backup-copy");
    tokio::fs::write(&copy, bytes).await.unwrap();
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(SqliteConnectOptions::new().filename(&copy).read_only(true))
        .await
        .unwrap();
    let integrity: String = sqlx::query_scalar("pragma integrity_check")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(integrity, "ok");
    let broken: Vec<(String,)> = sqlx::query_as("select \"table\" from pragma_foreign_key_check")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert!(broken.is_empty(), "{broken:?}");
    (pool, copy)
}

#[tokio::test]
async fn backup_is_a_whole_sqlite_file_with_the_pages_and_the_files() {
    let app = App::start().await;
    app.place("p", "SKILLS-14", "plan", SE, &html("計画", "控えに入る字"))
        .await;
    app.place("p", "SKILLS-14", "plan", SE, "<p>2</p>").await;
    app.place_file(
        "p",
        "SKILLS-14",
        &format!("{SE}&name=a.png"),
        "image/png",
        PNG,
    )
    .await;
    app.place_file(
        "p",
        "SKILLS-14",
        &format!("{SE}&name=a.png"),
        "image/png",
        PNG,
    )
    .await;
    let (status, _, json) = app
        .remove_files("project=p&key=SKILLS-14&name=a.png&n=1")
        .await;
    assert_eq!(status, StatusCode::OK, "{json}");

    let (status, headers, bytes) = app.send(get("/api/backup")).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    assert_eq!(
        headers.get(header::CONTENT_TYPE).unwrap(),
        "application/vnd.sqlite3"
    );
    assert_eq!(
        headers.get(header::CONTENT_DISPOSITION).unwrap(),
        "attachment; filename=\"pages.db\""
    );
    assert!(bytes.starts_with(b"SQLite format 3\0"));
    // 送り始めたら、一時の file は消す。
    assert_eq!(leftovers(&app.path), Vec::<String>::new());

    let (pool, copy) = open_copy(&bytes).await;
    let pages: Vec<(i64, Vec<u8>)> = sqlx::query_as("select n, body from pages order by n")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(pages.len(), 2);
    assert_eq!(pages[1], (2, b"<p>2</p>".to_vec()));
    let files: Vec<(i64, Vec<u8>)> = sqlx::query_as("select n, body from files")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(files, [(2, PNG.to_vec())]);
    let removed: i64 = sqlx::query_scalar("select count(*) from files_removed")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(removed, 1);
    pool.close().await;
    common::remove_db(&copy);
}

#[tokio::test]
async fn backup_of_an_empty_store_is_a_sqlite_file_too() {
    let app = App::start().await;
    let (status, _, bytes) = app.send(get("/api/backup")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(bytes.starts_with(b"SQLite format 3\0"));
    let (pool, copy) = open_copy(&bytes).await;
    pool.close().await;
    common::remove_db(&copy);
}

/// 時間切れは答えの頭までを数え、本文を送る間は数えない（大きな DB の file を送る途中で切らない）。
#[tokio::test]
async fn backup_body_is_not_cut_by_the_request_timeout() {
    let app = App::start_with(pagetray_next::Config {
        timeout: Duration::from_millis(500),
        ..Default::default()
    })
    .await;
    app.place("p", "k", "plan", SE, "<p>1</p>").await;
    let response = app.app.clone().oneshot(get("/api/backup")).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    // 時間切れの長さより後に読み始める。
    tokio::time::sleep(Duration::from_millis(800)).await;
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert!(bytes.starts_with(b"SQLite format 3\0"));
    let (pool, copy) = open_copy(&bytes).await;
    let pages: i64 = sqlx::query_scalar("select count(*) from pages")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(pages, 1);
    pool.close().await;
    common::remove_db(&copy);
}

#[tokio::test]
async fn backup_from_outside_the_host_is_forbidden() {
    let app = App::start().await;
    let request = Request::get("/api/backup")
        .header(header::HOST, "evil.example")
        .body(Body::empty())
        .unwrap();
    let (status, headers, body) = app.send(request).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    refused(&headers, &body, "Forbidden");
}

#[tokio::test]
async fn logs_are_the_marks_and_the_lines_as_text() {
    let dir = temp_db("logs").with_file_name("logs");
    let log = RunLog::at(dir.clone());
    log.start("9.9.9");
    let app = App::start_with(pagetray_next::Config {
        runlog: Some(log),
        ..Default::default()
    })
    .await;
    let (status, headers, body) = app.send(get("/api/logs")).await;
    let body = String::from_utf8(body).unwrap();
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        headers.get(header::CONTENT_TYPE).unwrap(),
        "text/plain; charset=utf-8"
    );
    assert!(body.starts_with("== runs.log\n"), "{body}");
    assert!(body.contains(" start version=9.9.9\n"), "{body}");
    assert!(body.contains("== server.log (oldest first)\n"), "{body}");
    // 機械の外からの名では断る。
    let request = Request::get("/api/logs")
        .header(header::HOST, "evil.example")
        .body(Body::empty())
        .unwrap();
    let (status, headers, body) = app.send(request).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    refused(&headers, &body, "Forbidden");
    common::remove_db(&dir.with_file_name("pages.db"));
}

/// 記録の置き場を付けない道筋（Router を直に呼ぶ試験）では、`GET /api/logs` は 404 `NotFound`（今の server と同じく 404）。
#[tokio::test]
async fn logs_without_a_place_for_them_are_not_found() {
    let app = App::start().await;
    let (status, headers, body) = app.send(get("/api/logs")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error_type(&headers), "NotFound");
    let message = refused(&headers, &body, "NotFound");
    assert!(message.contains("記録の置き場"), "{message}");
}
