//! ページの本文の道（`GET /{project}/{key}/{kind}-{n}.html`。手の道。0.8.x の server と同じ URL）の振る舞い。
mod common;

use axum::http::{StatusCode, header};
use common::{App, html, refused};

#[tokio::test]
async fn the_body_comes_back_byte_for_byte_in_a_sandbox() {
    let app = App::start().await;
    let page = html("題", "中身 <script>1</script>");
    app.place("p", "SKILLS-14", "proposal", "role=tpm", &page)
        .await;
    let (status, headers, body) = app.get("/p/SKILLS-14/proposal-1.html").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, page.as_bytes());
    assert_eq!(headers[header::CONTENT_TYPE], "text/html; charset=utf-8");
    assert_eq!(
        headers[header::CONTENT_SECURITY_POLICY],
        "sandbox allow-scripts"
    );
    assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    // CSP は 1 つだけ（守りの層が別の物を重ねない）。
    assert_eq!(
        headers
            .get_all(header::CONTENT_SECURITY_POLICY)
            .iter()
            .count(),
        1
    );
}

#[tokio::test]
async fn a_missing_page_or_a_url_of_the_wrong_shape_is_not_found() {
    let app = App::start().await;
    app.place("p", "SKILLS-14", "proposal", "role=tpm", "<p>1</p>")
        .await;
    for missing in [
        "/p/SKILLS-14/proposal-2.html",
        "/p/SKILLS-14/proposal-01.html",
        "/p/SKILLS-14/proposal-0.html",
        "/p/SKILLS-14/Proposal-1.html",
        "/p/SKILLS-14/proposal-1.htm",
        "/p/SKILLS-14/proposal-1",
        "/p/SKILLS-14/x.txt",
        "/p/other/proposal-1.html",
        "/other/SKILLS-14/proposal-1.html",
        "/p/../proposal-1.html",
        "/p/SKILLS-14/proposal-99999999999999999999.html",
    ] {
        let (status, headers, body) = app.get(missing).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{missing}");
        let message = refused(&headers, &body, "NotFound");
        assert!(message.contains("無い"), "{missing}: {message}");
    }
}

/// 消したページは 410（訳に消した時刻）。番号は 2 度使わない。
#[tokio::test]
async fn a_removed_page_is_gone_with_the_time_it_was_removed() {
    let app = App::start().await;
    app.execute(
        "insert into removed(project, key, kind, n, removed_at) values('p', 'k', 'plan', 1, '2026-11-24T01:02:03.456Z')",
    )
    .await;
    let (status, headers, body) = app.get("/p/k/plan-1.html").await;
    assert_eq!(status, StatusCode::GONE);
    let message = refused(&headers, &body, "Gone");
    assert!(message.contains("2026-11-24T01:02:03.456Z"), "{message}");
    // 消した番号の隣は、まだ無いだけ（404）。
    let (status, headers, body) = app.get("/p/k/plan-2.html").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    refused(&headers, &body, "NotFound");
}

/// 種類 files のページの URL（`/p/k/files-1.html`）は、file の URL（`/p/k/files/<語>-<n>.<拡張子>`）とぶつからない。
#[tokio::test]
async fn a_page_of_kind_files_has_its_own_url() {
    let app = App::start().await;
    app.place("p", "k", "files", "role=tpm", "<p>files</p>")
        .await;
    let (status, _, body) = app.get("/p/k/files-1.html").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, b"<p>files</p>");
}

/// `/api/` の下の 3 段の道は本文の道に入らず、生成した層が答える（`project` に `api` は置けない）。
#[tokio::test]
async fn paths_under_api_never_reach_the_body_route() {
    let app = App::start().await;
    let (status, headers, body) = app.get("/api/k/plan-1.html").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(common::error_type(&headers), "UnknownOperationException");
    assert_eq!(body, b"{}");
}
