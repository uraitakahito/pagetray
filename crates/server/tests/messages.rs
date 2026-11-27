//! メッセージの記録（処理より先に書く）: 置く・書き足す PutMessage（`PUT /api/projects/{project}/messages/{id}`）と、
//! 探す ListMessages（`GET /api/messages`）と、絞り込みの候補と件数 GetMessageFacets
//! （`GET /api/messages/facets`）と、新着の知らせ（`event: message-record`）。
//! pagetray は、roster が配ったメッセージの記録を写して見せるだけ（配らない・状態を自分で決めない）。
//! 決まりは docs/src/api.md の「メッセージの記録」。
mod common;

use std::time::Duration;

use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode, header};
use common::castlist::{ACTOR_A, AS_OF, AS_OF_STORED, Fake, NAME_A, closed_url};
use common::{App, HOST, error_type, get, json_request, keys, refused};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const AS_A: &str = "actorIssuer=tpm&actorRef=claude.session%3Aa";
const AS_PM: &str = "actorIssuer=tpm&actorRef=tpm.person%3Apm";
const NOBODY: &str = "actorIssuer=tpm&actorRef=claude.session%3Anobody";

/// 要る欄だけの本文（まだ届いていない）。
fn waiting() -> Value {
    json!({
        "heading": "request",
        "from": "t-1.swe.1",
        "to": "tpm",
        "text": "本文",
        "sentAt": "2026-11-24T00:00:00Z",
        "state": "waiting",
    })
}

/// `base` の欄を `change` の欄で替えた本文（`null` の欄は消す）。
fn with(mut base: Value, change: Value) -> Value {
    for (name, value) in change.as_object().expect("欄の組") {
        if value.is_null() {
            base.as_object_mut().unwrap().remove(name);
        } else {
            base[name] = value.clone();
        }
    }
    base
}

/// 置く・書き足す道（引数の `query` は `&` でつないだ物。空でもよい）。
fn message_uri(project: &str, id: &str, query: &str) -> String {
    let uri = format!("/api/projects/{project}/messages/{id}");
    if query.is_empty() {
        uri
    } else {
        format!("{uri}?{query}")
    }
}

/// 置く・書き足す（`PUT /api/projects/{project}/messages/{id}?{query}`。本文は JSON）。
async fn put(
    app: &App,
    project: &str,
    id: &str,
    query: &str,
    body: Value,
) -> (StatusCode, HeaderMap, Value) {
    app.send_json(json_request(
        "PUT",
        &message_uri(project, id, query),
        Some(body),
    ))
    .await
}

/// 置き（201）、記録を返す。
async fn place(app: &App, project: &str, id: &str, body: Value) -> Value {
    let (status, _, json) = put(app, project, id, "", body.clone()).await;
    assert_eq!(status, StatusCode::CREATED, "{id} {body}: {json}");
    json
}

/// 書き足し（200）、記録を返す。
async fn append(app: &App, project: &str, id: &str, body: Value) -> Value {
    let (status, _, json) = put(app, project, id, "", body.clone()).await;
    assert_eq!(status, StatusCode::OK, "{id} {body}: {json}");
    json
}

/// 一覧の 1 頁（200 を見る）。
async fn list(app: &App, query: &str) -> Value {
    let (status, _, json) = app.json(&format!("/api/messages?{query}")).await;
    assert_eq!(status, StatusCode::OK, "{query}: {json}");
    json
}

/// 一覧の答えの、メッセージの id の並び。
fn ids(json: &Value) -> Vec<String> {
    json["messages"]
        .as_array()
        .unwrap_or_else(|| panic!("{json}"))
        .iter()
        .map(|m| m["id"].as_str().unwrap_or_else(|| panic!("{m}")).to_owned())
        .collect()
}

/// `query` の一覧の 1 頁の id。
async fn listed(app: &App, query: &str) -> Vec<String> {
    ids(&list(app, query).await)
}

/// 何も残っていないか（メッセージ・送り手）。
async fn nothing_stored(app: &App) {
    assert_eq!(app.count("messages").await, 0, "messages");
    assert_eq!(app.count("senders").await, 0, "senders");
}

// --- 置く・書き足す ---

/// 初めての id は作る（201）。同じ project と id にもう 1 度打てば書き足す（200）。答えは、その記録。
#[tokio::test]
async fn the_first_put_creates_and_the_next_appends() {
    let app = App::start().await;
    let (status, headers, made) = put(&app, "p", "m-1", "", waiting()).await;
    assert_eq!(status, StatusCode::CREATED, "{made}");
    assert_eq!(headers[header::CONTENT_TYPE], "application/json");
    // 任意の欄（key・deliveredAt・via・deliveries・replyTo・sender）は、ある時だけ。状態の数（status）は本文に出ない。
    assert_eq!(
        keys(&made),
        [
            "createdAt",
            "from",
            "heading",
            "id",
            "project",
            "sentAt",
            "state",
            "text",
            "to",
            "updatedAt"
        ]
    );
    assert_eq!(made["project"], "p");
    assert_eq!(made["id"], "m-1");
    assert_eq!(made["heading"], "request");
    assert_eq!(made["from"], "t-1.swe.1");
    assert_eq!(made["to"], "tpm");
    assert_eq!(made["text"], "本文");
    assert_eq!(made["sentAt"], "2026-11-24T00:00:00Z");
    assert_eq!(made["state"], "waiting");
    // 初めて受けた時刻と、最後に書き足された時刻は、置いた時は同じ（pagetray の今）。
    assert_eq!(made["createdAt"], made["updatedAt"]);
    assert!(made["createdAt"].as_str().unwrap().ends_with('Z'), "{made}");

    tokio::time::sleep(Duration::from_millis(5)).await;
    let (status, _, again) = put(
        &app,
        "p",
        "m-1",
        "",
        with(
            waiting(),
            json!({ "state": "delivered", "deliveredAt": "2026-11-24T00:00:05Z" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again["state"], "delivered");
    assert_eq!(again["deliveredAt"], "2026-11-24T00:00:05Z");
    assert_eq!(again["createdAt"], made["createdAt"]);
    assert_ne!(again["updatedAt"], made["updatedAt"], "{again}");
    assert_eq!(app.count("messages").await, 1);
    // 一覧の行は、答えと同じ記録。
    assert_eq!(list(&app, "").await["messages"], json!([again]));
    // 同じ id でも、project が違えば別の記録。
    let (status, _, _) = put(&app, "q", "m-1", "", waiting()).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(app.count("messages").await, 2);
}

/// 任意の欄も、置いたとおりに返す。時刻は UTC・ミリ秒までの字で残す。
#[tokio::test]
async fn every_field_reads_back() {
    let app = App::start().await;
    let made = place(
        &app,
        "p",
        "01JD.a_b-9",
        json!({
            "key": "AI-25",
            "heading": "answer",
            "from": "pm.1",
            "to": "t-1.swe.1",
            "text": "1 行目\n2 行目",
            "sentAt": "2026-11-24T00:00:00.123Z",
            "state": "carried",
            "deliveredAt": "2026-11-24T00:00:01.5Z",
            "via": "session-start",
            "deliveries": 3,
            "replyTo": "m-0",
        }),
    )
    .await;
    assert_eq!(made["key"], "AI-25");
    assert_eq!(made["text"], "1 行目\n2 行目");
    assert_eq!(made["sentAt"], "2026-11-24T00:00:00.123Z");
    assert_eq!(made["state"], "carried");
    assert_eq!(made["deliveredAt"], "2026-11-24T00:00:01.5Z");
    assert_eq!(made["via"], "session-start");
    assert_eq!(made["deliveries"], 3);
    assert_eq!(made["replyTo"], "m-0");
    let stored: (String, String) = sqlx::query_as("select sent_at, delivered_at from messages")
        .fetch_one(app.db.read())
        .await
        .unwrap();
    assert_eq!(
        stored,
        (
            "2026-11-24T00:00:00.123Z".to_owned(),
            "2026-11-24T00:00:01.500Z".to_owned()
        )
    );
    // どの状態も受ける（pagetray は状態の移りを見ない。見せるだけ）。
    for (i, state) in [
        "waiting",
        "delivered",
        "undeliverable",
        "expired",
        "carried",
    ]
    .into_iter()
    .enumerate()
    {
        let made = place(
            &app,
            "p",
            &format!("s-{i}"),
            with(waiting(), json!({ "state": state })),
        )
        .await;
        assert_eq!(made["state"], state);
    }
}

/// 書き足すと、`state` は前へだけ進む（今より強い時だけ替わる）。`deliveries` は大きい方を保ち、`via` は、状態を替えた時か
/// 今が空の時だけ入る（書かなかった `via`・`deliveries` は前のまま）。
/// `deliveredAt` は、まだ無い時だけ入る（初めの時刻を保つ。返事の状態の `askedAt`・`answeredAt` と同じ考え）。
#[tokio::test]
async fn appending_updates_the_state_and_keeps_the_first_delivery_time() {
    let app = App::start().await;
    let made = place(&app, "p", "m-1", waiting()).await;
    assert!(made.get("deliveredAt").is_none(), "{made}");
    assert!(made.get("via").is_none(), "{made}");
    assert!(made.get("deliveries").is_none(), "{made}");

    let first = append(
        &app,
        "p",
        "m-1",
        with(
            waiting(),
            json!({
                "state": "delivered",
                "deliveredAt": "2026-11-24T00:00:05Z",
                "via": "prompt",
                "deliveries": 1,
            }),
        ),
    )
    .await;
    assert_eq!(first["state"], "delivered");
    assert_eq!(first["deliveredAt"], "2026-11-24T00:00:05Z");
    assert_eq!(first["via"], "prompt");
    assert_eq!(first["deliveries"], 1);

    // 違う届いた時刻が来ても、初めの時刻を保つ。状態が替わらないので via は前のまま。deliveries は大きい方。
    let second = append(
        &app,
        "p",
        "m-1",
        with(
            waiting(),
            json!({
                "state": "delivered",
                "deliveredAt": "2026-11-24T09:00:00Z",
                "via": "stop",
                "deliveries": 2,
            }),
        ),
    )
    .await;
    assert_eq!(second["deliveredAt"], "2026-11-24T00:00:05Z");
    assert_eq!(second["via"], "prompt");
    assert_eq!(second["deliveries"], 2);

    // 弱い状態（届いた後の期限切れ・まだ届いていない）では替わらない。任意の欄を書かなくても、前の値は消えない。
    // 小さい deliveries が来ても、大きい方を保つ。
    for change in [
        json!({ "state": "expired" }),
        json!({ "state": "undeliverable", "via": "stop" }),
        json!({ "state": "waiting", "deliveries": 1 }),
        json!({ "state": "carried" }),
    ] {
        let third = append(&app, "p", "m-1", with(waiting(), change.clone())).await;
        assert_eq!(third["state"], "delivered", "{change}");
        assert_eq!(third["deliveredAt"], "2026-11-24T00:00:05Z", "{change}");
        assert_eq!(third["via"], "prompt", "{change}");
        assert_eq!(third["deliveries"], 2, "{change}");
    }
}

/// 状態は waiting → expired → delivered と前へ進む。同じ強さ（waiting と carried・expired と undeliverable）では替わらない。
/// `via` は、状態を替えた時か、今が空の時だけ入る。
#[tokio::test]
async fn the_state_only_moves_forward() {
    let app = App::start().await;
    place(&app, "p", "m-1", waiting()).await;
    let same = append(
        &app,
        "p",
        "m-1",
        with(waiting(), json!({ "state": "carried", "via": "hand" })),
    )
    .await;
    assert_eq!(same["state"], "waiting");
    // 状態は替わらないが、空だった via は入る。
    assert_eq!(same["via"], "hand");
    let expired = append(
        &app,
        "p",
        "m-1",
        with(waiting(), json!({ "state": "expired", "via": "sweep" })),
    )
    .await;
    assert_eq!(expired["state"], "expired");
    assert_eq!(expired["via"], "sweep");
    let same = append(
        &app,
        "p",
        "m-1",
        with(
            waiting(),
            json!({ "state": "undeliverable", "via": "other" }),
        ),
    )
    .await;
    assert_eq!(same["state"], "expired");
    assert_eq!(same["via"], "sweep");
    let delivered = append(
        &app,
        "p",
        "m-1",
        with(waiting(), json!({ "state": "delivered", "via": "prompt" })),
    )
    .await;
    assert_eq!(delivered["state"], "delivered");
    assert_eq!(delivered["via"], "prompt");
    // 初めて置く時は、来た状態のまま（carried も）。carried の後の waiting では替わらない。
    let made = place(
        &app,
        "p",
        "m-2",
        with(waiting(), json!({ "state": "carried" })),
    )
    .await;
    assert_eq!(made["state"], "carried");
    let again = append(&app, "p", "m-2", waiting()).await;
    assert_eq!(again["state"], "carried");
}

/// 「渡した」の記録（受け手の process が写す）が、「送った」の記録（送り手の process が写す）より先に届いても、同じ記録になる:
/// 状態は delivered のまま・届いた時刻と渡した回数は残り・送り手は、後から来た送った側の物。castlist に聞くのは送った側の時だけ。
/// 「まだ届いていない n 件」も増えない。
#[tokio::test]
async fn a_delivery_that_arrives_before_the_send_keeps_its_state_and_gets_the_sender_later() {
    let fake = Fake::start().await;
    let app = App::with_castlist(&fake.url).await;
    // 渡した側（受け手 pm）が先。付随する id が付いていても、castlist に聞かず、送り手として残さない。
    let (status, _, first) = put(
        &app,
        "p",
        "m-1",
        AS_PM,
        with(
            waiting(),
            json!({
                "state": "delivered",
                "deliveredAt": "2026-11-24T00:00:05Z",
                "via": "prompt",
                "deliveries": 2,
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{first}");
    assert!(first.get("sender").is_none(), "{first}");
    assert!(
        fake.requests().is_empty(),
        "渡した側の書き込みで castlist に聞いた"
    );
    assert_eq!(app.count("senders").await, 0);

    // 送った側（送り手 a）が後。状態は戻らず、送り手が入る。
    let (status, _, second) = put(
        &app,
        "p",
        "m-1",
        AS_A,
        with(waiting(), json!({ "deliveries": 1 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(second["state"], "delivered");
    assert_eq!(second["deliveredAt"], "2026-11-24T00:00:05Z");
    assert_eq!(second["via"], "prompt");
    assert_eq!(second["deliveries"], 2);
    assert_eq!(second["sender"]["ref"], "claude.session:a");
    assert_eq!(second["sender"]["actor"]["id"], ACTOR_A);
    assert_eq!(fake.requests().len(), 1);
    assert_eq!(app.count("senders").await, 1);
    // まだ届いていない数は、順が入れ替わっても増えない。
    assert!(listed(&app, "state=waiting").await.is_empty());
    let facets = facets(&app, "").await;
    assert_eq!(counts(&facets, "states"), pairs([("delivered", 1)]));
}

/// `key`・`heading`・`from`・`to`・`text`・`sentAt`・`replyTo` は、初めに置いた値を保つ（違う値が来ても変えず、誤りにもしない）。
#[tokio::test]
async fn the_first_values_stay_when_other_values_come() {
    let app = App::start().await;
    let made = place(
        &app,
        "p",
        "m-1",
        with(waiting(), json!({ "key": "AI-25", "replyTo": "m-0" })),
    )
    .await;
    let again = append(
        &app,
        "p",
        "m-1",
        json!({
            "key": "AI-99",
            "heading": "other",
            "from": "someone",
            "to": "somebody",
            "text": "別の本文",
            "sentAt": "2026-12-01T00:00:00Z",
            "state": "delivered",
            "replyTo": "m-9",
        }),
    )
    .await;
    for field in [
        "key",
        "heading",
        "from",
        "to",
        "text",
        "sentAt",
        "replyTo",
        "createdAt",
    ] {
        assert_eq!(again[field], made[field], "{field}: {again}");
    }
    assert_eq!(again["state"], "delivered");
    // 初めに書かなかった任意の欄（key・replyTo）も、後から入れない。
    place(&app, "p", "m-2", waiting()).await;
    let again = append(
        &app,
        "p",
        "m-2",
        with(waiting(), json!({ "key": "AI-99", "replyTo": "m-9" })),
    )
    .await;
    assert!(again.get("key").is_none(), "{again}");
    assert!(again.get("replyTo").is_none(), "{again}");
}

/// 字の長さと形の誤りは 400（何も残さない）。長さは byte でなく字で数える。
#[tokio::test]
async fn bad_lengths_and_shapes_are_refused() {
    let app = App::start().await;
    let long_id = "a".repeat(129);
    for (project, id, change) in [
        // 要る欄が無い。
        ("p", "m", json!({ "heading": null })),
        ("p", "m", json!({ "from": null })),
        ("p", "m", json!({ "to": null })),
        ("p", "m", json!({ "text": null })),
        ("p", "m", json!({ "sentAt": null })),
        ("p", "m", json!({ "state": null })),
        // 長さ。
        ("p", "m", json!({ "text": "" })),
        ("p", "m", json!({ "text": "字".repeat(8001) })),
        ("p", "m", json!({ "from": "" })),
        ("p", "m", json!({ "from": "字".repeat(201) })),
        ("p", "m", json!({ "to": "" })),
        ("p", "m", json!({ "to": "a".repeat(201) })),
        // 形。
        ("p", "m", json!({ "heading": "Request" })),
        ("p", "m", json!({ "heading": "a".repeat(33) })),
        ("p", "m", json!({ "heading": "" })),
        ("p", "m", json!({ "state": "done" })),
        ("p", "m", json!({ "state": "" })),
        ("p", "m", json!({ "via": "Stop" })),
        ("p", "m", json!({ "via": "" })),
        ("p", "m", json!({ "deliveries": 0 })),
        ("p", "m", json!({ "deliveries": -1 })),
        ("p", "m", json!({ "deliveries": "2" })),
        ("p", "m", json!({ "replyTo": "a b" })),
        ("p", "m", json!({ "replyTo": "" })),
        ("p", "m", json!({ "key": "a/b" })),
        ("p", "m", json!({ "key": ".." })),
        // 時刻: UTC だけ・ミリ秒まで。
        ("p", "m", json!({ "sentAt": "2026-11-24T09:00:00+09:00" })),
        ("p", "m", json!({ "sentAt": "2026-11-24" })),
        ("p", "m", json!({ "sentAt": 1795478400 })),
        ("p", "m", json!({ "sentAt": "2026-11-24T00:00:00.1234Z" })),
        (
            "p",
            "m",
            json!({ "deliveredAt": "2026-11-24T00:00:00.1234Z" }),
        ),
        ("p", "m", json!({ "deliveredAt": "yesterday" })),
        // 道の project と id。
        ("api", "m", json!({})),
        (".", "m", json!({})),
        ("p", "a%20b", json!({})),
        ("p", "a%3Ab", json!({})),
        ("p", long_id.as_str(), json!({})),
        ("p", ".", json!({})),
        ("p", "..", json!({})),
    ] {
        let (status, headers, json) =
            put(&app, project, id, "", with(waiting(), change.clone())).await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "{project} {id} {change}: {json}"
        );
        assert!(
            matches!(
                error_type(&headers),
                "ValidationException" | "SerializationException"
            ),
            "{project} {id} {change}: {headers:?}"
        );
    }
    // JSON でない本文・本文の無い要求も 400。
    let request = Request::put(message_uri("p", "m", ""))
        .header(header::HOST, HOST)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{"))
        .unwrap();
    assert_eq!(app.send(request).await.0, StatusCode::BAD_REQUEST);
    let (status, _, _) = app
        .send_json(json_request("PUT", &message_uri("p", "m", ""), None))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    nothing_stored(&app).await;

    // 上限ちょうどは通る（字で数える。8000 字の日本語は 24000 byte）。id は 128 字まで。
    let id = "a".repeat(128);
    let made = place(
        &app,
        "p",
        &id,
        with(
            waiting(),
            json!({
                "text": "字".repeat(8000),
                "from": "字".repeat(200),
                "to": "a".repeat(200),
                "heading": format!("a{}", "-".repeat(31)),
            }),
        ),
    )
    .await;
    assert_eq!(made["id"], id.as_str());
    assert_eq!(made["text"].as_str().unwrap().chars().count(), 8000);
    // 書き足す時も、形は見る（前の値を保つ欄でも）。断れば何も替えない。
    let (status, _, _) = put(&app, "p", &id, "", with(waiting(), json!({ "text": "" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _, _) = put(
        &app,
        "p",
        &id,
        "",
        with(waiting(), json!({ "state": "done" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(list(&app, "").await["messages"][0]["state"], "waiting");
}

/// 本文は字のまま持つ（HTML として解釈しない。tag も文字参照もそのまま返す）。
#[tokio::test]
async fn the_text_is_kept_as_written() {
    let app = App::start().await;
    let text = "<script>alert(1)</script> &amp; \"q\"\n\t2 行目 ";
    let made = place(&app, "p", "m-1", with(waiting(), json!({ "text": text }))).await;
    assert_eq!(made["text"], text);
    assert_eq!(list(&app, "").await["messages"][0]["text"], text);
}

/// browser の中のページからは書けない（守りの層が `Origin` の付いた書く要求を 403）。
#[tokio::test]
async fn a_put_with_an_origin_is_refused() {
    let app = App::start().await;
    let request = Request::put(message_uri("p", "m-1", ""))
        .header(header::HOST, HOST)
        .header(header::ORIGIN, format!("http://{HOST}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(waiting().to_string()))
        .unwrap();
    let (status, headers, body) = app.send(request).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    refused(&headers, &body, "Forbidden");
    nothing_stored(&app).await;
}

// --- 探す ---

/// 5 通を、送った時刻を 1 日ずつずらして置く。
async fn five(app: &App) {
    for (project, id, day, change) in [
        (
            "p",
            "m-1",
            1,
            json!({ "key": "A-1", "from": "pm.1", "to": "t-1.swe.1", "text": "みかんを頼む" }),
        ),
        (
            "p",
            "m-2",
            2,
            json!({ "key": "A-1", "heading": "answer", "from": "t-1.swe.1", "to": "pm.1", "text": "りんごにする", "state": "delivered" }),
        ),
        (
            "p",
            "m-3",
            3,
            json!({ "key": "B-2", "from": "t-2.swe.1", "to": "tpm", "text": "みかんは無い", "state": "expired" }),
        ),
        (
            "q",
            "m-4",
            4,
            json!({ "key": "A-1", "from": "pm.1", "to": "tpm", "text": "ぶどう", "state": "carried" }),
        ),
        (
            "q",
            "m-5",
            5,
            json!({ "from": "tpm", "to": "t-9.qa.1", "text": "もも", "state": "undeliverable" }),
        ),
    ] {
        let body = with(
            with(
                waiting(),
                json!({ "sentAt": format!("2026-11-{day:02}T00:00:00Z") }),
            ),
            change,
        );
        place(app, project, id, body).await;
    }
}

/// どの条件も、書いた物だけで絞る（重ねれば、どれにも合う物）。並びは送った時刻の新しい順が既定。
#[tokio::test]
async fn filters_combine_and_sort_by_the_sent_time() {
    let app = App::start().await;
    five(&app).await;
    assert_eq!(listed(&app, "").await, ["m-5", "m-4", "m-3", "m-2", "m-1"]);
    assert_eq!(
        listed(&app, "sort=-sentAt").await,
        ["m-5", "m-4", "m-3", "m-2", "m-1"]
    );
    assert_eq!(
        listed(&app, "sort=sentAt").await,
        ["m-1", "m-2", "m-3", "m-4", "m-5"]
    );
    assert_eq!(listed(&app, "project=p").await, ["m-3", "m-2", "m-1"]);
    assert_eq!(listed(&app, "key=A-1").await, ["m-4", "m-2", "m-1"]);
    assert_eq!(listed(&app, "to=tpm").await, ["m-4", "m-3"]);
    assert_eq!(listed(&app, "from=pm.1").await, ["m-4", "m-1"]);
    assert_eq!(listed(&app, "state=waiting").await, ["m-1"]);
    assert_eq!(listed(&app, "state=undeliverable").await, ["m-5"]);
    assert_eq!(listed(&app, "heading=answer").await, ["m-2"]);
    assert_eq!(listed(&app, "project=p&key=A-1&from=pm.1").await, ["m-1"]);
    assert!(listed(&app, "project=q&state=waiting").await.is_empty());
    // q は、本文と from・to の部分一致（前後の空白を落とし、空なら絞らない）。
    assert_eq!(
        listed(&app, "q=%E3%81%BF%E3%81%8B%E3%82%93").await,
        ["m-3", "m-1"]
    );
    assert_eq!(listed(&app, "q=swe").await, ["m-3", "m-2", "m-1"]);
    assert_eq!(listed(&app, "q=qa.1").await, ["m-5"]);
    assert_eq!(listed(&app, "q=%20pm.1%20&project=q").await, ["m-4"]);
    assert_eq!(listed(&app, "q=%20").await.len(), 5);
    assert!(listed(&app, "q=nowhere").await.is_empty());
    // from・to は同じ値の物（部分では当たらない）。
    assert!(listed(&app, "to=tp").await.is_empty());
}

/// 頁は送った時刻と行の順で続ける（同じ時刻の行が頁の境に並んでも、抜けず重ならない）。
#[tokio::test]
async fn pages_continue_with_the_next_token() {
    let app = App::start().await;
    five(&app).await;
    // 同じ時刻の 3 通を足す。
    for id in ["s-1", "s-2", "s-3"] {
        place(
            &app,
            "p",
            id,
            with(waiting(), json!({ "sentAt": "2026-11-03T00:00:00Z" })),
        )
        .await;
    }
    for (sort, want) in [
        (
            "-sentAt",
            ["m-5", "m-4", "s-3", "s-2", "s-1", "m-3", "m-2", "m-1"],
        ),
        (
            "sentAt",
            ["m-1", "m-2", "m-3", "s-1", "s-2", "s-3", "m-4", "m-5"],
        ),
    ] {
        for max in [1, 2, 3, 8] {
            let mut found = Vec::new();
            let mut token: Option<String> = None;
            loop {
                let mut query = format!("sort={sort}&maxResults={max}");
                if let Some(t) = &token {
                    query.push_str(&format!("&nextToken={t}"));
                }
                let json = list(&app, &query).await;
                let page = ids(&json);
                assert!(page.len() <= max, "{query}: {json}");
                found.extend(page);
                match json["nextToken"].as_str() {
                    Some(t) => token = Some(t.to_owned()),
                    None => break,
                }
                assert!(found.len() <= 8, "終わらない: {found:?}");
            }
            assert_eq!(found, want, "sort={sort} maxResults={max}");
        }
    }
    // ちょうど 1 頁で終わる時は、次の印が無い。
    assert!(list(&app, "maxResults=8").await.get("nextToken").is_none());
}

/// nextToken は、同じ sort のメッセージの一覧の答えの物だけ。崩れた物・ほかの sort の物・ほかの検索の物は 400。
/// 知らない条件の値・形の違う値も 400。
#[tokio::test]
async fn bad_list_arguments_are_refused() {
    let app = App::start().await;
    five(&app).await;
    app.place("p", "A-1", "plan", "role=tpm", "<p>x</p>").await;
    app.place("p", "A-1", "plan", "role=tpm", "<p>y</p>").await;
    let token = list(&app, "maxResults=1").await["nextToken"]
        .as_str()
        .unwrap()
        .to_owned();
    let (_, _, pages) = app.json("/api/pages?maxResults=1").await;
    let page_token = pages["nextToken"].as_str().unwrap().to_owned();
    for query in [
        format!("sort=sentAt&nextToken={token}"),
        format!("nextToken={page_token}"),
        "nextToken=zz".to_owned(),
        format!("nextToken={}", &token[..token.len() - 2]),
    ] {
        let (status, headers, json) = app.json(&format!("/api/messages?{query}")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}: {json}");
        assert_eq!(error_type(&headers), "ValidationException", "{query}");
        assert!(
            json["message"].as_str().unwrap_or("").contains("nextToken"),
            "{query}: {json}"
        );
    }
    // メッセージの印は、ページの検索では使えない。
    let (status, _, _) = app.json(&format!("/api/pages?nextToken={token}")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let long = format!("to={}", "a".repeat(201));
    for query in [
        "state=done",
        "sort=createdAt",
        "sort=sent_at",
        "heading=Request",
        "project=a%20b",
        "key=a%2Fb",
        "to=",
        "from=",
        long.as_str(),
        "maxResults=0",
        "maxResults=1001",
        "maxResults=abc",
    ] {
        let (status, headers, json) = app.json(&format!("/api/messages?{query}")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}: {json}");
        assert!(
            matches!(
                error_type(&headers),
                "ValidationException" | "SerializationException"
            ),
            "{query}: {headers:?}"
        );
    }
    // 1 頁は既定 100。
    for i in 0..101 {
        place(&app, "many", &format!("n-{i}"), waiting()).await;
    }
    let json = list(&app, "project=many").await;
    assert_eq!(ids(&json).len(), 100);
    assert!(json["nextToken"].is_string(), "{json}");
}

/// 探す SQL（`pagetray_server::search::message_plans()`。条件を 1 つずつと、重ねた物）が、messages を索引なしに端から読まない。
/// 条件の無い一覧と nextToken は、送った時刻の索引（messages_sent）の順に読み、行を並べ替える一時の表を作らない。
/// project の条件は、一意の索引（project と id）で引いてよい（引いた行は並べ替える）。
#[tokio::test]
async fn the_list_sql_reads_messages_by_an_index() {
    let app = App::start().await;
    let plans = pagetray_server::search::message_plans();
    let names: Vec<&str> = plans.iter().map(|(name, _)| *name).collect();
    let ordered = ["plain", "oldest", "after", "after_oldest"];
    for name in ordered.iter().chain(&[
        "project", "key", "to", "from", "state", "heading", "q", "all",
    ]) {
        assert!(names.contains(name), "{name} が plans に無い: {names:?}");
    }
    for (name, sql) in &plans {
        let plan: Vec<(i64, i64, i64, String)> =
            sqlx::query_as(sqlx::AssertSqlSafe(format!("explain query plan {sql}")))
                .fetch_all(app.db.read())
                .await
                .unwrap_or_else(|e| panic!("{name}: {e}\n{sql}"));
        let details: Vec<&str> = plan.iter().map(|row| row.3.as_str()).collect();
        let bare: Vec<&&str> = details
            .iter()
            .filter(|d| {
                d.strip_prefix("SCAN ")
                    .is_some_and(|rest| !rest.contains("USING"))
            })
            .collect();
        assert!(bare.is_empty(), "{name} が端から読む: {details:?}\n{sql}");
        if ordered.contains(name) {
            assert!(
                details
                    .iter()
                    .any(|d| d.contains("USING INDEX messages_sent")),
                "{name} が送った時刻の索引で読まない: {details:?}\n{sql}"
            );
            assert!(
                !details.contains(&"USE TEMP B-TREE FOR ORDER BY"),
                "{name} が並べ替えの表を作る: {details:?}\n{sql}"
            );
        }
    }
}

// --- 絞り込みの候補と件数（GetMessageFacets） ---

/// 候補の答え（200 を見る）。
async fn facets(app: &App, query: &str) -> Value {
    let (status, _, json) = app.json(&format!("/api/messages/facets?{query}")).await;
    assert_eq!(status, StatusCode::OK, "{query}: {json}");
    json
}

/// 候補の並び 1 つを、(値, 件数) の並びにする。
fn counts(json: &Value, name: &str) -> Vec<(String, i64)> {
    json[name]
        .as_array()
        .unwrap_or_else(|| panic!("{name}: {json}"))
        .iter()
        .map(|c| {
            (
                c["value"]
                    .as_str()
                    .unwrap_or_else(|| panic!("{c}"))
                    .to_owned(),
                c["count"].as_i64().unwrap_or_else(|| panic!("{c}")),
            )
        })
        .collect()
}

fn pairs<const N: usize>(pairs: [(&str, i64); N]) -> Vec<(String, i64)> {
    pairs.into_iter().map(|(v, n)| (v.to_owned(), n)).collect()
}

/// 候補と件数は、置き場の全部（絞らない）。状態は決まった順で、件数が 0 の状態は出さない。受け手・送り手は件数の多い順
/// （同じなら名の順）、Key は新しく送られた順（Key の無い記録は数えない）、project は件数の多い順。
#[tokio::test]
async fn the_facets_count_the_whole_store() {
    let app = App::start().await;
    // 空の置き場は、どの並びも空。
    let empty = facets(&app, "").await;
    for name in ["states", "to", "from", "keys", "projects"] {
        assert_eq!(counts(&empty, name), [], "{name}");
    }
    five(&app).await;
    place(&app, "p", "m-6", with(waiting(), json!({ "key": "A-1" }))).await;
    let json = facets(&app, "").await;
    assert_eq!(
        counts(&json, "states"),
        pairs([
            ("waiting", 2),
            ("delivered", 1),
            ("undeliverable", 1),
            ("expired", 1),
            ("carried", 1),
        ])
    );
    assert_eq!(
        counts(&json, "to"),
        pairs([("tpm", 3), ("pm.1", 1), ("t-1.swe.1", 1), ("t-9.qa.1", 1)])
    );
    assert_eq!(
        counts(&json, "from"),
        pairs([("pm.1", 2), ("t-1.swe.1", 2), ("t-2.swe.1", 1), ("tpm", 1)])
    );
    assert_eq!(counts(&json, "keys"), pairs([("A-1", 4), ("B-2", 1)]));
    assert_eq!(counts(&json, "projects"), pairs([("p", 4), ("q", 2)]));
    // 欄は value と count だけ。
    assert_eq!(json["states"][0], json!({ "value": "waiting", "count": 2 }));
    // 件数が 0 の状態は出さない。
    let quiet = App::start().await;
    place(
        &quiet,
        "p",
        "m-1",
        with(waiting(), json!({ "state": "delivered" })),
    )
    .await;
    assert_eq!(
        counts(&facets(&quiet, "").await, "states"),
        pairs([("delivered", 1)])
    );
}

/// 受け手・送り手・Key の候補は 100 件まで。引数に書いた値（今絞っている値）は、100 件の外でも・置き場に無くても、
/// 後ろに足す（画面が、押して外せるように）。もう候補にある値は、2 度出さない。
#[tokio::test]
async fn the_facets_keep_the_picked_values() {
    let app = App::start().await;
    for i in 0..101 {
        place(
            &app,
            "p",
            &format!("m-{i}"),
            with(
                waiting(),
                json!({
                    "to": format!("to-{i:03}"),
                    "from": format!("from-{i:03}"),
                    "key": format!("K-{i}"),
                    "sentAt": format!("2026-11-01T00:{:02}:{:02}Z", i / 60, i % 60),
                }),
            ),
        )
        .await;
    }
    let json = facets(&app, "").await;
    for name in ["to", "from", "keys"] {
        assert_eq!(counts(&json, name).len(), 100, "{name}");
    }
    assert_eq!(counts(&json, "to")[99], ("to-099".to_owned(), 1));
    assert_eq!(counts(&json, "from")[99], ("from-099".to_owned(), 1));
    assert_eq!(counts(&json, "keys")[0], ("K-100".to_owned(), 1));
    assert_eq!(counts(&json, "keys")[99], ("K-1".to_owned(), 1));

    let json = facets(&app, "to=to-100&from=from-100&key=K-0").await;
    for (name, value) in [("to", "to-100"), ("from", "from-100"), ("keys", "K-0")] {
        let list = counts(&json, name);
        assert_eq!(list.len(), 101, "{name}");
        assert_eq!(list[100], (value.to_owned(), 1), "{name}");
    }
    // 置き場に無い値は、件数 0 で足す。
    let json = facets(&app, "to=nobody").await;
    assert_eq!(counts(&json, "to")[100], ("nobody".to_owned(), 0));
    assert_eq!(counts(&json, "from").len(), 100);
    // もう候補にある値は、足さない。
    let json = facets(&app, "to=to-000&from=from-000&key=K-100").await;
    for name in ["to", "from", "keys"] {
        assert_eq!(counts(&json, name).len(), 100, "{name}");
    }
}

/// 引数の形が違えば 400（ListMessages と同じ形の決まり）。知らない引数は読まない。
#[tokio::test]
async fn bad_facet_arguments_are_refused() {
    let app = App::start().await;
    for query in [
        format!("to={}", "a".repeat(201)),
        format!("from={}", "a".repeat(201)),
        "to=".to_owned(),
        "key=a%2Fb".to_owned(),
    ] {
        let (status, headers, body) = app.get(&format!("/api/messages/facets?{query}")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}");
        refused(&headers, &body, "ValidationException");
    }
    // 書く method は無い。
    let (status, _, _) = app
        .send_json(json_request(
            "POST",
            "/api/messages/facets",
            Some(json!({})),
        ))
        .await;
    assert_ne!(status, StatusCode::OK);
}

// --- 送り手 ---

/// 付随する id を書いて置けば、送り手を castlist に聞き、答えと一緒に残す。書き足す時は、初めの送り手を保つ。
#[tokio::test]
async fn a_message_from_a_known_sender_keeps_the_first_answer() {
    let fake = Fake::start().await;
    let app = App::with_castlist(&fake.url).await;
    let (status, _, made) = put(&app, "p", "m-1", AS_A, waiting()).await;
    assert_eq!(status, StatusCode::CREATED, "{made}");
    let sender = json!({
        "issuer": "tpm",
        "ref": "claude.session:a",
        "actor": { "id": ACTOR_A, "name": NAME_A, "kind": "ai" },
        "resolvedAt": AS_OF,
    });
    assert_eq!(made["sender"], sender);
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
    assert_eq!(list(&app, "").await["messages"][0]["sender"], sender);

    // 渡した側（ほかの状態）の書き足しは、付随する id が付いていても castlist に聞かず、送り手を替えない。
    let (status, _, again) = put(
        &app,
        "p",
        "m-1",
        AS_PM,
        with(waiting(), json!({ "state": "delivered" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again["state"], "delivered");
    assert_eq!(again["sender"], sender);
    assert_eq!(app.count("senders").await, 1);
    assert_eq!(fake.requests().len(), 1);
    // ほかの送り手が送った側として書き足しても（castlist には確かめる）、もう入っている送り手は替えない。行も増えない。
    let (status, _, again) = put(&app, "p", "m-1", AS_PM, waiting()).await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again["sender"], sender);
    assert_eq!(app.count("senders").await, 1);
    assert_eq!(fake.requests().len(), 2);
    // 付随する id を書かずに書き足しても、送り手は残る。
    let (status, _, again) = put(&app, "p", "m-1", "", waiting()).await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again["sender"], sender);
    // 送り手が空の記録は、後から来た送った側の書き込みの送り手で埋める。
    place(&app, "p", "m-2", waiting()).await;
    let (status, _, again) = put(&app, "p", "m-2", AS_A, waiting()).await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again["sender"], sender);
    assert_eq!(app.count("senders").await, 2);
}

/// castlist に登録の無い送り手は 400（`fieldList` の path は `/actorRef`）、聞けなければ 503。どちらも何も残さず、
/// 書き足しなら何も替えない。
#[tokio::test]
async fn an_unknown_or_unreachable_sender_stores_nothing() {
    let fake = Fake::start().await;
    let app = App::with_castlist(&fake.url).await;
    let (status, headers, json) = put(&app, "p", "m-1", NOBODY, waiting()).await;
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
    nothing_stored(&app).await;
    // 送った側の書き足しでも、知らない送り手は断る（何も替わらない）。
    place(&app, "p", "m-1", waiting()).await;
    let (status, _, _) = put(
        &app,
        "p",
        "m-1",
        NOBODY,
        with(waiting(), json!({ "via": "hand", "deliveries": 3 })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let kept = &list(&app, "").await["messages"][0];
    assert!(kept.get("via").is_none(), "{kept}");
    assert!(kept.get("deliveries").is_none(), "{kept}");

    let app = App::with_castlist(&closed_url()).await;
    let (status, headers, json) = put(&app, "p", "m-1", AS_A, waiting()).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{json}");
    assert_eq!(error_type(&headers), "Unavailable", "{json}");
    assert!(
        json["message"]
            .as_str()
            .unwrap_or("")
            .starts_with("castlist に聞けない"),
        "{json}"
    );
    nothing_stored(&app).await;
    // 付随する id を書かなければ、castlist に聞かずに置ける（送り手は残さない）。
    let made = place(&app, "p", "m-1", waiting()).await;
    assert!(made.get("sender").is_none(), "{made}");
    // castlist が止まっていても、渡した側の書き足し（付随する id つき）は通る。初めて置く時も。
    for id in ["m-1", "m-2"] {
        let (status, _, json) = put(
            &app,
            "p",
            id,
            AS_PM,
            with(
                waiting(),
                json!({ "state": "delivered", "deliveredAt": "2026-11-24T00:00:05Z" }),
            ),
        )
        .await;
        assert!(status.is_success(), "{id}: {status} {json}");
        assert_eq!(json["state"], "delivered", "{id}");
        assert_eq!(json["deliveredAt"], "2026-11-24T00:00:05Z", "{id}");
        assert!(json.get("sender").is_none(), "{id}: {json}");
    }
    assert_eq!(app.count("senders").await, 0);
}

/// castlist を設定していなければ、聞かずに付随する id だけを残す。
#[tokio::test]
async fn without_castlist_only_the_attached_ids_are_kept() {
    let app = App::start().await;
    let (status, _, made) = put(&app, "p", "m-1", AS_A, waiting()).await;
    assert_eq!(status, StatusCode::CREATED, "{made}");
    assert_eq!(
        made["sender"],
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
            None
        )]
    );
}

/// 付随する id は両方書くか両方書かないかで、形も見る。合わなければ castlist に聞かずに 400。
/// 本文が断られる時も、castlist には聞かない（検査の順: 引数と本文 → 付随する id → castlist → DB）。
#[tokio::test]
async fn actor_params_must_come_together_and_in_shape() {
    let fake = Fake::start().await;
    let app = App::with_castlist(&fake.url).await;
    for actor in [
        "actorIssuer=tpm",
        "actorRef=claude.session%3Aa",
        "actorIssuer=TPM&actorRef=claude.session%3Aa",
        "actorIssuer=tpm&actorRef=claude.session",
        "actorIssuer=tpm&actorRef=",
    ] {
        let (status, headers, json) = put(&app, "p", "m-1", actor, waiting()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{actor}: {json}");
        assert_eq!(error_type(&headers), "ValidationException", "{actor}");
    }
    let (_, _, json) = put(&app, "p", "m-1", "actorIssuer=tpm", waiting()).await;
    assert!(
        json["message"].as_str().unwrap_or("").contains("両方"),
        "{json}"
    );
    // 渡した側の書き込み（castlist に聞かない）でも、組と形は見る。
    for actor in [
        "actorIssuer=tpm",
        "actorIssuer=TPM&actorRef=claude.session%3Aa",
    ] {
        let (status, _, json) = put(
            &app,
            "p",
            "m-1",
            actor,
            with(waiting(), json!({ "state": "delivered" })),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{actor}: {json}");
    }
    for (project, change) in [
        ("p", json!({ "text": "" })),
        ("p", json!({ "sentAt": "2026-11-24T00:00:00.1234Z" })),
        ("api", json!({})),
    ] {
        let (status, _, json) = put(&app, project, "m-1", AS_A, with(waiting(), change)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
    }
    assert!(fake.requests().is_empty(), "断る物を castlist に聞いた");
    nothing_stored(&app).await;
}

// --- 新着の知らせ ---

/// 聞き始める。
async fn listen(app: &App) -> Body {
    let response = app
        .app
        .clone()
        .oneshot(get("/api/events"))
        .await
        .expect("oneshot");
    assert_eq!(response.status(), StatusCode::OK);
    response.into_body()
}

/// 次の event（名と、data の JSON）。keep-alive の注釈は読み飛ばす。5 秒で来なければ落ちる。
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

/// 置く・書き足すたびに（替わらなかった時も）、`event: message-record` で、一覧の 1 件と同じ形の記録を流す。断った物は流さない。
#[tokio::test]
async fn every_put_is_pushed_as_a_message_event() {
    let app = App::start().await;
    let mut body = listen(&app).await;
    let (status, _, made) = put(
        &app,
        "p",
        "m-1",
        AS_A,
        with(
            waiting(),
            json!({ "key": "A-1", "via": "prompt", "deliveries": 1, "replyTo": "m-0", "sentAt": "2026-11-24T00:00:00.5Z" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{made}");
    let (name, json) = next(&mut body).await;
    assert_eq!(name.as_deref(), Some("message-record"));
    assert_eq!(json, made);
    assert_eq!(json, list(&app, "").await["messages"][0]);
    assert!(json.get("status").is_none(), "{json}");

    // 断った物（400）は流さず、次の書き足しが次の知らせ。同じ中身の書き足しも流す。
    let (status, _, _) = put(&app, "p", "m-1", "", with(waiting(), json!({ "text": "" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    for _ in 0..2 {
        let again = append(
            &app,
            "p",
            "m-1",
            with(
                waiting(),
                json!({ "state": "delivered", "deliveredAt": "2026-11-24T00:00:05Z" }),
            ),
        )
        .await;
        let (name, json) = next(&mut body).await;
        assert_eq!(name.as_deref(), Some("message-record"));
        assert_eq!(json, again);
        assert_eq!(json["deliveredAt"], "2026-11-24T00:00:05Z");
    }
    // ページの知らせは前のまま（名の無い event）。
    app.place("p", "A-1", "plan", "role=tpm", "<p>x</p>").await;
    let (name, json) = next(&mut body).await;
    assert_eq!(name, None);
    assert_eq!(json["path"], "/p/A-1/plan-1.html");
}
