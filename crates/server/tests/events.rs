//! 新着の知らせ（SSE。`GET /api/events`）。ページを置くと名の無い event、file を置くと `event: file`、返事と印を書くと
//! `event: reply`・`event: review`（替わらなかった時も）。消した時は送らない。中身は、API の答えと同じ camelCase の記録
//! （ページの記録に `url` は入らない）。遅れて取りこぼした聞き手は、捨てた分を飛ばして続ける（容量 64）。

mod common;

use std::time::Duration;

use axum::body::Body;
use axum::http::{StatusCode, header};
use common::{App, PNG, get, html};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

/// 聞き始める。200・`text/event-stream` を見て、本文（流れ続ける物）を返す。
async fn listen(s: &App) -> Body {
    let response = s
        .app
        .clone()
        .oneshot(get("/api/events"))
        .await
        .expect("oneshot");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "text/event-stream"
    );
    response.into_body()
}

/// 次の event（名と、data の JSON）。keep-alive の注釈（`:` で始まる行）は読み飛ばす。5 秒で来なければ落ちる。
async fn next(body: &mut Body) -> (Option<String>, Value) {
    loop {
        let frame = tokio::time::timeout(Duration::from_secs(5), body.frame())
            .await
            .expect("5 秒で知らせが来ない")
            .expect("流れが切れた")
            .expect("流れの誤り");
        let Ok(data) = frame.into_data() else {
            continue;
        };
        let text = String::from_utf8(data.to_vec()).expect("UTF-8");
        let mut name = None;
        let mut json = None;
        for line in text.lines() {
            if let Some(v) = line.strip_prefix("event: ") {
                name = Some(v.to_owned());
            } else if let Some(v) = line.strip_prefix("data: ") {
                json = Some(serde_json::from_str(v).unwrap_or_else(|e| panic!("{e}: {v}")));
            }
        }
        if let Some(json) = json {
            return (name, json);
        }
    }
}

/// まだ何も来ていないか（200 ms 待つ）。
async fn quiet(body: &mut Body) {
    let waited = tokio::time::timeout(Duration::from_millis(200), body.frame()).await;
    assert!(waited.is_err(), "知らせが来た: {waited:?}");
}

#[tokio::test]
async fn a_put_is_pushed_to_listeners() {
    let s = App::start().await;
    let mut body = listen(&s).await;
    s.place(
        "p",
        "SKILLS-14",
        "result",
        "role=software-engineer&workspace=workspace:1",
        &html("結果", "できた"),
    )
    .await;
    let (name, json) = next(&mut body).await;
    // 名の無い event（前からの受け手は、これだけを読む）。
    assert_eq!(name, None);
    assert_eq!(json["path"], "/p/SKILLS-14/result-1.html");
    assert_eq!(json["role"], "software-engineer");
    assert_eq!(json["title"], "結果");
    assert_eq!(json["n"], "1");
    assert!(json.get("url").is_none(), "{json}");
    assert!(json.get("location").is_none(), "{json}");
    // 中身は、一覧の行と同じ記録（camelCase。時刻も同じ字）。
    let (_, _, listed) = s.json("/api/pages?project=p").await;
    assert_eq!(json, listed["pages"][0]);
    assert!(json["createdAt"].is_string(), "{json}");
}

#[tokio::test]
async fn a_file_is_pushed_as_a_file_event() {
    let s = App::start().await;
    let mut body = listen(&s).await;
    s.place_file("p", "K-1", "name=shot.png&role=tpm", "image/png", PNG)
        .await;
    let (name, json) = next(&mut body).await;
    assert_eq!(name.as_deref(), Some("file"));
    assert_eq!(json["path"], "/p/K-1/files/shot-1.png");
    assert_eq!(json["project"], "p");
    assert_eq!(json["key"], "K-1");
    assert!(json.get("url").is_none(), "{json}");
    let (_, _, listed) = s.json("/api/files?project=p").await;
    assert_eq!(json, listed["files"][0]);
}

#[tokio::test]
async fn replies_and_review_marks_are_pushed_even_when_nothing_changes() {
    let s = App::start().await;
    s.place("p", "K-1", "review", "role=staff-engineer", "<p>x</p>")
        .await;
    let page = "/p/K-1/review-1.html";
    let mut body = listen(&s).await;

    let (status, _, answer) = s.reply(page, json!({ "state": "waiting" })).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    let (name, json) = next(&mut body).await;
    assert_eq!(name.as_deref(), Some("reply"));
    assert_eq!(json, answer);
    assert_eq!(json["path"], page);
    assert!(json["askedAt"].is_string(), "{json}");
    // 同じ相手を待つ間の waiting は何も替えないが、知らせは送る。
    let (status, _, again) = s.reply(page, json!({ "state": "waiting" })).await;
    assert_eq!(status, StatusCode::OK);
    let (name, json) = next(&mut body).await;
    assert_eq!(name.as_deref(), Some("reply"));
    assert_eq!(json, again);

    let (status, _, marked) = s
        .review(
            page,
            Some(json!({ "verdict": "approve", "targets": ["/p/K-1/plan-1.html"] })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{marked}");
    let (name, json) = next(&mut body).await;
    assert_eq!(name.as_deref(), Some("review"));
    assert_eq!(json, marked);
    assert_eq!(json["verdict"], "approve");
    // 本文の無い印（何も替えない）も、知らせは送る。
    let (status, _, _) = s.review(page, None).await;
    assert_eq!(status, StatusCode::OK);
    let (name, _) = next(&mut body).await;
    assert_eq!(name.as_deref(), Some("review"));
}

#[tokio::test]
async fn refused_writes_and_deletes_push_nothing() {
    let s = App::start().await;
    s.place("p", "K-1", "plan", "role=tpm", "<p>x</p>").await;
    s.place_file("p", "K-1", "name=shot.png&role=tpm", "image/png", PNG)
        .await;
    let mut body = listen(&s).await;
    // 断った書き込み（409・404）は知らせない。
    let (status, _, _) = s
        .reply(
            "/p/K-1/plan-1.html",
            json!({ "state": "answered", "on": "review" }),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _, _) = s
        .reply("/p/K-1/plan-9.html", json!({ "state": "waiting" }))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // 消した時は送らない。
    let (status, _, json) = s.remove("project=p").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let (status, _, json) = s.remove_files("project=p").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    quiet(&mut body).await;
    // 次に置いた物が、次の知らせ。
    s.place("p", "K-1", "plan", "role=tpm", "<p>y</p>").await;
    let (name, json) = next(&mut body).await;
    assert_eq!(name, None);
    assert_eq!(json["path"], "/p/K-1/plan-2.html");
}

#[tokio::test]
async fn a_lagging_listener_drops_what_it_missed_and_goes_on() {
    let s = App::start().await;
    let mut body = listen(&s).await;
    // 読まずに 70 枚。覚えておけるのは新しい 64 件なので、初めの 6 件は捨てる。
    for _ in 0..70 {
        s.place("p", "K-1", "result", "role=tpm", "<p>x</p>").await;
    }
    let (_, json) = next(&mut body).await;
    assert_eq!(json["path"], "/p/K-1/result-7.html");
    for n in 8..=70 {
        let (_, json) = next(&mut body).await;
        assert_eq!(json["path"], format!("/p/K-1/result-{n}.html"));
    }
    s.place("p", "K-1", "result", "role=tpm", "<p>x</p>").await;
    let (_, json) = next(&mut body).await;
    assert_eq!(json["path"], "/p/K-1/result-71.html");
}

#[tokio::test]
async fn every_listener_hears_each_put() {
    let s = App::start().await;
    let mut a = listen(&s).await;
    let mut b = listen(&s).await;
    s.place("p", "K-1", "plan", "role=tpm", "<p>x</p>").await;
    assert_eq!(next(&mut a).await.1["path"], "/p/K-1/plan-1.html");
    assert_eq!(next(&mut b).await.1["path"], "/p/K-1/plan-1.html");
}
