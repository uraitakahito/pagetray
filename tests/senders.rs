//! 送り手の記録。置く時に、付随する id（`actor_issuer`・`actor_ref`）で castlist に送り手を聞き、その答えを残す。
//! castlist は、試験の中で待ち受ける偽物（common/castlist.rs）。

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use common::castlist::{ACTOR_A, ACTOR_B, AS_OF, Fake, NAME_A, NAME_B, closed_url};
use common::{PNG, SE, Server, html, request};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use tower::ServiceExt;

const AS_A: &str = "actor_issuer=tpm&actor_ref=claude.session:a";
const AS_PM: &str = "actor_issuer=tpm&actor_ref=tpm.person:pm";

/// 何も残っていないか（ページ・file・送り手）。
async fn nothing_stored(s: &Server) {
    assert_eq!(s.count("pages").await, 0, "pages");
    assert_eq!(s.count("files").await, 0, "files");
    assert_eq!(s.count("senders").await, 0, "senders");
}

#[tokio::test]
async fn a_page_from_a_known_sender_keeps_the_answer() {
    let fake = Fake::start().await;
    let s = Server::with_castlist(&fake.url).await;
    let (status, json) = s
        .put(&format!("{SE}&kind=plan&{AS_A}"), &html("計画", "本文"))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    assert_eq!(
        json["sender"],
        json!({
            "issuer": "tpm",
            "ref": "claude.session:a",
            "actor": { "id": ACTOR_A, "name": NAME_A, "kind": "ai" },
            "resolved_at": AS_OF,
        })
    );
    assert_eq!(
        s.senders().await,
        [(
            "tpm".to_owned(),
            "claude.session:a".to_owned(),
            Some(ACTOR_A.to_owned()),
            Some(NAME_A.to_owned()),
            Some("ai".to_owned()),
            Some(AS_OF.to_owned()),
        )]
    );
    let listed = s.json("/api/pages?project=p").await;
    assert_eq!(listed["pages"][0]["sender"], json["sender"]);

    // 一覧の UI: castlist の名（castlist のアクターの画面への link）と、その下に小さく付随する id。
    // link の頭は、聞いた先（container の中から届く所）でなく、既定の `http://127.0.0.1:8802`（host の browser から開く所）。
    let page = s.text("/?project=p").await;
    let link = format!(
        r#"<a class="actor" href="http://127.0.0.1:8802/actors/{ACTOR_A}" target="_blank" rel="noopener">{NAME_A}</a>"#
    );
    assert!(page.contains(&link), "{page}");
    assert!(!page.contains(&fake.url), "{page}");
    assert!(page.contains("<small>claude.session:a</small>"), "{page}");
}

#[tokio::test]
async fn a_file_from_a_known_sender_keeps_the_answer() {
    // 聞くのは `PAGETRAY_CASTLIST_URL`（偽の castlist）、画面の link の頭は `PAGETRAY_CASTLIST_LINK_URL`。
    let fake = Fake::start().await;
    let s = Server::with_castlist_link(&fake.url, "http://localhost:18802").await;
    let (status, _, json) = s
        .put_file(&format!("{SE}&name=a.png&{AS_PM}"), Some("image/png"), PNG)
        .await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    assert_eq!(fake.requests().len(), 1, "偽の castlist に聞いていない");
    assert_eq!(json["sender"]["actor"]["name"], NAME_B);
    assert_eq!(json["sender"]["actor"]["kind"], "human");
    assert_eq!(json["sender"]["ref"], "tpm.person:pm");
    assert_eq!(json["sender"]["resolved_at"], AS_OF);
    assert_eq!(s.senders().await.len(), 1);
    let listed = s.json("/api/files?project=p").await;
    assert_eq!(listed["files"][0]["sender"], json["sender"]);

    let page = s.text("/p/SKILLS-14/").await;
    assert!(
        page.contains(&format!(
            r#"href="http://localhost:18802/actors/{ACTOR_B}""#
        )),
        "{page}"
    );
    assert!(!page.contains(&fake.url), "{page}");
    assert!(page.contains("<small>tpm.person:pm</small>"), "{page}");
}

/// castlist への要求: `Host: 127.0.0.1`（castlist はほかの Host を断る）、`Origin` は付けない、本文は issuer と refs。
#[tokio::test]
async fn castlist_is_asked_as_127_0_0_1_without_an_origin() {
    let fake = Fake::start().await;
    let s = Server::with_castlist(&fake.url).await;
    s.place(&format!("{SE}&kind=plan&{AS_A}"), "<p>x</p>").await;
    let requests = fake.requests();
    assert_eq!(requests.len(), 1, "{requests:?}");
    let (head, body) = requests[0].split_once("\r\n\r\n").unwrap();
    let mut lines = head.split("\r\n");
    assert_eq!(lines.next(), Some("POST /v1/resolve HTTP/1.1"));
    let headers: Vec<(String, String)> = lines
        .map(|l| {
            let (name, value) = l.split_once(':').unwrap();
            (name.to_ascii_lowercase(), value.trim().to_owned())
        })
        .collect();
    let value = |name: &str| {
        headers
            .iter()
            .filter(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
            .collect::<Vec<_>>()
    };
    assert_eq!(value("host"), ["127.0.0.1"]);
    assert!(value("origin").is_empty(), "{head}");
    assert_eq!(value("content-type"), ["application/json"]);
    let body: Value = serde_json::from_str(body).unwrap();
    assert_eq!(
        body,
        json!({ "issuer": "tpm", "refs": ["claude.session:a"] })
    );
}

#[tokio::test]
async fn when_castlist_is_not_listening_nothing_is_stored() {
    let s = Server::with_castlist(&closed_url()).await;
    let (status, json) = s.put(&format!("{SE}&kind=plan&{AS_A}"), "<p>x</p>").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{json}");
    assert!(
        json["error"]
            .as_str()
            .unwrap()
            .starts_with("castlist に聞けない"),
        "{json}"
    );
    let (status, _, json) = s
        .put_file(&format!("{SE}&name=a.png&{AS_A}"), Some("image/png"), PNG)
        .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{json}");
    nothing_stored(&s).await;
}

/// castlist が 1 秒で答えなければ、待たずに断る。
#[tokio::test]
async fn when_castlist_is_too_slow_nothing_is_stored() {
    let fake = Fake::slow(Duration::from_millis(1500)).await;
    let s = Server::with_castlist(&fake.url).await;
    let started = Instant::now();
    let (status, json) = s.put(&format!("{SE}&kind=plan&{AS_A}"), "<p>x</p>").await;
    let took = started.elapsed();
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{json}");
    assert!(
        json["error"]
            .as_str()
            .unwrap()
            .starts_with("castlist に聞けない"),
        "{json}"
    );
    assert!(took >= Duration::from_millis(900), "{took:?}");
    assert!(took < Duration::from_millis(1400), "{took:?}");
    let (status, _, _) = s
        .put_file(&format!("{SE}&name=a.png&{AS_A}"), Some("image/png"), PNG)
        .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    nothing_stored(&s).await;
}

#[tokio::test]
async fn when_castlist_answers_an_error_nothing_is_stored() {
    let fake = Fake::answering(503, Duration::ZERO).await;
    let s = Server::with_castlist(&fake.url).await;
    let (status, json) = s.put(&format!("{SE}&kind=plan&{AS_A}"), "<p>x</p>").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{json}");
    nothing_stored(&s).await;
}

#[tokio::test]
async fn a_sender_castlist_does_not_know_is_refused() {
    let fake = Fake::start().await;
    let s = Server::with_castlist(&fake.url).await;
    let unknown = "actor_issuer=tpm&actor_ref=claude.session:nobody";
    let (status, json) = s
        .put(&format!("{SE}&kind=plan&{unknown}"), "<p>x</p>")
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{json}");
    assert!(
        json["error"]
            .as_str()
            .unwrap()
            .starts_with("castlist に登録の無い送り手"),
        "{json}"
    );
    let (status, _, json) = s
        .put_file(
            &format!("{SE}&name=a.png&{unknown}"),
            Some("image/png"),
            PNG,
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{json}");
    nothing_stored(&s).await;
}

/// castlist の URL が無ければ、付随する id だけを残す（答えの欄は空。画面では「未確認」）。
#[tokio::test]
async fn without_castlist_only_the_attached_ids_are_kept() {
    let s = Server::start().await;
    let (status, json) = s.put(&format!("{SE}&kind=plan&{AS_A}"), "<p>x</p>").await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    assert_eq!(
        json["sender"],
        json!({ "issuer": "tpm", "ref": "claude.session:a", "actor": null })
    );
    let (status, _, file) = s
        .put_file(&format!("{SE}&name=a.png&{AS_A}"), Some("image/png"), PNG)
        .await;
    assert_eq!(status, StatusCode::CREATED, "{file}");
    assert_eq!(file["sender"], json["sender"]);
    let unconfirmed = (
        "tpm".to_owned(),
        "claude.session:a".to_owned(),
        None,
        None,
        None,
        None,
    );
    assert_eq!(s.senders().await, [unconfirmed.clone(), unconfirmed]);
    let listed = s.json("/api/pages?project=p").await;
    assert_eq!(listed["pages"][0]["sender"], json["sender"]);

    let page = s.text("/p/SKILLS-14/").await;
    assert!(
        page.contains("未確認<small>claude.session:a</small>"),
        "{page}"
    );
    assert!(!page.contains("/actors/"), "{page}");
}

/// 付随する id を書かなければ、前と同じ（送り手の記録は作らず、答えに `sender` は出ない）。
#[tokio::test]
async fn without_actor_params_nothing_about_the_sender_is_kept() {
    let s = Server::with_castlist(&closed_url()).await;
    let (status, json) = s.put(&format!("{SE}&kind=plan"), "<p>x</p>").await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    assert!(json.get("sender").is_none(), "{json}");
    let (status, _, file) = s
        .put_file(&format!("{SE}&name=a.png"), Some("image/png"), PNG)
        .await;
    assert_eq!(status, StatusCode::CREATED);
    assert!(file.get("sender").is_none(), "{file}");
    assert_eq!(s.count("senders").await, 0);
    let listed = s.json("/api/pages?project=p").await;
    assert!(listed["pages"][0].get("sender").is_none());
    let page = s.text("/p/SKILLS-14/").await;
    assert!(!page.contains("未確認"), "{page}");
}

#[tokio::test]
async fn actor_params_must_come_together_and_in_shape() {
    let fake = Fake::start().await;
    let s = Server::with_castlist(&fake.url).await;
    for actor in [
        "actor_issuer=tpm",
        "actor_ref=claude.session:a",
        "actor_issuer=TPM&actor_ref=claude.session:a",
        "actor_issuer=&actor_ref=claude.session:a",
        "actor_issuer=tpm&actor_ref=claude.session",
        "actor_issuer=tpm&actor_ref=claude.session:",
        "actor_issuer=tpm&actor_ref=:a",
        "actor_issuer=tpm&actor_ref=Claude:a",
        "actor_issuer=tpm&actor_ref=claude.session:a%20b",
        "actor_issuer=tpm&actor_ref=claude.session:a%2Fb",
    ] {
        let (status, json) = s.put(&format!("{SE}&kind=plan&{actor}"), "<p>x</p>").await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{actor}: {json}");
        let (status, _, json) = s
            .put_file(&format!("{SE}&name=a.png&{actor}"), Some("image/png"), PNG)
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{actor}: {json}");
    }
    let long = format!(
        "actor_issuer=tpm&actor_ref=claude.session:{}",
        "a".repeat(257)
    );
    let (status, _) = s.put(&format!("{SE}&kind=plan&{long}"), "<p>x</p>").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(fake.requests().is_empty(), "断る物を castlist に聞いた");
    nothing_stored(&s).await;
    // 値には : があってよい（初めの : で分ける）。
    let (status, json) = s
        .put(
            &format!("{SE}&kind=plan&actor_issuer=tpm&actor_ref=cmux.workspace.ref:workspace:3"),
            "<p>x</p>",
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{json}");
}

/// 一覧の UI から上げた file は、付随する id を読まない（browser から送り手を名乗らせない）。
#[tokio::test]
async fn the_list_ui_upload_ignores_actor_params() {
    let s = Server::with_castlist(&closed_url()).await;
    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/files?project=p&key=K-1&name=a.png&{AS_A}"))
        .header(header::HOST, "127.0.0.1:8801")
        .header(header::ORIGIN, "http://127.0.0.1:8801")
        .header("sec-fetch-site", "same-origin")
        .header(header::CONTENT_TYPE, "image/png")
        .body(Body::from(PNG))
        .unwrap();
    let (status, _, body) = s.send(request).await;
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(status, StatusCode::CREATED, "{json}");
    assert_eq!(json["role"], "product-manager");
    assert!(json.get("sender").is_none(), "{json}");
    assert_eq!(s.count("senders").await, 0);
}

/// `?actor=<castlist のアクターの id>` で、その送り手の物だけに絞る。
#[tokio::test]
async fn the_actor_filter_keeps_only_that_senders_pages_and_files() {
    let fake = Fake::start().await;
    let s = Server::with_castlist(&fake.url).await;
    s.place(&format!("{SE}&kind=plan&{AS_A}"), "<title>a の計画</title>")
        .await;
    s.place(
        &format!("{SE}&kind=brief&{AS_PM}"),
        "<title>pm の依頼</title>",
    )
    .await;
    s.place(&format!("{SE}&kind=note"), "<title>送り手なし</title>")
        .await;
    s.place_file(&format!("{SE}&name=a.png&{AS_A}"), "image/png", PNG)
        .await;
    s.place_file(&format!("{SE}&name=b.png&{AS_PM}"), "image/png", PNG)
        .await;
    s.place_file(&format!("{SE}&name=c.png"), "image/png", PNG)
        .await;

    assert_eq!(
        s.paths(&format!("actor={ACTOR_A}")).await,
        ["/p/SKILLS-14/plan-1.html"]
    );
    assert_eq!(
        s.paths(&format!("actor={ACTOR_B}&kind=brief")).await,
        ["/p/SKILLS-14/brief-1.html"]
    );
    assert!(s.paths("actor=019a-nobody").await.is_empty());
    assert_eq!(s.paths("").await.len(), 3);
    assert_eq!(
        s.file_paths(&format!("actor={ACTOR_A}")).await,
        ["/p/SKILLS-14/files/a-1.png"]
    );
    assert_eq!(s.file_paths("").await.len(), 3);

    // 一覧の UI も、同じ引数で絞る。絞り込みの列に「送り手」（castlist の名と件数）が出る。
    let page = s.text(&format!("/?actor={ACTOR_A}")).await;
    assert!(
        page.contains(r#"<li data-path="/p/SKILLS-14/plan-1.html""#),
        "{page}"
    );
    assert!(!page.contains(r#"<li data-path="/p/SKILLS-14/brief-1.html""#));
    assert!(page.contains("絞り込みを外す"));
    let page = s.text("/").await;
    assert!(page.contains("<h2>送り手</h2>"), "{page}");
    assert!(
        page.contains(&format!(
            r#"<a href="/?actor={ACTOR_A}"><span>{NAME_A}</span><span class="n">1</span></a>"#
        )),
        "{page}"
    );
}

/// ページや file を消しても、送り手の記録は残る（外部キーは合ったまま）。
#[tokio::test]
async fn removing_a_page_keeps_its_sender() {
    let s = Server::start().await;
    s.place(&format!("{SE}&kind=plan&{AS_A}"), "<p>x</p>").await;
    s.place_file(&format!("{SE}&name=a.png&{AS_A}"), "image/png", PNG)
        .await;
    let (status, json) = s.remove("project=p").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["removed"].as_array().unwrap().len(), 1);
    let (status, _) = s.remove_files("project=p").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(s.count("pages").await, 0);
    assert_eq!(s.count("files").await, 0);
    assert_eq!(s.count("senders").await, 2);
    assert_eq!(s.foreign_key_violations().await, 0);
}

/// castlist を待つ間、書く接続を握らない。castlist が 900 ms かかる時に 10 枚を同時に置いても、
/// その間の一覧と、送り手の無い置く要求は待たされない。
#[tokio::test]
async fn waiting_for_castlist_does_not_hold_the_write_connection() {
    let fake = Fake::slow(Duration::from_millis(900)).await;
    let s = Server::with_castlist(&fake.url).await;
    let mut puts = Vec::new();
    for i in 0..10 {
        let app = s.app.clone();
        puts.push(tokio::spawn(async move {
            let uri = format!("/api/pages?{SE}&kind=result&{AS_A}");
            let body = Body::from(format!("<title>{i}</title>"));
            let response = app.oneshot(request("POST", &uri, body)).await.unwrap();
            let status = response.status();
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            (status, serde_json::from_slice::<Value>(&bytes).unwrap())
        }));
    }
    // 10 枚が castlist を待ち始めるまで。
    tokio::time::sleep(Duration::from_millis(150)).await;
    let started = Instant::now();
    s.json("/api/pages?project=p").await;
    let listed = started.elapsed();
    assert!(
        listed < Duration::from_millis(200),
        "一覧が {listed:?} 待った"
    );
    let started = Instant::now();
    s.place(&format!("{SE}&kind=note"), "<p>x</p>").await;
    let placed = started.elapsed();
    assert!(
        placed < Duration::from_millis(200),
        "置くのが {placed:?} 待った"
    );
    for put in puts {
        let (status, json) = put.await.unwrap();
        assert_eq!(status, StatusCode::CREATED, "{json}");
        assert_eq!(json["sender"]["actor"]["name"], NAME_A, "{json}");
    }
    assert_eq!(s.count("senders").await, 10);
    assert_eq!(fake.requests().len(), 10);
}
