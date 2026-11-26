//! SetReplyState（`PUT …/pages/{n}/reply`）と、返事の状態で探す（`GET /api/pages?reply&on`）振る舞い（処理より先に書く）。
//! v0.8.0 の tag の tests/api.rs と tests/review.rs の返事の試験を、1.0.0 の道と欄の名で見る。
mod common;

use std::time::Duration;

use axum::http::{Request, StatusCode, header};
use common::{App, HOST, error_type, html, json_request, keys};
use serde_json::{Value, json};

const CONFIRM: &str = "/p/SKILLS-14/confirm-1.html";
const BRIEF: &str = "/p/SKILLS-14/brief-1.html";
const REVIEW: &str = "/p/SKILLS-14/review-1.html";
const SE: &str = "role=software-engineer&workspace=workspace:1";

fn waiting(on: Option<&str>) -> Value {
    match on {
        Some(on) => json!({ "state": "waiting", "on": on }),
        None => json!({ "state": "waiting" }),
    }
}

fn answered(on: Option<&str>) -> Value {
    match on {
        Some(on) => json!({ "state": "answered", "on": on }),
        None => json!({ "state": "answered" }),
    }
}

/// 返事の状態を書き、200 を見て、ページの記録を返す。
async fn ok(app: &App, page: &str, body: Value) -> Value {
    let (status, _, json) = app.reply(page, body.clone()).await;
    assert_eq!(status, StatusCode::OK, "{page} {body}: {json}");
    json
}

/// 確認・計画・レビューの依頼書・レビューのページを 1 枚ずつ置く。
async fn setup() -> App {
    let app = App::start().await;
    for kind in ["confirm", "plan", "brief", "review"] {
        app.place("p", "SKILLS-14", kind, SE, &html(kind, "本文"))
            .await;
    }
    app
}

#[tokio::test]
async fn a_page_waits_for_a_reply_until_it_is_answered() {
    let app = setup().await;
    // 置いただけでは、返事の状態は無い（欄も出ない）。
    let (_, _, listed) = app.json("/api/pages?project=p&kind=confirm").await;
    assert!(listed["pages"][0].get("askedAt").is_none());
    assert!(listed["pages"][0].get("answeredAt").is_none());
    assert!(app.paths("project=p&reply=waiting").await.is_empty());
    assert!(app.paths("project=p&reply=answered").await.is_empty());

    let asked = ok(&app, CONFIRM, waiting(None)).await;
    assert_eq!(asked["path"], CONFIRM);
    assert_eq!(asked["title"], "confirm");
    let asked_at = asked["askedAt"].as_str().unwrap().to_owned();
    assert!(asked_at.ends_with('Z'), "{asked_at}");
    // Product Manager の待ちには waitsOn が出ない。答えに url は無い。
    assert_eq!(
        keys(&asked),
        [
            "askedAt",
            "createdAt",
            "key",
            "kind",
            "n",
            "path",
            "project",
            "role",
            "size",
            "title",
            "workspace"
        ]
    );
    assert_eq!(app.paths("project=p&reply=waiting").await, [CONFIRM]);
    assert!(app.paths("project=p&reply=answered").await.is_empty());

    // 同じ状態を 2 度書いても、時刻は初めの物を保つ。
    tokio::time::sleep(Duration::from_millis(5)).await;
    assert_eq!(ok(&app, CONFIRM, waiting(None)).await, asked);
    assert_eq!(ok(&app, CONFIRM, waiting(Some("pm"))).await, asked);

    let done = ok(&app, CONFIRM, answered(None)).await;
    assert_eq!(done["askedAt"], asked_at.as_str());
    let answered_at = done["answeredAt"].as_str().unwrap().to_owned();
    assert!(
        instant(&answered_at) > instant(&asked_at),
        "{answered_at} {asked_at}"
    );
    assert!(app.paths("project=p&reply=waiting").await.is_empty());
    assert_eq!(app.paths("project=p&reply=answered").await, [CONFIRM]);
    tokio::time::sleep(Duration::from_millis(5)).await;
    assert_eq!(ok(&app, CONFIRM, answered(None)).await, done);

    // 一覧の記録にも出る（答えと同じ記録）。
    let (_, _, listed) = app.json("/api/pages?project=p&kind=confirm").await;
    assert_eq!(listed["pages"][0], done);

    // 答えた後にもう 1 度求めると、求めた時刻は今になり、答えた時刻は消える。
    tokio::time::sleep(Duration::from_millis(5)).await;
    let again = ok(&app, CONFIRM, waiting(None)).await;
    assert!(instant(again["askedAt"].as_str().unwrap()) > instant(&answered_at));
    assert!(again.get("answeredAt").is_none());
    assert_eq!(app.paths("reply=waiting").await, [CONFIRM]);
}

#[tokio::test]
async fn a_page_nobody_asked_about_can_be_marked_answered() {
    let app = setup().await;
    let done = ok(&app, CONFIRM, answered(None)).await;
    assert!(done["answeredAt"].is_string());
    assert_eq!(done["askedAt"], done["answeredAt"]);
    assert_eq!(ok(&app, CONFIRM, answered(Some("pm"))).await, done);
}

#[tokio::test]
async fn a_review_wait_is_not_a_reply_the_product_manager_owes() {
    let app = setup().await;
    ok(&app, CONFIRM, waiting(None)).await;
    let brief = ok(&app, BRIEF, waiting(Some("review"))).await;
    assert_eq!(brief["waitsOn"], "review");
    assert!(brief["askedAt"].is_string());
    let escalated = ok(&app, REVIEW, waiting(Some("tpm"))).await;
    assert_eq!(escalated["waitsOn"], "tpm");

    // on が無ければ Product Manager の分だけ。
    assert_eq!(app.paths("reply=waiting").await, [CONFIRM]);
    assert_eq!(app.paths("reply=waiting&on=pm").await, [CONFIRM]);
    assert_eq!(app.paths("reply=waiting&on=review").await, [BRIEF]);
    assert_eq!(app.paths("reply=waiting&on=tpm").await, [REVIEW]);
    // Product Manager の待ちの記録には waitsOn が出ない。
    let (_, _, listed) = app.json("/api/pages?kind=confirm").await;
    assert!(listed["pages"][0].get("waitsOn").is_none(), "{listed}");

    // 答えると、その相手の「答えた」に入る。Product Manager の「答えた」には入らない。
    let done = ok(&app, BRIEF, answered(Some("review"))).await;
    assert_eq!(done["waitsOn"], "review");
    assert!(done["answeredAt"].is_string());
    assert!(app.paths("reply=answered").await.is_empty());
    assert_eq!(app.paths("reply=answered&on=review").await, [BRIEF]);
    // 同じ答えを 2 度書いても、同じ記録（答えた時刻を保ち、待ちの相手も替えない）。
    tokio::time::sleep(Duration::from_millis(5)).await;
    assert_eq!(ok(&app, BRIEF, answered(Some("review"))).await, done);
    // 同じ相手の待ちを 2 度書いても、同じ記録。
    assert_eq!(ok(&app, REVIEW, waiting(Some("tpm"))).await, escalated);
}

/// 409 `Conflict` の時は、何も替えない。
async fn conflict(app: &App, page: &str, body: Value) {
    let (status, headers, json) = app.reply(page, body.clone()).await;
    assert_eq!(status, StatusCode::CONFLICT, "{page} {body}: {json}");
    assert_eq!(error_type(&headers), "Conflict", "{page} {body}");
    assert!(
        json["message"]
            .as_str()
            .unwrap_or("")
            .contains("何も替えていない"),
        "{json}"
    );
}

#[tokio::test]
async fn answering_for_the_wrong_party_changes_nothing() {
    let app = setup().await;
    // 待ちにしていない依頼書を、レビューの相手として答えたにはできない。
    conflict(&app, BRIEF, answered(Some("review"))).await;
    conflict(&app, BRIEF, answered(Some("tpm"))).await;
    assert!(app.paths("reply=answered").await.is_empty());
    assert!(app.paths("reply=answered&on=review").await.is_empty());
    let (_, _, listed) = app.json("/api/pages?kind=brief").await;
    assert!(listed["pages"][0].get("askedAt").is_none(), "{listed}");

    // レビューを待つ依頼書は、on の無い答え（Product Manager）でも、ほかの相手でも替わらない。
    let brief = ok(&app, BRIEF, waiting(Some("review"))).await;
    for body in [answered(None), answered(Some("pm")), answered(Some("tpm"))] {
        conflict(&app, BRIEF, body).await;
    }
    assert_eq!(app.paths("reply=waiting&on=review").await, [BRIEF]);
    assert!(app.paths("reply=answered").await.is_empty());

    // Product Manager の確認を、レビューの相手として答えたにはできない。
    ok(&app, CONFIRM, waiting(None)).await;
    conflict(&app, CONFIRM, answered(Some("review"))).await;
    assert_eq!(app.paths("reply=waiting").await, [CONFIRM]);

    // 待ちの側も、今ほかの相手を待っているページは替えない（待ちを黙って移さない）。
    conflict(&app, BRIEF, waiting(None)).await;
    conflict(&app, BRIEF, waiting(Some("tpm"))).await;
    conflict(&app, CONFIRM, waiting(Some("review"))).await;
    assert_eq!(app.paths("reply=waiting&on=review").await, [BRIEF]);
    assert_eq!(app.paths("reply=waiting").await, [CONFIRM]);
    let (_, _, listed) = app.json("/api/pages?kind=brief").await;
    assert_eq!(listed["pages"][0], brief);

    // 答えた後の求め直しは、相手を替えてよい。
    ok(&app, BRIEF, answered(Some("review"))).await;
    let again = ok(&app, BRIEF, waiting(Some("tpm"))).await;
    assert_eq!(again["waitsOn"], "tpm");
    assert!(again.get("answeredAt").is_none());
    // Product Manager に替えれば、waitsOn は消える。
    ok(&app, BRIEF, answered(Some("tpm"))).await;
    let pm = ok(&app, BRIEF, waiting(None)).await;
    assert!(pm.get("waitsOn").is_none(), "{pm}");
    assert_eq!(app.paths("reply=waiting").await, [BRIEF, CONFIRM]);
}

#[tokio::test]
async fn bad_reply_arguments_are_refused_and_a_missing_page_is_not_found() {
    let app = setup().await;
    let uri = |path: &str| format!("{path}/reply");
    let page = "/api/projects/p/keys/SKILLS-14/kinds/confirm/pages";
    for (uri, body) in [
        // state は要る。waiting か answered だけ。
        (uri(&format!("{page}/1")), json!({})),
        (uri(&format!("{page}/1")), json!({ "state": "done" })),
        (
            uri(&format!("{page}/1")),
            json!({ "state": "waiting", "on": "staff" }),
        ),
        (uri(&format!("{page}/1")), json!({ "state": 1 })),
        // 番号は頭に 0 の無い 1 以上の数の字。
        (uri(&format!("{page}/0")), waiting(None)),
        (uri(&format!("{page}/01")), waiting(None)),
        (uri(&format!("{page}/x")), waiting(None)),
        // 種類は名の形。project・key は . と .. が駄目。
        (
            uri("/api/projects/p/keys/SKILLS-14/kinds/Confirm/pages/1"),
            waiting(None),
        ),
        (
            uri("/api/projects/../keys/SKILLS-14/kinds/confirm/pages/1"),
            waiting(None),
        ),
        (
            uri("/api/projects/p/keys/./kinds/confirm/pages/1"),
            waiting(None),
        ),
        (
            uri("/api/projects/a%20b/keys/SKILLS-14/kinds/confirm/pages/1"),
            waiting(None),
        ),
    ] {
        let (status, headers, json) = app
            .send_json(json_request("PUT", &uri, Some(body.clone())))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri} {body}: {json}");
        assert!(
            matches!(
                error_type(&headers),
                "ValidationException" | "SerializationException"
            ),
            "{uri}: {headers:?}"
        );
    }
    // JSON でない本文と、本文の無い要求も 400。
    let request = Request::put(uri(&format!("{page}/1")))
        .header(header::HOST, HOST)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from("state=waiting"))
        .unwrap();
    assert_eq!(app.send(request).await.0, StatusCode::BAD_REQUEST);
    let (status, _, _) = app
        .send_json(json_request("PUT", &uri(&format!("{page}/1")), None))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(app.paths("reply=waiting").await.is_empty());

    for path in [
        "/p/SKILLS-14/confirm-2.html",
        "/other/SKILLS-14/confirm-1.html",
        "/p/other/confirm-1.html",
        "/p/SKILLS-14/result-1.html",
    ] {
        let (status, headers, json) = app.reply(path, answered(None)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}: {json}");
        assert_eq!(error_type(&headers), "NotFound", "{path}");
        assert!(
            json["message"].as_str().unwrap_or("").contains("無い"),
            "{json}"
        );
    }

    // 消したページも 404（410 でない）。
    ok(&app, CONFIRM, waiting(None)).await;
    let (status, _, _) = app.remove("project=p&key=SKILLS-14&kind=confirm&n=1").await;
    assert_eq!(status, StatusCode::OK);
    let (status, headers, _) = app.reply(CONFIRM, answered(None)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error_type(&headers), "NotFound");
    assert!(app.paths("reply=waiting").await.is_empty());
}

#[tokio::test]
async fn a_page_in_a_browser_cannot_write_a_reply() {
    let app = setup().await;
    for origin in ["null", "http://127.0.0.1:8801", "https://evil.example"] {
        let mut request = json_request(
            "PUT",
            "/api/projects/p/keys/SKILLS-14/kinds/confirm/pages/1/reply",
            Some(waiting(None)),
        );
        request
            .headers_mut()
            .insert(header::ORIGIN, origin.parse().unwrap());
        let (status, headers, _) = app.send(request).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{origin}");
        assert_eq!(error_type(&headers), "Forbidden", "{origin}");
    }
    assert!(app.paths("reply=waiting").await.is_empty());
}

#[tokio::test]
async fn a_reply_survives_a_restart() {
    let mut app = setup().await;
    ok(&app, CONFIRM, waiting(None)).await;
    ok(&app, REVIEW, waiting(Some("tpm"))).await;
    app.reopen().await;
    assert_eq!(app.paths("reply=waiting").await, [CONFIRM]);
    assert_eq!(app.paths("reply=waiting&on=tpm").await, [REVIEW]);
}

/// 送り手のあるページの返事の答えには、送り手が出る。
#[tokio::test]
async fn the_answer_has_the_sender_when_the_page_has_one() {
    let app = App::start().await;
    app.place(
        "p",
        "k",
        "confirm",
        "role=tpm&actorIssuer=tpm&actorRef=claude.session%3Aa",
        "<p>x</p>",
    )
    .await;
    let asked = ok(&app, "/p/k/confirm-1.html", waiting(None)).await;
    assert_eq!(
        asked["sender"],
        json!({ "issuer": "tpm", "ref": "claude.session:a" })
    );
}

/// DB の誤りは 500、書く接続が空かなければ 503。どちらも何も替えない。
#[tokio::test]
async fn a_db_error_is_internal_and_a_busy_writer_is_unavailable() {
    let app = setup().await;
    let held = app.db.write().acquire().await.expect("書く接続");
    let (status, headers, json) = app.reply(CONFIRM, waiting(None)).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{json}");
    assert_eq!(error_type(&headers), "Unavailable");
    drop(held);
    assert!(app.paths("reply=waiting").await.is_empty());

    app.execute("drop table page_targets").await;
    let (status, headers, json) = app.reply(CONFIRM, waiting(None)).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{json}");
    assert_eq!(error_type(&headers), "InternalError");
    let asked: Option<String> =
        common::scalar(&app, "select asked_at from pages where kind = 'confirm'").await;
    assert_eq!(asked, None);
}

/// 答えの時刻を、比べられる時刻にする。答えの字は小数の後ろの 0 を落とす（`…46.300Z` は `…46.3Z`）ので、
/// 字のまま比べると `…46.315Z` が `…46.3Z` より前になる（CI で、ミリ秒が 0 で終わる時だけ落ちた）。
fn instant(s: &str) -> time::OffsetDateTime {
    time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|e| panic!("{s}: {e}"))
}
