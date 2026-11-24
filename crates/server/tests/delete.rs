//! DeletePages（`DELETE /api/pages?project&key&kind&n`）の振る舞い（処理より先に書く）。
//! 今の server の tests/api.rs・tests/review.rs・tests/senders.rs の消す試験を、新しい答えの形で見る。
mod common;

use axum::http::{StatusCode, header};
use common::{App, HOST, error_type, html, refused};
use serde_json::json;

const SE: &str = "role=software-engineer&workspace=workspace:1";

#[tokio::test]
async fn a_removed_page_is_gone_and_its_number_is_never_used_again() {
    let app = App::start().await;
    for text in ["一つ目の計画", "二つ目の計画"] {
        app.place("p", "SKILLS-14", "plan", SE, &html(text, text))
            .await;
    }
    app.place("p", "SKILLS-14", "result", SE, &html("結果", "残る"))
        .await;

    let (status, _, json) = app.remove("project=p&key=SKILLS-14&kind=plan&n=2").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json, json!({ "removed": ["/p/SKILLS-14/plan-2.html"] }));

    let (status, headers, body) = app.get("/p/SKILLS-14/plan-2.html").await;
    assert_eq!(status, StatusCode::GONE);
    let message = refused(&headers, &body, "Gone");
    assert!(message.starts_with("そのページは消した（"), "{message}");
    // 消した時刻は、残す字（UTC・ミリ秒まで・24 字）。
    let at: String = common::scalar(&app, "select removed_at from removed").await;
    assert_eq!(at.len(), 24, "{at}");
    assert!(at.ends_with('Z'), "{at}");
    assert!(message.contains(&at), "{message}");

    // ほかのページは残る。一覧にも検索にも、消したページは出ない。索引の行も消える。
    let (_, _, body) = app.get("/p/SKILLS-14/plan-1.html").await;
    assert_eq!(body, html("一つ目の計画", "一つ目の計画").as_bytes());
    assert_eq!(
        app.paths("project=p").await,
        ["/p/SKILLS-14/result-1.html", "/p/SKILLS-14/plan-1.html"]
    );
    assert!(app.paths("q=二つ目の計画").await.is_empty());
    assert_eq!(
        app.paths("q=一つ目の計画").await,
        ["/p/SKILLS-14/plan-1.html"]
    );
    assert_eq!(app.count("pages_fts").await, 2);
    // 一番大きい番号を消しても、次は 3（古い URL が別のページを指さない）。
    assert_eq!(
        app.place("p", "SKILLS-14", "plan", SE, "<p>3</p>").await,
        "3"
    );
    // 取り込みで消した番号を名指しても入らない。
    let (status, _, _) = app
        .put(
            "p",
            "SKILLS-14",
            "plan",
            &format!("{SE}&n=2"),
            "<p>生き返らない</p>",
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        app.get("/p/SKILLS-14/plan-2.html").await.0,
        StatusCode::GONE
    );
    // 2 度目は消す物が無い（消した時刻も替えない）。
    let (status, _, json) = app.remove("project=p&key=SKILLS-14&kind=plan&n=2").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json, json!({ "removed": [] }));
    let again: String = common::scalar(&app, "select removed_at from removed").await;
    assert_eq!(again, at);
}

#[tokio::test]
async fn a_key_a_kind_or_a_whole_project_is_removed_at_once() {
    let app = App::start().await;
    app.place("p", "SKILLS-14", "plan", SE, "<p>1</p>").await;
    app.place("p", "SKILLS-14", "result", SE, "<p>1</p>").await;
    app.place("p", "SKILLS-14", "plan", SE, "<p>2</p>").await;
    app.place("p", "t-x", "brief", "role=tpm", "<p>1</p>").await;
    app.place("p", "t-y", "brief", "role=tpm", "<p>1</p>").await;
    app.place("p", "t-y", "plan", "role=tpm", "<p>1</p>").await;
    app.place("q", "t-x", "brief", "role=tpm", "<p>1</p>").await;

    // 消した path は置いた順。
    let (_, _, json) = app.remove("project=p&key=SKILLS-14").await;
    assert_eq!(
        json["removed"],
        json!([
            "/p/SKILLS-14/plan-1.html",
            "/p/SKILLS-14/result-1.html",
            "/p/SKILLS-14/plan-2.html"
        ])
    );
    // 種類だけでも（project の中の、その種類をみな）。
    let (_, _, json) = app.remove("project=p&kind=brief").await;
    assert_eq!(
        json["removed"],
        json!(["/p/t-x/brief-1.html", "/p/t-y/brief-1.html"])
    );
    assert_eq!(app.paths("project=p").await, ["/p/t-y/plan-1.html"]);

    let (_, _, json) = app.remove("project=p").await;
    assert_eq!(json["removed"], json!(["/p/t-y/plan-1.html"]));
    // ほかの project は残る。
    assert_eq!(app.paths("").await, ["/q/t-x/brief-1.html"]);
    assert_eq!(app.count("pages_fts").await, 1);
    assert_eq!(app.count("removed").await, 6);
    assert_eq!(
        app.place("p", "t-x", "brief", "role=tpm", "<p>2</p>").await,
        "2"
    );
    // 合う物が無ければ空。
    let (status, _, json) = app.remove("project=nothing").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json, json!({ "removed": [] }));
}

#[tokio::test]
async fn removing_needs_a_project_and_a_full_name_for_one_page() {
    let app = App::start().await;
    app.place("p", "SKILLS-14", "plan", SE, "<p>1</p>").await;
    for (query, words) in [
        ("key=SKILLS-14", None),
        ("", None),
        ("project=", None),
        ("project=..", Some("project")),
        ("project=.", Some("project")),
        ("project=p&key=..", Some("key")),
        ("project=a%20b", None),
        (
            "project=p&kind=plan&n=1",
            Some("n を書くときは key と kind も書く"),
        ),
        (
            "project=p&key=SKILLS-14&n=1",
            Some("n を書くときは key と kind も書く"),
        ),
        ("project=p&n=1", Some("n を書くときは key と kind も書く")),
        ("project=p&key=SKILLS-14&kind=plan&n=0", None),
        ("project=p&key=SKILLS-14&kind=plan&n=01", None),
        ("project=p&kind=Plan", None),
    ] {
        let (status, headers, json) = app.remove(query).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}: {json}");
        assert!(
            matches!(
                error_type(&headers),
                "ValidationException" | "SerializationException"
            ),
            "{query}: {headers:?}"
        );
        if let Some(words) = words {
            assert!(
                json["message"].as_str().unwrap_or("").contains(words),
                "{query}: {json}"
            );
        }
    }
    // browser の中のページからは消せない（同じ Origin でも）。
    for origin in ["null", "http://127.0.0.1:8801"] {
        let request = axum::http::Request::delete("/api/pages?project=p")
            .header(header::HOST, HOST)
            .header(header::ORIGIN, origin)
            .body(axum::body::Body::empty())
            .unwrap();
        let (status, headers, _) = app.send(request).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{origin}");
        assert_eq!(error_type(&headers), "Forbidden");
    }
    assert_eq!(app.paths("project=p").await, ["/p/SKILLS-14/plan-1.html"]);
}

/// ページを消しても、送り手の行は残る（外のキーは合ったまま）。
#[tokio::test]
async fn removing_a_page_keeps_its_sender() {
    let app = App::start().await;
    let as_a = "role=tpm&actorIssuer=tpm&actorRef=claude.session%3Aa";
    app.place("p", "k", "plan", as_a, "<p>x</p>").await;
    app.place("p", "k", "plan", as_a, "<p>y</p>").await;
    let (status, _, json) = app.remove("project=p").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["removed"].as_array().unwrap().len(), 2);
    assert_eq!(app.count("pages").await, 0);
    assert_eq!(app.count("senders").await, 2);
    assert_eq!(app.foreign_key_violations().await, 0);
}

/// レビューのページを消すと、見た物も消える（cascade）。見られたページを消しても、レビューの見た物は残る。
#[tokio::test]
async fn removing_a_review_page_removes_what_it_reviewed() {
    let app = App::start().await;
    app.place("p", "k", "plan", "role=tpm", "<p>計画</p>").await;
    app.place("p", "k", "review", "role=tpm", "<p>レビュー</p>")
        .await;
    let (status, _, _) = app
        .review(
            "/p/k/review-1.html",
            Some(json!({ "verdict": "reject", "targets": ["/p/k/plan-1.html"] })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(app.count("page_targets").await, 1);
    app.remove("project=p&key=k&kind=plan&n=1").await;
    assert_eq!(app.count("page_targets").await, 1);
    app.remove("project=p&key=k&kind=review&n=1").await;
    assert_eq!(app.count("page_targets").await, 0);
    assert_eq!(app.foreign_key_violations().await, 0);
}

/// 消した番号は、起こし直しても残る。
#[tokio::test]
async fn a_removed_number_survives_a_restart() {
    let mut app = App::start().await;
    app.place("p", "k", "plan", "role=tpm", "<p>1</p>").await;
    app.remove("project=p").await;
    app.reopen().await;
    assert_eq!(app.get("/p/k/plan-1.html").await.0, StatusCode::GONE);
    assert_eq!(
        app.place("p", "k", "plan", "role=tpm", "<p>2</p>").await,
        "2"
    );
}

/// DB の誤りは 500（何も消さない）、書く接続が空かなければ 503。
#[tokio::test]
async fn a_db_error_is_internal_and_a_busy_writer_is_unavailable() {
    let app = App::start().await;
    app.place("p", "k", "plan", "role=tpm", "<p>1</p>").await;
    let held = app.db.write().acquire().await.expect("書く接続");
    let (status, headers, json) = app.remove("project=p").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{json}");
    assert_eq!(error_type(&headers), "Unavailable");
    drop(held);

    app.execute("drop table pages_fts").await;
    let (status, headers, json) = app.remove("project=p").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{json}");
    assert_eq!(error_type(&headers), "InternalError");
    assert_eq!(app.count("pages").await, 1);
    assert_eq!(app.count("removed").await, 0);
}
