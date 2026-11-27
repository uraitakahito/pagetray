//! ページの一覧の画面が要る読む操作（処理より先に書く）: 絞り込みの候補と待ちの数 GetPageFacets（`GET /api/pages/facets`）・
//! ページを見たレビュー ListReviews（`GET /api/reviews`）・受ける file の種類 GetFileRules（`GET /api/files/rules`）。
//! 画面（`/`）が DB から直に読んでいる物を、API で読めるようにする（画面を callboard に移すため）。数え方は画面と同じ SQL。
mod common;

use axum::http::StatusCode;
use common::castlist::{ACTOR_A, Fake, NAME_A};
use common::{App, html, refused};
use serde_json::{Value, json};

const SE: &str = "role=software-engineer&workspace=workspace:1";
const TPM: &str = "role=tpm&workspace=workspace:2";
const AS_A: &str = "actorIssuer=tpm&actorRef=claude.session%3Aa";

async fn ok(app: &App, uri: &str) -> Value {
    let (status, _, json) = app.json(uri).await;
    assert_eq!(status, StatusCode::OK, "{uri}: {json}");
    json
}

/// 候補の並び 1 つを、(値, 件数, 添える字) の並びにする。添える字が無ければ空。
fn counts(json: &Value, name: &str) -> Vec<(String, i64, String)> {
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
                c.get("note")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
            )
        })
        .collect()
}

fn triples<const N: usize>(items: [(&str, i64, &str); N]) -> Vec<(String, i64, String)> {
    items
        .into_iter()
        .map(|(v, n, note)| (v.to_owned(), n, note.to_owned()))
        .collect()
}

async fn place(app: &App, project: &str, key: &str, kind: &str, query: &str) -> String {
    let n = app
        .place(project, key, kind, query, &html(kind, "本文"))
        .await;
    format!("/{project}/{key}/{kind}-{n}.html")
}

// --- GetPageFacets ---

/// 候補と待ちの数は、置き場の全部（絞らない）。役割・種類・project・判定は件数の多い順（同じなら名の順）、
/// workspace と Key は新しく置かれた順。workspace には、その ID で置かれた Key を添える。件数が 0 の値は出さない。
#[tokio::test]
async fn the_page_facets_count_the_whole_store() {
    let app = App::start().await;
    let empty = ok(&app, "/api/pages/facets").await;
    for name in [
        "senders",
        "roles",
        "workspaces",
        "kinds",
        "keys",
        "projects",
        "verdicts",
    ] {
        assert_eq!(counts(&empty, name), [], "{name}");
    }
    for name in ["waiting", "answered", "reviewWaiting", "escalated"] {
        assert_eq!(empty[name], 0, "{name}");
    }

    let confirm = place(&app, "p", "A-1", "confirm", SE).await;
    let confirm2 = place(&app, "p", "A-1", "confirm", SE).await;
    let plan = place(&app, "p", "A-1", "plan", SE).await;
    let brief = place(&app, "p", "B-2", "brief", TPM).await;
    let review = place(&app, "q", "B-2", "review", "role=staff-engineer").await;
    // 返事: Product Manager の返事待ち 1・答えた 1、レビュー待ち 1、上げた 1。
    for (page, body) in [
        (&confirm, json!({ "state": "waiting" })),
        (&confirm2, json!({ "state": "waiting" })),
        (&confirm2, json!({ "state": "answered" })),
        (&brief, json!({ "state": "waiting", "on": "review" })),
        (&review, json!({ "state": "waiting", "on": "tpm" })),
    ] {
        let (status, _, json) = app.reply(page, body).await;
        assert_eq!(status, StatusCode::OK, "{page}: {json}");
    }
    let (status, _, json) = app
        .review(
            &review,
            Some(json!({ "verdict": "concerns", "targets": [plan] })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{json}");

    let json = ok(&app, "/api/pages/facets").await;
    assert_eq!(json["waiting"], 1);
    assert_eq!(json["answered"], 1);
    assert_eq!(json["reviewWaiting"], 1);
    assert_eq!(json["escalated"], 1);
    assert_eq!(
        counts(&json, "roles"),
        triples([
            ("software-engineer", 3, ""),
            ("staff-engineer", 1, ""),
            ("tpm", 1, "")
        ])
    );
    assert_eq!(
        counts(&json, "kinds"),
        triples([
            ("confirm", 2, ""),
            ("brief", 1, ""),
            ("plan", 1, ""),
            ("review", 1, "")
        ])
    );
    assert_eq!(
        counts(&json, "projects"),
        triples([("p", 4, ""), ("q", 1, "")])
    );
    assert_eq!(counts(&json, "verdicts"), triples([("concerns", 1, "")]));
    // Key と workspace は、新しく置かれた順。workspace の空の値（書かずに置いた）も 1 つの候補。
    assert_eq!(
        counts(&json, "keys"),
        triples([("B-2", 2, ""), ("A-1", 3, "")])
    );
    assert_eq!(
        counts(&json, "workspaces"),
        triples([
            ("", 1, ""),
            ("workspace:2", 1, "B-2"),
            ("workspace:1", 3, "A-1")
        ])
    );
    // 送り手の答えの無い置き場では、送り手の候補は無い。
    assert_eq!(counts(&json, "senders"), []);
    // 添える字の無い候補に、note の欄は無い。
    assert_eq!(
        json["roles"][0],
        json!({ "value": "software-engineer", "count": 3 })
    );
}

/// 送り手の候補は、castlist の答えのある送り手だけ。値はアクターの id、添える字はその名。
#[tokio::test]
async fn the_sender_facet_carries_the_actor_and_its_name() {
    let fake = Fake::start().await;
    let app = App::with_castlist(&fake.url).await;
    place(&app, "p", "A-1", "plan", &format!("{SE}&{AS_A}")).await;
    place(&app, "p", "A-1", "plan", &format!("{SE}&{AS_A}")).await;
    place(&app, "p", "A-1", "plan", SE).await;
    let json = ok(&app, "/api/pages/facets").await;
    assert_eq!(counts(&json, "senders"), triples([(ACTOR_A, 2, NAME_A)]));
}

/// 引数は無い（書いても読まない）。書く method は無い。
#[tokio::test]
async fn the_page_facets_take_no_arguments() {
    let app = App::start().await;
    place(&app, "p", "A-1", "plan", SE).await;
    let json = ok(&app, "/api/pages/facets?project=zzz").await;
    assert_eq!(counts(&json, "projects"), triples([("p", 1, "")]));
}

// --- ListReviews ---

/// 名指したページ（path）ごとに、そのページを見たレビューのページ（古い順）。判定は、ある時だけ。
#[tokio::test]
async fn reviews_are_listed_for_the_named_pages() {
    let app = App::start().await;
    let plan = place(&app, "p", "A-1", "plan", SE).await;
    let brief = place(&app, "p", "A-1", "brief", SE).await;
    let other = place(&app, "p", "A-1", "plan", SE).await;
    let first = place(&app, "p", "A-1", "review", "role=staff-engineer").await;
    let second = place(&app, "q", "Z-9", "review", "role=staff-engineer").await;
    for (review, body) in [
        (
            &first,
            json!({ "verdict": "concerns", "targets": [plan, brief, "https://github.com/o/r/pull/1"] }),
        ),
        (&second, json!({ "targets": [plan] })),
    ] {
        let (status, _, json) = app.review(review, Some(body)).await;
        assert_eq!(status, StatusCode::OK, "{json}");
    }
    // 見た物の印の無いページ（レビューでない種類）が同じ物を指していても、数えない。
    let json = ok(
        &app,
        &format!("/api/reviews?target={plan}&target={other}&target={brief}"),
    )
    .await;
    assert_eq!(
        json["reviews"],
        json!([
            { "target": plan, "path": first, "verdict": "concerns" },
            { "target": brief, "path": first, "verdict": "concerns" },
            { "target": plan, "path": second },
        ])
    );
    // 見られていないページだけなら、空。
    let json = ok(&app, &format!("/api/reviews?target={other}")).await;
    assert_eq!(json["reviews"], json!([]));
    // 置き場に無い path でも、空（形は見ない）。
    let json = ok(&app, "/api/reviews?target=/no/such/plan-1.html").await;
    assert_eq!(json["reviews"], json!([]));
}

/// 名指すページは 1 つから 200 まで。無ければ・超えれば 400。
#[tokio::test]
async fn reviews_need_one_to_two_hundred_targets() {
    let app = App::start().await;
    let (status, headers, body) = app.get("/api/reviews").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    refused(&headers, &body, "ValidationException");
    let many = |n: usize| {
        (0..n)
            .map(|i| format!("target=/p/K/plan-{}.html", i + 1))
            .collect::<Vec<_>>()
            .join("&")
    };
    let (status, _, json) = app.json(&format!("/api/reviews?{}", many(200))).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let (status, headers, body) = app.get(&format!("/api/reviews?{}", many(201))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    refused(&headers, &body, "ValidationException");
}

// --- GetFileRules ---

/// 受ける file の種類（拡張子と、返す時の Content-Type）と、1 つの大きさの上限。並びは、置き場が断る時の文と同じ。
#[tokio::test]
async fn the_file_rules_list_the_accepted_types_and_the_size_limit() {
    let app = App::start().await;
    let json = ok(&app, "/api/files/rules").await;
    assert_eq!(json["maxBytes"], 16 * 1024 * 1024);
    let types: Vec<(&str, &str)> = json["types"]
        .as_array()
        .unwrap_or_else(|| panic!("{json}"))
        .iter()
        .map(|t| (t["ext"].as_str().unwrap(), t["type"].as_str().unwrap()))
        .collect();
    assert_eq!(
        types,
        [
            ("png", "image/png"),
            ("jpg", "image/jpeg"),
            ("jpeg", "image/jpeg"),
            ("gif", "image/gif"),
            ("webp", "image/webp"),
            ("pdf", "application/pdf"),
            ("txt", "text/plain; charset=utf-8"),
            ("log", "text/plain; charset=utf-8"),
            ("md", "text/markdown; charset=utf-8"),
            ("csv", "text/csv; charset=utf-8"),
            ("json", "application/json; charset=utf-8"),
        ]
    );
    // 受ける種類の file は置け、種類に無い拡張子は 415。
    let (status, _, _) = app
        .send(common::put_file(
            &common::files_uri("p", "A-1", "name=a.txt&role=tpm"),
            Some("text/plain"),
            "x",
        ))
        .await;
    assert_eq!(status, StatusCode::CREATED);
}
