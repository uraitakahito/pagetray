//! CreatePage の振る舞い（処理より先に書く）。0.8.x の server の「置く」の決まり（docs/src/next.md の「ページを置く」）を、
//! 1.0.0 の道と欄の名で見る。送り手（actorIssuer・actorRef）は tests/senders.rs。
mod common;

use axum::body::Body;
use axum::http::{StatusCode, header};
use common::{App, error_type, html, pages_uri, put_page, scalar};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

const SE: &str = "role=software-engineer&workspace=workspace:1";

#[tokio::test]
async fn numbers_start_at_one_and_follow_per_project_key_and_kind() {
    let app = App::start().await;
    let (status, headers, json) = app
        .put("p", "SKILLS-14", "plan", SE, html("計画", "本文"))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    assert_eq!(json["n"], "1");
    assert_eq!(json["project"], "p");
    assert_eq!(json["key"], "SKILLS-14");
    assert_eq!(json["kind"], "plan");
    assert_eq!(json["path"], "/p/SKILLS-14/plan-1.html");
    assert_eq!(json["url"], "http://127.0.0.1:8801/p/SKILLS-14/plan-1.html");
    assert_eq!(
        headers[header::LOCATION],
        "http://127.0.0.1:8801/p/SKILLS-14/plan-1.html"
    );
    assert_eq!(json["title"], "計画");
    assert_eq!(json["role"], "software-engineer");
    assert_eq!(json["workspace"], "workspace:1");

    assert_eq!(
        app.place("p", "SKILLS-14", "plan", SE, "<p>2</p>").await,
        "2"
    );
    assert_eq!(
        app.place("p", "SKILLS-14", "result", SE, "<p>1</p>").await,
        "1"
    );
    assert_eq!(
        app.place("p", "t-x", "plan", "role=tpm", "<p>1</p>").await,
        "1"
    );
    assert_eq!(
        app.place("q", "SKILLS-14", "plan", "role=tpm", "<p>1</p>")
            .await,
        "1"
    );
    assert_eq!(
        app.place("p", "SKILLS-14", "plan", SE, "<p>3</p>").await,
        "3"
    );
}

/// 答えはページの記録だけ: 本文は入らず、置いた時は返事・判定・見た物・送り手の欄も無い。
#[tokio::test]
async fn the_answer_is_the_page_record_without_the_body() {
    let app = App::start().await;
    let page = html("題", "中身");
    let (status, _, json) = app.put("p", "k", "plan", "role=tpm", page.clone()).await;
    assert_eq!(status, StatusCode::CREATED);
    let mut keys: Vec<&str> = json
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort();
    assert_eq!(
        keys,
        vec![
            "createdAt",
            "key",
            "kind",
            "n",
            "path",
            "project",
            "role",
            "size",
            "title",
            "url",
            "workspace"
        ]
    );
    // 本文の byte の数（日本語は 1 字 3 byte）。
    assert_eq!(json["size"], page.len() as i64);
    // workspace を書かなければ空。
    assert_eq!(json["workspace"], "");
}

#[tokio::test]
async fn fifty_parallel_puts_get_fifty_distinct_numbers_without_gaps() {
    let app = App::start().await;
    let mut tasks = Vec::new();
    for i in 0..50 {
        let router = app.app.clone();
        tasks.push(tokio::spawn(async move {
            let uri = pages_uri("p", "SKILLS-14", "result", SE);
            let response = router
                .oneshot(put_page(&uri, format!("<title>{i}</title>")))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::CREATED);
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            let json: Value = serde_json::from_slice(&bytes).unwrap();
            json["n"].as_str().unwrap().parse::<i64>().unwrap()
        }));
    }
    let mut numbers = Vec::new();
    for task in tasks {
        numbers.push(task.await.unwrap());
    }
    numbers.sort();
    assert_eq!(numbers, (1..=50).collect::<Vec<i64>>());
    assert_eq!(app.count("pages").await, 50);
    assert_eq!(app.count("pages_fts").await, 50);
}

/// 消した番号は 2 度使わない: 任せた番号は、置いた番号と消した番号の最大の次。
#[tokio::test]
async fn a_removed_number_is_never_used_again() {
    let app = App::start().await;
    assert_eq!(
        app.place("p", "k", "plan", "role=tpm", "<p>1</p>").await,
        "1"
    );
    app.execute(
        "insert into removed(project, key, kind, n, removed_at) values('p', 'k', 'plan', 5, '2026-11-24T00:00:00.000Z')",
    )
    .await;
    assert_eq!(
        app.place("p", "k", "plan", "role=tpm", "<p>6</p>").await,
        "6"
    );
    // 消した番号しか無い種類も、その次から。
    app.execute(
        "insert into removed(project, key, kind, n, removed_at) values('p', 'k', 'brief', 2, '2026-11-24T00:00:00.000Z')",
    )
    .await;
    assert_eq!(
        app.place("p", "k", "brief", "role=tpm", "<p>3</p>").await,
        "3"
    );
}

#[tokio::test]
async fn an_import_keeps_its_number_and_time_and_a_taken_number_is_a_conflict() {
    let app = App::start().await;
    let query = "role=tpm&n=3&createdAt=2026-11-20T01%3A02%3A03Z";
    let (status, _, json) = app
        .put("p", "k", "brief", query, "<title>古い</title>")
        .await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    assert_eq!(json["n"], "3");
    assert_eq!(json["createdAt"], "2026-11-20T01:02:03Z");
    // 残す字は、いつも UTC・ミリ秒まで・24 字。
    let stored: String = scalar(&app, "select created_at from pages where n = 3").await;
    assert_eq!(stored, "2026-11-20T01:02:03.000Z");

    // 置いた番号は 409。何も替えない（本文も索引も）。
    let (status, headers, json) = app.put("p", "k", "brief", query, "<title>別</title>").await;
    assert_eq!(status, StatusCode::CONFLICT, "{json}");
    assert_eq!(error_type(&headers), "Conflict");
    assert!(json["message"].as_str().unwrap().contains("番号"), "{json}");
    let (_, _, body) = app.get("/p/k/brief-3.html").await;
    assert_eq!(body, "<title>古い</title>".as_bytes());
    assert_eq!(app.count("pages").await, 1);
    assert_eq!(app.count("pages_fts").await, 1);

    // 番号を任せると、取り込んだ番号の次になる。
    assert_eq!(
        app.place("p", "k", "brief", "role=tpm", "<p>x</p>").await,
        "4"
    );

    // 消した番号も 409。
    app.execute(
        "insert into removed(project, key, kind, n, removed_at) values('p', 'k', 'brief', 9, '2026-11-24T00:00:00.000Z')",
    )
    .await;
    let (status, headers, _) = app.put("p", "k", "brief", "role=tpm&n=9", "<p>x</p>").await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_type(&headers), "Conflict");
    assert_eq!(app.count("pages").await, 2);
    assert_eq!(app.count("pages_fts").await, 2);
    // 別の種類・別の Key の同じ番号は置ける。
    assert_eq!(
        app.place("p", "k", "plan", "role=tpm&n=3", "<p>x</p>")
            .await,
        "3"
    );
    assert_eq!(
        app.place("p", "k2", "brief", "role=tpm&n=3", "<p>x</p>")
            .await,
        "3"
    );
}

#[tokio::test]
async fn created_at_is_utc_to_the_millisecond_and_now_when_missing() {
    let app = App::start().await;
    for (given, answered, stored) in [
        (
            "2026-11-20T01%3A02%3A03.5Z",
            "2026-11-20T01:02:03.5Z",
            "2026-11-20T01:02:03.500Z",
        ),
        (
            "2026-11-20T01%3A02%3A03.123Z",
            "2026-11-20T01:02:03.123Z",
            "2026-11-20T01:02:03.123Z",
        ),
        (
            "0001-01-01T00%3A00%3A00Z",
            "0001-01-01T00:00:00Z",
            "0001-01-01T00:00:00.000Z",
        ),
        (
            "9999-12-31T23%3A59%3A59.999Z",
            "9999-12-31T23:59:59.999Z",
            "9999-12-31T23:59:59.999Z",
        ),
    ] {
        let (status, _, json) = app
            .put(
                "p",
                "k",
                "plan",
                &format!("role=tpm&createdAt={given}"),
                "<p>x</p>",
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{given}: {json}");
        assert_eq!(json["createdAt"], answered, "{given}");
        let n = json["n"].as_str().unwrap();
        let got: String = scalar(
            &app,
            &format!("select created_at from pages where kind = 'plan' and n = {n}"),
        )
        .await;
        assert_eq!(got, stored, "{given}");
    }
    // 書かなければ今（UTC・ミリ秒まで・24 字）。
    let before = pagetray_server::db::now();
    let (_, _, json) = app.put("p", "k", "result", "role=tpm", "<p>x</p>").await;
    let after = pagetray_server::db::now();
    let got: String = scalar(&app, "select created_at from pages where kind = 'result'").await;
    assert_eq!(got.len(), 24, "{got}");
    assert!(got.ends_with('Z'), "{got}");
    assert!(before <= got && got <= after, "{before} {got} {after}");
    assert!(json["createdAt"].as_str().unwrap().ends_with('Z'), "{json}");
}

#[tokio::test]
async fn the_title_is_the_first_title_tag_in_any_case() {
    let app = App::start().await;
    for (page, title) in [
        (html("計画", "本文"), "計画"),
        (
            "<html><head><TITLE lang=\"ja\">\n SKILLS-14 計画: A &amp; B\n</TITLE></head></html>"
                .to_owned(),
            "SKILLS-14 計画: A & B",
        ),
        ("<title>一</title><title>二</title>".to_owned(), "一"),
        ("<h1>題</h1>".to_owned(), ""),
    ] {
        let (status, _, json) = app.put("p", "k", "plan", "role=tpm", page.clone()).await;
        assert_eq!(status, StatusCode::CREATED, "{page}");
        assert_eq!(json["title"], title, "{page}");
    }
}

/// 全文の索引には、題と、tag・script・style・comment を除いた字が入る。
#[tokio::test]
async fn the_search_index_gets_the_title_and_the_plain_text() {
    let app = App::start().await;
    let page = "<title>一覧の題</title><style>body{color:red}</style><h1>一覧の UI</h1><!-- 隠す --><p>絞り込み<br>並べ替え &lt;b&gt;</p><script>let a = 1;</script>";
    app.place("p", "k", "plan", "role=tpm", page).await;
    let (title, text): (String, String) = sqlx::query_as("select title, text from pages_fts")
        .fetch_one(app.db.read())
        .await
        .unwrap();
    assert_eq!(title, "一覧の題");
    assert_eq!(text, "一覧の題 一覧の UI 絞り込み 並べ替え <b>");
    // 3 字ずつの索引で、日本語も引ける。
    let hits: i64 = scalar(
        &app,
        "select count(*) from pages_fts where pages_fts match '\"並べ替\"'",
    )
    .await;
    assert_eq!(hits, 1);
}

/// 本文は UTF-8 でなくても受け、置いたままの byte で残す（題は読めない byte を置き換えて取る）。
#[tokio::test]
async fn a_body_that_is_not_utf8_is_kept_byte_for_byte() {
    let app = App::start().await;
    let page: Vec<u8> = b"<title>a\xffb</title><p>\xfe</p>".to_vec();
    let (status, _, json) = app.put("p", "k", "plan", "role=tpm", page.clone()).await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    assert_eq!(json["title"], "a\u{fffd}b");
    assert_eq!(json["size"], page.len() as i64);
    let (_, _, body) = app.get("/p/k/plan-1.html").await;
    assert_eq!(body, page);
}

#[tokio::test]
async fn bad_arguments_are_refused_with_a_validation_error() {
    let app = App::start().await;
    let long = "a".repeat(256);
    let segment_255 = "a".repeat(255);
    let kind_33 = format!("a{}", "b".repeat(32));
    let ws_65 = "w".repeat(65);
    for (project, key, kind, query) in [
        // project・key: 英数字と . _ - で 255 字まで。. と .. と api（project）は駄目。
        ("a%20b", "k", "plan", "role=tpm"),
        (long.as_str(), "k", "plan", "role=tpm"),
        ("api", "k", "plan", "role=tpm"),
        (".", "k", "plan", "role=tpm"),
        ("..", "k", "plan", "role=tpm"),
        ("p", "..", "plan", "role=tpm"),
        ("p", ".", "plan", "role=tpm"),
        ("p", "a%2Fb", "plan", "role=tpm"),
        ("p", long.as_str(), "plan", "role=tpm"),
        // kind・role: 英小文字で始まり、英小文字・数字・- で 32 字まで。role は要る。
        ("p", "k", "Plan", "role=tpm"),
        ("p", "k", "1plan", "role=tpm"),
        ("p", "k", kind_33.as_str(), "role=tpm"),
        ("p", "k", "plan", ""),
        ("p", "k", "plan", "role=T%20P"),
        ("p", "k", "plan", "role="),
        // workspace: 英数字と : . _ - で 64 字まで。
        ("p", "k", "plan", "role=tpm&workspace=a%20b"),
        // n: 頭に 0 の無い 1 以上の数の字。
        ("p", "k", "plan", "role=tpm&n=0"),
        ("p", "k", "plan", "role=tpm&n=01"),
        ("p", "k", "plan", "role=tpm&n=-1"),
        ("p", "k", "plan", "role=tpm&n=abc"),
        ("p", "k", "plan", "role=tpm&n=1234567890123456789"),
        // createdAt: UTC の RFC 3339 で、ミリ秒まで。
        ("p", "k", "plan", "role=tpm&createdAt=yesterday"),
        (
            "p",
            "k",
            "plan",
            "role=tpm&createdAt=2026-11-20T10%3A02%3A03%2B09%3A00",
        ),
        (
            "p",
            "k",
            "plan",
            "role=tpm&createdAt=2026-11-20T01%3A02%3A03.1234Z",
        ),
        // 0000 年は、答えの時刻に書けない（生成した層が断る）。
        (
            "p",
            "k",
            "plan",
            "role=tpm&createdAt=0000-01-01T00%3A00%3A00Z",
        ),
        // 送り手の付随する id は、両方書くか、両方書かない（形と castlist に聞く所は tests/senders.rs）。
        ("p", "k", "plan", "role=tpm&actorIssuer=tpm"),
        ("p", "k", "plan", "role=tpm&actorRef=claude.session%3As-1"),
    ] {
        let (status, headers, json) = app.put(project, key, kind, query, "<p>x</p>").await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "{project} {key} {kind} {query}: {json}"
        );
        assert!(
            matches!(
                error_type(&headers),
                "ValidationException" | "SerializationException"
            ),
            "{project} {key} {kind} {query}: {headers:?}"
        );
    }
    // workspace の 65 字は 400、64 字と project・key の 255 字は通る。
    let (status, _, _) = app
        .put(
            "p",
            "k",
            "plan",
            &format!("role=tpm&workspace={ws_65}"),
            "<p>x</p>",
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(app.count("pages").await, 0);
    let ws_64 = "w".repeat(64);
    let (status, _, json) = app
        .put(
            &segment_255,
            &segment_255,
            "plan",
            &format!("role=tpm&workspace={ws_64}"),
            "<p>x</p>",
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    assert_eq!(app.count("pages").await, 1);
}

#[tokio::test]
async fn the_messages_say_what_is_wrong() {
    let app = App::start().await;
    for (project, key, query, body, words) in [
        ("api", "k", "role=tpm", "<p>x</p>", "api"),
        ("p", "..", "role=tpm", "<p>x</p>", "key"),
        (
            "p",
            "k",
            "role=tpm&createdAt=2026-11-20T01%3A02%3A03.1234Z",
            "<p>x</p>",
            "ミリ秒",
        ),
        ("p", "k", "role=tpm&actorIssuer=tpm", "<p>x</p>", "両方"),
        // 空の本文は、生成した層が断る（欄の path は /body）。
        ("p", "k", "role=tpm", "", "/body"),
    ] {
        let (status, headers, json) = app.put(project, key, "plan", query, body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}: {json}");
        assert_eq!(error_type(&headers), "ValidationException", "{query}");
        assert!(
            json["message"].as_str().unwrap_or("").contains(words),
            "{query}: {json}"
        );
    }
    assert_eq!(app.count("pages").await, 0);
    assert_eq!(app.count("senders").await, 0);
}

/// 本文は HTML（`Content-Type: text/html`。charset は付いてよい）。ほかの種類は生成した層が断る。
#[tokio::test]
async fn the_body_must_be_html() {
    let app = App::start().await;
    let uri = pages_uri("p", "k", "plan", "role=tpm");
    let request = |content_type: Option<&str>| {
        let mut builder = axum::http::Request::post(&uri).header(header::HOST, common::HOST);
        if let Some(t) = content_type {
            builder = builder.header(header::CONTENT_TYPE, t);
        }
        builder.body(Body::from("<p>x</p>")).unwrap()
    };
    let (status, _, _) = app.send(request(Some("text/html; charset=utf-8"))).await;
    assert_eq!(status, StatusCode::CREATED);
    for t in [
        None,
        Some("application/json"),
        Some("application/x-www-form-urlencoded"),
    ] {
        let (status, headers, _) = app.send(request(t)).await;
        assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE, "{t:?}");
        assert_eq!(
            error_type(&headers),
            "UnsupportedMediaTypeException",
            "{t:?}"
        );
    }
    assert_eq!(app.count("pages").await, 1);
}
