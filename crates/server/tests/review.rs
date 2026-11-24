//! AddReviewMarks（`POST …/pages/{n}/review`）と、判定で探す（`GET /api/pages?verdict`）振る舞い（処理より先に書く）。
//! 今の server の tests/review.rs の印の試験を、新しい道と欄の名で見る。
mod common;

use axum::http::{Request, StatusCode, header};
use common::{App, HOST, error_type, html, json_request};
use serde_json::{Value, json};

const BRIEF: &str = "/p/SKILLS-14/brief-1.html";
const REVIEW: &str = "/p/SKILLS-14/review-1.html";
const CONFIRM: &str = "/p/SKILLS-14/confirm-1.html";
const PLAN: &str = "/p/SKILLS-14/plan-1.html";
const PR: &str = "https://github.com/o/r/pull/191";
const SE: &str = "role=software-engineer&workspace=workspace:1";

/// 確認・計画・レビューの依頼書・レビューのページを 1 枚ずつ置く。
async fn setup() -> App {
    let app = App::start().await;
    for kind in ["confirm", "plan", "brief", "review"] {
        app.place("p", "SKILLS-14", kind, SE, &html(kind, "本文"))
            .await;
    }
    app
}

/// 印を書き、200 を見て、ページの記録を返す。
async fn ok(app: &App, page: &str, body: Value) -> Value {
    let (status, _, json) = app.review(page, Some(body.clone())).await;
    assert_eq!(status, StatusCode::OK, "{page} {body}: {json}");
    json
}

/// 400 で断り、何も替えない。
async fn refused(app: &App, page: &str, body: Value) {
    let (status, headers, json) = app.review(page, Some(body.clone())).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{page} {body}: {json}");
    assert!(
        matches!(
            error_type(&headers),
            "ValidationException" | "SerializationException"
        ),
        "{page} {body}: {headers:?}"
    );
}

/// レビューのページの、残っている判定と見た物。
async fn marks(app: &App) -> Value {
    let (status, _, json) = app.json("/api/pages?kind=review").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let page = &json["pages"][0];
    json!({ "verdict": page.get("verdict"), "targets": page.get("targets") })
}

#[tokio::test]
async fn a_review_page_keeps_its_verdict_and_what_it_reviewed() {
    let app = setup().await;
    let page = ok(
        &app,
        REVIEW,
        json!({ "verdict": "concerns", "targets": [PLAN, PR] }),
    )
    .await;
    assert_eq!(page["path"], REVIEW);
    assert_eq!(page["verdict"], "concerns");
    assert_eq!(page["targets"], json!([PLAN, PR]));
    assert!(page.get("url").is_none(), "{page}");

    // 見た物は足すだけで、同じ物は 1 つ（1 度の要求の中で重ねても）、書いた順。判定は書き直せる。
    let page = ok(
        &app,
        REVIEW,
        json!({ "verdict": "approve", "targets": [PR, "/p/SKILLS-14/plan-2.html", "/p/SKILLS-14/plan-2.html"] }),
    )
    .await;
    assert_eq!(page["verdict"], "approve");
    assert_eq!(
        page["targets"],
        json!([PLAN, PR, "/p/SKILLS-14/plan-2.html"])
    );
    // 判定を書かなければ、判定は替わらない。
    let page = ok(&app, REVIEW, json!({ "targets": [] })).await;
    assert_eq!(page["verdict"], "approve");
    let page = ok(&app, REVIEW, json!({})).await;
    assert_eq!(page["verdict"], "approve");
    for verdict in ["reject", "not-assessed", "concerns"] {
        let page = ok(&app, REVIEW, json!({ "verdict": verdict })).await;
        assert_eq!(page["verdict"], verdict);
        assert_eq!(page["targets"].as_array().unwrap().len(), 3);
    }

    // 依頼書にも見る物を書ける（判定は書けない）。
    let brief = ok(&app, BRIEF, json!({ "targets": [PLAN] })).await;
    assert_eq!(brief["targets"], json!([PLAN]));
    assert!(brief.get("verdict").is_none());

    // 一覧の記録にも出て、判定で絞れる。
    assert_eq!(app.paths("verdict=concerns").await, [REVIEW]);
    assert!(app.paths("verdict=reject").await.is_empty());
    assert_eq!(
        marks(&app).await,
        json!({ "verdict": "concerns", "targets": [PLAN, PR, "/p/SKILLS-14/plan-2.html"] })
    );
    // 見た物の無いページには、欄が出ない。
    let (_, _, listed) = app.json("/api/pages?kind=confirm").await;
    assert!(listed["pages"][0].get("targets").is_none(), "{listed}");
    assert!(listed["pages"][0].get("verdict").is_none(), "{listed}");
}

/// 本文の無い要求は、何も替えずに記録を返す（今の server と同じ）。
#[tokio::test]
async fn a_request_without_a_body_changes_nothing() {
    let app = setup().await;
    ok(
        &app,
        REVIEW,
        json!({ "verdict": "reject", "targets": [PLAN] }),
    )
    .await;
    let (status, _, page) = app.review(REVIEW, None).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["verdict"], "reject");
    assert_eq!(page["targets"], json!([PLAN]));
}

#[tokio::test]
async fn bad_review_marks_are_refused_and_a_missing_page_is_not_found() {
    let app = setup().await;
    for (page, body) in [
        // 判定は review のページだけ。決まった 4 つの字だけ。
        (BRIEF, json!({ "verdict": "approve" })),
        (CONFIRM, json!({ "verdict": "reject", "targets": [PLAN] })),
        (REVIEW, json!({ "verdict": "done" })),
        // 見た物は、置き場の path か https の URL だけ。
        (
            REVIEW,
            json!({ "targets": ["http://127.0.0.1:8801/p/SKILLS-14/plan-1.html"] }),
        ),
        (REVIEW, json!({ "targets": ["plan-1"] })),
        (REVIEW, json!({ "targets": ["/p/SKILLS-14/plan-1"] })),
        (REVIEW, json!({ "targets": ["/p/../plan-1.html"] })),
        (REVIEW, json!({ "targets": ["/p/SKILLS-14/plan-01.html"] })),
        (REVIEW, json!({ "targets": ["https://"] })),
        (
            REVIEW,
            json!({ "targets": ["https://github.com/o/r/pull/1\nx"] }),
        ),
        (
            REVIEW,
            json!({ "targets": ["https://github.com/o/r pull/1"] }),
        ),
        (
            REVIEW,
            json!({ "targets": [format!("https://example.com/{}", "a".repeat(2048))] }),
        ),
        (REVIEW, json!({ "targets": [""] })),
        // 形の違う本文。
        (REVIEW, json!({ "targets": "plan-1" })),
        (REVIEW, json!({ "targets": [1] })),
        (REVIEW, json!({ "verdict": 1 })),
    ] {
        refused(&app, page, body).await;
    }
    assert_eq!(
        marks(&app).await,
        json!({ "verdict": null, "targets": null })
    );
    // 1 度に 100 個を超える見た物は 400。
    let many: Vec<String> = (0..101)
        .map(|i| format!("https://github.com/o/r/pull/{i}"))
        .collect();
    refused(&app, REVIEW, json!({ "targets": many })).await;
    // JSON でない本文も 400。
    let request = Request::post("/api/projects/p/keys/SKILLS-14/kinds/review/pages/1/review")
        .header(header::HOST, HOST)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from("verdict=approve"))
        .unwrap();
    assert_eq!(app.send(request).await.0, StatusCode::BAD_REQUEST);
    // 道の誤り（番号・種類・. と ..）。
    for uri in [
        "/api/projects/p/keys/SKILLS-14/kinds/review/pages/0/review",
        "/api/projects/p/keys/SKILLS-14/kinds/Review/pages/1/review",
        "/api/projects/../keys/SKILLS-14/kinds/review/pages/1/review",
        "/api/projects/p/keys/../kinds/review/pages/1/review",
    ] {
        let (status, _, json) = app
            .send_json(json_request("POST", uri, Some(json!({}))))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}: {json}");
    }
    assert_eq!(
        marks(&app).await,
        json!({ "verdict": null, "targets": null })
    );

    for page in ["/p/SKILLS-14/review-2.html", "/q/SKILLS-14/review-1.html"] {
        let (status, headers, json) = app
            .review(page, Some(json!({ "verdict": "approve" })))
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{page}: {json}");
        assert_eq!(error_type(&headers), "NotFound");
    }

    // 字の数は byte でなく字で数える（2048 字の URL は受ける）。
    let long = format!("https://example.com/{}", "字".repeat(2048 - 20));
    let page = ok(&app, REVIEW, json!({ "targets": [long] })).await;
    assert_eq!(page["targets"].as_array().unwrap().len(), 1);
    // ページ 1 枚の見た物は 100 個まで。超える要求は、判定も含めて何も替えない。
    let many: Vec<String> = (0..99)
        .map(|i| format!("https://github.com/o/r/pull/{i}"))
        .collect();
    ok(&app, REVIEW, json!({ "targets": many })).await;
    let (status, _, json) = app
        .review(
            REVIEW,
            Some(json!({ "verdict": "reject", "targets": [PR] })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
    assert!(
        json["message"].as_str().unwrap_or("").contains("100"),
        "{json}"
    );
    let now = marks(&app).await;
    assert_eq!(now["targets"].as_array().unwrap().len(), 100);
    assert!(now["verdict"].is_null(), "{now}");
    // もうある物だけなら、足しても 100 個のまま（通る）。
    ok(
        &app,
        REVIEW,
        json!({ "targets": ["https://github.com/o/r/pull/0"] }),
    )
    .await;
    assert!(app.paths("verdict=approve").await.is_empty());
}

/// 消したページには書けない（404。410 でない）。
#[tokio::test]
async fn a_removed_page_is_not_found() {
    let app = setup().await;
    app.remove("project=p&key=SKILLS-14&kind=review&n=1").await;
    let (status, headers, _) = app
        .review(REVIEW, Some(json!({ "verdict": "approve" })))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error_type(&headers), "NotFound");
}

#[tokio::test]
async fn a_page_in_a_browser_cannot_write_review_marks() {
    let app = setup().await;
    for origin in ["null", "http://127.0.0.1:8801"] {
        let mut request = json_request(
            "POST",
            "/api/projects/p/keys/SKILLS-14/kinds/review/pages/1/review",
            Some(json!({ "verdict": "approve" })),
        );
        request
            .headers_mut()
            .insert(header::ORIGIN, origin.parse().unwrap());
        let (status, headers, _) = app.send(request).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{origin}");
        assert_eq!(error_type(&headers), "Forbidden");
    }
    assert_eq!(
        marks(&app).await,
        json!({ "verdict": null, "targets": null })
    );
}

#[tokio::test]
async fn review_marks_survive_a_restart() {
    let mut app = setup().await;
    ok(
        &app,
        REVIEW,
        json!({ "verdict": "reject", "targets": [PLAN] }),
    )
    .await;
    let (status, _, _) = app
        .reply(REVIEW, json!({ "state": "waiting", "on": "tpm" }))
        .await;
    assert_eq!(status, StatusCode::OK);
    app.reopen().await;
    let (_, _, json) = app.json("/api/pages?reply=waiting&on=tpm").await;
    assert_eq!(json["pages"][0]["verdict"], "reject");
    assert_eq!(json["pages"][0]["targets"], json!([PLAN]));
    assert_eq!(json["pages"][0]["waitsOn"], "tpm");
}

/// DB の誤りは 500、書く接続が空かなければ 503。どちらも何も替えない。
#[tokio::test]
async fn a_db_error_is_internal_and_a_busy_writer_is_unavailable() {
    let app = setup().await;
    let held = app.db.write().acquire().await.expect("書く接続");
    let (status, headers, json) = app
        .review(REVIEW, Some(json!({ "verdict": "approve" })))
        .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{json}");
    assert_eq!(error_type(&headers), "Unavailable");
    drop(held);

    app.execute("drop table page_targets").await;
    let (status, headers, json) = app
        .review(
            REVIEW,
            Some(json!({ "verdict": "approve", "targets": [PLAN] })),
        )
        .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{json}");
    assert_eq!(error_type(&headers), "InternalError");
    let verdict: Option<String> =
        common::scalar(&app, "select verdict from pages where kind = 'review'").await;
    assert_eq!(verdict, None);
}
