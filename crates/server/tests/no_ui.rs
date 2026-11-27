//! 画面は pagetray に無い（2.0.0 で外した。画面は callboard が持つ）。前の画面の道は、どれも 404。
//! 置いたページの本文・file の中身・API（`/api/` の下と `/healthz`）・新着の知らせ・手の道（backup・logs）は、今までどおり。
//! 画面の form からの file の上げ（`Origin` の付いた POST を通す、ただ 1 つの例外）も、画面と一緒に無くした。
mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use common::{App, HOST, error_type, files_uri, html};
use serde_json::json;

const SE: &str = "role=software-engineer&workspace=workspace:1";

#[tokio::test]
async fn the_former_screen_paths_are_404_and_no_answer_is_a_screen() {
    let app = App::start().await;
    app.place("p", "A-1", "plan", SE, &html("計画", "本文"))
        .await;
    for path in [
        "/",
        "/?project=p",
        "/?open=%2Fp%2FA-1%2Fplan-1.html",
        "/p/",
        "/p/A-1/",
        "/p/A-1/?kind=plan",
        "/messages",
        "/messages?state=waiting",
    ] {
        let (status, headers, body) = app.get(path).await;
        let body = String::from_utf8_lossy(&body);
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}: {body}");
        let content_type = headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(
            !content_type.starts_with("text/html"),
            "{path}: {content_type}"
        );
        assert!(!body.contains("ページの置き場"), "{path}: {body}");
        assert!(!body.contains("メッセージの記録"), "{path}: {body}");
        assert_eq!(
            headers.get(header::X_CONTENT_TYPE_OPTIONS).unwrap(),
            "nosniff",
            "{path}"
        );
    }
}

#[tokio::test]
async fn pages_files_and_the_api_still_answer() {
    let app = App::start().await;
    app.place("p", "A-1", "plan", SE, &html("計画", "本文"))
        .await;
    let (status, _, _) = app
        .send(common::put_file(
            &files_uri("p", "A-1", "name=a.txt&role=tpm"),
            Some("text/plain"),
            "hello",
        ))
        .await;
    assert_eq!(status, StatusCode::CREATED);
    // 置いたページの本文と、file の中身（URL は前のまま）。
    let (status, headers, body) = app.get("/p/A-1/plan-1.html").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        headers[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("text/html")
    );
    assert!(String::from_utf8_lossy(&body).contains("本文"));
    let (status, _, body) = app.get("/p/A-1/files/a-1.txt").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, b"hello");
    // 読む操作（画面を callboard に移すために足した物も）。
    for path in [
        "/healthz",
        "/api/pages?project=p",
        "/api/pages/facets",
        "/api/reviews?target=%2Fp%2FA-1%2Fplan-1.html",
        "/api/files?project=p",
        "/api/files/rules",
        "/api/messages",
        "/api/messages/facets",
    ] {
        let (status, _, json) = app.json(path).await;
        assert_eq!(status, StatusCode::OK, "{path}: {json}");
    }
}

/// 画面の form からの上げ（`Origin` が置き場自身の POST）は、もう通さない。browser の中のページからは、何も書けない。
/// 役割を書かない上げは、どこからでも 400（役割は、いつも要る）。
#[tokio::test]
async fn a_page_in_a_browser_cannot_upload_a_file_any_more() {
    let app = App::start().await;
    let request = |origin: Option<&str>, query: &str| {
        let mut builder = Request::builder()
            .method("POST")
            .uri(files_uri("p", "A-1", query))
            .header(header::HOST, HOST)
            .header(header::CONTENT_TYPE, "text/plain");
        if let Some(origin) = origin {
            builder = builder
                .header(header::ORIGIN, origin)
                .header("sec-fetch-site", "same-origin");
        }
        builder.body(Body::from("hello")).unwrap()
    };
    let own = format!("http://{HOST}");
    for query in ["name=a.txt", "name=a.txt&role=product-manager"] {
        let (status, headers, _) = app.send(request(Some(&own), query)).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{query}");
        assert_eq!(error_type(&headers), "Forbidden");
    }
    assert_eq!(app.count("files").await, 0);
    // browser でない要求（Origin が無い）は、役割を書けば置ける。書かなければ 400。
    let (status, headers, _) = app.send(request(None, "name=a.txt")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_type(&headers), "ValidationException");
    let (status, _, body) = app
        .send_json(request(None, "name=a.txt&role=product-manager"))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["role"], json!("product-manager"));
}
