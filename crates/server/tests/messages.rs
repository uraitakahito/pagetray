//! メッセージの記録（処理より先に書く）: 置く・書き足す PutMessage（`PUT /api/projects/{project}/messages/{id}`）と、
//! 探す ListMessages（`GET /api/messages`）と、新着の知らせ（`event: message`）と、メッセージの一覧の画面（`/messages`）。
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
use sha2::{Digest, Sha256};
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

/// 書き足すと、`state`・`via`・`deliveries` は新しい値になる（書かなかった `via`・`deliveries` は前のまま）。
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

    // 違う届いた時刻が来ても、初めの時刻を保つ。via と deliveries は新しい値。
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
    assert_eq!(second["via"], "stop");
    assert_eq!(second["deliveries"], 2);

    // 任意の欄を書かずに状態だけを書き足しても、前の値は消えない。
    let third = append(
        &app,
        "p",
        "m-1",
        with(waiting(), json!({ "state": "expired" })),
    )
    .await;
    assert_eq!(third["state"], "expired");
    assert_eq!(third["deliveredAt"], "2026-11-24T00:00:05Z");
    assert_eq!(third["via"], "stop");
    assert_eq!(third["deliveries"], 2);
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

    // ほかの送り手が書き足しても（castlist には確かめる）、記録の送り手は初めの物。送り手の行も増えない。
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
    assert_eq!(fake.requests().len(), 2);
    // 付随する id を書かずに書き足しても、送り手は残る。
    let (status, _, again) = put(&app, "p", "m-1", "", waiting()).await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again["sender"], sender);
    // 送り手を書かずに置いた記録には、後から送り手を入れない。
    place(&app, "p", "m-2", waiting()).await;
    let (status, _, again) = put(&app, "p", "m-2", AS_A, waiting()).await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert!(again.get("sender").is_none(), "{again}");
    assert_eq!(app.count("senders").await, 1);
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
    // 書き足しでも、知らない送り手は断る（状態は替わらない）。
    place(&app, "p", "m-1", waiting()).await;
    let (status, _, _) = put(
        &app,
        "p",
        "m-1",
        NOBODY,
        with(waiting(), json!({ "state": "delivered" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(list(&app, "").await["messages"][0]["state"], "waiting");

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

/// 置く・書き足すたびに（替わらなかった時も）、`event: message` で、一覧の 1 件と同じ形の記録を流す。断った物は流さない。
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
    assert_eq!(name.as_deref(), Some("message"));
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
        assert_eq!(name.as_deref(), Some("message"));
        assert_eq!(json, again);
        assert_eq!(json["deliveredAt"], "2026-11-24T00:00:05Z");
    }
    // ページの知らせは前のまま（名の無い event）。
    app.place("p", "A-1", "plan", "role=tpm", "<p>x</p>").await;
    let (name, json) = next(&mut body).await;
    assert_eq!(name, None);
    assert_eq!(json["path"], "/p/A-1/plan-1.html");
}

// --- メッセージの一覧の画面 ---

/// 画面を開き、200 と HTML を見て、本文の字を返す。
async fn text(app: &App, uri: &str) -> String {
    let (status, headers, body) = app.get(uri).await;
    let page = String::from_utf8(body).expect("UTF-8");
    assert_eq!(status, StatusCode::OK, "{uri}: {page}");
    assert_eq!(
        headers[header::CONTENT_TYPE],
        "text/html; charset=utf-8",
        "{uri}"
    );
    page
}

/// 画面に出たメッセージ（`<project>/<id>`）を、出た順に。
fn shown(page: &str) -> Vec<&str> {
    page.split("<li data-id=\"")
        .skip(1)
        .map(|rest| rest.split('"').next().unwrap())
        .collect()
}

/// 画面の 1 行（`<li data-id="<project>/<id>"` から `</li>` まで）。
fn row<'a>(page: &'a str, id: &str) -> &'a str {
    let start = page
        .find(&format!("<li data-id=\"{id}\""))
        .unwrap_or_else(|| panic!("{id} の行が無い: {page}"));
    let rest = &page[start..];
    &rest[..rest.find("</li>").expect("行の終わり")]
}

/// 「続きを出す」の link の先（HTML の字の `&#38;` を戻した物）。無ければ None。
fn more(page: &str) -> Option<String> {
    let rest = page.split(r#"<a class="more" href=""#).nth(1)?;
    Some(rest.split('"').next()?.replace("&#38;", "&"))
}

/// ページの一覧には、メッセージの一覧への link が 1 つ足される。メッセージの一覧からは、ページの一覧へ戻れる。
#[tokio::test]
async fn the_page_list_and_the_message_list_link_to_each_other() {
    let app = App::start().await;
    let page = text(&app, "/").await;
    assert_eq!(
        page.matches(r#"<a class="btn" href="/messages">メッセージ</a>"#)
            .count(),
        1,
        "{page}"
    );
    let page = text(&app, "/messages").await;
    assert!(
        page.contains(r#"<a class="btn" href="/">ページ</a>"#),
        "{page}"
    );
    assert!(page.contains("合うメッセージは無い"), "{page}");
    // 区画の名は、ページの一覧と同じ語。
    for (id, name) in [
        ("toolbar", "操作の帯"),
        ("filters", "絞り込みの列"),
        ("list", "一覧の列"),
    ] {
        assert_eq!(
            page.matches(&format!(r#" id="{id}""#)).count(),
            1,
            "id=\"{id}\""
        );
        assert!(
            page.contains(&format!(r#"aria-label="{name}""#)),
            "aria-label=\"{name}\""
        );
    }
}

/// 1 行に: 送った時刻・Key・見出し・「送り手 → 受け手」・状態の字・届いた時刻・渡した時・渡し直しの数・本文。
#[tokio::test]
async fn a_row_shows_the_record_with_the_state_in_words() {
    let app = App::start().await;
    five(&app).await;
    append(
        &app,
        "p",
        "m-2",
        with(
            waiting(),
            json!({
                "state": "delivered",
                "deliveredAt": "2026-11-02T00:00:07Z",
                "via": "session-start",
                "deliveries": 3,
            }),
        ),
    )
    .await;
    let page = text(&app, "/messages").await;
    assert_eq!(shown(&page), ["q/m-5", "q/m-4", "p/m-3", "p/m-2", "p/m-1"]);
    let line = row(&page, "p/m-2");
    assert!(line.contains(r#"data-state="delivered""#), "{line}");
    assert!(
        line.contains(r#"<time datetime="2026-11-02T00:00:00.000Z">11-02 00:00</time>"#),
        "{line}"
    );
    assert!(line.contains("<span>A-1</span>"), "{line}");
    assert!(
        line.contains(r#"<span class="name">answer</span>"#),
        "{line}"
    );
    assert!(
        line.contains(r#"<span class="route">t-1.swe.1 → pm.1</span>"#),
        "{line}"
    );
    assert!(
        line.contains(r#"<span class="state delivered">届いた</span>"#),
        "{line}"
    );
    assert!(
        line.contains(
            r#"<span>届いた <time datetime="2026-11-02T00:00:07.000Z">11-02 00:00</time></span>"#
        ),
        "{line}"
    );
    assert!(
        line.contains("<span>渡した時: session-start</span>"),
        "{line}"
    );
    // 渡した回数が 3 なら、渡し直しは 2 度。
    assert!(line.contains("<span>渡し直し 2 度</span>"), "{line}");
    assert!(line.contains("<pre>りんごにする</pre>"), "{line}");
    // 状態の字。
    for (id, state, label) in [
        ("p/m-1", "waiting", "まだ届いていない"),
        ("p/m-3", "expired", "期限切れ"),
        ("q/m-4", "carried", "cmux で運んだ"),
        ("q/m-5", "undeliverable", "渡せない"),
    ] {
        assert!(
            row(&page, id).contains(&format!(r#"<span class="state {state}">{label}</span>"#)),
            "{id}: {}",
            row(&page, id)
        );
    }
    // 届いた時刻・渡した時の無い行には、その欄を出さない。渡した回数が 1 の行に「渡し直し」は出さない。
    let line = row(&page, "p/m-1");
    assert!(!line.contains("届いた "), "{line}");
    assert!(!line.contains("渡した時"), "{line}");
    assert!(!line.contains("渡し直し"), "{line}");
    append(
        &app,
        "p",
        "m-1",
        with(waiting(), json!({ "state": "delivered", "deliveries": 1 })),
    )
    .await;
    let page = text(&app, "/messages").await;
    assert!(!row(&page, "p/m-1").contains("渡し直し"));
    // Key の無い記録は、Key の欄を出さない。
    assert!(!row(&page, "q/m-5").contains("<span></span>"));
}

/// 本文は必ず逃がす（HTML として出さない）。from・to・絞り込みの値も。
#[tokio::test]
async fn the_text_is_escaped_on_the_screen() {
    let app = App::start().await;
    place(
        &app,
        "p",
        "m-1",
        with(
            waiting(),
            json!({
                "text": "<script>alert(1)</script><img src=x onerror=alert(2)> & \"q\"",
                "from": "<b>from</b>",
                "to": "\"><script>alert(3)</script>",
            }),
        ),
    )
    .await;
    // 長い本文（頭だけ見せる方）も逃がす。
    place(
        &app,
        "p",
        "m-2",
        with(
            waiting(),
            json!({ "text": format!("<script>alert(4)</script>{}", "あ".repeat(400)) }),
        ),
    )
    .await;
    for uri in [
        "/messages",
        "/messages?to=%22%3E%3Cscript%3Ealert(5)%3C/script%3E&from=%3Cb%3E",
    ] {
        let page = text(&app, uri).await;
        assert!(!page.contains("<script>alert"), "{uri}: {page}");
        assert!(!page.contains("<img"), "{uri}: {page}");
        assert!(!page.contains("<b>"), "{uri}: {page}");
    }
    let page = text(&app, "/messages").await;
    assert!(
        page.contains("&#60;script&#62;alert(1)&#60;/script&#62;&#60;img src=x onerror=alert(2)&#62; &#38; &#34;q&#34;"),
        "{page}"
    );
    assert!(page.contains("&#60;b&#62;from&#60;/b&#62; → "), "{page}");
    assert_eq!(
        page.matches("&#60;script&#62;alert(4)").count(),
        2,
        "{page}"
    );
}

/// 長い本文は頭だけ見せて、開くと全部（script が無くても開ける `<details>`）。短い本文はそのまま。
#[tokio::test]
async fn a_long_text_shows_its_head_and_opens_to_the_whole() {
    let app = App::start().await;
    let long = format!("頭の行\n{}終わり", "あ".repeat(400));
    place(&app, "p", "long", with(waiting(), json!({ "text": long }))).await;
    place(
        &app,
        "p",
        "lines",
        with(waiting(), json!({ "text": "1\n2\n3\n4\n5\n6\n7" })),
    )
    .await;
    place(
        &app,
        "p",
        "short",
        with(waiting(), json!({ "text": "短い\n2 行目" })),
    )
    .await;
    let page = text(&app, "/messages").await;
    let line = row(&page, "p/long");
    let head: String = long.replace('\n', " ").chars().take(120).collect();
    assert!(
        line.contains(&format!("<details><summary>{head}…</summary>")),
        "{line}"
    );
    assert!(
        line.contains(&format!("<pre>{long}</pre></details>")),
        "{line}"
    );
    // 行の多い本文も、頭だけ。
    let line = row(&page, "p/lines");
    assert!(
        line.contains(
            "<details><summary>1 2 3 4 5 6 7…</summary><pre>1\n2\n3\n4\n5\n6\n7</pre></details>"
        ),
        "{line}"
    );
    let line = row(&page, "p/short");
    assert!(!line.contains("<details>"), "{line}");
    assert!(line.contains("<pre>短い\n2 行目</pre>"), "{line}");
}

/// 絞り込み（状態・受け手・送り手・Key・project）は link で動き、条件は URL に出る。候補と件数と
/// 「まだ届いていない n 件」は、絞りに関わらず置き場の全部。
#[tokio::test]
async fn filters_narrow_the_list_and_the_undelivered_count_stays() {
    let app = App::start().await;
    five(&app).await;
    place(&app, "p", "m-6", with(waiting(), json!({ "key": "A-1" }))).await;
    let page = text(&app, "/messages").await;
    assert!(
        page.contains("<title>まだ届いていない 2 件 ・ メッセージの記録</title>"),
        "{page}"
    );
    assert!(
        page.contains(r#"<a class="btn" id="undelivered" href="/messages?state=waiting" title="まだ届いていないメッセージだけに絞る">まだ届いていない 2 件</a>"#),
        "{page}"
    );
    for choice in [
        r#"<h2>状態</h2>"#,
        r#"<a href="/messages?state=waiting"><span>まだ届いていない</span><span class="n">2</span></a>"#,
        r#"<a href="/messages?state=carried"><span>cmux で運んだ</span><span class="n">1</span></a>"#,
        r#"<h2>受け手</h2>"#,
        r#"<a href="/messages?to=tpm"><span>tpm</span><span class="n">3</span></a>"#,
        r#"<h2>送り手</h2>"#,
        r#"<a href="/messages?from=pm.1"><span>pm.1</span><span class="n">2</span></a>"#,
        r#"<h2>Key</h2>"#,
        r#"<a href="/messages?key=A-1"><span>A-1</span><span class="n">4</span></a>"#,
        r#"<h2>project</h2>"#,
        r#"<a href="/messages?project=q"><span>q</span><span class="n">2</span></a>"#,
    ] {
        assert!(page.contains(choice), "{choice}: {page}");
    }
    assert!(!page.contains("絞り込みを外す"), "{page}");

    let page = text(&app, "/messages?state=waiting").await;
    assert_eq!(shown(&page), ["p/m-6", "p/m-1"]);
    // 選んだ候補は印が付き、もう 1 度押すと外れる。ほかの候補は、今の条件に重ねる。
    assert!(
        page.contains(r#"<a href="/messages" class="on" aria-current="true"><span>まだ届いていない</span><span class="n">2</span></a>"#),
        "{page}"
    );
    assert!(
        page.contains(r#"<a href="/messages?key=A-1&#38;state=waiting"><span>A-1</span>"#),
        "{page}"
    );
    assert!(
        page.contains(r#"<a class="btn on" id="undelivered" href="/messages?state=waiting" title="まだ届いていないメッセージだけに絞る" aria-current="true">まだ届いていない 2 件</a>"#),
        "{page}"
    );
    assert!(
        page.contains(r#"<a class="btn" href="/messages">絞り込みを外す</a>"#),
        "{page}"
    );
    // 絞っても、数と候補は置き場の全部。
    assert!(
        page.contains(r#"<span>tpm</span><span class="n">3</span>"#),
        "{page}"
    );

    assert_eq!(
        shown(&text(&app, "/messages?to=tpm&from=pm.1").await),
        ["q/m-4"]
    );
    assert_eq!(
        shown(&text(&app, "/messages?project=p&key=A-1").await),
        ["p/m-6", "p/m-2", "p/m-1"]
    );
    // 空の条件と知らない状態は、条件が無いのと同じ。
    assert_eq!(
        shown(&text(&app, "/messages?state=done&to=").await).len(),
        6
    );
    // まだ届いていない物が無ければ、数の釦は隠す。
    let quiet = App::start().await;
    place(
        &quiet,
        "p",
        "m-1",
        with(waiting(), json!({ "state": "delivered" })),
    )
    .await;
    let page = text(&quiet, "/messages").await;
    assert!(page.contains("<title>メッセージの記録</title>"), "{page}");
    assert!(
        page.contains(
            r#"title="まだ届いていないメッセージだけに絞る" hidden>まだ届いていない 0 件</a>"#
        ),
        "{page}"
    );
    // 同じ名の引数を 2 度書いた物は 400。
    let (status, headers, body) = app.get("/messages?to=a&to=b").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    refused(&headers, &body, "ValidationException");
}

/// 1 度に 200 件。続きは link で出す（同じ時刻の行が境に並んでも抜けない）。
#[tokio::test]
async fn long_lists_continue_with_a_link() {
    let app = App::start().await;
    for i in 0..205 {
        place(&app, "p", &format!("m-{i}"), waiting()).await;
    }
    let page = text(&app, "/messages?project=p").await;
    assert_eq!(shown(&page).len(), 200);
    assert_eq!(shown(&page)[0], "p/m-204");
    let next = more(&page).expect("続きの link");
    assert!(next.starts_with("/messages?project=p&at="), "{next}");
    let rest = text(&app, &next).await;
    assert_eq!(shown(&rest), ["p/m-4", "p/m-3", "p/m-2", "p/m-1", "p/m-0"]);
    assert!(more(&rest).is_none());
    // 読めない続きの印は、書かないのと同じ（頭から出す）。
    assert_eq!(shown(&text(&app, "/messages?at=zz").await).len(), 200);
}

/// 画面の中の 1 つの `<tag>`…`</tag>` の中身の SHA-256（CSP の `'sha256-…'`）。
fn inline_hash(page: &str, tag: &str) -> String {
    let body = page
        .split_once(&format!("<{tag}>"))
        .and_then(|(_, rest)| rest.split_once(&format!("</{tag}>")))
        .map_or("", |(body, _)| body);
    assert!(!body.is_empty(), "<{tag}> が無い");
    format!(
        "'sha256-{}'",
        aws_smithy_types::base64::encode(Sha256::digest(body.as_bytes()))
    )
}

/// メッセージの一覧の答えにも CSP が付き、inline の style と script は、この画面の物（hash）だけ。
/// 新着の知らせ（`message`）を聞いて読み直す script が入っている。
#[tokio::test]
async fn the_message_list_carries_its_own_csp() {
    let app = App::start().await;
    five(&app).await;
    let (status, headers, body) = app.get("/messages?state=waiting").await;
    assert_eq!(status, StatusCode::OK);
    let page = String::from_utf8(body).unwrap();
    assert_eq!(
        headers[header::CONTENT_SECURITY_POLICY].to_str().unwrap(),
        format!(
            "default-src 'none'; script-src {}; style-src {}; img-src 'self' data: blob:; connect-src 'self'; frame-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'",
            inline_hash(&page, "script"),
            inline_hash(&page, "style"),
        )
    );
    assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    assert!(page.contains(r#"new EventSource("/api/events")"#), "{page}");
    assert!(page.contains(r#"addEventListener("message""#), "{page}");
}
