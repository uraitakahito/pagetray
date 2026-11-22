//! 残した記録を読む道（決まりは docs/src/api.md の `GET /api/logs`）。

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use common::Server;
use pagetray::runlog::RunLog;

#[tokio::test]
async fn 記録の置き場が無ければ_404() {
    let server = Server::start().await;
    let (status, _, _) = server.get("/api/logs").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn 印と記録を_字で返す() {
    let dir = std::env::temp_dir().join(format!("pagetray-logs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let runlog = RunLog::at(dir.clone());
    runlog.start("9.9.9");
    let server = Server::with_runlog(runlog).await;
    let (status, headers, body) = server.get("/api/logs").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "text/plain; charset=utf-8");
    let text = String::from_utf8(body).unwrap();
    assert!(text.starts_with("== runs.log"), "{text}");
    assert!(text.contains(" start version=9.9.9\n"), "{text}");
    assert!(text.contains("== server.log"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn 機械の外からの名では断る() {
    let server =
        Server::with_runlog(RunLog::at(std::env::temp_dir().join("pagetray-logs-none"))).await;
    let request = Request::builder()
        .uri("/api/logs")
        .header(header::HOST, "evil.example")
        .body(Body::empty())
        .unwrap();
    let (status, _, _) = server.send(request).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}
