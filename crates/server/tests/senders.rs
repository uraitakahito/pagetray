//! 送り手の記録（処理より先に書く）。置く時に、付随する id（`actorIssuer`・`actorRef`）で castlist に送り手を聞き
//! （生成した castlist の client。TLS の無い HTTP/1.1・1 秒・送り直さない）、その答えを残す。
//! castlist は、試験の中で待ち受ける偽物（common/castlist.rs）。決まりは docs/src/api.md の「送り手」。
mod common;

use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::StatusCode;
use common::castlist::{
    ACTOR_A, ACTOR_B, AS_OF, AS_OF_STORED, Fake, NAME_A, NAME_B, Reply, answer, closed_url,
};
use common::{App, PNG, error_type, html, pages_uri, put_page, scalar};
use http_body_util::BodyExt;
use pagetray_server::castlist::Castlist;
use serde_json::{Value, json};
use tower::ServiceExt;

const SE: &str = "role=software-engineer&workspace=workspace:1";
const AS_A: &str = "actorIssuer=tpm&actorRef=claude.session%3Aa";
const AS_PM: &str = "actorIssuer=tpm&actorRef=tpm.person%3Apm";
const NOBODY: &str = "actorIssuer=tpm&actorRef=claude.session%3Anobody";

/// 何も残っていないか（ページ・索引・送り手）。
async fn nothing_stored(app: &App) {
    assert_eq!(app.count("pages").await, 0, "pages");
    assert_eq!(app.count("pages_fts").await, 0, "pages_fts");
    assert_eq!(app.count("senders").await, 0, "senders");
}

fn query(actor: &str) -> String {
    format!("{SE}&{actor}")
}

/// 聞けない時の答え: 503 `Unavailable`。`message` は「castlist に聞けない」で始まる。
fn assert_unavailable(status: StatusCode, headers: &axum::http::HeaderMap, json: &Value) {
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{json}");
    assert_eq!(error_type(headers), "Unavailable", "{json}");
    assert!(
        json["message"]
            .as_str()
            .unwrap_or("")
            .starts_with("castlist に聞けない"),
        "{json}"
    );
}

#[tokio::test]
async fn a_page_from_a_known_sender_keeps_the_answer() {
    let fake = Fake::start().await;
    let app = App::with_castlist(&fake.url).await;
    let (status, _, json) = app
        .put("p", "SKILLS-14", "plan", &query(AS_A), html("計画", "本文"))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    assert_eq!(
        json["sender"],
        json!({
            "issuer": "tpm",
            "ref": "claude.session:a",
            "actor": { "id": ACTOR_A, "name": NAME_A, "kind": "ai" },
            "resolvedAt": AS_OF,
        })
    );
    // 答えた時刻（asOf）は、pagetray の残す字（24 字）で残す。
    assert_eq!(
        app.senders().await,
        [(
            "tpm".to_owned(),
            "claude.session:a".to_owned(),
            Some(ACTOR_A.to_owned()),
            Some(NAME_A.to_owned()),
            Some("ai".to_owned()),
            Some(AS_OF_STORED.to_owned()),
        )]
    );
    // ページは、足した送り手の行を指す。行を足した時刻も残す字。
    let pointed: i64 = scalar(
        &app,
        "select count(*) from pages p join senders s on s.id = p.sender_id",
    )
    .await;
    assert_eq!(pointed, 1);
    let created_at: String = scalar(&app, "select created_at from senders").await;
    assert_eq!(created_at.len(), 24, "{created_at}");
    assert!(created_at.ends_with('Z'), "{created_at}");

    let (status, _, json) = app
        .put("p", "SKILLS-14", "brief", &query(AS_PM), "<p>x</p>")
        .await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    assert_eq!(json["sender"]["actor"]["id"], ACTOR_B);
    assert_eq!(json["sender"]["actor"]["name"], NAME_B);
    assert_eq!(json["sender"]["actor"]["kind"], "human");
    assert_eq!(fake.requests().len(), 2);
}

/// 送り手の行は、置くたびに 1 行足す（同じ送り手でもまとめない）。
#[tokio::test]
async fn every_put_adds_its_own_sender_row() {
    let fake = Fake::start().await;
    let app = App::with_castlist(&fake.url).await;
    for _ in 0..2 {
        app.place("p", "k", "plan", &query(AS_A), "<p>x</p>").await;
    }
    assert_eq!(app.senders().await.len(), 2);
    let distinct: i64 = scalar(&app, "select count(distinct sender_id) from pages").await;
    assert_eq!(distinct, 2);
}

/// castlist への要求: `POST /api/resolve`、`Host: 127.0.0.1`（castlist はほかの Host を断る。つなぐ先の名が
/// localhost でも）、`Origin` は付けない、本文は issuer と refs。
#[tokio::test]
async fn castlist_is_asked_as_127_0_0_1_without_an_origin() {
    let fake = Fake::start().await;
    let app = App::with_castlist(&format!("http://localhost:{}", fake.port)).await;
    app.place("p", "k", "plan", &query(AS_A), "<p>x</p>").await;
    let requests = fake.requests();
    assert_eq!(requests.len(), 1, "{requests:?}");
    let (head, body) = requests[0].split_once("\r\n\r\n").unwrap();
    let mut lines = head.split("\r\n");
    assert_eq!(lines.next(), Some("POST /api/resolve HTTP/1.1"));
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
    assert_eq!(value("host"), ["127.0.0.1"], "{head}");
    assert!(value("origin").is_empty(), "{head}");
    assert_eq!(value("content-type"), ["application/json"], "{head}");
    let body: Value = serde_json::from_str(body).unwrap();
    assert_eq!(
        body,
        json!({ "issuer": "tpm", "refs": ["claude.session:a"] })
    );
}

#[tokio::test]
async fn when_castlist_is_not_listening_nothing_is_stored() {
    let app = App::with_castlist(&closed_url()).await;
    let (status, headers, json) = app.put("p", "k", "plan", &query(AS_A), "<p>x</p>").await;
    assert_unavailable(status, &headers, &json);
    nothing_stored(&app).await;
}

/// castlist が 1 秒で答えなければ、待たずに断る。送り直さない（castlist に聞くのは 1 度だけ）。
#[tokio::test]
async fn when_castlist_is_too_slow_nothing_is_stored() {
    let fake = Fake::slow(Duration::from_millis(1500)).await;
    let app = App::with_castlist(&fake.url).await;
    let started = Instant::now();
    let (status, headers, json) = app.put("p", "k", "plan", &query(AS_A), "<p>x</p>").await;
    let took = started.elapsed();
    assert_unavailable(status, &headers, &json);
    assert!(took >= Duration::from_millis(900), "{took:?}");
    assert!(took < Duration::from_millis(1400), "{took:?}");
    nothing_stored(&app).await;
    // 答えの遅い castlist が、後から 2 度目を受けていないか（送り直していないか）。
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert_eq!(fake.requests().len(), 1);
}

/// 200 でない答えは、聞けなかったとする（送り直さない。castlist の 503 は @retryable だが、1 度だけ聞く）。
#[tokio::test]
async fn when_castlist_answers_an_error_nothing_is_stored() {
    for code in [400, 403, 500, 503] {
        let fake = Fake::answering(code).await;
        let app = App::with_castlist(&fake.url).await;
        let (status, headers, json) = app.put("p", "k", "plan", &query(AS_A), "<p>x</p>").await;
        assert_unavailable(status, &headers, &json);
        nothing_stored(&app).await;
        assert_eq!(fake.requests().len(), 1, "{code}: 送り直した");
    }
}

/// 読めない答え・形の合わない答えは、聞けなかったとする。
#[tokio::test]
async fn when_the_answer_is_unreadable_nothing_is_stored() {
    let known =
        r#"{"id":"019a0000-0000-7000-8000-00000000000a","name":"t-23zs.swe.1","kind":"ai"}"#;
    let with_actor = |actor: &str| {
        format!(r#"{{"asOf":"{AS_OF}","results":[{{"ref":"claude.session:a","actor":{actor}}}]}}"#)
    };
    for body in [
        "not json".to_owned(),
        "{}".to_owned(),
        // 聞いていない送り手の答え。
        format!(r#"{{"asOf":"{AS_OF}","results":[{{"ref":"claude.session:b","actor":{known}}}]}}"#),
        // 時刻の無い答え・読めない時刻。
        format!(r#"{{"results":[{{"ref":"claude.session:a","actor":{known}}}]}}"#),
        with_actor(known).replace(AS_OF, "yesterday"),
        // id に、URL を壊す字がある・id が無い。
        with_actor(&known.replace("019a0000-", "../")),
        with_actor(r#"{"name":"t-23zs.swe.1","kind":"ai"}"#),
        // 種類が ai・human でない。
        with_actor(&known.replace(r#""ai""#, r#""robot""#)),
        // 名が空・129 字・制御字を持つ。
        with_actor(&known.replace("t-23zs.swe.1", "")),
        with_actor(&known.replace("t-23zs.swe.1", &"a".repeat(129))),
        with_actor(&known.replace("t-23zs.swe.1", "a\\u0007b")),
    ] {
        let fake = Fake::body(body.clone()).await;
        let app = App::with_castlist(&fake.url).await;
        let (status, headers, json) = app.put("p", "k", "plan", &query(AS_A), "<p>x</p>").await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}: {json}");
        assert_unavailable(status, &headers, &json);
        nothing_stored(&app).await;
    }
    // 128 字の名は受ける。
    let long = "a".repeat(128);
    let fake = Fake::body(with_actor(&known.replace("t-23zs.swe.1", &long))).await;
    let app = App::with_castlist(&fake.url).await;
    let (status, _, json) = app.put("p", "k", "plan", &query(AS_A), "<p>x</p>").await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    assert_eq!(json["sender"]["actor"]["name"], long);
}

/// chunked の答えも読む。
#[tokio::test]
async fn a_chunked_answer_is_read() {
    let fake = Fake::with(Duration::ZERO, |asked| {
        Reply::Chunked(answer(asked).to_string())
    })
    .await;
    let app = App::with_castlist(&fake.url).await;
    let (status, _, json) = app.put("p", "k", "plan", &query(AS_A), "<p>x</p>").await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    assert_eq!(json["sender"]["actor"]["name"], NAME_A);
}

/// castlist に登録の無い送り手は 400 `ValidationException`（castlist の PutEvent が、引けない `by` を 400 にするのと同じ）。
/// 何も残さない。
#[tokio::test]
async fn a_sender_castlist_does_not_know_is_refused() {
    let fake = Fake::start().await;
    let app = App::with_castlist(&fake.url).await;
    let (status, headers, json) = app.put("p", "k", "plan", &query(NOBODY), "<p>x</p>").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
    assert_eq!(error_type(&headers), "ValidationException", "{json}");
    assert!(
        json["message"]
            .as_str()
            .unwrap_or("")
            .starts_with("castlist に登録の無い送り手"),
        "{json}"
    );
    assert_eq!(json["fieldList"][0]["path"], "/actorRef", "{json}");
    assert_eq!(fake.requests().len(), 1);
    nothing_stored(&app).await;
}

/// castlist の URL が無ければ、聞かずに付随する id だけを残す（答えの欄は空）。
#[tokio::test]
async fn without_castlist_only_the_attached_ids_are_kept() {
    let app = App::start().await;
    let (status, _, json) = app.put("p", "k", "plan", &query(AS_A), "<p>x</p>").await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    assert_eq!(
        json["sender"],
        json!({ "issuer": "tpm", "ref": "claude.session:a" })
    );
    assert_eq!(
        app.senders().await,
        [(
            "tpm".to_owned(),
            "claude.session:a".to_owned(),
            None,
            None,
            None,
            None,
        )]
    );
}

/// 付随する id を書かなければ、送り手の行を作らず、答えに `sender` は出ない（castlist にも聞かない）。
#[tokio::test]
async fn without_actor_params_nothing_about_the_sender_is_kept() {
    let app = App::with_castlist(&closed_url()).await;
    let (status, _, json) = app.put("p", "k", "plan", SE, "<p>x</p>").await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    assert!(json.get("sender").is_none(), "{json}");
    assert_eq!(app.count("senders").await, 0);
    let sender_id: Option<i64> = scalar(&app, "select sender_id from pages").await;
    assert_eq!(sender_id, None);
}

/// 付随する id は両方書くか両方書かないかで、形も見る。合わなければ castlist に聞かずに 400。
/// ほかの引数が断られる時も、castlist には聞かない（検査の順: 引数 → 付随する id → castlist → DB）。
#[tokio::test]
async fn actor_params_must_come_together_and_in_shape() {
    let fake = Fake::start().await;
    let app = App::with_castlist(&fake.url).await;
    let long = format!(
        "actorIssuer=tpm&actorRef=claude.session%3A{}",
        "a".repeat(257)
    );
    for (project, actor) in [
        ("p", "actorIssuer=tpm"),
        ("p", "actorRef=claude.session%3Aa"),
        ("p", "actorIssuer=TPM&actorRef=claude.session%3Aa"),
        ("p", "actorIssuer=&actorRef=claude.session%3Aa"),
        ("p", "actorIssuer=tpm&actorRef="),
        ("p", "actorIssuer=tpm&actorRef=claude.session"),
        ("p", "actorIssuer=tpm&actorRef=claude.session%3A"),
        ("p", "actorIssuer=tpm&actorRef=%3Aa"),
        ("p", "actorIssuer=tpm&actorRef=Claude%3Aa"),
        ("p", "actorIssuer=tpm&actorRef=claude.session%3Aa%01b"),
        ("p", long.as_str()),
        // ほかの引数が合わない。
        ("api", AS_A),
    ] {
        let (status, headers, json) = app
            .put(project, "k", "plan", &query(actor), "<p>x</p>")
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{actor}: {json}");
        assert_eq!(error_type(&headers), "ValidationException", "{actor}");
    }
    let (_, _, json) = app
        .put("p", "k", "plan", &query("actorIssuer=tpm"), "<p>x</p>")
        .await;
    assert!(
        json["message"].as_str().unwrap_or("").contains("両方"),
        "{json}"
    );
    assert!(fake.requests().is_empty(), "断る物を castlist に聞いた");
    nothing_stored(&app).await;
    // 値には :・空白・/ があってよい（castlist の ai.common#ActorRef と同じ形。初めの : で分ける）。
    for actor in [
        "actorIssuer=tpm&actorRef=cmux.workspace.ref%3Aworkspace%3A3",
        "actorIssuer=tpm&actorRef=claude.session%3Aa%20b",
        "actorIssuer=tpm&actorRef=claude.session%3Aa%2Fb",
    ] {
        let (status, _, json) = app.put("p", "k", "plan", &query(actor), "<p>x</p>").await;
        // 偽の castlist は知らないので、castlist に聞いた上での 400。
        assert_eq!(status, StatusCode::BAD_REQUEST, "{actor}: {json}");
        assert!(
            json["message"]
                .as_str()
                .unwrap_or("")
                .starts_with("castlist に登録の無い送り手"),
            "{actor}: {json}"
        );
    }
    assert_eq!(fake.requests().len(), 3);
}

/// castlist に聞いた後で番号が使われていて 409 なら、送り手の行も残さない（同じ取引）。
#[tokio::test]
async fn a_conflict_after_asking_keeps_no_sender() {
    let fake = Fake::start().await;
    let app = App::with_castlist(&fake.url).await;
    let import = format!("{}&n=1", query(AS_A));
    app.place("p", "k", "plan", &import, "<p>1</p>").await;
    let (status, headers, json) = app.put("p", "k", "plan", &import, "<p>2</p>").await;
    assert_eq!(status, StatusCode::CONFLICT, "{json}");
    assert_eq!(error_type(&headers), "Conflict");
    assert_eq!(app.count("senders").await, 1);
    assert_eq!(app.count("pages").await, 1);
}

/// castlist を待つ間、書く接続を握らない。castlist が 900 ms かかる時に 10 枚を同時に置いても、
/// その間の読む要求（ページの本文）と、送り手の無い置く要求は待たされない。
#[tokio::test]
async fn waiting_for_castlist_does_not_hold_the_write_connection() {
    let fake = Fake::slow(Duration::from_millis(900)).await;
    let app = App::with_castlist(&fake.url).await;
    app.place("p", "k", "note", SE, "<p>x</p>").await;
    let mut puts = Vec::new();
    for i in 0..10 {
        let router = app.app.clone();
        puts.push(tokio::spawn(async move {
            let uri = pages_uri("p", "k", "result", &query(AS_A));
            let request = put_page(&uri, Body::from(format!("<title>{i}</title>")));
            let response = router.oneshot(request).await.unwrap();
            let status = response.status();
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            (
                status,
                serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null),
            )
        }));
    }
    // 10 枚が castlist を待ち始めるまで。
    tokio::time::sleep(Duration::from_millis(150)).await;
    let started = Instant::now();
    let (status, _, _) = app.get("/p/k/note-1.html").await;
    let read = started.elapsed();
    assert_eq!(status, StatusCode::OK);
    assert!(
        read < Duration::from_millis(200),
        "読むのが {read:?} 待った"
    );
    let started = Instant::now();
    app.place("p", "k", "note", SE, "<p>y</p>").await;
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
    assert_eq!(app.count("senders").await, 10);
    assert_eq!(fake.requests().len(), 10);
}

// --- file の送り手（CreateFile。ページと同じ決まり）---

/// file を置く引数（`SE` と名と、付随する id）。
fn file_query(name: &str, actor: &str) -> String {
    format!("{SE}&name={name}&{actor}")
}

#[tokio::test]
async fn a_file_from_a_known_sender_keeps_the_answer() {
    let fake = Fake::start().await;
    let app = App::with_castlist(&fake.url).await;
    let (status, _, json) = app
        .put_file(
            "p",
            "SKILLS-14",
            &file_query("a.png", AS_PM),
            Some("image/png"),
            PNG,
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    assert_eq!(fake.requests().len(), 1, "偽の castlist に聞いていない");
    assert_eq!(
        json["sender"],
        json!({
            "issuer": "tpm",
            "ref": "tpm.person:pm",
            "actor": { "id": ACTOR_B, "name": NAME_B, "kind": "human" },
            "resolvedAt": AS_OF,
        })
    );
    assert_eq!(
        app.senders().await,
        [(
            "tpm".to_owned(),
            "tpm.person:pm".to_owned(),
            Some(ACTOR_B.to_owned()),
            Some(NAME_B.to_owned()),
            Some("human".to_owned()),
            Some(AS_OF_STORED.to_owned()),
        )]
    );
    let pointed: i64 = scalar(
        &app,
        "select count(*) from files f join senders s on s.id = f.sender_id",
    )
    .await;
    assert_eq!(pointed, 1);
    // 一覧の行にも、同じ送り手が出る。
    let (_, _, listed) = app.json("/api/files?project=p").await;
    assert_eq!(listed["files"][0]["sender"], json["sender"]);
}

/// castlist の無い pagetray は、聞かずに付随する id だけを残す。付随する id が無ければ送り手の行を作らない。
#[tokio::test]
async fn a_file_without_castlist_keeps_only_the_attached_ids() {
    let app = App::start().await;
    let (status, _, json) = app
        .put_file("p", "k", &file_query("a.png", AS_A), Some("image/png"), PNG)
        .await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    assert_eq!(
        json["sender"],
        json!({ "issuer": "tpm", "ref": "claude.session:a" })
    );
    let (status, _, json) = app
        .put_file(
            "p",
            "k",
            &format!("{SE}&name=b.png"),
            Some("image/png"),
            PNG,
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    assert!(json.get("sender").is_none(), "{json}");
    assert_eq!(app.count("senders").await, 1);
}

/// file の送り手も、聞けなければ 503・知らなければ 400（何も残さない）。
#[tokio::test]
async fn a_file_from_an_unknown_or_unreachable_sender_stores_nothing() {
    let fake = Fake::start().await;
    let app = App::with_castlist(&fake.url).await;
    let (status, headers, json) = app
        .put_file(
            "p",
            "k",
            &file_query("a.png", NOBODY),
            Some("image/png"),
            PNG,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
    assert_eq!(error_type(&headers), "ValidationException");
    assert_eq!(json["fieldList"][0]["path"], "/actorRef", "{json}");
    let closed = App::with_castlist(&closed_url()).await;
    let (status, headers, json) = closed
        .put_file("p", "k", &file_query("a.png", AS_A), Some("image/png"), PNG)
        .await;
    assert_unavailable(status, &headers, &json);
    for app in [&app, &closed] {
        assert_eq!(app.count("files").await, 0);
        assert_eq!(app.count("senders").await, 0);
    }
}

/// castlist には、ほかの検査（400・415）を見終えてから聞く。断る物は castlist に聞かない。
#[tokio::test]
async fn castlist_is_asked_after_the_other_file_checks() {
    let fake = Fake::start().await;
    let app = App::with_castlist(&fake.url).await;
    for (query, content_type, body, status) in [
        (
            format!("role=tpm&name=a.png&n=0&{AS_A}"),
            "image/png",
            PNG,
            StatusCode::BAD_REQUEST,
        ),
        (
            format!("name=a.png&{AS_A}"),
            "image/png",
            PNG,
            StatusCode::BAD_REQUEST,
        ),
        (
            file_query("a.svg", AS_A),
            "image/svg+xml",
            &b"<svg/>"[..],
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ),
        (
            file_query("a.png", AS_A),
            "image/jpeg",
            PNG,
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ),
        (
            file_query("a.png", AS_A),
            "image/png",
            b"%PDF-1.7",
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ),
    ] {
        let (got, _, json) = app
            .put_file("p", "k", &query, Some(content_type), body.to_vec())
            .await;
        assert_eq!(got, status, "{query}: {json}");
    }
    assert!(fake.requests().is_empty(), "{:?}", fake.requests());
    assert_eq!(app.count("senders").await, 0);
}

/// castlist に聞いた後で番号が使われていて 409 なら、送り手の行も残さない（同じ取引）。
#[tokio::test]
async fn a_file_conflict_after_asking_keeps_no_sender() {
    let fake = Fake::start().await;
    let app = App::with_castlist(&fake.url).await;
    let import = format!("{}&n=1", file_query("a.png", AS_A));
    app.place_file("p", "k", &import, "image/png", PNG).await;
    let (status, headers, json) = app
        .put_file("p", "k", &import, Some("image/png"), PNG)
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{json}");
    assert_eq!(error_type(&headers), "Conflict");
    assert_eq!(app.count("senders").await, 1);
    assert_eq!(app.count("files").await, 1);
}

/// browser の中のページからの上げ（`Origin` 付き）は、付随する id を付けても断る（2.0.0 で画面の form の例外を無くした）。
/// 何も残さず、castlist にも聞かない。
#[tokio::test]
async fn a_browser_upload_with_actor_params_is_refused() {
    let app = App::with_castlist(&closed_url()).await;
    let request =
        axum::http::Request::post(common::files_uri("p", "K-1", &format!("name=a.png&{AS_A}")))
            .header(axum::http::header::HOST, common::HOST)
            .header(
                axum::http::header::ORIGIN,
                format!("http://{}", common::HOST),
            )
            .header("sec-fetch-site", "same-origin")
            .header(axum::http::header::CONTENT_TYPE, "image/png")
            .body(Body::from(PNG))
            .unwrap();
    let (status, _, json) = app.send_json(request).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{json}");
    assert_eq!(app.count("files").await, 0);
    assert_eq!(app.count("senders").await, 0);
}

/// `actor=<castlist のアクターの id>` で、その送り手の file だけ（castlist の答えの無い送り手は当たらない）。
#[tokio::test]
async fn the_actor_filter_keeps_only_that_senders_files() {
    let fake = Fake::start().await;
    let app = App::with_castlist(&fake.url).await;
    app.place_file("p", "k", &file_query("a.png", AS_A), "image/png", PNG)
        .await;
    app.place_file("p", "k", &file_query("b.png", AS_PM), "image/png", PNG)
        .await;
    app.place_file("p", "k", &format!("{SE}&name=c.png"), "image/png", PNG)
        .await;
    let bare = App::start().await;
    bare.place_file("p", "k", &file_query("d.png", AS_A), "image/png", PNG)
        .await;
    assert_eq!(
        app.file_paths(&format!("actor={ACTOR_A}")).await,
        ["/p/k/files/a-1.png"]
    );
    assert_eq!(
        app.file_paths(&format!("actor={ACTOR_B}")).await,
        ["/p/k/files/b-1.png"]
    );
    assert_eq!(app.file_paths("").await.len(), 3);
    // ページの送り手では当たらない（file の一覧はページを見ない）。
    assert!(app.paths(&format!("actor={ACTOR_A}")).await.is_empty());
    // castlist の答えの無い送り手は当たらない。
    assert!(
        bare.file_paths(&format!("actor={ACTOR_A}"))
            .await
            .is_empty()
    );
}

/// file を消しても、送り手の行は残る（外のキーは合ったまま）。
#[tokio::test]
async fn removing_a_file_keeps_its_sender() {
    let app = App::start().await;
    app.place_file("p", "k", &file_query("a.png", AS_A), "image/png", PNG)
        .await;
    let (status, _, json) = app.remove_files("project=p").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(app.count("files").await, 0);
    assert_eq!(app.count("senders").await, 1);
    assert_eq!(app.foreign_key_violations().await, 0);
}

// --- castlist の居場所（PAGETRAY_CASTLIST_URL）---

const API: &str = "http://castlist.box:8080";

#[test]
fn only_http_host_and_port_is_taken() {
    for ok in [
        "http://127.0.0.1:8802",
        "http://127.0.0.1:8802/",
        "http://192.168.64.1:8802",
        "http://localhost:8802",
        "http://castlist.box:8080",
    ] {
        let c = Castlist::configure(Some(ok))
            .unwrap_or_else(|e| panic!("{ok}: {e}"))
            .unwrap();
        assert_eq!(c.api(), ok.trim_end_matches('/'));
    }
    for ng in [
        "127.0.0.1:8802",
        "https://127.0.0.1:8802",
        "http://127.0.0.1",
        "http://127.0.0.1:",
        "http://127.0.0.1:x",
        "http://127.0.0.1:8802/v1",
        "http://a@127.0.0.1:8802",
        "http://127.0.0.1:8802?x",
        "http://127.0.0.1:99999",
        "http://127.0.0.1:0",
        "http://:8802",
    ] {
        let api = Castlist::configure(Some(ng)).map(|_| ());
        assert!(
            api.as_ref()
                .is_err_and(|e| e.contains("PAGETRAY_CASTLIST_URL") && e.contains(ng)),
            "{ng}: {api:?}"
        );
    }
}

/// castlist の URL が無ければ（空でも）、castlist に聞かない。
#[test]
fn without_the_api_url_there_is_no_castlist() {
    for api in [None, Some("")] {
        assert!(Castlist::configure(api).unwrap().is_none(), "{api:?}");
    }
    assert_eq!(Castlist::configure(Some(API)).unwrap().unwrap().api(), API);
}
