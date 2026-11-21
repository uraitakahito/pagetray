//! 道筋ごとの試験（JSON の APIと本文）。

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use common::{PNG, SE, Server, html, request};
use http_body_util::BodyExt;
use pagetray::{AppState, db::Db, router};
use serde_json::Value;
use std::path::PathBuf;
use tower::ServiceExt;

#[tokio::test]
async fn numbers_start_at_one_and_follow_per_kind() {
    let s = Server::start().await;
    let (status, json) = s
        .put(&format!("{SE}&kind=plan"), &html("計画", "本文"))
        .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(json["n"], 1);
    assert_eq!(json["path"], "/p/SKILLS-14/plan-1.html");
    assert_eq!(json["url"], "http://127.0.0.1:8801/p/SKILLS-14/plan-1.html");
    assert_eq!(json["title"], "計画");
    assert_eq!(json["role"], "software-engineer");
    assert_eq!(json["workspace"], "workspace:1");
    assert!(json["created_at"].as_str().unwrap().ends_with('Z'));

    assert_eq!(s.place(&format!("{SE}&kind=plan"), "<p>2</p>").await, 2);
    assert_eq!(s.place(&format!("{SE}&kind=result"), "<p>1</p>").await, 1);
    assert_eq!(
        s.place("project=p&key=t-x&kind=plan&role=tpm", "<p>1</p>")
            .await,
        1
    );
}

/// 置く・探すの答えが、呼ぶ側（CLI）の読む型（crates/api の PageRecord）で読めるか。
/// 欄を消す・名や型を変えると、ここが落ちる（API の版を上げる変更。docs/src/api.md）。
#[tokio::test]
async fn answers_parse_as_the_shared_page_record() {
    use pagetray_api::PageRecord;
    let s = Server::start().await;
    let (_, placed) = s
        .put(&format!("{SE}&kind=plan"), &html("計画", "本文"))
        .await;
    let placed: PageRecord = serde_json::from_value(placed).unwrap();
    assert_eq!(placed.path, "/p/SKILLS-14/plan-1.html");
    assert_eq!(
        placed.url.as_deref(),
        Some("http://127.0.0.1:8801/p/SKILLS-14/plan-1.html")
    );

    let listed = s.json("/api/pages?project=p").await;
    let listed: Vec<PageRecord> = serde_json::from_value(listed["pages"].clone()).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].url, None);
    assert_eq!(
        PageRecord {
            url: None,
            ..placed
        },
        listed[0]
    );
}

#[tokio::test]
async fn fifty_parallel_puts_get_fifty_distinct_numbers_without_gaps() {
    let s = Server::start().await;
    let mut tasks = Vec::new();
    for i in 0..50 {
        let app = s.app.clone();
        tasks.push(tokio::spawn(async move {
            let uri = format!("/api/pages?{SE}&kind=result");
            let body = Body::from(format!("<title>{i}</title>"));
            let response = app.oneshot(request("POST", &uri, body)).await.unwrap();
            assert_eq!(response.status(), StatusCode::CREATED);
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            serde_json::from_slice::<Value>(&bytes).unwrap()["n"]
                .as_i64()
                .unwrap()
        }));
    }
    let mut numbers = Vec::new();
    for task in tasks {
        numbers.push(task.await.unwrap());
    }
    numbers.sort();
    assert_eq!(numbers, (1..=50).collect::<Vec<i64>>());
}

#[tokio::test]
async fn the_body_comes_back_byte_for_byte_in_a_sandbox() {
    let s = Server::start().await;
    let page = html("題", "中身 <script>1</script>");
    s.place(&format!("{SE}&kind=proposal"), &page).await;
    let (status, headers, body) = s.get("/p/SKILLS-14/proposal-1.html").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, page.as_bytes());
    assert_eq!(headers[header::CONTENT_TYPE], "text/html; charset=utf-8");
    assert_eq!(
        headers[header::CONTENT_SECURITY_POLICY],
        "sandbox allow-scripts"
    );

    for missing in [
        "/p/SKILLS-14/proposal-2.html",
        "/p/SKILLS-14/proposal-01.html",
        "/p/SKILLS-14/x.txt",
        "/p/other/proposal-1.html",
    ] {
        assert_eq!(s.get(missing).await.0, StatusCode::NOT_FOUND, "{missing}");
    }
}

#[tokio::test]
async fn bad_arguments_are_refused() {
    let s = Server::start().await;
    for query in [
        "key=k&kind=plan&role=tpm",
        "project=a/b&key=k&kind=plan&role=tpm",
        "project=api&key=k&kind=plan&role=tpm",
        "project=p&key=..&kind=plan&role=tpm",
        "project=p&key=k&kind=Plan&role=tpm",
        "project=p&key=k&kind=plan&role=T%20P",
        "project=p&key=k&kind=plan&role=tpm&workspace=a%20b",
        "project=p&key=k&kind=plan&role=tpm&n=0",
        "project=p&key=k&kind=plan&role=tpm&created_at=yesterday",
    ] {
        let (status, _, _) = s
            .send(request(
                "POST",
                &format!("/api/pages?{query}"),
                Body::from("<p>x</p>"),
            ))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}");
    }
    let (status, json) = s.put("project=p&key=k&kind=plan&role=tpm", "").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json["error"], "本文が空");
    assert_eq!(
        s.get("/api/pages?sort=title").await.0,
        StatusCode::BAD_REQUEST
    );
    assert!(s.paths("").await.is_empty());
}

#[tokio::test]
async fn a_page_over_the_limit_is_refused() {
    let s = Server::start().await;
    let big = vec![b'a'; pagetray::MAX_BODY + 1];
    let (status, _, _) = s
        .send(request(
            "POST",
            &format!("/api/pages?{SE}&kind=plan"),
            Body::from(big),
        ))
        .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn an_import_keeps_its_number_and_time_and_a_taken_number_is_a_conflict() {
    let s = Server::start().await;
    let query = "project=p&key=k&kind=brief&role=tpm&n=3&created_at=2026-11-20T01:02:03Z";
    let (status, json) = s.put(query, "<title>古い</title>").await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(json["n"], 3);
    assert_eq!(json["created_at"], "2026-11-20T01:02:03Z");

    let (status, json) = s.put(query, "<title>別</title>").await;
    assert_eq!(status, StatusCode::CONFLICT, "{json}");
    let (_, _, body) = s.get("/p/k/brief-3.html").await;
    assert_eq!(body, "<title>古い</title>".as_bytes());

    // 番号を任せると、取り込んだ番号の次になる。
    assert_eq!(
        s.place("project=p&key=k&kind=brief&role=tpm", "<p>x</p>")
            .await,
        4
    );
    // 検索の索引も、失敗した分は入っていない。
    assert!(s.paths("q=別").await.is_empty());
}

#[tokio::test]
async fn filters_combine_and_sort_by_time() {
    let s = Server::start().await;
    let at = |day: u8| format!("created_at=2026-11-{day:02}T00:00:00Z");
    s.place(
        &format!(
            "project=p&key=A-1&kind=brief&role=tpm&workspace=w:1&{}",
            at(1)
        ),
        "<p>a</p>",
    )
    .await;
    s.place(
        &format!(
            "project=p&key=A-1&kind=plan&role=software-engineer&workspace=w:2&{}",
            at(2)
        ),
        "<p>b</p>",
    )
    .await;
    s.place(
        &format!(
            "project=p&key=B-2&kind=plan&role=software-engineer&workspace=w:3&{}",
            at(3)
        ),
        "<p>c</p>",
    )
    .await;
    s.place(
        &format!(
            "project=q&key=A-1&kind=result&role=devops-engineer&workspace=w:2&{}",
            at(4)
        ),
        "<p>d</p>",
    )
    .await;

    let all = [
        "/q/A-1/result-1.html",
        "/p/B-2/plan-1.html",
        "/p/A-1/plan-1.html",
        "/p/A-1/brief-1.html",
    ];
    assert_eq!(s.paths("").await, all);
    assert_eq!(s.paths("sort=-created_at").await, all);
    let mut oldest = all;
    oldest.reverse();
    assert_eq!(s.paths("sort=created_at").await, oldest);

    assert_eq!(
        s.paths("role=software-engineer").await,
        ["/p/B-2/plan-1.html", "/p/A-1/plan-1.html"]
    );
    assert_eq!(
        s.paths("workspace=w:2").await,
        ["/q/A-1/result-1.html", "/p/A-1/plan-1.html"]
    );
    assert_eq!(
        s.paths("workspace=w:2&project=p").await,
        ["/p/A-1/plan-1.html"]
    );
    assert_eq!(s.paths("key=A-1&kind=brief").await, ["/p/A-1/brief-1.html"]);
    assert_eq!(s.paths("role=tpm&kind=plan").await, [] as [&str; 0]);
    assert_eq!(s.paths("limit=2").await, all[..2]);
    assert_eq!(s.paths("before=2026-11-03T00:00:00Z").await, all[2..]);
    assert_eq!(s.paths("after=2026-11-03T00:00:00Z").await, all[..1]);
    assert_eq!(
        s.paths("sort=created_at&after=2026-11-01T00:00:00Z&limit=2")
            .await,
        ["/p/A-1/plan-1.html", "/p/B-2/plan-1.html"]
    );

    let json = s.json("/api/pages?kind=brief").await;
    let page = &json["pages"][0];
    assert_eq!(page["size"], 8);
    assert!(page.get("body").is_none());
}

#[tokio::test]
async fn search_finds_japanese_text_and_titles_but_not_markup() {
    let s = Server::start().await;
    s.place(
        &format!("{SE}&kind=plan"),
        &html("置き場の計画", "一覧の UI で絞り込みと並べ替えができる"),
    )
    .await;
    s.place(
        &format!("{SE}&kind=result"),
        &html("結果", "container を起こした"),
    )
    .await;

    let plan = ["/p/SKILLS-14/plan-1.html"];
    assert_eq!(s.paths("q=絞り込み").await, plan);
    assert_eq!(s.paths("q=置き場の計画").await, plan);
    assert_eq!(s.paths("q=絞り込み%20並べ替え").await, plan);
    assert_eq!(s.paths("q=絞り込み%20起こした").await, [] as [&str; 0]);
    // 3 字に満たない語は、端から見て探す。
    assert_eq!(s.paths("q=UI").await, plan);
    assert_eq!(s.paths("q=結果").await, ["/p/SKILLS-14/result-1.html"]);
    // 検索の記号は、ただの字として扱う。
    assert_eq!(s.paths("q=%22絞り込み").await, [] as [&str; 0]);
    assert_eq!(s.paths("q=AND%20OR%20NOT").await, [] as [&str; 0]);
    // tag と style の中身は索引に入らない。
    assert_eq!(s.paths("q=margin").await, [] as [&str; 0]);
    assert_eq!(s.paths("q=doctype").await, [] as [&str; 0]);
    assert_eq!(s.paths("q=container&kind=plan").await, [] as [&str; 0]);
    assert_eq!(s.paths("q=CONTAINER").await, ["/p/SKILLS-14/result-1.html"]);
}

#[tokio::test]
async fn the_version_is_told_by_a_route_and_on_every_answer() {
    let s = Server::start().await;
    let json = s.json("/api/version").await;
    assert_eq!(json["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(json["api"], pagetray::API_VERSION);
    // 通った答えにも、無いページにも、誤りにも付く。
    for uri in [
        "/healthz",
        "/api/pages",
        "/p/K-1/result-9.html",
        "/api/pages?sort=x",
    ] {
        let (_, headers, _) = s.get(uri).await;
        assert_eq!(headers[pagetray::API_HEADER], "1", "{uri}");
    }
    // guard が断った答えにも付く。
    let refused = Request::builder()
        .uri("/healthz")
        .header(header::HOST, "evil.example")
        .body(Body::empty())
        .unwrap();
    let (status, headers, _) = s.send(refused).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(headers[pagetray::API_HEADER], "1");
}

#[tokio::test]
async fn only_local_hosts_are_served() {
    let s = Server::start().await;
    for (host, expected) in [
        ("127.0.0.1:8801", StatusCode::OK),
        ("localhost:8801", StatusCode::OK),
        ("127.0.0.1", StatusCode::OK),
        ("evil.example:8801", StatusCode::FORBIDDEN),
        ("127.0.0.1.evil.example", StatusCode::FORBIDDEN),
    ] {
        let request = Request::builder()
            .uri("/healthz")
            .header(header::HOST, host)
            .body(Body::empty())
            .unwrap();
        assert_eq!(s.send(request).await.0, expected, "{host}");
    }
    let no_host = Request::builder()
        .uri("/healthz")
        .body(Body::empty())
        .unwrap();
    assert_eq!(s.send(no_host).await.0, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_page_in_a_browser_cannot_put() {
    let s = Server::start().await;
    for origin in ["null", "http://127.0.0.1:8801", "https://evil.example"] {
        let request = Request::builder()
            .method("POST")
            .uri(format!("/api/pages?{SE}&kind=plan"))
            .header(header::HOST, "127.0.0.1:8801")
            .header(header::ORIGIN, origin)
            .body(Body::from("<p>x</p>"))
            .unwrap();
        assert_eq!(s.send(request).await.0, StatusCode::FORBIDDEN, "{origin}");
    }
    assert!(s.paths("").await.is_empty());
}

#[tokio::test]
async fn the_backup_is_a_database_that_opens_with_every_page() {
    let s = Server::start().await;
    s.place(&format!("{SE}&kind=plan"), &html("計画", "控えに入る字"))
        .await;
    s.place(&format!("{SE}&kind=plan"), "<p>2</p>").await;
    let (status, headers, bytes) = s.get("/api/backup").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "application/vnd.sqlite3");
    assert!(bytes.starts_with(b"SQLite format 3\0"));

    let mut copy = s.path.clone().into_os_string();
    copy.push(".restored");
    let copy = PathBuf::from(copy);
    std::fs::write(&copy, &bytes).unwrap();
    let restored = Db::open(&copy).await.unwrap();
    assert_eq!(
        restored
            .body("p", "SKILLS-14", "plan", 2)
            .await
            .unwrap()
            .unwrap(),
        b"<p>2</p>"
    );
    restored.close().await;
    for suffix in ["", "-wal", "-shm"] {
        let mut name = copy.clone().into_os_string();
        name.push(suffix);
        let _ = std::fs::remove_file(name);
    }

    // 写しを作る時の file は残さない。
    let dir = s.path.parent().unwrap();
    let stem = s.path.file_name().unwrap().to_string_lossy().into_owned();
    let left: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(&format!("{stem}.backup-")))
        .collect();
    assert!(left.is_empty(), "{left:?}");
}

#[tokio::test]
async fn pages_survive_a_restart() {
    let s = Server::start().await;
    s.place(&format!("{SE}&kind=plan"), "<title>残る</title>")
        .await;
    s.db.close().await;
    let again = Db::open(&s.path).await.unwrap();
    let app = router(AppState::new(again.clone()));
    let response = app
        .oneshot(request("GET", "/p/SKILLS-14/plan-1.html", Body::empty()))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    again.close().await;
}

#[tokio::test]
async fn a_removed_page_is_gone_and_its_number_is_never_used_again() {
    let s = Server::start().await;
    for text in ["一つ目の計画", "二つ目の計画"] {
        s.place(&format!("{SE}&kind=plan"), &html(text, text)).await;
    }
    s.place(&format!("{SE}&kind=result"), &html("結果", "残る"))
        .await;

    let (status, json) = s.remove("project=p&key=SKILLS-14&kind=plan&n=2").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        json["removed"],
        serde_json::json!(["/p/SKILLS-14/plan-2.html"])
    );

    let (status, _, body) = s.get("/p/SKILLS-14/plan-2.html").await;
    assert_eq!(status, StatusCode::GONE);
    let message: Value = serde_json::from_slice(&body).unwrap();
    assert!(
        message["error"]
            .as_str()
            .unwrap()
            .starts_with("そのページは消した（")
    );
    // ほかのページは残る。一覧にも検索にも、消したページは出ない。
    assert_eq!(
        s.text("/p/SKILLS-14/plan-1.html").await,
        html("一つ目の計画", "一つ目の計画")
    );
    assert_eq!(
        s.paths("project=p").await,
        ["/p/SKILLS-14/result-1.html", "/p/SKILLS-14/plan-1.html"]
    );
    assert!(s.paths("q=二つ目の計画").await.is_empty());
    assert_eq!(
        s.paths("q=一つ目の計画").await,
        ["/p/SKILLS-14/plan-1.html"]
    );
    // 一番大きい番号を消しても、次は 3（古い URL が別のページを指さない）。
    assert_eq!(s.place(&format!("{SE}&kind=plan"), "<p>3</p>").await, 3);
    // 取り込みで消した番号を名指しても入らない。
    let (status, _) = s
        .put(&format!("{SE}&kind=plan&n=2"), "<p>生き返らない</p>")
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(s.get("/p/SKILLS-14/plan-2.html").await.0, StatusCode::GONE);
    // 2 度目は消す物が無い。
    let (_, json) = s.remove("project=p&key=SKILLS-14&kind=plan&n=2").await;
    assert_eq!(json["removed"], serde_json::json!([]));
}

#[tokio::test]
async fn a_key_or_a_whole_project_is_removed_at_once() {
    let s = Server::start().await;
    s.place(&format!("{SE}&kind=plan"), "<p>1</p>").await;
    s.place(&format!("{SE}&kind=result"), "<p>1</p>").await;
    s.place("project=p&key=t-x&kind=brief&role=tpm", "<p>1</p>")
        .await;
    s.place("project=q&key=t-x&kind=brief&role=tpm", "<p>1</p>")
        .await;

    let (_, json) = s.remove("project=p&key=SKILLS-14").await;
    assert_eq!(
        json["removed"],
        serde_json::json!(["/p/SKILLS-14/plan-1.html", "/p/SKILLS-14/result-1.html"])
    );
    assert_eq!(s.paths("project=p").await, ["/p/t-x/brief-1.html"]);

    let (_, json) = s.remove("project=p").await;
    assert_eq!(json["removed"], serde_json::json!(["/p/t-x/brief-1.html"]));
    // ほかの project は残る。
    assert_eq!(s.paths("").await, ["/q/t-x/brief-1.html"]);
    assert_eq!(
        s.place("project=p&key=t-x&kind=brief&role=tpm", "<p>2</p>")
            .await,
        2
    );
}

#[tokio::test]
async fn removing_needs_a_project_and_a_full_name_for_one_page() {
    let s = Server::start().await;
    s.place(&format!("{SE}&kind=plan"), "<p>1</p>").await;
    for (query, message) in [
        ("key=SKILLS-14", None),
        ("project=..", Some("project は")),
        (
            "project=p&kind=plan&n=1",
            Some("n を書くときは key と kind も書く"),
        ),
        (
            "project=p&key=SKILLS-14&n=1",
            Some("n を書くときは key と kind も書く"),
        ),
        ("project=p&key=SKILLS-14&kind=plan&n=0", Some("n は 1 以上")),
        ("project=p&kind=Plan", Some("kind は")),
    ] {
        let uri = format!("/api/pages?{query}");
        let (status, _, body) = s.send(request("DELETE", &uri, Body::empty())).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}");
        if let Some(message) = message {
            let json: Value = serde_json::from_slice(&body).unwrap();
            assert!(
                json["error"].as_str().unwrap().starts_with(message),
                "{query}: {json}"
            );
        }
    }
    // browser の中のページからは消せない。
    let mut from_page = request("DELETE", "/api/pages?project=p", Body::empty());
    from_page
        .headers_mut()
        .insert(header::ORIGIN, "null".parse().unwrap());
    assert_eq!(s.send(from_page).await.0, StatusCode::FORBIDDEN);
    assert_eq!(s.paths("project=p").await, ["/p/SKILLS-14/plan-1.html"]);
}

#[tokio::test]
async fn a_file_comes_back_byte_for_byte_with_its_type() {
    let s = Server::start().await;
    let (status, headers, json) = s
        .put_file(&format!("{SE}&name=dot.png"), Some("image/png"), PNG)
        .await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    assert_eq!(json["path"], "/p/SKILLS-14/files/dot-1.png");
    assert_eq!(
        json["url"],
        "http://127.0.0.1:8801/p/SKILLS-14/files/dot-1.png"
    );
    assert_eq!(headers[header::LOCATION], json["url"].as_str().unwrap());
    assert_eq!(json["name"], "dot");
    assert_eq!(json["ext"], "png");
    assert_eq!(json["type"], "image/png");
    assert_eq!(json["size"], PNG.len());
    assert_eq!(json["role"], "software-engineer");
    assert_eq!(json["workspace"], "workspace:1");
    assert!(json["created_at"].as_str().unwrap().ends_with('Z'));
    // `shasum -a 256` の値。
    assert_eq!(
        json["sha256"],
        "4ff6ab670a58c14270e034e2090d9a432caa263a14e0a25785386b0c12f880b5"
    );

    let (status, headers, body) = s.get("/p/SKILLS-14/files/dot-1.png").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, PNG);
    assert_eq!(headers[header::CONTENT_TYPE], "image/png");
    assert_eq!(headers[header::CONTENT_SECURITY_POLICY], "sandbox");
    assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    assert_eq!(
        headers[header::CACHE_CONTROL],
        "max-age=31536000, immutable"
    );

    // 字の file は、表の Content-Type（charset 付き）で返る。送った header の引数は問わない。
    let text = "一行目\nsecond\n".as_bytes();
    s.place_file(
        &format!("{SE}&name=notes.md"),
        "Text/Markdown; charset=UTF-8",
        text,
    )
    .await;
    let (_, headers, body) = s.get("/p/SKILLS-14/files/notes-1.md").await;
    assert_eq!(body, text);
    assert_eq!(
        headers[header::CONTENT_TYPE],
        "text/markdown; charset=utf-8"
    );

    for missing in [
        "/p/SKILLS-14/files/dot-2.png",
        "/p/SKILLS-14/files/dot-01.png",
        "/p/SKILLS-14/files/dot-1.jpg",
        "/p/SKILLS-14/files/dot.png",
        "/p/SKILLS-14/files/dot-1.svg",
        "/p/other/files/dot-1.png",
    ] {
        assert_eq!(s.get(missing).await.0, StatusCode::NOT_FOUND, "{missing}");
    }
}

#[tokio::test]
async fn files_are_numbered_per_name_and_a_removed_number_is_never_used_again() {
    let s = Server::start().await;
    let q = format!("{SE}&name=dot.png");
    assert_eq!(s.place_file(&q, "image/png", PNG).await, 1);
    assert_eq!(s.place_file(&q, "image/png", PNG).await, 2);
    // 名か拡張子が違えば、別に数える。
    assert_eq!(
        s.place_file(
            &format!("{SE}&name=dot.pdf"),
            "application/pdf",
            b"%PDF-1.7"
        )
        .await,
        1
    );
    assert_eq!(
        s.place_file(&format!("{SE}&name=other.png"), "image/png", PNG)
            .await,
        1
    );

    let (status, json) = s
        .remove_files("project=p&key=SKILLS-14&name=dot.png&n=2")
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        json["removed"],
        serde_json::json!(["/p/SKILLS-14/files/dot-2.png"])
    );
    let (status, _, body) = s.get("/p/SKILLS-14/files/dot-2.png").await;
    assert_eq!(status, StatusCode::GONE);
    let message: Value = serde_json::from_slice(&body).unwrap();
    assert!(
        message["error"]
            .as_str()
            .unwrap()
            .starts_with("その file は消した（")
    );
    assert_eq!(
        s.get("/p/SKILLS-14/files/dot-1.png").await.0,
        StatusCode::OK
    );
    // 一番大きい番号を消しても、次は 3。
    assert_eq!(s.place_file(&q, "image/png", PNG).await, 3);
    // 消した番号を名指しても入らない。
    let (status, _, _) = s
        .put_file(&format!("{q}&n=2"), Some("image/png"), PNG)
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        s.get("/p/SKILLS-14/files/dot-2.png").await.0,
        StatusCode::GONE
    );
    // 2 度目は消す物が無い。
    let (_, json) = s
        .remove_files("project=p&key=SKILLS-14&name=dot.png&n=2")
        .await;
    assert_eq!(json["removed"], serde_json::json!([]));
}

#[tokio::test]
async fn an_imported_file_keeps_its_number_and_time_and_a_taken_number_is_a_conflict() {
    let s = Server::start().await;
    let q = "project=p&key=k&name=dot.png&role=tpm&n=3&created_at=2026-11-20T01:02:03Z";
    let (status, _, json) = s.put_file(q, Some("image/png"), PNG).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(json["n"], 3);
    assert_eq!(json["created_at"], "2026-11-20T01:02:03Z");
    let (status, _, json) = s.put_file(q, Some("image/png"), PNG).await;
    assert_eq!(status, StatusCode::CONFLICT, "{json}");
    assert_eq!(
        s.place_file("project=p&key=k&name=dot.png&role=tpm", "image/png", PNG)
            .await,
        4
    );
}

#[tokio::test]
async fn bad_file_arguments_are_refused() {
    let s = Server::start().await;
    for query in [
        "key=k&name=a.png&role=tpm",
        "project=api&key=k&name=a.png&role=tpm",
        "project=p&key=..&name=a.png&role=tpm",
        "project=p&key=k&role=tpm",
        "project=p&key=k&name=a&role=tpm",
        "project=p&key=k&name=A.png&role=tpm",
        "project=p&key=k&name=-a.png&role=tpm",
        "project=p&key=k&name=a.b.png&role=tpm",
        "project=p&key=k&name=a-1.png.&role=tpm",
        "project=p&key=k&name=a.png&role=T",
        "project=p&key=k&name=a.png&role=tpm&workspace=a%20b",
        "project=p&key=k&name=a.png&role=tpm&n=0",
        "project=p&key=k&name=a.png&role=tpm&created_at=yesterday",
    ] {
        let (status, _, _) = s.put_file(query, Some("image/png"), PNG).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}");
    }
    let q = "project=p&key=k&name=a.png&role=tpm";
    let (status, _, json) = s.put_file(q, Some("image/png"), b"").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json["error"], "本文が空");
    let (status, _, json) = s.put_file(q, None, PNG).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json["error"], "Content-Type が要る");
    assert_eq!(
        s.get("/api/files?sort=name").await.0,
        StatusCode::BAD_REQUEST
    );
    assert!(s.file_paths("").await.is_empty());
}

#[tokio::test]
async fn a_file_over_the_limit_is_refused() {
    let s = Server::start().await;
    let mut big = PNG.to_vec();
    big.resize(pagetray::MAX_FILE + 1, 0);
    let (status, _, _) = s
        .send({
            let mut r = request(
                "POST",
                &format!("/api/files?{SE}&name=big.png"),
                Body::from(big),
            );
            r.headers_mut()
                .insert(header::CONTENT_TYPE, "image/png".parse().unwrap());
            r
        })
        .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert!(s.file_paths("").await.is_empty());
}

#[tokio::test]
async fn a_type_or_contents_that_do_not_match_the_extension_are_refused() {
    let s = Server::start().await;
    for (name, content_type, bytes) in [
        // 中身が拡張子と合わない。
        ("a.png", "image/png", &b"%PDF-1.7 ..."[..]),
        ("a.jpg", "image/jpeg", PNG),
        ("a.gif", "image/gif", b"GIF90a"),
        ("a.webp", "image/webp", b"RIFF\0\0\0\0WAVEfmt "),
        ("a.pdf", "application/pdf", PNG),
        ("a.txt", "text/plain", b"\xff\xfe\x00a"),
        ("a.json", "application/json", b"{\"a\":\"\0\"}"),
        // 受けない拡張子。
        ("a.svg", "image/svg+xml", b"<svg/>"),
        ("a.html", "text/html", b"<p>x</p>"),
        ("a.mp4", "video/mp4", b"\0\0\0\x18ftypmp42"),
        ("a.PNG", "image/png", PNG),
        // Content-Type が拡張子と合わない。
        ("a.png", "image/jpeg", PNG),
        ("a.png", "application/octet-stream", PNG),
        ("a.md", "text/plain", b"# x"),
    ] {
        let (status, _, json) = s
            .put_file(&format!("{SE}&name={name}"), Some(content_type), bytes)
            .await;
        assert_eq!(
            status,
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "{name} {content_type}: {json}"
        );
    }
    assert!(s.file_paths("").await.is_empty());
    // 合っていれば通る。
    for (name, content_type, bytes) in [
        ("a.jpg", "image/jpeg", &b"\xff\xd8\xff\xe0"[..]),
        ("a.jpeg", "IMAGE/JPEG", b"\xff\xd8\xff\xdb"),
        ("a.gif", "image/gif", b"GIF89a"),
        ("a.webp", "image/webp", b"RIFF\0\0\0\0WEBPVP8 "),
        ("a.log", "text/plain; charset=utf-8", "ログ".as_bytes()),
        ("a.csv", "text/csv", b"a,b\n1,2\n"),
        ("a.json", "application/json", b"{}"),
    ] {
        s.place_file(&format!("{SE}&name={name}"), content_type, bytes)
            .await;
    }
}

#[tokio::test]
async fn file_filters_combine_and_sort_by_time() {
    let s = Server::start().await;
    let at = |day: u8| format!("created_at=2026-11-{day:02}T00:00:00Z");
    for (query, day) in [
        (
            "project=p&key=A-1&name=screen.png&role=tpm&workspace=w:1",
            1,
        ),
        (
            "project=p&key=A-1&name=shot.png&role=software-engineer&workspace=w:2",
            2,
        ),
        (
            "project=p&key=B-2&name=screen.png&role=software-engineer&workspace=w:3",
            3,
        ),
        (
            "project=q&key=A-1&name=screen.png&role=devops-engineer&workspace=w:2",
            4,
        ),
    ] {
        s.place_file(&format!("{query}&{}", at(day)), "image/png", PNG)
            .await;
    }
    let all = [
        "/q/A-1/files/screen-1.png",
        "/p/B-2/files/screen-1.png",
        "/p/A-1/files/shot-1.png",
        "/p/A-1/files/screen-1.png",
    ];
    assert_eq!(s.file_paths("").await, all);
    assert_eq!(s.file_paths("sort=-created_at").await, all);
    let mut oldest = all;
    oldest.reverse();
    assert_eq!(s.file_paths("sort=created_at").await, oldest);
    assert_eq!(
        s.file_paths("role=software-engineer").await,
        ["/p/B-2/files/screen-1.png", "/p/A-1/files/shot-1.png"]
    );
    assert_eq!(
        s.file_paths("workspace=w:2&project=p").await,
        ["/p/A-1/files/shot-1.png"]
    );
    assert_eq!(
        s.file_paths("key=A-1&project=p").await,
        ["/p/A-1/files/shot-1.png", "/p/A-1/files/screen-1.png"]
    );
    assert_eq!(s.file_paths("q=shot").await, ["/p/A-1/files/shot-1.png"]);
    assert_eq!(s.file_paths("q=SCREEN.PNG").await.len(), 3);
    assert_eq!(s.file_paths("q=pdf").await, [] as [&str; 0]);
    assert_eq!(s.file_paths("limit=2").await, all[..2]);
    assert_eq!(s.file_paths("before=2026-11-03T00:00:00Z").await, all[2..]);
    assert_eq!(s.file_paths("after=2026-11-03T00:00:00Z").await, all[..1]);
    // ページの一覧には出ない。
    assert!(s.paths("").await.is_empty());

    let json = s.json("/api/files?key=B-2").await;
    let file = &json["files"][0];
    assert_eq!(file["size"], PNG.len());
    assert_eq!(file["type"], "image/png");
    assert!(file.get("body").is_none());
    assert!(file.get("url").is_none());
}

/// 置く・探すの答えが、呼ぶ側の読む型（crates/api の FileRecord）で読めるか。
#[tokio::test]
async fn file_answers_parse_as_the_shared_file_record() {
    use pagetray_api::FileRecord;
    let s = Server::start().await;
    let (_, _, placed) = s
        .put_file(&format!("{SE}&name=dot.png"), Some("image/png"), PNG)
        .await;
    let placed: FileRecord = serde_json::from_value(placed).unwrap();
    assert_eq!(placed.path, "/p/SKILLS-14/files/dot-1.png");
    let listed = s.json("/api/files?project=p").await;
    let listed: Vec<FileRecord> = serde_json::from_value(listed["files"].clone()).unwrap();
    assert_eq!(
        FileRecord {
            url: None,
            ..placed
        },
        listed[0]
    );
}

#[tokio::test]
async fn files_of_a_key_or_a_whole_project_are_removed_at_once() {
    let s = Server::start().await;
    s.place_file(&format!("{SE}&name=a.png"), "image/png", PNG)
        .await;
    s.place_file(&format!("{SE}&name=b.txt"), "text/plain", b"b")
        .await;
    s.place_file("project=p&key=t-x&name=a.png&role=tpm", "image/png", PNG)
        .await;
    s.place_file("project=q&key=t-x&name=a.png&role=tpm", "image/png", PNG)
        .await;
    // ページは消えない。
    s.place(&format!("{SE}&kind=plan"), "<p>1</p>").await;

    let (_, json) = s.remove_files("project=p&key=SKILLS-14").await;
    assert_eq!(
        json["removed"],
        serde_json::json!(["/p/SKILLS-14/files/a-1.png", "/p/SKILLS-14/files/b-1.txt"])
    );
    assert_eq!(s.file_paths("project=p").await, ["/p/t-x/files/a-1.png"]);
    assert_eq!(s.paths("").await, ["/p/SKILLS-14/plan-1.html"]);

    // 名だけでも、project の中のその名をみな消す。
    let (_, json) = s.remove_files("project=p&name=a.png").await;
    assert_eq!(json["removed"], serde_json::json!(["/p/t-x/files/a-1.png"]));
    assert_eq!(s.file_paths("").await, ["/q/t-x/files/a-1.png"]);
    assert_eq!(
        s.place_file(&format!("{SE}&name=a.png"), "image/png", PNG)
            .await,
        2
    );
}

#[tokio::test]
async fn removing_files_needs_a_project_and_a_full_name_for_one_file() {
    let s = Server::start().await;
    s.place_file(&format!("{SE}&name=a.png"), "image/png", PNG)
        .await;
    for (query, message) in [
        ("key=SKILLS-14", None),
        ("project=..", Some("project は")),
        (
            "project=p&name=a.png&n=1",
            Some("n を書くときは key と name も書く"),
        ),
        (
            "project=p&key=SKILLS-14&n=1",
            Some("n を書くときは key と name も書く"),
        ),
        (
            "project=p&key=SKILLS-14&name=a.png&n=0",
            Some("n は 1 以上"),
        ),
        ("project=p&name=A.png", Some("name は")),
        ("project=p&name=a", Some("name は")),
    ] {
        let uri = format!("/api/files?{query}");
        let (status, _, body) = s.send(request("DELETE", &uri, Body::empty())).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}");
        if let Some(message) = message {
            let json: Value = serde_json::from_slice(&body).unwrap();
            assert!(
                json["error"].as_str().unwrap().starts_with(message),
                "{query}: {json}"
            );
        }
    }
    assert_eq!(s.file_paths("").await, ["/p/SKILLS-14/files/a-1.png"]);
}

/// browser から来た書く要求（`Origin` 付き）の、通す・断るの組み合わせ（docs/src/api.md の「守り」）。
/// 通すのは、一覧の UI の form が file を上げる 1 つの形だけ。
#[tokio::test]
async fn only_the_list_ui_may_put_files_from_a_browser() {
    let s = Server::start().await;
    s.place(&format!("{SE}&kind=plan"), "<p>x</p>").await;
    s.place_file(&format!("{SE}&name=old.png"), "image/png", PNG)
        .await;
    let ui = "http://127.0.0.1:8801";
    let put = format!("/api/files?{SE}&name=a.png");
    let cases: [(&str, &str, &str, Option<&str>, StatusCode); 13] = [
        // 一覧の UI の form（同じ Host・port の Origin）。
        ("POST", &put, ui, None, StatusCode::CREATED),
        ("POST", &put, ui, Some("same-origin"), StatusCode::CREATED),
        // 本文の枠の中のページ（sandbox なので null）・ほかの port・ほかの名・https。
        ("POST", &put, "null", None, StatusCode::FORBIDDEN),
        (
            "POST",
            &put,
            "http://127.0.0.1:8800",
            None,
            StatusCode::FORBIDDEN,
        ),
        (
            "POST",
            &put,
            "http://localhost:8801",
            None,
            StatusCode::FORBIDDEN,
        ),
        (
            "POST",
            &put,
            "https://127.0.0.1:8801",
            None,
            StatusCode::FORBIDDEN,
        ),
        (
            "POST",
            &put,
            "https://evil.example",
            None,
            StatusCode::FORBIDDEN,
        ),
        // 同じ Origin でも、browser がほかの site からと言う物。
        ("POST", &put, ui, Some("cross-site"), StatusCode::FORBIDDEN),
        ("POST", &put, ui, Some("same-site"), StatusCode::FORBIDDEN),
        // 同じ Origin でも、ほかの道（ページを置く・消す）。
        (
            "POST",
            &format!("/api/pages?{SE}&kind=plan"),
            ui,
            None,
            StatusCode::FORBIDDEN,
        ),
        (
            "DELETE",
            "/api/files?project=p",
            ui,
            None,
            StatusCode::FORBIDDEN,
        ),
        (
            "DELETE",
            "/api/pages?project=p",
            ui,
            None,
            StatusCode::FORBIDDEN,
        ),
        (
            "DELETE",
            "/api/files?project=p",
            "null",
            None,
            StatusCode::FORBIDDEN,
        ),
    ];
    for (method, uri, origin, site, expected) in cases {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, "127.0.0.1:8801")
            .header(header::ORIGIN, origin)
            .header(header::CONTENT_TYPE, "image/png");
        if let Some(site) = site {
            request = request.header("sec-fetch-site", site);
        }
        let body = if uri.starts_with("/api/pages") {
            Body::from("<p>x</p>")
        } else {
            Body::from(PNG)
        };
        let (status, _, _) = s.send(request.body(body).unwrap()).await;
        assert_eq!(status, expected, "{method} {uri} Origin: {origin} {site:?}");
    }
    // 断った物は、何も置かず、何も消していない。
    assert_eq!(s.paths("").await, ["/p/SKILLS-14/plan-1.html"]);
    assert_eq!(
        s.file_paths("").await,
        [
            "/p/SKILLS-14/files/a-2.png",
            "/p/SKILLS-14/files/a-1.png",
            "/p/SKILLS-14/files/old-1.png"
        ]
    );
}

/// 一覧の UI から上げた file は、引数に何が書いてあっても、役割 product-manager・workspace 空で記録する。
/// 番号と時刻も、置き場が決める（browser から取り込みはできない）。`Origin` の無い要求（CLI・tpm）は前のまま。
#[tokio::test]
async fn files_from_the_list_ui_are_recorded_as_the_product_manager() {
    let s = Server::start().await;
    let request = Request::builder()
        .method("POST")
        .uri(format!(
            "/api/files?{SE}&name=a.png&n=7&created_at=2020-01-01T00:00:00Z"
        ))
        .header(header::HOST, "127.0.0.1:8801")
        .header(header::ORIGIN, "http://127.0.0.1:8801")
        .header("sec-fetch-site", "same-origin")
        .header(header::CONTENT_TYPE, "image/png")
        .body(Body::from(PNG))
        .unwrap();
    let (status, _, body) = s.send(request).await;
    assert_eq!(status, StatusCode::CREATED);
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["role"], "product-manager");
    assert_eq!(json["workspace"], "");
    assert_eq!(json["n"], 1);
    assert_ne!(json["created_at"], "2020-01-01T00:00:00Z");
    // 役割を書かなくても上げられる（UI は書かない）。
    let request = Request::builder()
        .method("POST")
        .uri("/api/files?project=p&key=SKILLS-14&name=b.png")
        .header(header::HOST, "localhost:18801")
        .header(header::ORIGIN, "http://localhost:18801")
        .header(header::CONTENT_TYPE, "image/png")
        .body(Body::from(PNG))
        .unwrap();
    let (status, _, body) = s.send(request).await;
    assert_eq!(status, StatusCode::CREATED);
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["role"], "product-manager");

    // Origin の無い要求は、引数の役割のまま。役割が無ければ断る。
    let (status, _, json) = s
        .put_file(&format!("{SE}&name=c.png"), Some("image/png"), PNG)
        .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(json["role"], "software-engineer");
    assert_eq!(json["workspace"], "workspace:1");
    let (status, _, json) = s
        .put_file("project=p&key=SKILLS-14&name=c.png", Some("image/png"), PNG)
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        json["error"],
        "role は英小文字で始まり、英小文字・数字・- で 32 字まで"
    );
}

#[tokio::test]
async fn the_backup_holds_the_files() {
    let s = Server::start().await;
    s.place_file(&format!("{SE}&name=a.png"), "image/png", PNG)
        .await;
    s.place_file(&format!("{SE}&name=a.png"), "image/png", PNG)
        .await;
    s.remove_files("project=p&key=SKILLS-14&name=a.png&n=1")
        .await;
    let (status, _, bytes) = s.get("/api/backup").await;
    assert_eq!(status, StatusCode::OK);

    let mut copy = s.path.clone().into_os_string();
    copy.push(".files");
    let copy = PathBuf::from(copy);
    std::fs::write(&copy, &bytes).unwrap();
    let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}", copy.display()))
        .await
        .unwrap();
    let count = |sql: &'static str| {
        let pool = pool.clone();
        async move {
            sqlx::query_scalar::<_, i64>(sql)
                .fetch_one(&pool)
                .await
                .unwrap()
        }
    };
    assert_eq!(count("select count(*) from files").await, 1);
    assert_eq!(count("select count(*) from files_removed").await, 1);
    let body: Vec<u8> = sqlx::query_scalar("select body from files where n = 2")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(body, PNG);
    pool.close().await;
    for suffix in ["", "-wal", "-shm"] {
        let mut name = copy.clone().into_os_string();
        name.push(suffix);
        let _ = std::fs::remove_file(name);
    }
}

#[tokio::test]
async fn files_survive_a_restart() {
    let s = Server::start().await;
    s.place_file(&format!("{SE}&name=a.png"), "image/png", PNG)
        .await;
    s.db.close().await;
    let again = Db::open(&s.path).await.unwrap();
    let app = router(AppState::new(again.clone()));
    let response = app
        .oneshot(request("GET", "/p/SKILLS-14/files/a-1.png", Body::empty()))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(bytes, PNG);
    again.close().await;
}

/// file のエンドポイントを足しても、ページの URL と答えは前のまま。
#[tokio::test]
async fn pages_and_files_live_side_by_side() {
    let s = Server::start().await;
    let page = html("図のあるページ", "<img src=\"files/dot-1.png\">");
    s.place(&format!("{SE}&kind=result"), &page).await;
    s.place_file(&format!("{SE}&name=dot.png"), "image/png", PNG)
        .await;
    // ページの名の file（kind が files でも）とぶつからない。
    s.place(&format!("{SE}&kind=files"), "<p>files という種類</p>")
        .await;
    assert_eq!(s.text("/p/SKILLS-14/result-1.html").await, page);
    assert_eq!(
        s.text("/p/SKILLS-14/files-1.html").await,
        "<p>files という種類</p>"
    );
    assert_eq!(s.get("/p/SKILLS-14/files/dot-1.png").await.2, PNG);
    assert_eq!(
        s.paths("").await,
        ["/p/SKILLS-14/files-1.html", "/p/SKILLS-14/result-1.html"]
    );
    assert_eq!(s.file_paths("").await, ["/p/SKILLS-14/files/dot-1.png"]);
    let (_, headers, _) = s.get("/p/SKILLS-14/result-1.html").await;
    assert_eq!(
        headers[header::CONTENT_SECURITY_POLICY],
        "sandbox allow-scripts"
    );
    assert_eq!(headers[pagetray::API_HEADER], "1");
    let (_, headers, _) = s.get("/p/SKILLS-14/files/dot-1.png").await;
    assert_eq!(headers[pagetray::API_HEADER], "1");
    // ページを消しても file は残り、file を消してもページは残る。
    s.remove("project=p&key=SKILLS-14&kind=result").await;
    assert_eq!(
        s.get("/p/SKILLS-14/files/dot-1.png").await.0,
        StatusCode::OK
    );
    s.remove_files("project=p&key=SKILLS-14").await;
    assert_eq!(s.get("/p/SKILLS-14/files-1.html").await.0, StatusCode::OK);
}

#[tokio::test]
async fn a_put_file_is_pushed_as_a_named_event_and_pages_stay_unnamed() {
    let s = Server::start().await;
    let response = s
        .app
        .clone()
        .oneshot(request("GET", "/api/events", Body::empty()))
        .await
        .unwrap();
    let mut body = response.into_body();
    let mut next = async || {
        let frame = tokio::time::timeout(std::time::Duration::from_secs(5), body.frame())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        String::from_utf8(frame.into_data().unwrap().to_vec()).unwrap()
    };

    s.place_file(&format!("{SE}&name=dot.png"), "image/png", PNG)
        .await;
    let text = next().await;
    let data = text.strip_prefix("event: file\ndata: ").unwrap();
    let json: Value = serde_json::from_str(data.trim()).unwrap();
    assert_eq!(json["path"], "/p/SKILLS-14/files/dot-1.png");
    assert_eq!(json["type"], "image/png");

    s.place(&format!("{SE}&kind=result"), &html("結果", "できた"))
        .await;
    let text = next().await;
    let json: Value = serde_json::from_str(text.strip_prefix("data: ").unwrap().trim()).unwrap();
    assert_eq!(json["path"], "/p/SKILLS-14/result-1.html");
}
