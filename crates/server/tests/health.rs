//! GetHealth の振る舞い（処理より先に書く）。生成した service に本物の handler を入れ、Router を通して要求を 1 つ送る。
mod common;

use axum::http::StatusCode;
use common::App;

#[tokio::test]
async fn health_says_ok_with_the_version() {
    let app = App::start().await;
    let (status, headers, body) = app.get("/healthz").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["content-type"], "application/json");
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        json,
        serde_json::json!({ "status": "ok", "version": env!("CARGO_PKG_VERSION") })
    );
}

/// 今の server の版の道と header は無い（版は service の版と、定義の smithy diff で守る）。
#[tokio::test]
async fn the_old_version_route_and_header_are_gone() {
    let app = App::start().await;
    let (status, headers, _) = app.get("/healthz").await;
    assert_eq!(status, StatusCode::OK);
    assert!(headers.get("pagetray-api").is_none(), "{headers:?}");
    let (status, headers, body) = app.get("/api/version").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(headers.get("pagetray-api").is_none(), "{headers:?}");
    assert_eq!(body, b"{}");
}

#[tokio::test]
async fn an_unknown_path_is_a_json_404_from_the_generated_layer() {
    let app = App::start().await;
    for path in [
        "/v1/nope",
        "/api/nope",
        "/api/a/b",
        "/api/a/b/c",
        "/api/a/b/c/d",
    ] {
        let (status, headers, body) = app.get(path).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
        assert_eq!(
            common::error_type(&headers),
            "UnknownOperationException",
            "{path}"
        );
        assert_eq!(body, b"{}", "{path}");
    }
}
