//! 画面（一覧の UI。`/`・`/{project}/`・`/{project}/{key}/`）。今の server の tests/ui.rs と同じ振る舞いを、新しい server で見る。
//! 違うのは、置く・書く API の道と、残す時刻の字（24 字）と、続きの印 `at`（前の頁の最後の行の時刻と id。同じ時刻の行が
//! 境に並んでも抜けない）。画面の CSP・送り手の出し方・URL に入る字の逃がし方も、ここで見る。

mod common;

use axum::http::{StatusCode, header};
use common::castlist::{ACTOR_A, ACTOR_B, Fake, NAME_A, NAME_B, Reply};
use common::{App, PNG, html};
use pagetray_next::castlist::Castlist;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// `createdAt` の引数（2026-11 の `day` 日の 0 時）。
fn at(day: u8) -> String {
    format!("createdAt=2026-11-{day:02}T00%3A00%3A00Z")
}

/// 役割・workspace・Key・種類の違う 4 枚を、1 日ずつずらして置く。
async fn seeded() -> App {
    let s = App::start().await;
    s.place(
        "p",
        "A-1",
        "brief",
        &format!("role=tpm&{}", at(1)),
        &html("依頼書", "頼む"),
    )
    .await;
    s.place(
        "p",
        "A-1",
        "plan",
        &format!("role=software-engineer&workspace=w%3A2&{}", at(2)),
        &html("計画", "絞り込みを作る"),
    )
    .await;
    s.place(
        "p",
        "B-2",
        "plan",
        &format!("role=software-engineer&workspace=w%3A3&{}", at(3)),
        &html("別の計画", "並べ替え"),
    )
    .await;
    s.place(
        "q",
        "A-1",
        "result",
        &format!("role=devops-engineer&workspace=w%3A2&{}", at(4)),
        &html("結果", "起こした"),
    )
    .await;
    s
}

/// 画面を開き、200 と HTML を見て、本文の字を返す。
async fn text(s: &App, uri: &str) -> String {
    let (status, headers, body) = s.get(uri).await;
    let page = String::from_utf8(body).expect("UTF-8");
    assert_eq!(status, StatusCode::OK, "{uri}: {page}");
    assert_eq!(
        headers[header::CONTENT_TYPE],
        "text/html; charset=utf-8",
        "{uri}"
    );
    page
}

/// 一覧に出たページの path を、出た順に。
fn listed(page: &str) -> Vec<&str> {
    page.split("<li data-path=\"")
        .skip(1)
        .map(|rest| rest.split('"').next().unwrap())
        .collect()
}

/// 「続きを出す」の link の先（HTML の字の `&#38;` を戻した物）。無ければ None。
fn more(page: &str) -> Option<String> {
    let rest = page.split(r#"<a class="more" href=""#).nth(1)?;
    Some(rest.split('"').next()?.replace("&#38;", "&"))
}

#[tokio::test]
async fn the_index_lists_every_page_newest_first_with_the_filter_choices() {
    let s = seeded().await;
    let page = text(&s, "/").await;
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
    // 時刻は、残した字（24 字）と、script が動かない時の UTC の短い字。
    assert!(
        page.contains(
            r#"<li data-path="/q/A-1/result-1.html" data-at="2026-11-04T00:00:00.000Z">"#
        ),
        "{page}"
    );
    assert!(page.contains(r#"<time datetime="2026-11-04T00:00:00.000Z">11-04 00:00</time>"#));
}

#[tokio::test]
async fn filters_narrow_the_list_and_show_in_the_links() {
    let s = seeded().await;
    let page = text(&s, "/?role=software-engineer").await;
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
        listed(&text(&s, "/?workspace=w:2").await),
        ["/q/A-1/result-1.html", "/p/A-1/plan-1.html"]
    );
    // 空の workspace も条件（workspace を書かずに置いたページ）。
    assert_eq!(
        listed(&text(&s, "/?workspace=").await),
        ["/p/A-1/brief-1.html"]
    );
    assert_eq!(
        listed(
            &text(
                &s,
                "/?workspace=w:2&role=software-engineer&kind=plan&key=A-1&project=p"
            )
            .await
        ),
        ["/p/A-1/plan-1.html"]
    );
    let none = text(&s, "/?role=tpm&kind=plan").await;
    assert!(listed(&none).is_empty());
    assert!(none.contains("合うページは無い"));
    // ほかの空の条件は、書かないのと同じ。
    for uri in [
        "/?role=",
        "/?project=&key=&kind=&q=&reply=&on=&verdict=&actor=",
    ] {
        let page = text(&s, uri).await;
        assert_eq!(listed(&page).len(), 4, "{uri}");
        assert!(!page.contains("絞り込みを外す"), "{uri}");
    }
}

#[tokio::test]
async fn sorting_flips_with_a_link() {
    let s = seeded().await;
    let page = text(&s, "/?sort=created_at&kind=plan").await;
    assert_eq!(listed(&page), ["/p/A-1/plan-1.html", "/p/B-2/plan-1.html"]);
    assert!(
        page.contains(r#"<a class="btn" href="/?kind=plan" title="並びを替える">並び: 古い順</a>"#),
        "{page}"
    );
    let page = text(&s, "/?kind=plan").await;
    assert!(page.contains(
        r#"href="/?kind=plan&#38;sort=created_at" title="並びを替える">並び: 新しい順</a>"#
    ));
    // 並びの値は created_at だけ（ほかは書かないのと同じ）。
    let page = text(&s, "/?kind=plan&sort=-created_at").await;
    assert_eq!(listed(&page), ["/p/B-2/plan-1.html", "/p/A-1/plan-1.html"]);
    assert!(page.contains(
        r#"href="/?kind=plan&#38;sort=created_at" title="並びを替える">並び: 新しい順</a>"#
    ));
}

#[tokio::test]
async fn search_keeps_the_other_conditions() {
    let s = seeded().await;
    let page = text(
        &s,
        "/?q=%E7%B5%9E%E3%82%8A%E8%BE%BC%E3%81%BF&role=software-engineer",
    )
    .await;
    assert_eq!(listed(&page), ["/p/A-1/plan-1.html"]);
    assert!(page.contains(r#"<input type="hidden" name="role" value="software-engineer">"#));
    assert!(page.contains(r#"<input type="search" name="q" value="絞り込み""#));
    // 空の検索は、条件が無いのと同じ。
    assert_eq!(
        listed(&text(&s, "/?q=&role=tpm").await),
        ["/p/A-1/brief-1.html"]
    );
}

#[tokio::test]
async fn project_and_key_folders_are_filtered_lists() {
    let s = seeded().await;
    assert_eq!(
        listed(&text(&s, "/p/").await),
        [
            "/p/B-2/plan-1.html",
            "/p/A-1/plan-1.html",
            "/p/A-1/brief-1.html"
        ]
    );
    assert_eq!(
        listed(&text(&s, "/p/A-1/").await),
        ["/p/A-1/plan-1.html", "/p/A-1/brief-1.html"]
    );
    assert!(listed(&text(&s, "/none/").await).is_empty());
}

/// 4 つの区画に、決めた id と読み上げ用の名が 1 つずつ付いているか（名は docs の用語集と同じ。
/// 試験や script が区画を名指す手がかりなので、変える時は用語集と一緒に変える）。
#[tokio::test]
async fn the_four_regions_carry_their_ids_and_names() {
    let s = seeded().await;
    for uri in ["/", "/?open=/p/A-1/plan-1.html", "/?role=tpm"] {
        let page = text(&s, uri).await;
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
/// 上げる枠の script は、新しい API の道（`POST /api/projects/{p}/keys/{k}/files?name=…`）に送る。
#[tokio::test]
async fn the_key_view_shows_its_files_and_the_upload_box() {
    let s = seeded().await;
    s.place_file(
        "p",
        "A-1",
        "name=screen.png&role=tpm&createdAt=2026-11-05T01%3A02%3A03Z",
        "image/png",
        PNG,
    )
    .await;
    s.place_file(
        "p",
        "A-1",
        "name=notes.txt&role=software-engineer",
        "text/plain",
        "x".repeat(2048).as_bytes(),
    )
    .await;
    s.place_file("p", "B-2", "name=other.png&role=tpm", "image/png", PNG)
        .await;
    for uri in [
        "/p/A-1/",
        "/?project=p&key=A-1",
        "/p/A-1/?open=/p/A-1/plan-1.html",
    ] {
        let page = text(&s, uri).await;
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
        assert!(page.contains(r#"<a class="name" href="/p/A-1/files/screen-1.png" target="_blank" rel="noopener">screen-1.png</a><span class="who">70 B ・ tpm ・ <time datetime="2026-11-05T01:02:03.000Z">11-05 01:02</time></span><code class="ref">files/screen-1.png</code>"#), "{page}");
        // 図でない file は、拡張子を出す。
        assert!(page.contains(r#"tabindex="-1" aria-hidden="true">txt</a>"#));
        assert!(page.contains("2.0 KiB ・ software-engineer"));
    }
    // 上げられる拡張子と Content-Type と上限と、送る先の道を、script に渡す。
    let page = text(&s, "/p/A-1/").await;
    assert!(
        page.contains(r#"data-types="[[&#34;png&#34;,&#34;image/png&#34;],[&#34;jpg&#34;"#),
        "{page}"
    );
    assert!(
        page.contains(r#"data-max="16777216" data-upload="/api/projects/p/keys/A-1/files">"#),
        "{page}"
    );
    assert!(!page.contains("/api/files?"), "{page}");

    // Key を選んでいない画面には出ない。file の無い Key は、無いと出す。project が api の Key にも出さない。
    for uri in [
        "/",
        "/p/",
        "/?key=A-1",
        "/?project=p&role=tpm",
        "/?project=api&key=A-1",
    ] {
        let page = text(&s, uri).await;
        assert!(!page.contains(r#"id="files""#), "{uri}");
        assert!(!page.contains(r#"id="upload""#), "{uri}");
    }
    let page = text(&s, "/q/A-1/").await;
    assert!(page.contains("この Key の file は無い"));
    assert!(page.contains(r#"id="upload""#));
}

#[tokio::test]
async fn the_chosen_page_shows_in_the_viewer_sandboxed() {
    let s = seeded().await;
    let page = text(&s, "/?open=/p/A-1/plan-1.html").await;
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
        "/p/A-1/plan-01.html",
    ] {
        let page = text(&s, &format!("/?open={open}")).await;
        assert!(!page.contains("<iframe"), "{open}");
        assert!(!page.contains("evil"), "{open}");
    }
}

#[tokio::test]
async fn text_from_pages_and_urls_is_escaped() {
    let s = App::start().await;
    s.place(
        "p",
        "SKILLS-14",
        "plan",
        "role=software-engineer",
        "<title>&lt;script&gt;alert(1)&lt;/script&gt; \"q\"</title>",
    )
    .await;
    let page = text(&s, "/?q=%22%3E%3Cscript%3Ealert(2)%3C/script%3E").await;
    assert!(!page.contains("<script>alert"), "{page}");
    let page = text(&s, "/").await;
    assert!(!page.contains("<script>alert"), "{page}");
    assert!(
        page.contains("&#60;script&#62;alert(1)") || page.contains("&lt;script&gt;alert(1)"),
        "{page}"
    );
}

#[tokio::test]
async fn long_lists_continue_with_a_link() {
    let s = App::start().await;
    for i in 0..205 {
        let at = format!("createdAt=2026-11-01T00%3A{:02}%3A{:02}Z", i / 60, i % 60);
        s.place("p", "k", "result", &format!("role=tpm&{at}"), "<p>x</p>")
            .await;
    }
    let page = text(&s, "/").await;
    assert_eq!(listed(&page).len(), 200);
    assert_eq!(listed(&page)[199], "/p/k/result-6.html");
    let next = more(&page).unwrap_or_else(|| panic!("{}", &page[page.len() - 3000..]));
    assert!(next.starts_with("/?at="), "{next}");
    let rest = text(&s, &next).await;
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

    let page = text(&s, "/?sort=created_at").await;
    assert_eq!(listed(&page)[0], "/p/k/result-1.html");
    let next = more(&page).expect("続きを出す");
    assert!(next.starts_with("/?sort=created_at&at="), "{next}");
    assert_eq!(
        listed(&text(&s, &next).await),
        [
            "/p/k/result-201.html",
            "/p/k/result-202.html",
            "/p/k/result-203.html",
            "/p/k/result-204.html",
            "/p/k/result-205.html"
        ]
    );
    // 読めない続きの印と、ほかの並びの印は、書かないのと同じ（頭から出す）。
    let top = listed(&text(&s, "/").await)
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let other = more(&page).unwrap().replace("/?sort=created_at&", "/?");
    for uri in ["/?at=zz", "/?at=2026-11-01T00:00:05Z", other.as_str()] {
        assert_eq!(listed(&text(&s, uri).await), top, "{uri}");
    }
}

/// 続きの印は、前の頁の最後の行の時刻と id。同じ時刻の行が頁の境に並んでも、抜けず重ならない。
#[tokio::test]
async fn continuing_keeps_pages_that_share_a_time() {
    let s = App::start().await;
    for _ in 0..205 {
        s.place(
            "p",
            "k",
            "result",
            "role=tpm&createdAt=2026-11-01T00%3A00%3A00Z",
            "<p>x</p>",
        )
        .await;
    }
    let page = text(&s, "/").await;
    let mut seen: Vec<String> = listed(&page).into_iter().map(str::to_owned).collect();
    let rest = text(&s, &more(&page).expect("続きを出す")).await;
    seen.extend(listed(&rest).into_iter().map(str::to_owned));
    let expected: Vec<String> = (1..=205)
        .rev()
        .map(|n| format!("/p/k/result-{n}.html"))
        .collect();
    assert_eq!(seen, expected);
}

/// 返事の状態を書く（`PUT …/pages/{n}/reply`）。
async fn mark(s: &App, path: &str, state: &str, on: Option<&str>) {
    let body = match on {
        None => json!({ "state": state }),
        Some(on) => json!({ "state": state, "on": on }),
    };
    let (status, _, json) = s.reply(path, body).await;
    assert_eq!(status, StatusCode::OK, "{path}: {json}");
}

#[tokio::test]
async fn pages_waiting_for_a_reply_are_marked_counted_and_filtered() {
    let s = seeded().await;
    // 返事の状態が無い間は、印も、操作の帯の数も、絞り込みの見出しも出ない。
    let page = text(&s, "/").await;
    assert!(page.contains("<title>ページの置き場</title>"));
    assert!(page.contains(r#"id="waiting" href="/?reply=waiting""#));
    assert!(page.contains(" hidden>返事待ち 0 件</a>"));
    assert!(!page.contains("<h2>返事</h2>"));
    assert!(!page.contains(r#" data-reply=""#));
    assert!(!page.contains(r#"class="reply "#));

    mark(&s, "/p/A-1/plan-1.html", "waiting", None).await;
    mark(&s, "/p/B-2/plan-1.html", "answered", None).await;
    let page = text(&s, "/?sort=created_at").await;
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
    assert!(page.contains(r#"<li data-path="/p/A-1/plan-1.html" data-at="2026-11-02T00:00:00.000Z" data-reply="waiting">"#), "{page}");
    assert!(page.contains(
        r#"<span class="name">plan-1</span><span class="reply waiting">返事待ち</span>"#
    ));
    assert!(page.contains(r#"<li data-path="/p/B-2/plan-1.html" data-at="2026-11-03T00:00:00.000Z" data-reply="answered">"#));
    assert!(page.contains(r#"<span class="reply answered">答えた</span>"#));
    // 返事を求めていないページの行には、何も足さない。
    assert!(
        page.contains(
            r#"<li data-path="/p/A-1/brief-1.html" data-at="2026-11-01T00:00:00.000Z"><a"#
        )
    );
    // 絞った画面でも、数と tab の題は置き場の全部。
    let page = text(&s, "/?project=q").await;
    assert!(page.contains("<title>返事待ち 1 件 ・ ページの置き場</title>"));
    assert!(page.contains(">返事待ち 1 件</a>"), "{page}");

    let page = text(&s, "/?reply=waiting").await;
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
    let page = text(&s, "/?reply=answered&key=A-1").await;
    assert!(listed(&page).is_empty());
    // 知らない値は、条件が無いのと同じ。
    let page = text(&s, "/?reply=done").await;
    assert_eq!(listed(&page).len(), 4);
    assert!(!page.contains("絞り込みを外す"));

    // 答えると、数は 0 に戻り、操作の帯の釦は隠れる。
    mark(&s, "/p/A-1/plan-1.html", "answered", None).await;
    let page = text(&s, "/").await;
    assert!(page.contains("<title>ページの置き場</title>"));
    assert!(page.contains(" hidden>返事待ち 0 件</a>"));
    assert!(page.contains(r#"<span>答えた</span><span class="n">2</span>"#));
    assert!(!page.contains("<span>返事待ち</span>"));
}

/// レビューの印を書く（`POST …/pages/{n}/review`）。
async fn review_marks(s: &App, path: &str, body: Value) {
    let (status, _, json) = s.review(path, Some(body)).await;
    assert_eq!(status, StatusCode::OK, "{path}: {json}");
}

#[tokio::test]
async fn reviews_show_their_verdict_what_they_reviewed_and_who_waits() {
    let s = App::start().await;
    for (kind, day) in [("confirm", 1), ("plan", 2), ("brief", 3), ("review", 4)] {
        s.place(
            "p",
            "K-1",
            kind,
            &format!("role=software-engineer&{}", at(day)),
            &html(kind, "本文"),
        )
        .await;
    }
    review_marks(
        &s,
        "/p/K-1/brief-1.html",
        json!({ "targets": ["/p/K-1/plan-1.html", "https://github.com/o/r/pull/191"] }),
    )
    .await;
    mark(&s, "/p/K-1/brief-1.html", "waiting", Some("review")).await;
    mark(&s, "/p/K-1/confirm-1.html", "waiting", None).await;

    let page = text(&s, "/?sort=created_at").await;
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
    assert!(page.contains(r#"<li data-path="/p/K-1/brief-1.html" data-at="2026-11-03T00:00:00.000Z" data-reply="waiting" data-on="review">"#), "{page}");
    assert!(page.contains(r#"<span class="reply waiting review">レビュー待ち</span>"#));
    // 依頼書の行に、見る物の link（置き場のページは一覧の中で開き、PR は別の tab）。
    assert!(page.contains(r#"<span class="links targets">見る物: <a href="/?sort=created_at&#38;open=%2Fp%2FK-1%2Fplan-1.html">plan-1</a> <a href="https://github.com/o/r/pull/191" target="_blank" rel="noopener">r#191</a></span>"#), "{page}");
    // 判定がまだ無いので、判定の見出しも、見られた側の「レビュー:」も出ない。
    assert!(!page.contains("<h2>判定</h2>"));
    assert!(!page.contains("links reviews"));

    // 判定を書き、依頼書の待ちを外す。
    review_marks(
        &s,
        "/p/K-1/review-1.html",
        json!({ "verdict": "concerns", "targets": ["/p/K-1/plan-1.html"] }),
    )
    .await;
    mark(&s, "/p/K-1/brief-1.html", "answered", Some("review")).await;
    let page = text(&s, "/").await;
    assert!(page.contains(" hidden>レビュー待ち 0 件</a>"));
    assert!(page.contains(r#"<span class="reply answered review">レビュー済み</span>"#));
    assert!(page.contains(r#"<li data-path="/p/K-1/review-1.html" data-at="2026-11-04T00:00:00.000Z" data-verdict="concerns">"#), "{page}");
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
    let page = text(&s, "/?verdict=concerns").await;
    assert_eq!(listed(&page), ["/p/K-1/review-1.html"]);
    assert!(page.contains(r#"<input type="hidden" name="verdict" value="concerns">"#));
    assert!(page.contains("絞り込みを外す"));
    // Product Manager の「答えた」に、レビュー済みの依頼書は入らない。
    let page = text(&s, "/?reply=answered").await;
    assert!(listed(&page).is_empty());
    let page = text(&s, "/?reply=answered&on=review").await;
    assert_eq!(listed(&page), ["/p/K-1/brief-1.html"]);

    // 上げると、上げた件の釦と印が出て、決まると消える。
    mark(&s, "/p/K-1/review-1.html", "waiting", Some("tpm")).await;
    let page = text(&s, "/").await;
    assert!(page.contains(r#"id="escalated" href="/?reply=waiting&#38;on=tpm" title="判定を受けずに TPM 担当へ上げ、まだ決まっていないレビューだけに絞る">上げた 1 件</a>"#), "{page}");
    assert!(page.contains(r#"<span class="reply waiting tpm">上げた</span>"#));
    assert!(page.contains("<title>返事待ち 1 件 ・ ページの置き場</title>"));
    let page = text(&s, "/?reply=waiting&on=tpm").await;
    assert_eq!(listed(&page), ["/p/K-1/review-1.html"]);
    assert!(page.contains(r#"<a class="btn on" id="escalated" href="/?reply=waiting&#38;on=tpm""#));
    mark(&s, "/p/K-1/review-1.html", "answered", Some("tpm")).await;
    let page = text(&s, "/").await;
    assert!(page.contains(" hidden>上げた 0 件</a>"));
    assert!(page.contains(r#"<span class="reply answered tpm">決まった</span>"#));

    // 知らない値は、条件が無いのと同じ。on は reply と組の時だけ効く。
    let page = text(&s, "/?verdict=done").await;
    assert_eq!(listed(&page).len(), 4);
    let page = text(&s, "/?on=review").await;
    assert_eq!(listed(&page).len(), 4);
    assert!(!page.contains("絞り込みを外す"));
}

// --- 送り手 ---

const AS_A: &str = "role=tpm&actorIssuer=tpm&actorRef=claude.session%3Aa";
const AS_PM: &str = "role=tpm&actorIssuer=tpm&actorRef=tpm.person%3Apm";

/// castlist（聞く先 `api`・画面の link の頭 `link`）に聞く server を、同じ DB の上に作る。
fn with_castlist(s: &App, api: &str, link: Option<&str>) -> App {
    let castlist = Castlist::configure(Some(api), link)
        .expect("castlist の URL")
        .expect("castlist がある");
    let app = pagetray_next::app_with(
        s.db.clone(),
        pagetray_next::Config {
            castlist: Some(castlist),
            ..Default::default()
        },
    );
    App {
        app,
        db: s.db.clone(),
        // 同じ DB を、元の App が消す。こちらは消さない（Drop が消すのは temp_db の folder だけ）。
        path: common::temp_db("ui-view"),
    }
}

/// 行の送り手: castlist の名（castlist のアクターの画面への link。頭は `PAGETRAY_CASTLIST_LINK_URL`）と、その下に小さく付随する id。
#[tokio::test]
async fn senders_show_their_castlist_name_linked_and_the_ref_small() {
    let fake = Fake::start().await;
    let plain = App::start().await;
    let s = with_castlist(&plain, &fake.url, Some("http://localhost:18802"));
    s.place("p", "K-1", "plan", AS_A, "<p>x</p>").await;
    s.place_file("p", "K-1", &format!("name=a.png&{AS_PM}"), "image/png", PNG)
        .await;
    let page = text(&s, "/p/K-1/").await;
    assert!(page.contains(&format!(
        r#"<span class="sender"><a class="actor" href="http://localhost:18802/actors/{ACTOR_A}" target="_blank" rel="noopener">{NAME_A}</a><small>claude.session:a</small></span>"#
    )), "{page}");
    assert!(page.contains(&format!(
        r#"<span class="sender"><a class="actor" href="http://localhost:18802/actors/{ACTOR_B}" target="_blank" rel="noopener">{NAME_B}</a><small>tpm.person:pm</small></span>"#
    )), "{page}");
    // link は聞く先から作らない。
    assert!(!page.contains(&fake.url), "{page}");
    // link の頭を書かなければ、既定の http://127.0.0.1:8802。
    let default = with_castlist(&plain, &fake.url, None);
    let page = text(&default, "/p/K-1/").await;
    assert!(
        page.contains(&format!(r#"href="http://127.0.0.1:8802/actors/{ACTOR_A}""#)),
        "{page}"
    );
    // castlist を設定していない server は、答えのある送り手も名だけ（link は無い）。
    let page = text(&plain, "/p/K-1/").await;
    assert!(
        page.contains(&format!(
            r#"<span class="sender">{NAME_A}<small>claude.session:a</small></span>"#
        )),
        "{page}"
    );
    assert!(!page.contains("/actors/"), "{page}");
}

/// castlist の答えの無い送り手（castlist の無い server が置いた物）は「未確認」。castlist を設定しても link は無い。
#[tokio::test]
async fn senders_without_an_answer_read_unconfirmed() {
    let fake = Fake::start().await;
    let s = App::start().await;
    s.place("p", "K-1", "plan", AS_A, "<p>x</p>").await;
    s.place_file("p", "K-1", &format!("name=a.png&{AS_A}"), "image/png", PNG)
        .await;
    s.place("p", "K-1", "note", "role=tpm", "<p>送り手なし</p>")
        .await;
    for view in [&s, &with_castlist(&s, &fake.url, None)] {
        let page = text(view, "/p/K-1/").await;
        assert_eq!(
            page.matches(r#"<span class="sender">未確認<small>claude.session:a</small></span>"#)
                .count(),
            2,
            "{page}"
        );
        assert!(!page.contains("/actors/"), "{page}");
        // 送り手の無い行には何も出さない。答えの無い送り手は、絞り込みの候補にならない。
        assert_eq!(page.matches(r#"class="sender""#).count(), 2, "{page}");
        assert!(!page.contains("<h2>送り手</h2>"), "{page}");
    }
    assert!(fake.requests().is_empty());
}

/// 絞り込みの「送り手」: castlist の答えのある送り手だけを、ページの多い順に 100 まで。名で出し、id で絞る。
#[tokio::test]
async fn the_sender_choices_are_the_resolved_ones_most_pages_first_up_to_100() {
    let fake = Fake::with(std::time::Duration::ZERO, |asked| {
        let results: Vec<Value> = asked["refs"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|r| {
                let r = r.as_str().unwrap_or("");
                let i: u64 = r.trim_start_matches("claude.session:s").parse().unwrap();
                json!({ "ref": r, "actor": {
                    "id": format!("019a0000-0000-7000-8000-{i:012x}"),
                    "name": format!("s{i:03}"),
                    "kind": "ai",
                } })
            })
            .collect();
        Reply::Status(
            200,
            json!({ "asOf": "2026-11-22T04:00:02Z", "results": results }).to_string(),
        )
    })
    .await;
    let plain = App::start().await;
    let s = with_castlist(&plain, &fake.url, None);
    for i in 0..=100 {
        s.place(
            "p",
            "K-1",
            "plan",
            &format!("role=tpm&actorIssuer=tpm&actorRef=claude.session%3As{i}"),
            "<p>x</p>",
        )
        .await;
    }
    // s100 だけ 2 枚。いちばん上に出る。
    s.place(
        "p",
        "K-1",
        "plan",
        "role=tpm&actorIssuer=tpm&actorRef=claude.session%3As100",
        "<p>x</p>",
    )
    .await;
    let page = text(&s, "/").await;
    let filters = page
        .split(r#"<nav id="filters""#)
        .nth(1)
        .and_then(|rest| rest.split("</nav>").next())
        .expect("絞り込みの列");
    let senders = filters
        .split("<h2>送り手</h2>")
        .nth(1)
        .and_then(|rest| rest.split("</ul>").next())
        .expect("送り手の見出し");
    assert_eq!(senders.matches("/?actor=").count(), 100, "{senders}");
    let s100 = "019a0000-0000-7000-8000-000000000064";
    assert!(
        senders.contains(&format!(
            r#"<li><a href="/?actor={s100}"><span>s100</span><span class="n">2</span></a></li>"#
        )),
        "{senders}"
    );
    assert!(senders.find("s100").unwrap() < senders.find("s000").unwrap());
    // 名でなく id で絞る。
    let page = text(&s, &format!("/?actor={s100}")).await;
    assert_eq!(listed(&page).len(), 2);
    assert!(page.contains(r#"<a href="/" class="on" aria-current="true"><span>s100</span>"#));
}

/// 付随する id は castlist と同じ広い形（空白・`?`・`&`・`#`・`<` なども通る）。画面の字はみな逃がし、URL に入れる値は
/// %XX にする（link の条件・探す form の隠れた欄）。
#[tokio::test]
async fn wide_refs_and_conditions_are_escaped_in_text_and_links() {
    let s = App::start().await;
    let wide = "claude.session:a b?c&d#e<x>\"y'";
    s.place(
        "p",
        "K-1",
        "plan",
        "role=tpm&actorIssuer=tpm&actorRef=claude.session%3Aa%20b%3Fc%26d%23e%3Cx%3E%22y%27",
        "<p>x</p>",
    )
    .await;
    let (_, _, listed_json) = s.json("/api/pages?project=p").await;
    assert_eq!(listed_json["pages"][0]["sender"]["ref"], wide);
    let page = text(&s, "/p/K-1/").await;
    assert!(
        page.contains("<small>claude.session:a b?c&#38;d#e&#60;x&#62;&#34;y&#39;</small>"),
        "{page}"
    );
    assert!(!page.contains("<x>"), "{page}");

    // URL の条件に入った字は、link では %XX、隠れた欄では HTML の字の逃がし。
    let page = text(&s, "/?actor=a%20b%3Fc%26d%23e%2F%3Cx%3E").await;
    assert!(
        page.contains(
            r#"href="/?actor=a%20b%3Fc%26d%23e%2F%3Cx%3E&#38;sort=created_at" title="並びを替える""#
        ),
        "{page}"
    );
    assert!(
        page.contains(r#"<input type="hidden" name="actor" value="a b?c&#38;d#e/&#60;x&#62;">"#),
        "{page}"
    );
    assert!(!page.contains("<x>"), "{page}");
}

// --- 画面の頭 ---

/// inline の物（`<style>`・`<script>`）の中身の SHA-256（CSP の `'sha256-…'` の字）。
fn inline_hash(page: &str, tag: &str) -> String {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    assert_eq!(page.matches(&open).count(), 1, "{tag}");
    let body = page
        .split(&open)
        .nth(1)
        .and_then(|rest| rest.split(&close).next())
        .expect("inline の中身");
    format!(
        "'sha256-{}'",
        aws_smithy_types::base64::encode(Sha256::digest(body.as_bytes()))
    )
}

/// 画面の答えの CSP: inline の style と script は、画面の物（hash）だけ。ほかの所の script・style・枠・送り先を断る。
/// 本文の枠は置き場のページだけ、知らせと上げる枠の送り先は置き場だけ、図は置き場と、上げる前の見本（blob:）と favicon（data:）。
#[tokio::test]
async fn the_screens_carry_a_csp_that_allows_only_their_own_inline_code() {
    let s = seeded().await;
    for uri in ["/", "/p/", "/p/A-1/?open=/p/A-1/plan-1.html"] {
        let (status, headers, body) = s.get(uri).await;
        assert_eq!(status, StatusCode::OK);
        let page = String::from_utf8(body).unwrap();
        let csp = headers[header::CONTENT_SECURITY_POLICY].to_str().unwrap();
        assert_eq!(
            csp,
            format!(
                "default-src 'none'; script-src {}; style-src {}; img-src 'self' data: blob:; connect-src 'self'; frame-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'",
                inline_hash(&page, "script"),
                inline_hash(&page, "style"),
            ),
            "{uri}"
        );
        assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
        assert_eq!(headers[header::REFERRER_POLICY], "no-referrer");
    }
    // 本文の道は、前のまま（隔てた枠）。
    let (_, headers, _) = s.get("/p/A-1/plan-1.html").await;
    assert_eq!(
        headers[header::CONTENT_SECURITY_POLICY],
        "sandbox allow-scripts"
    );
}
