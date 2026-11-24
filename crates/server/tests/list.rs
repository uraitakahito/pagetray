//! ListPages（`GET /api/pages`）の振る舞い（処理より先に書く）。今の server の「探す」の決まりを、新しい引数の名で見る。
//! 頁は `createdAt` と行の順で続ける（今の server の、同じ時刻の行が頁の境で抜けるか重なる不具合を直した形）。
mod common;

use axum::http::StatusCode;
use common::castlist::{ACTOR_A, ACTOR_B, Fake};
use common::{App, error_type, html, keys, page_paths};
use serde_json::Value;

const SE: &str = "role=software-engineer&workspace=workspace:1";

/// `day` 日の 0 時に置いた（取り込みの）時刻の引数。
fn at(day: u8) -> String {
    format!("createdAt=2026-11-{day:02}T00%3A00%3A00Z")
}

/// 一覧の 1 頁（200 を見る）。
async fn list(app: &App, query: &str) -> Value {
    let (status, _, json) = app.json(&format!("/api/pages?{query}")).await;
    assert_eq!(status, StatusCode::OK, "{query}: {json}");
    json
}

/// `query` の一覧を、`maxResults` ずつ nextToken で終わりまで辿り、頁ごとの path を返す。
async fn walk(app: &App, query: &str, max: usize) -> Vec<Vec<String>> {
    let mut pages = Vec::new();
    let mut token: Option<String> = None;
    loop {
        let mut q = format!("{query}&maxResults={max}");
        if let Some(t) = &token {
            q.push_str(&format!("&nextToken={t}"));
        }
        let json = list(app, &q).await;
        let paths = page_paths(&json);
        assert!(paths.len() <= max, "{q}: {json}");
        pages.push(paths);
        match json["nextToken"].as_str() {
            Some(t) => token = Some(t.to_owned()),
            None => return pages,
        }
        assert!(pages.len() < 100, "終わらない: {pages:?}");
    }
}

/// 4 枚を、1 日ずつ違う時刻で置く（今の tests/api.rs の filters_combine_and_sort_by_time と同じ見本）。
async fn four(app: &App) {
    app.place(
        "p",
        "A-1",
        "brief",
        &format!("role=tpm&workspace=w:1&{}", at(1)),
        "<p>a</p>",
    )
    .await;
    app.place(
        "p",
        "A-1",
        "plan",
        &format!("role=software-engineer&workspace=w:2&{}", at(2)),
        "<p>b</p>",
    )
    .await;
    app.place(
        "p",
        "B-2",
        "plan",
        &format!("role=software-engineer&workspace=w:3&{}", at(3)),
        "<p>c</p>",
    )
    .await;
    app.place(
        "q",
        "A-1",
        "result",
        &format!("role=devops-engineer&workspace=w:2&{}", at(4)),
        "<p>d</p>",
    )
    .await;
}

const ALL: [&str; 4] = [
    "/q/A-1/result-1.html",
    "/p/B-2/plan-1.html",
    "/p/A-1/plan-1.html",
    "/p/A-1/brief-1.html",
];

#[tokio::test]
async fn filters_combine_and_sort_by_time() {
    let app = App::start().await;
    four(&app).await;

    assert_eq!(app.paths("").await, ALL);
    assert_eq!(app.paths("sort=-createdAt").await, ALL);
    let mut oldest = ALL;
    oldest.reverse();
    assert_eq!(app.paths("sort=createdAt").await, oldest);

    assert_eq!(
        app.paths("role=software-engineer").await,
        ["/p/B-2/plan-1.html", "/p/A-1/plan-1.html"]
    );
    assert_eq!(
        app.paths("workspace=w:2").await,
        ["/q/A-1/result-1.html", "/p/A-1/plan-1.html"]
    );
    assert_eq!(
        app.paths("workspace=w:2&project=p").await,
        ["/p/A-1/plan-1.html"]
    );
    assert_eq!(app.paths("project=q").await, ["/q/A-1/result-1.html"]);
    assert_eq!(
        app.paths("key=A-1&kind=brief").await,
        ["/p/A-1/brief-1.html"]
    );
    assert_eq!(
        app.paths("kind=plan").await,
        ["/p/B-2/plan-1.html", "/p/A-1/plan-1.html"]
    );
    assert!(app.paths("role=tpm&kind=plan").await.is_empty());
    assert!(app.paths("project=r").await.is_empty());

    // 1 頁の数で切り、nextToken で続ける。
    let first = list(&app, "maxResults=2").await;
    assert_eq!(page_paths(&first), ALL[..2]);
    let token = first["nextToken"].as_str().expect("次の頁の印");
    let rest = list(&app, &format!("maxResults=2&nextToken={token}")).await;
    assert_eq!(page_paths(&rest), ALL[2..]);
    assert!(rest.get("nextToken").is_none(), "{rest}");
    assert_eq!(
        walk(&app, "sort=createdAt", 3).await,
        [
            vec![
                "/p/A-1/brief-1.html",
                "/p/A-1/plan-1.html",
                "/p/B-2/plan-1.html"
            ],
            vec!["/q/A-1/result-1.html"],
        ]
    );
    // ちょうど終わる頁には、nextToken が無い。
    let all = list(&app, "maxResults=4").await;
    assert_eq!(page_paths(&all), ALL);
    assert!(all.get("nextToken").is_none(), "{all}");

    // 記録に本文は無い。大きさは byte の数。
    let json = list(&app, "kind=brief").await;
    let page = &json["pages"][0];
    assert_eq!(page["size"], 8);
    assert!(page.get("body").is_none());
    assert_eq!(page["createdAt"], "2026-11-01T00:00:00Z");
}

/// 一覧の記録は、置いたページの記録と同じ欄（url と Location は無い）。返事・判定・見た物・送り手は、ある時だけ。
#[tokio::test]
async fn the_record_has_the_fields_of_the_put_answer_without_the_url() {
    let app = App::start().await;
    let (_, _, placed) = app
        .put("p", "SKILLS-14", "plan", SE, html("計画", "本文"))
        .await;
    let json = list(&app, "project=p").await;
    let listed = &json["pages"][0];
    assert_eq!(
        keys(listed),
        [
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
    let mut placed = placed;
    placed.as_object_mut().unwrap().remove("url");
    assert_eq!(listed, &placed);
    assert_eq!(keys(&json), ["pages"]);
}

/// 空の workspace（`workspace=`）も条件: workspace を書かずに置いたページに当たる。
#[tokio::test]
async fn an_empty_workspace_is_a_condition() {
    let app = App::start().await;
    app.place("p", "k", "plan", "role=tpm", "<p>1</p>").await;
    app.place("p", "k", "plan", "role=tpm&workspace=w:1", "<p>2</p>")
        .await;
    assert_eq!(app.paths("workspace=").await, ["/p/k/plan-1.html"]);
    assert_eq!(app.paths("workspace=w:1").await, ["/p/k/plan-2.html"]);
    assert_eq!(app.paths("").await.len(), 2);
}

#[tokio::test]
async fn search_finds_japanese_text_and_titles_but_not_markup() {
    let app = App::start().await;
    app.place(
        "p",
        "SKILLS-14",
        "plan",
        SE,
        &html("置き場の計画", "一覧の UI で絞り込みと並べ替えができる"),
    )
    .await;
    app.place(
        "p",
        "SKILLS-14",
        "result",
        SE,
        &html("結果", "container を起こした"),
    )
    .await;

    let plan = ["/p/SKILLS-14/plan-1.html"];
    assert_eq!(app.paths("q=絞り込み").await, plan);
    assert_eq!(app.paths("q=置き場の計画").await, plan);
    assert_eq!(app.paths("q=絞り込み%20並べ替え").await, plan);
    assert!(app.paths("q=絞り込み%20起こした").await.is_empty());
    // 前後の空白は落とし、空なら絞らない。
    assert_eq!(app.paths("q=%20絞り込み%20").await, plan);
    assert_eq!(app.paths("q=%20%20").await.len(), 2);
    assert_eq!(app.paths("q=").await.len(), 2);
    // 3 字に満たない語は、q 全体を端から見て探す（大小を分ける）。
    assert_eq!(app.paths("q=UI").await, plan);
    assert!(app.paths("q=ui").await.is_empty());
    assert_eq!(app.paths("q=結果").await, ["/p/SKILLS-14/result-1.html"]);
    assert_eq!(app.paths("q=UI%20で絞り").await, plan);
    assert!(app.paths("q=UI%20絞り込み").await.is_empty());
    // 検索の記号は、ただの字として扱う。
    assert!(app.paths("q=%22絞り込み").await.is_empty());
    assert!(app.paths("q=AND%20OR%20NOT").await.is_empty());
    assert!(app.paths("q=絞り*").await.is_empty());
    // tag と style の中身は索引に入らない。
    assert!(app.paths("q=margin").await.is_empty());
    assert!(app.paths("q=doctype").await.is_empty());
    assert!(app.paths("q=container&kind=plan").await.is_empty());
    // 3 字以上の語は大小を問わない。
    assert_eq!(
        app.paths("q=CONTAINER").await,
        ["/p/SKILLS-14/result-1.html"]
    );
}

/// 直した不具合: 同じ時刻の行が頁の境に並んでも、抜けず重ならない（今の server は created_at の字だけで続けていた）。
#[tokio::test]
async fn pages_with_the_same_time_are_neither_dropped_nor_repeated() {
    let app = App::start().await;
    // 同じ時刻の 7 枚と、前後に 1 枚ずつ。
    app.place(
        "p",
        "k",
        "brief",
        &format!("role=tpm&{}", at(1)),
        "<p>0</p>",
    )
    .await;
    for i in 0..7 {
        let kind = ["plan", "result", "note"][i % 3];
        app.place("p", "k", kind, &format!("role=tpm&{}", at(2)), "<p>x</p>")
            .await;
    }
    app.place(
        "p",
        "k",
        "brief",
        &format!("role=tpm&{}", at(3)),
        "<p>9</p>",
    )
    .await;
    for sort in ["sort=-createdAt", "sort=createdAt"] {
        let whole = app.paths(&format!("{sort}&maxResults=1000")).await;
        assert_eq!(whole.len(), 9);
        for max in [1, 2, 3, 4] {
            let pages = walk(&app, sort, max).await;
            let walked: Vec<String> = pages.concat();
            assert_eq!(walked, whole, "{sort} maxResults={max}: {pages:?}");
        }
    }
    // 同じ時刻の中は置いた順（新しい順なら後に置いた物が先）。
    let newest = app.paths("maxResults=3").await;
    assert_eq!(
        newest,
        ["/p/k/brief-2.html", "/p/k/plan-3.html", "/p/k/note-2.html"]
    );
}

/// 辿る間に置いたページ（前の頁より新しい）は、新しい順の続きに出ない（重ならない）。
#[tokio::test]
async fn a_page_put_while_walking_does_not_repeat_or_shift_the_rest() {
    let app = App::start().await;
    four(&app).await;
    let first = list(&app, "maxResults=2").await;
    let token = first["nextToken"].as_str().unwrap().to_owned();
    app.place(
        "p",
        "A-1",
        "plan",
        &format!("role=tpm&{}", at(5)),
        "<p>e</p>",
    )
    .await;
    let rest = list(&app, &format!("maxResults=2&nextToken={token}")).await;
    assert_eq!(page_paths(&rest), ALL[2..]);
}

/// nextToken は、同じ sort の一覧の答えの物だけ。崩れた物・ほかの sort の物・ほかの検索の物は 400。
#[tokio::test]
async fn a_token_from_another_query_or_a_broken_one_is_refused() {
    let app = App::start().await;
    four(&app).await;
    let newest = list(&app, "maxResults=1").await;
    let token = newest["nextToken"].as_str().unwrap().to_owned();
    let oldest = list(&app, "sort=createdAt&maxResults=1").await;
    let old_token = oldest["nextToken"].as_str().unwrap().to_owned();
    // 中を読まずに渡すと続く。ほかの条件は見ない。
    assert_eq!(
        app.paths(&format!("maxResults=1&nextToken={token}")).await,
        ALL[1..2]
    );
    assert_eq!(
        app.paths(&format!("sort=-createdAt&maxResults=1&nextToken={token}"))
            .await,
        ALL[1..2]
    );
    let other_kind: String = "files\n-createdAt\n2026-11-01T00:00:00.000Z\n1"
        .bytes()
        .map(|b| format!("{b:02x}"))
        .collect();
    let short: String = "pages\n-createdAt\n2026-11-01T00:00:00.000Z"
        .bytes()
        .map(|b| format!("{b:02x}"))
        .collect();
    let bad_id: String = "pages\n-createdAt\n2026-11-01T00:00:00.000Z\nx"
        .bytes()
        .map(|b| format!("{b:02x}"))
        .collect();
    for query in [
        format!("sort=createdAt&nextToken={token}"),
        format!("nextToken={old_token}"),
        format!("nextToken={other_kind}"),
        format!("nextToken={short}"),
        format!("nextToken={bad_id}"),
        "nextToken=zz".to_owned(),
        "nextToken=abc".to_owned(),
        format!("nextToken={}", &token[..token.len() - 2]),
    ] {
        let (status, headers, json) = app.json(&format!("/api/pages?{query}")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}: {json}");
        assert_eq!(error_type(&headers), "ValidationException", "{query}");
        assert!(
            json["message"].as_str().unwrap_or("").contains("nextToken"),
            "{query}: {json}"
        );
    }
}

/// 1 頁の数は既定 100。1〜1000 の外は、定義が 400 で断る（今の server は丸めていた）。
#[tokio::test]
async fn max_results_defaults_to_100_and_must_be_1_to_1000() {
    let app = App::start().await;
    for i in 0..101 {
        app.place("p", "k", "plan", "role=tpm", &format!("<p>{i}</p>"))
            .await;
    }
    let json = list(&app, "").await;
    assert_eq!(page_paths(&json).len(), 100);
    let token = json["nextToken"].as_str().expect("101 枚目がある");
    let rest = list(&app, &format!("nextToken={token}")).await;
    assert_eq!(page_paths(&rest), ["/p/k/plan-1.html"]);
    assert_eq!(page_paths(&list(&app, "maxResults=1000").await).len(), 101);
    assert_eq!(
        page_paths(&list(&app, "maxResults=1").await),
        ["/p/k/plan-101.html"]
    );
    for query in ["maxResults=0", "maxResults=1001", "maxResults=-1"] {
        let (status, headers, _) = app.json(&format!("/api/pages?{query}")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}");
        assert_eq!(error_type(&headers), "ValidationException", "{query}");
    }
}

#[tokio::test]
async fn bad_list_arguments_are_refused() {
    let app = App::start().await;
    for query in [
        "reply=done",
        "reply=waiting&on=staff",
        // on は reply と一緒にだけ。
        "on=review",
        "on=pm",
        "verdict=done",
        "sort=created_at",
        "sort=-created_at",
        "sort=title",
        "project=a%20b",
        "key=a%2Fb",
        "kind=Plan",
        "role=T",
        "workspace=a%20b",
        // 送り手は castlist のアクターの id の形。
        "actor=019a-nobody",
        "maxResults=abc",
    ] {
        let (status, headers, json) = app.json(&format!("/api/pages?{query}")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}: {json}");
        assert!(
            matches!(
                error_type(&headers),
                "ValidationException" | "SerializationException"
            ),
            "{query}: {headers:?}"
        );
    }
    let (_, _, json) = app.json("/api/pages?on=review").await;
    assert!(
        json["message"].as_str().unwrap_or("").contains("reply"),
        "{json}"
    );
}

/// `actor=<castlist のアクターの id>` で、その送り手のページだけ（castlist の答えの無い送り手は当たらない）。
#[tokio::test]
async fn the_actor_filter_keeps_only_that_senders_pages() {
    let fake = Fake::start().await;
    let app = App::with_castlist(&fake.url).await;
    let as_a = "role=tpm&actorIssuer=tpm&actorRef=claude.session%3Aa";
    let as_pm = "role=tpm&actorIssuer=tpm&actorRef=tpm.person%3Apm";
    app.place("p", "SKILLS-14", "plan", as_a, "<title>a の計画</title>")
        .await;
    app.place("p", "SKILLS-14", "brief", as_pm, "<title>pm の依頼</title>")
        .await;
    app.place(
        "p",
        "SKILLS-14",
        "note",
        "role=tpm",
        "<title>送り手なし</title>",
    )
    .await;
    assert_eq!(
        app.paths(&format!("actor={ACTOR_A}")).await,
        ["/p/SKILLS-14/plan-1.html"]
    );
    assert_eq!(
        app.paths(&format!("actor={ACTOR_B}&kind=brief")).await,
        ["/p/SKILLS-14/brief-1.html"]
    );
    assert!(
        app.paths(&format!("actor={ACTOR_B}&kind=plan"))
            .await
            .is_empty()
    );
    assert!(
        app.paths("actor=019a0000-0000-7000-8000-0000000000ff")
            .await
            .is_empty()
    );
    assert_eq!(app.paths("").await.len(), 3);
    // 記録に送り手が出る（送り手の無いページには欄が無い）。
    let json = list(&app, "kind=plan").await;
    assert_eq!(json["pages"][0]["sender"]["actor"]["id"], ACTOR_A);
    assert_eq!(json["pages"][0]["sender"]["ref"], "claude.session:a");
    let json = list(&app, "kind=note").await;
    assert!(json["pages"][0].get("sender").is_none(), "{json}");
}

/// castlist の無い pagetray が置いたページ（送り手の答えが無い）は、送り手で絞っても当たらない。
#[tokio::test]
async fn a_sender_without_an_answer_is_not_found_by_actor() {
    let app = App::start().await;
    app.place(
        "p",
        "k",
        "plan",
        "role=tpm&actorIssuer=tpm&actorRef=claude.session%3Aa",
        "<p>x</p>",
    )
    .await;
    assert!(app.paths(&format!("actor={ACTOR_A}")).await.is_empty());
    let json = list(&app, "").await;
    assert_eq!(
        keys(&json["pages"][0]["sender"]),
        ["issuer", "ref"],
        "{json}"
    );
}

/// DB が読めなければ 500 `InternalError`（答えは決まった字で、中の誤りの字を出さない）。
#[tokio::test]
async fn a_db_error_is_an_internal_error() {
    let app = App::start().await;
    app.place("p", "k", "plan", "role=tpm", "<p>x</p>").await;
    assert_eq!(app.paths("").await, ["/p/k/plan-1.html"]);
    app.execute("drop table page_targets").await;
    let (status, headers, json) = app.json("/api/pages").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{json}");
    assert_eq!(error_type(&headers), "InternalError");
    let message = json["message"].as_str().unwrap_or("");
    assert!(!message.contains("page_targets"), "{message}");
}

/// 読む接続がみな塞がっていれば、待ちきれずに 503 `Unavailable`。
#[tokio::test]
async fn a_list_waits_for_a_read_connection_and_then_is_unavailable() {
    let app = App::start().await;
    let mut held = Vec::new();
    for _ in 0..pagetray_next::db::READS {
        held.push(app.db.read().acquire().await.expect("読む接続"));
    }
    let (status, headers, json) = app.json("/api/pages").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{json}");
    assert_eq!(error_type(&headers), "Unavailable");
    drop(held);
    assert!(app.paths("").await.is_empty());
}

// --- 索引 ---

/// 探す SQL（`pagetray_next::search::plans()`。条件を 1 つずつと、重ねた物）が、pages を索引なしに端から読まない。
/// 絞る列に索引のある条件（project・key・送り手・返事待ち・nextToken）は、pages を索引で引く（SEARCH。返事待ちは、
/// 返事待ちだけの部分の索引で引いた id から）。
/// ほかの条件と、条件の無い一覧は、時刻の索引（pages_created）の順に読み、1 頁で止まる。条件の無い一覧と nextToken は、
/// 行を並べ替える一時の表を作らない（絞った物は、引いた行を並べ替えてよい。1 枚の見た物を書いた順に繋ぐ小さな並べ替えは別）。
/// 短い語の検索（instr）は、今の server と同じく索引の表を端から見る（決め: 短い語は trigram の索引で引けない）。
#[tokio::test]
async fn the_list_sql_reads_pages_by_an_index() {
    let app = App::start().await;
    let plans = pagetray_next::search::plans();
    let names: Vec<&str> = plans.iter().map(|(name, _)| *name).collect();
    let searched = [
        "project",
        "project_key",
        "project_key_kind",
        "actor",
        "waiting",
        "waiting_review",
        "after",
        "after_oldest",
    ];
    let ordered = ["plain", "oldest", "after", "after_oldest"];
    let others = [
        "kind",
        "role",
        "workspace",
        "answered",
        "verdict",
        "fts",
        "scan",
        "all",
    ];
    for name in searched.iter().chain(&ordered).chain(&others) {
        assert!(names.contains(name), "{name} が plans に無い: {names:?}");
    }
    for (name, sql) in &plans {
        let plan: Vec<(i64, i64, i64, String)> =
            sqlx::query_as(sqlx::AssertSqlSafe(format!("explain query plan {sql}")))
                .fetch_all(app.db.read())
                .await
                .unwrap_or_else(|e| panic!("{name}: {e}\n{sql}"));
        let details: Vec<&str> = plan.iter().map(|row| row.3.as_str()).collect();
        // 索引なしに端から読む表（SCAN <表> の後に USING が無い）は、全文の索引の表（短い語）だけ。
        let bare: Vec<&&str> = details
            .iter()
            .filter(|d| {
                d.strip_prefix("SCAN ")
                    .is_some_and(|rest| !rest.contains("USING") && !rest.starts_with("pages_fts"))
            })
            .collect();
        assert!(bare.is_empty(), "{name} が端から読む: {details:?}\n{sql}");
        if searched.contains(name) {
            assert!(
                details.iter().any(|d| d.starts_with("SEARCH p USING")),
                "{name} が pages を索引で引かない: {details:?}\n{sql}"
            );
        }
        if ordered.contains(name) {
            assert!(
                !details.contains(&"USE TEMP B-TREE FOR ORDER BY"),
                "{name} が並べ替えの表を作る: {details:?}\n{sql}"
            );
        }
    }
}
