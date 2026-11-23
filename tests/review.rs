//! レビューの印の試験: 待ちの相手（`on`）・判定・見た物（`POST /api/pages/review`）。

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use common::{SE, Server, html, request};
use http_body_util::BodyExt;
use pagetray::{AppState, db::Db, router};
use serde_json::{Value, json};
use tower::ServiceExt;

async fn reply(s: &Server, query: &str) -> (StatusCode, Value) {
    let uri = format!("/api/pages/reply?{query}");
    let (status, _, body) = s.send(request("POST", &uri, Body::empty())).await;
    (status, serde_json::from_slice(&body).unwrap())
}

async fn review(s: &Server, query: &str, body: Value) -> (StatusCode, Value) {
    let uri = format!("/api/pages/review?{query}");
    let request = Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::HOST, "127.0.0.1:8801")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let (status, _, body) = s.send(request).await;
    // 引数の形の誤り（axum が断る物）は JSON でない。
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

const BRIEF: &str = "project=p&key=SKILLS-14&kind=brief";
const REVIEW: &str = "project=p&key=SKILLS-14&kind=review";
const CONFIRM: &str = "project=p&key=SKILLS-14&kind=confirm";
const PLAN: &str = "/p/SKILLS-14/plan-1.html";
const PR: &str = "https://github.com/o/r/pull/191";

/// 確認・計画・レビューの依頼書・レビューのページを 1 枚ずつ置く。
async fn setup() -> Server {
    let s = Server::start().await;
    for kind in ["confirm", "plan", "brief", "review"] {
        s.place(&format!("{SE}&kind={kind}"), &html(kind, "本文"))
            .await;
    }
    s
}

#[tokio::test]
async fn a_review_wait_is_not_a_reply_the_product_manager_owes() {
    let s = setup().await;
    reply(&s, &format!("{CONFIRM}&n=1&state=waiting")).await;
    let (status, brief) = reply(&s, &format!("{BRIEF}&n=1&state=waiting&on=review")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(brief["waits_on"], "review");
    assert!(brief["asked_at"].is_string());
    let (_, escalated) = reply(&s, &format!("{REVIEW}&n=1&state=waiting&on=tpm")).await;
    assert_eq!(escalated["waits_on"], "tpm");

    // on が無ければ、今までどおり Product Manager の分だけ。
    assert_eq!(
        s.paths("reply=waiting").await,
        ["/p/SKILLS-14/confirm-1.html"]
    );
    assert_eq!(
        s.paths("reply=waiting&on=pm").await,
        ["/p/SKILLS-14/confirm-1.html"]
    );
    assert_eq!(
        s.paths("reply=waiting&on=review").await,
        ["/p/SKILLS-14/brief-1.html"]
    );
    assert_eq!(
        s.paths("reply=waiting&on=tpm").await,
        ["/p/SKILLS-14/review-1.html"]
    );
    // Product Manager の待ちの記録には waits_on が出ない。
    let listed = s.json("/api/pages?kind=confirm").await;
    assert!(listed["pages"][0].get("waits_on").is_none());

    // 答えると、その相手の「答えた」に入る。Product Manager の「答えた」には入らない。
    let (status, done) = reply(&s, &format!("{BRIEF}&n=1&state=answered&on=review")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(done["waits_on"], "review");
    assert!(done["answered_at"].is_string());
    assert!(s.paths("reply=answered").await.is_empty());
    assert_eq!(
        s.paths("reply=answered&on=review").await,
        ["/p/SKILLS-14/brief-1.html"]
    );
    // 同じ答えを 2 度書いても、同じ記録。
    let (status, again) = reply(&s, &format!("{BRIEF}&n=1&state=answered&on=review")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again, done);
    let record: pagetray_api::PageRecord = serde_json::from_value(done).unwrap();
    assert_eq!(record.waits_on.as_deref(), Some("review"));
}

#[tokio::test]
async fn answering_for_the_wrong_party_changes_nothing() {
    let s = setup().await;
    // 待ちにしていない依頼書を、レビューの相手として答えたにはできない（Product Manager の「答えた」に入らない）。
    let (status, body) = reply(&s, &format!("{BRIEF}&n=1&state=answered&on=review")).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(body["error"].is_string());
    assert!(s.paths("reply=answered").await.is_empty());
    assert!(s.paths("reply=answered&on=review").await.is_empty());

    // レビューを待つ依頼書は、on の無い答え（Product Manager）でも、ほかの相手でも替わらない。
    reply(&s, &format!("{BRIEF}&n=1&state=waiting&on=review")).await;
    for query in [
        format!("{BRIEF}&n=1&state=answered"),
        format!("{BRIEF}&n=1&state=answered&on=pm"),
        format!("{BRIEF}&n=1&state=answered&on=tpm"),
    ] {
        let (status, _) = reply(&s, &query).await;
        assert_eq!(status, StatusCode::CONFLICT, "{query}");
    }
    assert_eq!(
        s.paths("reply=waiting&on=review").await,
        ["/p/SKILLS-14/brief-1.html"]
    );
    assert!(s.paths("reply=answered").await.is_empty());

    // Product Manager の確認を、レビューの相手として答えたにはできない。
    reply(&s, &format!("{CONFIRM}&n=1&state=waiting")).await;
    let (status, _) = reply(&s, &format!("{CONFIRM}&n=1&state=answered&on=review")).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        s.paths("reply=waiting").await,
        ["/p/SKILLS-14/confirm-1.html"]
    );

    // 待ちの側も、今ほかの相手を待っているページは替えない（待ちを黙って移さない）。
    for query in [
        format!("{BRIEF}&n=1&state=waiting"),
        format!("{BRIEF}&n=1&state=waiting&on=tpm"),
        format!("{CONFIRM}&n=1&state=waiting&on=review"),
    ] {
        let (status, _) = reply(&s, &query).await;
        assert_eq!(status, StatusCode::CONFLICT, "{query}");
    }
    assert_eq!(
        s.paths("reply=waiting&on=review").await,
        ["/p/SKILLS-14/brief-1.html"]
    );
    assert_eq!(
        s.paths("reply=waiting").await,
        ["/p/SKILLS-14/confirm-1.html"]
    );
    // 答えた後の求め直しは、相手を替えてよい。
    reply(&s, &format!("{BRIEF}&n=1&state=answered&on=review")).await;
    let (status, again) = reply(&s, &format!("{BRIEF}&n=1&state=waiting&on=tpm")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again["waits_on"], "tpm");

    // 誤った相手の名と、reply の無い on は 400。
    let (status, _) = reply(&s, &format!("{BRIEF}&n=1&state=waiting&on=staff")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _, _) = s.get("/api/pages?reply=waiting&on=staff").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _, _) = s.get("/api/pages?on=review").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_review_page_keeps_its_verdict_and_what_it_reviewed() {
    let s = setup().await;
    let (status, page) = review(
        &s,
        &format!("{REVIEW}&n=1"),
        json!({ "verdict": "concerns", "targets": [PLAN, PR] }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["path"], "/p/SKILLS-14/review-1.html");
    assert_eq!(page["verdict"], "concerns");
    assert_eq!(page["targets"], json!([PLAN, PR]));

    // 見た物は足すだけで、同じ物は 1 つ。判定は書き直せる。
    let (_, page) = review(
        &s,
        &format!("{REVIEW}&n=1"),
        json!({ "verdict": "approve", "targets": [PR, "/p/SKILLS-14/plan-2.html"] }),
    )
    .await;
    assert_eq!(page["verdict"], "approve");
    assert_eq!(
        page["targets"],
        json!([PLAN, PR, "/p/SKILLS-14/plan-2.html"])
    );
    // 判定を書かなければ、判定は替わらない。
    let (_, page) = review(&s, &format!("{REVIEW}&n=1"), json!({ "targets": [] })).await;
    assert_eq!(page["verdict"], "approve");

    // 依頼書にも見る物を書ける（判定は書けない）。
    let (status, brief) = review(&s, &format!("{BRIEF}&n=1"), json!({ "targets": [PLAN] })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(brief["targets"], json!([PLAN]));
    assert!(brief.get("verdict").is_none());

    // 一覧の記録にも出て、判定で絞れ、呼ぶ側の型で読める。
    assert_eq!(
        s.paths("verdict=approve").await,
        ["/p/SKILLS-14/review-1.html"]
    );
    assert!(s.paths("verdict=reject").await.is_empty());
    let listed = s.json("/api/pages?kind=review").await;
    let record: pagetray_api::PageRecord =
        serde_json::from_value(listed["pages"][0].clone()).unwrap();
    assert_eq!(record.verdict.as_deref(), Some("approve"));
    assert_eq!(record.targets.len(), 3);
    // 見た物の無いページには、欄が出ない。
    let listed = s.json("/api/pages?kind=confirm").await;
    assert!(listed["pages"][0].get("targets").is_none());
    assert!(listed["pages"][0].get("verdict").is_none());
}

#[tokio::test]
async fn bad_review_marks_are_refused_and_a_missing_page_is_not_found() {
    let s = setup().await;
    for (query, body) in [
        // 判定は review のページだけ。
        (format!("{BRIEF}&n=1"), json!({ "verdict": "approve" })),
        (format!("{REVIEW}&n=1"), json!({ "verdict": "done" })),
        // 見た物は、置き場の path か https の URL だけ。
        (
            format!("{REVIEW}&n=1"),
            json!({ "targets": ["http://127.0.0.1:8801/p/SKILLS-14/plan-1.html"] }),
        ),
        (format!("{REVIEW}&n=1"), json!({ "targets": ["plan-1"] })),
        (
            format!("{REVIEW}&n=1"),
            json!({ "targets": ["/p/SKILLS-14/plan-1"] }),
        ),
        (
            format!("{REVIEW}&n=1"),
            json!({ "targets": ["https://github.com/o/r/pull/1\nx"] }),
        ),
        (
            format!("{REVIEW}&n=1"),
            json!({ "targets": [format!("https://example.com/{}", "a".repeat(2048))] }),
        ),
        (format!("{REVIEW}&n=1"), json!({ "targets": "plan-1" })),
        // 引数の誤り。
        (format!("{REVIEW}&n=0"), json!({})),
        ("project=p&kind=review&n=1".to_owned(), json!({})),
    ] {
        let (status, _) = review(&s, &query, body.clone()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query} {body}");
    }
    let (status, _) = review(
        &s,
        &format!("{REVIEW}&n=2"),
        json!({ "verdict": "approve" }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // 字の数は byte でなく字で数える（2048 字の URL は受ける）。
    let long = format!("https://example.com/{}", "字".repeat(2048 - 20));
    let (status, page) = review(&s, &format!("{REVIEW}&n=1"), json!({ "targets": [long] })).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    // ページ 1 枚の見た物は 100 個まで。超える要求は、判定も含めて何も替えない。
    let many: Vec<String> = (0..99)
        .map(|i| format!("https://github.com/o/r/pull/{i}"))
        .collect();
    let (status, _) = review(&s, &format!("{REVIEW}&n=1"), json!({ "targets": many })).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = review(
        &s,
        &format!("{REVIEW}&n=1"),
        json!({ "verdict": "reject", "targets": [PR] }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let listed = s.json("/api/pages?kind=review").await;
    assert_eq!(listed["pages"][0]["targets"].as_array().unwrap().len(), 100);
    assert!(listed["pages"][0].get("verdict").is_none());
    let (status, _, _) = s.get("/api/pages?verdict=done").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(s.paths("verdict=approve").await.is_empty());

    // browser の中のページからは書けない。
    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/pages/review?{REVIEW}&n=1"))
        .header(header::HOST, "127.0.0.1:8801")
        .header(header::ORIGIN, "null")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "verdict": "approve" }).to_string()))
        .unwrap();
    assert_eq!(s.send(request).await.0, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn removing_a_review_page_removes_what_it_reviewed() {
    let s = setup().await;
    review(
        &s,
        &format!("{REVIEW}&n=1"),
        json!({ "verdict": "reject", "targets": [PLAN] }),
    )
    .await;
    assert_eq!(s.count("page_targets").await, 1);
    s.remove("project=p&key=SKILLS-14&kind=review&n=1").await;
    assert_eq!(s.count("page_targets").await, 0);
    assert_eq!(s.foreign_key_violations().await, 0);
}

#[tokio::test]
async fn review_marks_survive_a_restart() {
    let s = setup().await;
    review(
        &s,
        &format!("{REVIEW}&n=1"),
        json!({ "verdict": "reject", "targets": [PLAN] }),
    )
    .await;
    reply(&s, &format!("{REVIEW}&n=1&state=waiting&on=tpm")).await;
    s.db.close().await;
    let db = Db::open(&s.path).await.unwrap();
    let app = router(AppState::new(db));
    let response = app
        .oneshot(request(
            "GET",
            "/api/pages?reply=waiting&on=tpm",
            Body::empty(),
        ))
        .await
        .unwrap();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["pages"][0]["verdict"], "reject");
    assert_eq!(json["pages"][0]["targets"][0], PLAN);
    assert_eq!(json["pages"][0]["waits_on"], "tpm");
}

#[tokio::test]
async fn a_review_mark_is_pushed_as_a_named_event() {
    let s = setup().await;
    let response = s
        .app
        .clone()
        .oneshot(request("GET", "/api/events", Body::empty()))
        .await
        .unwrap();
    let mut body = response.into_body();
    review(
        &s,
        &format!("{REVIEW}&n=1"),
        json!({ "verdict": "approve", "targets": [PLAN] }),
    )
    .await;
    let frame = tokio::time::timeout(std::time::Duration::from_secs(5), body.frame())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let text = String::from_utf8(frame.into_data().unwrap().to_vec()).unwrap();
    let data = text.strip_prefix("event: review\ndata: ").unwrap();
    let json: Value = serde_json::from_str(data.trim()).unwrap();
    assert_eq!(json["path"], "/p/SKILLS-14/review-1.html");
    assert_eq!(json["verdict"], "approve");
}
