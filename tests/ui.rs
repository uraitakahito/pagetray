//! 一覧の UI と、新着の push の試験。

mod common;

use axum::body::Body;
use axum::http::{StatusCode, header};
use common::{PNG, SE, Server, html, request};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

/// 役割・workspace・Key・種類の違う 4 枚を、1 日ずつずらして置く。
async fn seeded() -> Server {
    let s = Server::start().await;
    let at = |day: u8| format!("created_at=2026-11-{day:02}T00:00:00Z");
    s.place(
        &format!("project=p&key=A-1&kind=brief&role=tpm&{}", at(1)),
        &html("依頼書", "頼む"),
    )
    .await;
    s.place(
        &format!(
            "project=p&key=A-1&kind=plan&role=software-engineer&workspace=w:2&{}",
            at(2)
        ),
        &html("計画", "絞り込みを作る"),
    )
    .await;
    s.place(
        &format!(
            "project=p&key=B-2&kind=plan&role=software-engineer&workspace=w:3&{}",
            at(3)
        ),
        &html("別の計画", "並べ替え"),
    )
    .await;
    s.place(
        &format!(
            "project=q&key=A-1&kind=result&role=devops-engineer&workspace=w:2&{}",
            at(4)
        ),
        &html("結果", "起こした"),
    )
    .await;
    s
}

/// 一覧に出たページの path を、出た順に。
fn listed(page: &str) -> Vec<&str> {
    page.split("<li data-path=\"")
        .skip(1)
        .map(|rest| rest.split('"').next().unwrap())
        .collect()
}

#[tokio::test]
async fn the_index_lists_every_page_newest_first_with_the_filter_choices() {
    let s = seeded().await;
    let page = s.text("/").await;
    assert_eq!(
        listed(&page),
        [
            "/q/A-1/result-1.html",
            "/p/B-2/plan-1.html",
            "/p/A-1/plan-1.html",
            "/p/A-1/brief-1.html"
        ]
    );
    // 絞り込みの候補は、DB にある値と件数。
    assert!(page.contains(r#"<a href="/?role=software-engineer"><span>software-engineer</span><span class="n">2</span></a>"#), "{page}");
    assert!(page.contains(r#"<a href="/?role=tpm"><span>tpm</span><span class="n">1</span></a>"#));
    assert!(
        page.contains(r#"<a href="/?kind=plan"><span>plan</span><span class="n">2</span></a>"#)
    );
    assert!(page.contains(r#"<a href="/?key=A-1"><span>A-1</span><span class="n">3</span></a>"#));
    // 絞り込みの列の見出しは「Key」（「鍵」とは書かない）。
    assert!(page.contains("<h2>Key</h2>"));
    assert!(!page.contains("<h2>鍵</h2>"));
    // workspace の ID には、その ID で置かれた Key を添える。ID の無い分は「不明」。
    assert!(page.contains(r#"<a href="/?workspace=w%3A3"><span>w:3<small>B-2</small></span><span class="n">1</span></a>"#));
    assert!(
        page.contains(
            r#"<a href="/?workspace="><span>（不明）</span><span class="n">1</span></a>"#
        )
    );
    assert!(page.contains(r#"<a href="/?workspace=w%3A2"><span>w:2<small>A-1</small></span><span class="n">2</span></a>"#));
    assert!(page.contains("並び: 新しい順"));
    assert!(!page.contains("<iframe"));
    assert!(!page.contains("絞り込みを外す"));
}

#[tokio::test]
async fn filters_narrow_the_list_and_show_in_the_links() {
    let s = seeded().await;
    let page = s.text("/?role=software-engineer").await;
    assert_eq!(listed(&page), ["/p/B-2/plan-1.html", "/p/A-1/plan-1.html"]);
    // 選んだ候補は、もう 1 度押すと外れる。ほかの候補は、今の条件に足される。
    assert!(
        page.contains(
            r#"<a href="/" class="on" aria-current="true"><span>software-engineer</span>"#
        ),
        "{page}"
    );
    assert!(page.contains(r#"<a href="/?role=software-engineer&#38;workspace=w%3A2">"#));
    assert!(page.contains(r#"<a class="btn" href="/">絞り込みを外す</a>"#));
    // ページを選ぶ link は、条件を持ったまま。
    assert!(
        page.contains(r#"<a href="/?role=software-engineer&#38;open=%2Fp%2FB-2%2Fplan-1.html">"#)
    );

    assert_eq!(
        listed(&s.text("/?workspace=w:2").await),
        ["/q/A-1/result-1.html", "/p/A-1/plan-1.html"]
    );
    assert_eq!(
        listed(&s.text("/?workspace=").await),
        ["/p/A-1/brief-1.html"]
    );
    assert_eq!(
        listed(
            &s.text("/?workspace=w:2&role=software-engineer&kind=plan&key=A-1&project=p")
                .await
        ),
        ["/p/A-1/plan-1.html"]
    );
    let none = s.text("/?role=tpm&kind=plan").await;
    assert!(listed(&none).is_empty());
    assert!(none.contains("合うページは無い"));
}

#[tokio::test]
async fn sorting_flips_with_a_link() {
    let s = seeded().await;
    let page = s.text("/?sort=created_at&kind=plan").await;
    assert_eq!(listed(&page), ["/p/A-1/plan-1.html", "/p/B-2/plan-1.html"]);
    assert!(
        page.contains(r#"<a class="btn" href="/?kind=plan" title="並びを替える">並び: 古い順</a>"#),
        "{page}"
    );
    let page = s.text("/?kind=plan").await;
    assert!(page.contains(
        r#"href="/?kind=plan&#38;sort=created_at" title="並びを替える">並び: 新しい順</a>"#
    ));
}

#[tokio::test]
async fn search_keeps_the_other_conditions() {
    let s = seeded().await;
    let page = s
        .text("/?q=%E7%B5%9E%E3%82%8A%E8%BE%BC%E3%81%BF&role=software-engineer")
        .await;
    assert_eq!(listed(&page), ["/p/A-1/plan-1.html"]);
    assert!(page.contains(r#"<input type="hidden" name="role" value="software-engineer">"#));
    assert!(page.contains(r#"<input type="search" name="q" value="絞り込み""#));
    // 空の検索は、条件が無いのと同じ。
    assert_eq!(
        listed(&s.text("/?q=&role=tpm").await),
        ["/p/A-1/brief-1.html"]
    );
}

#[tokio::test]
async fn project_and_key_folders_are_filtered_lists() {
    let s = seeded().await;
    assert_eq!(
        listed(&s.text("/p/").await),
        [
            "/p/B-2/plan-1.html",
            "/p/A-1/plan-1.html",
            "/p/A-1/brief-1.html"
        ]
    );
    assert_eq!(
        listed(&s.text("/p/A-1/").await),
        ["/p/A-1/plan-1.html", "/p/A-1/brief-1.html"]
    );
    assert!(listed(&s.text("/none/").await).is_empty());
}

/// 4 つの区画に、決めた id と読み上げ用の名が 1 つずつ付いているか（名は docs の用語集と同じ。
/// 試験や script が区画を名指す手がかりなので、変える時は用語集と一緒に変える）。
#[tokio::test]
async fn the_four_regions_carry_their_ids_and_names() {
    let s = seeded().await;
    for uri in ["/", "/?open=/p/A-1/plan-1.html", "/?role=tpm"] {
        let page = s.text(uri).await;
        for region in [
            r#"<header id="toolbar" aria-label="操作の帯">"#,
            r#"<nav id="filters" aria-label="絞り込みの列">"#,
            r#"<div id="list" role="region" aria-label="一覧の列">"#,
            r#"<section id="viewer" aria-label="本文の枠">"#,
        ] {
            assert_eq!(page.matches(region).count(), 1, "{uri}: {region}");
        }
        for id in ["toolbar", "filters", "list", "viewer", "fresh"] {
            assert_eq!(
                page.matches(&format!(r#" id="{id}""#)).count(),
                1,
                "{uri}: {id}"
            );
        }
    }
}

/// Key の画面（project と Key を 1 つずつ選んだ時）だけ、file の枠と上げる枠が出る。
/// file の枠は、script が無くても、図の見本・名・大きさ・上げた人・時刻・ページから参照する path を出す。
#[tokio::test]
async fn the_key_view_shows_its_files_and_the_upload_box() {
    let s = seeded().await;
    s.place_file(
        "project=p&key=A-1&name=screen.png&role=tpm&created_at=2026-11-05T01:02:03Z",
        "image/png",
        PNG,
    )
    .await;
    s.place_file(
        "project=p&key=A-1&name=notes.txt&role=software-engineer",
        "text/plain",
        "x".repeat(2048).as_bytes(),
    )
    .await;
    s.place_file(
        "project=p&key=B-2&name=other.png&role=tpm",
        "image/png",
        PNG,
    )
    .await;
    for uri in [
        "/p/A-1/",
        "/?project=p&key=A-1",
        "/p/A-1/?open=/p/A-1/plan-1.html",
    ] {
        let page = s.text(uri).await;
        for region in [
            r#"<div id="files" class="box" role="region" aria-label="file の枠">"#,
            r#"<div id="upload" class="box" role="region" aria-label="上げる枠" data-project="p" data-key="A-1""#,
            r#"<input type="file" accept=".png,.jpg,.jpeg,.gif,.webp,.pdf,.txt,.log,.md,.csv,.json">"#,
            r#"<input type="checkbox" name="reviewed" required> 秘密（token・鍵・個人の情報・ほかの人の画面）が写っていないか見た"#,
            r#"<button type="submit" disabled>送る</button>"#,
            r#"<p class="say nojs">上げるには script が要る。</p>"#,
        ] {
            assert_eq!(page.matches(region).count(), 1, "{uri}: {region}\n{page}");
        }
        // 新しい順。ほかの Key の file は出ない。
        let files: Vec<&str> = page
            .split("<li data-file=\"")
            .skip(1)
            .map(|rest| rest.split('"').next().unwrap())
            .collect();
        assert_eq!(
            files,
            ["/p/A-1/files/notes-1.txt", "/p/A-1/files/screen-1.png"],
            "{uri}"
        );
        assert!(page.contains(r#"<img src="/p/A-1/files/screen-1.png" loading="lazy" alt="">"#));
        assert!(page.contains(r#"<a class="name" href="/p/A-1/files/screen-1.png" target="_blank" rel="noopener">screen-1.png</a><span class="who">70 B ・ tpm ・ <time datetime="2026-11-05T01:02:03Z">11-05 01:02</time></span><code class="ref">files/screen-1.png</code>"#), "{page}");
        // 図でない file は、拡張子を出す。
        assert!(page.contains(r#"tabindex="-1" aria-hidden="true">txt</a>"#));
        assert!(page.contains("2.0 KiB ・ software-engineer"));
    }
    // 上げられる拡張子と Content-Type を、script に渡す。
    let page = s.text("/p/A-1/").await;
    assert!(
        page.contains(r#"data-types="[[&#34;png&#34;,&#34;image/png&#34;],[&#34;jpg&#34;"#),
        "{page}"
    );
    assert!(page.contains(r#"data-max="16777216""#));

    // Key を選んでいない画面には出ない。file の無い Key は、無いと出す。
    for uri in ["/", "/p/", "/?key=A-1", "/?project=p&role=tpm"] {
        let page = s.text(uri).await;
        assert!(!page.contains(r#"id="files""#), "{uri}");
        assert!(!page.contains(r#"id="upload""#), "{uri}");
    }
    let page = s.text("/q/A-1/").await;
    assert!(page.contains("この Key の file は無い"));
    assert!(page.contains(r#"id="upload""#));
}

#[tokio::test]
async fn the_chosen_page_shows_in_the_viewer_sandboxed() {
    let s = seeded().await;
    let page = s.text("/?open=/p/A-1/plan-1.html").await;
    assert!(
        page.contains(r#"<iframe src="/p/A-1/plan-1.html" sandbox="allow-scripts""#),
        "{page}"
    );
    assert!(page.contains(r#"<body data-viewer="/p/A-1/plan-1.html">"#));
    assert!(
        page.contains(
            r#"<a href="/?open=%2Fp%2FA-1%2Fplan-1.html" class="on" aria-current="true">"#
        )
    );
    // 置き場の外を指す物は、本文の枠に出さない。
    for open in [
        "https://evil.example/",
        "//evil.example/k/plan-1.html",
        "/api/backup",
        "javascript:alert(1)",
    ] {
        let page = s.text(&format!("/?open={open}")).await;
        assert!(!page.contains("<iframe"), "{open}");
        assert!(!page.contains("evil"), "{open}");
    }
}

#[tokio::test]
async fn text_from_pages_and_urls_is_escaped() {
    let s = Server::start().await;
    s.place(
        &format!("{SE}&kind=plan"),
        "<title>&lt;script&gt;alert(1)&lt;/script&gt; \"q\"</title>",
    )
    .await;
    let page = s.text("/?q=%22%3E%3Cscript%3Ealert(2)%3C/script%3E").await;
    assert!(!page.contains("<script>alert"), "{page}");
    let page = s.text("/").await;
    assert!(!page.contains("<script>alert"), "{page}");
    assert!(
        page.contains("&#60;script&#62;alert(1)") || page.contains("&lt;script&gt;alert(1)"),
        "{page}"
    );
}

#[tokio::test]
async fn long_lists_continue_with_a_link() {
    let s = Server::start().await;
    for i in 0..205 {
        let at = format!("created_at=2026-11-01T00:{:02}:{:02}Z", i / 60, i % 60);
        s.place(
            &format!("project=p&key=k&kind=result&role=tpm&{at}"),
            "<p>x</p>",
        )
        .await;
    }
    let page = s.text("/").await;
    assert_eq!(listed(&page).len(), 200);
    assert!(
        page.contains(r#"<a class="more" href="/?at=2026-11-01T00%3A00%3A05Z">続きを出す</a>"#),
        "{}",
        &page[page.len() - 3000..]
    );
    let rest = s.text("/?at=2026-11-01T00:00:05Z").await;
    assert_eq!(
        listed(&rest),
        [
            "/p/k/result-5.html",
            "/p/k/result-4.html",
            "/p/k/result-3.html",
            "/p/k/result-2.html",
            "/p/k/result-1.html"
        ]
    );
    assert!(!rest.contains("続きを出す"));
    let oldest = s.text("/?sort=created_at&at=2026-11-01T00:03:19Z").await;
    assert_eq!(
        listed(&oldest),
        [
            "/p/k/result-201.html",
            "/p/k/result-202.html",
            "/p/k/result-203.html",
            "/p/k/result-204.html",
            "/p/k/result-205.html"
        ]
    );
}

#[tokio::test]
async fn a_put_is_pushed_to_listeners() {
    let s = Server::start().await;
    let response = s
        .app
        .clone()
        .oneshot(request("GET", "/api/events", Body::empty()))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "text/event-stream"
    );
    let mut body = response.into_body();

    s.place(&format!("{SE}&kind=result"), &html("結果", "できた"))
        .await;
    let frame = tokio::time::timeout(std::time::Duration::from_secs(5), body.frame())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let text = String::from_utf8(frame.into_data().unwrap().to_vec()).unwrap();
    let json: Value = serde_json::from_str(text.strip_prefix("data: ").unwrap().trim()).unwrap();
    assert_eq!(json["path"], "/p/SKILLS-14/result-1.html");
    assert_eq!(json["role"], "software-engineer");
    assert_eq!(json["title"], "結果");
}

/// 返事の状態を書く。
async fn mark(s: &Server, page: &str, state: &str) {
    let uri = format!("/api/pages/reply?{page}&state={state}");
    let (status, _, _) = s.send(request("POST", &uri, Body::empty())).await;
    assert_eq!(status, StatusCode::OK, "{uri}");
}

#[tokio::test]
async fn pages_waiting_for_a_reply_are_marked_counted_and_filtered() {
    let s = seeded().await;
    // 返事の状態が無い間は、印も、操作の帯の数も、絞り込みの見出しも出ない。
    let page = s.text("/").await;
    assert!(page.contains("<title>ページの置き場</title>"));
    assert!(page.contains(r#"id="waiting" href="/?reply=waiting""#));
    assert!(page.contains(" hidden>返事待ち 0 件</a>"));
    assert!(!page.contains("<h2>返事</h2>"));
    assert!(!page.contains(r#" data-reply=""#));
    assert!(!page.contains(r#"class="reply "#));

    mark(&s, "project=p&key=A-1&kind=plan&n=1", "waiting").await;
    mark(&s, "project=p&key=B-2&kind=plan&n=1", "answered").await;
    let page = s.text("/?sort=created_at").await;
    assert!(page.contains("<title>返事待ち 1 件 ・ ページの置き場</title>"));
    // 操作の帯の数は、絞り込みに関わらず置き場の全部。押す先は、返事待ちだけ（並びは保つ）。
    assert!(page.contains(r#"id="waiting" href="/?reply=waiting&#38;sort=created_at" title="Product Manager の返事を待っているページだけに絞る">返事待ち 1 件</a>"#), "{page}");
    assert!(page.contains("<h2>返事</h2>"));
    assert!(
        page.contains(
            r#"<a href="/?reply=waiting&#38;sort=created_at"><span>返事待ち</span><span class="n">1</span></a>"#
        ),
        "{page}"
    );
    assert!(
        page.contains(
            r#"<a href="/?reply=answered&#38;sort=created_at"><span>答えた</span><span class="n">1</span></a>"#
        )
    );
    assert!(page.contains(r#"<li data-path="/p/A-1/plan-1.html" data-at="2026-11-02T00:00:00Z" data-reply="waiting">"#), "{page}");
    assert!(page.contains(
        r#"<span class="name">plan-1</span><span class="reply waiting">返事待ち</span>"#
    ));
    assert!(page.contains(r#"<li data-path="/p/B-2/plan-1.html" data-at="2026-11-03T00:00:00Z" data-reply="answered">"#));
    assert!(page.contains(r#"<span class="reply answered">答えた</span>"#));
    // 返事を求めていないページの行には、何も足さない。
    assert!(
        page.contains(r#"<li data-path="/p/A-1/brief-1.html" data-at="2026-11-01T00:00:00Z"><a"#)
    );

    let page = s.text("/?reply=waiting").await;
    assert_eq!(listed(&page), ["/p/A-1/plan-1.html"]);
    assert!(page.contains(r#"<a class="btn on" id="waiting" href="/?reply=waiting""#));
    assert!(
        page.contains(r#"<a href="/" class="on" aria-current="true"><span>返事待ち</span>"#),
        "{page}"
    );
    assert!(page.contains("絞り込みを外す"));
    // 絞り込みは、ほかの条件と重なり、探す form も持ち越す。
    assert!(page.contains(r#"<input type="hidden" name="reply" value="waiting">"#));
    assert!(page.contains(r#"href="/?kind=plan&#38;reply=waiting""#));
    let page = s.text("/?reply=answered&key=A-1").await;
    assert!(listed(&page).is_empty());
    // 知らない値は、条件が無いのと同じ。
    let page = s.text("/?reply=done").await;
    assert_eq!(listed(&page).len(), 4);
    assert!(!page.contains("絞り込みを外す"));

    // 答えると、数は 0 に戻り、操作の帯の釦は隠れる。
    mark(&s, "project=p&key=A-1&kind=plan&n=1", "answered").await;
    let page = s.text("/").await;
    assert!(page.contains("<title>ページの置き場</title>"));
    assert!(page.contains(" hidden>返事待ち 0 件</a>"));
    assert!(page.contains(r#"<span>答えた</span><span class="n">2</span>"#));
    assert!(!page.contains("<span>返事待ち</span>"));
}

/// レビューの印を書く（`POST /api/pages/review`）。
async fn review_marks(s: &Server, page: &str, body: Value) {
    let request = axum::http::Request::builder()
        .method("POST")
        .uri(format!("/api/pages/review?{page}"))
        .header(header::HOST, "127.0.0.1:8801")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let (status, _, _) = s.send(request).await;
    assert_eq!(status, StatusCode::OK, "{page}");
}

#[tokio::test]
async fn reviews_show_their_verdict_what_they_reviewed_and_who_waits() {
    let s = Server::start().await;
    let at = |day: u8| format!("created_at=2026-11-{day:02}T00:00:00Z");
    for (kind, day) in [("confirm", 1), ("plan", 2), ("brief", 3), ("review", 4)] {
        s.place(
            &format!(
                "project=p&key=K-1&kind={kind}&role=software-engineer&{}",
                at(day)
            ),
            &html(kind, "本文"),
        )
        .await;
    }
    review_marks(
        &s,
        "project=p&key=K-1&kind=brief&n=1",
        serde_json::json!({ "targets": ["/p/K-1/plan-1.html", "https://github.com/o/r/pull/191"] }),
    )
    .await;
    mark(&s, "project=p&key=K-1&kind=brief&n=1&on=review", "waiting").await;
    mark(&s, "project=p&key=K-1&kind=confirm&n=1", "waiting").await;

    let page = s.text("/?sort=created_at").await;
    // tab の題と「返事待ち」は Product Manager の分だけ。レビュー待ちは別の釦。
    assert!(page.contains("<title>返事待ち 1 件 ・ ページの置き場</title>"));
    assert!(page.contains(r#"id="waiting" href="/?reply=waiting&#38;sort=created_at" title="Product Manager の返事を待っているページだけに絞る">返事待ち 1 件</a>"#), "{page}");
    assert!(page.contains(r#"id="review-waiting" href="/?reply=waiting&#38;on=review&#38;sort=created_at" title="レビューの判定を待っている依頼書だけに絞る">レビュー待ち 1 件</a>"#), "{page}");
    assert!(page.contains(" hidden>上げた 0 件</a>"));
    assert!(
        page.contains(
            r#"<a href="/?reply=waiting&#38;on=review&#38;sort=created_at"><span>レビュー待ち</span><span class="n">1</span></a>"#
        ),
        "{page}"
    );
    assert!(page.contains(r#"<li data-path="/p/K-1/brief-1.html" data-at="2026-11-03T00:00:00Z" data-reply="waiting" data-on="review">"#), "{page}");
    assert!(page.contains(r#"<span class="reply waiting review">レビュー待ち</span>"#));
    // 依頼書の行に、見る物の link（置き場のページは一覧の中で開き、PR は別の tab）。
    assert!(page.contains(r#"<span class="links targets">見る物: <a href="/?sort=created_at&#38;open=%2Fp%2FK-1%2Fplan-1.html">plan-1</a> <a href="https://github.com/o/r/pull/191" target="_blank" rel="noopener">r#191</a></span>"#), "{page}");
    // 判定がまだ無いので、判定の見出しも、見られた側の「レビュー:」も出ない。
    assert!(!page.contains("<h2>判定</h2>"));
    assert!(!page.contains("links reviews"));

    // 判定を書き、依頼書の待ちを外す。
    review_marks(
        &s,
        "project=p&key=K-1&kind=review&n=1",
        serde_json::json!({ "verdict": "concerns", "targets": ["/p/K-1/plan-1.html"] }),
    )
    .await;
    mark(&s, "project=p&key=K-1&kind=brief&n=1&on=review", "answered").await;
    let page = s.text("/").await;
    assert!(page.contains(" hidden>レビュー待ち 0 件</a>"));
    assert!(page.contains(r#"<span class="reply answered review">レビュー済み</span>"#));
    assert!(page.contains(r#"<li data-path="/p/K-1/review-1.html" data-at="2026-11-04T00:00:00Z" data-verdict="concerns">"#), "{page}");
    assert!(page.contains(
        r#"<span class="name">review-1</span><span class="verdict concerns">気になる所あり</span>"#
    ));
    assert!(page.contains(r#"<span class="links targets">見た物: <a href="/?open=%2Fp%2FK-1%2Fplan-1.html">plan-1</a></span>"#), "{page}");
    // 見られた計画の行に、それを見たレビューと判定。
    assert!(page.contains(r#"<span class="links reviews">レビュー: <a href="/?open=%2Fp%2FK-1%2Freview-1.html">review-1</a><span class="verdict concerns">気になる所あり</span></span>"#), "{page}");
    assert!(page.contains("<h2>判定</h2>"));
    assert!(
        page.contains(r#"<a href="/?verdict=concerns"><span>気になる所あり</span><span class="n">1</span></a>"#),
        "{page}"
    );
    // 判定で絞れ、探す form も持ち越す。
    let page = s.text("/?verdict=concerns").await;
    assert_eq!(listed(&page), ["/p/K-1/review-1.html"]);
    assert!(page.contains(r#"<input type="hidden" name="verdict" value="concerns">"#));
    assert!(page.contains("絞り込みを外す"));
    // Product Manager の「答えた」に、レビュー済みの依頼書は入らない。
    let page = s.text("/?reply=answered").await;
    assert!(listed(&page).is_empty());
    let page = s.text("/?reply=answered&on=review").await;
    assert_eq!(listed(&page), ["/p/K-1/brief-1.html"]);

    // 上げると、上げた件の釦と印が出て、決まると消える。
    mark(&s, "project=p&key=K-1&kind=review&n=1&on=tpm", "waiting").await;
    let page = s.text("/").await;
    assert!(page.contains(r#"id="escalated" href="/?reply=waiting&#38;on=tpm" title="判定を受けずに TPM 担当へ上げ、まだ決まっていないレビューだけに絞る">上げた 1 件</a>"#), "{page}");
    assert!(page.contains(r#"<span class="reply waiting tpm">上げた</span>"#));
    assert!(page.contains("<title>返事待ち 1 件 ・ ページの置き場</title>"));
    let page = s.text("/?reply=waiting&on=tpm").await;
    assert_eq!(listed(&page), ["/p/K-1/review-1.html"]);
    assert!(page.contains(r#"<a class="btn on" id="escalated" href="/?reply=waiting&#38;on=tpm""#));
    mark(&s, "project=p&key=K-1&kind=review&n=1&on=tpm", "answered").await;
    let page = s.text("/").await;
    assert!(page.contains(" hidden>上げた 0 件</a>"));
    assert!(page.contains(r#"<span class="reply answered tpm">決まった</span>"#));

    // 知らない値は、条件が無いのと同じ。on は reply と組の時だけ効く。
    let page = s.text("/?verdict=done").await;
    assert_eq!(listed(&page).len(), 4);
    let page = s.text("/?on=review").await;
    assert_eq!(listed(&page).len(), 4);
    assert!(!page.contains("絞り込みを外す"));
}
