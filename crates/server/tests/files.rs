//! file（CreateFile・ListFiles・DeleteFiles と、file の中身の道 `GET /{project}/{key}/files/{語}-{n}.{拡張子}`）の振る舞い
//! （処理より先に書く）。v0.8.0 の tag の tests/api.rs の file の試験を、1.0.0 の道と欄の名で見る。
//! 送り手は tests/senders.rs、本文の上限は tests/guard.rs。
mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use common::{App, HOST, PNG, error_type, file_paths, files_uri, html, keys, refused, scalar};
use serde_json::{Value, json};

const SE: &str = "role=software-engineer&workspace=workspace:1";

/// `SE` に名（`<語>.<拡張子>`）を足した引数。
fn named(name: &str) -> String {
    format!("{SE}&name={name}")
}

/// `day` 日の 0 時に置いた（取り込みの）時刻の引数。
fn at(day: u8) -> String {
    format!("createdAt=2026-11-{day:02}T00%3A00%3A00Z")
}

/// 一覧の 1 頁（200 を見る）。
async fn list(app: &App, query: &str) -> Value {
    let (status, _, json) = app.json(&format!("/api/files?{query}")).await;
    assert_eq!(status, StatusCode::OK, "{query}: {json}");
    json
}

/// 一覧の UI の form が上げる要求（`Origin` は `http://<Host>`。`site` は `Sec-Fetch-Site`）。
fn from_ui(uri: &str, host: &str, origin: &str, site: Option<&str>) -> Request<Body> {
    let mut builder = Request::post(uri)
        .header(header::HOST, host)
        .header(header::ORIGIN, origin)
        .header(header::CONTENT_TYPE, "image/png");
    if let Some(site) = site {
        builder = builder.header("sec-fetch-site", site);
    }
    builder.body(Body::from(PNG)).unwrap()
}

#[tokio::test]
async fn a_file_comes_back_byte_for_byte_with_its_type() {
    let app = App::start().await;
    let (status, headers, json) = app
        .put_file("p", "SKILLS-14", &named("dot.png"), Some("image/png"), PNG)
        .await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    assert_eq!(json["path"], "/p/SKILLS-14/files/dot-1.png");
    assert_eq!(
        json["url"],
        "http://127.0.0.1:8801/p/SKILLS-14/files/dot-1.png"
    );
    assert_eq!(headers[header::LOCATION], json["url"].as_str().unwrap());
    assert_eq!(json["project"], "p");
    assert_eq!(json["key"], "SKILLS-14");
    assert_eq!(json["name"], "dot");
    assert_eq!(json["ext"], "png");
    assert_eq!(json["n"], "1");
    assert_eq!(json["type"], "image/png");
    assert_eq!(json["size"], PNG.len());
    assert_eq!(json["role"], "software-engineer");
    assert_eq!(json["workspace"], "workspace:1");
    assert!(json["createdAt"].as_str().unwrap().ends_with('Z'), "{json}");
    // `shasum -a 256` の値（16 進の小文字）。
    assert_eq!(
        json["sha256"],
        "4ff6ab670a58c14270e034e2090d9a432caa263a14e0a25785386b0c12f880b5"
    );

    let (status, headers, body) = app.get("/p/SKILLS-14/files/dot-1.png").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, PNG);
    assert_eq!(headers[header::CONTENT_TYPE], "image/png");
    assert_eq!(headers[header::CONTENT_SECURITY_POLICY], "sandbox");
    assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    assert_eq!(
        headers[header::CACHE_CONTROL],
        "max-age=31536000, immutable"
    );
    assert_eq!(
        headers
            .get_all(header::CONTENT_SECURITY_POLICY)
            .iter()
            .count(),
        1
    );

    // 字の file は、表の Content-Type（charset 付き）で返る。送った header の大小と引数は問わない。
    let text = "一行目\nsecond\n".as_bytes();
    app.place_file(
        "p",
        "SKILLS-14",
        &named("notes.md"),
        "Text/Markdown; charset=UTF-8",
        text,
    )
    .await;
    let (_, headers, body) = app.get("/p/SKILLS-14/files/notes-1.md").await;
    assert_eq!(body, text);
    assert_eq!(
        headers[header::CONTENT_TYPE],
        "text/markdown; charset=utf-8"
    );

    for missing in [
        "/p/SKILLS-14/files/dot-2.png",
        "/p/SKILLS-14/files/dot-01.png",
        "/p/SKILLS-14/files/dot-0.png",
        "/p/SKILLS-14/files/dot-1.jpg",
        "/p/SKILLS-14/files/dot.png",
        "/p/SKILLS-14/files/dot-1.svg",
        "/p/SKILLS-14/files/dot-1.PNG",
        "/p/SKILLS-14/files/Dot-1.png",
        "/p/SKILLS-14/files/dot-1.png.png",
        "/p/SKILLS-14/files/dot-99999999999999999999.png",
        "/p/other/files/dot-1.png",
        "/p/../files/dot-1.png",
    ] {
        let (status, headers, body) = app.get(missing).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{missing}");
        let message = refused(&headers, &body, "NotFound");
        assert!(message.contains("無い"), "{missing}: {message}");
    }
}

/// 答えは file の記録だけ: 中身は入らず、送り手の欄は付随する id を書いた時だけ。
#[tokio::test]
async fn the_answer_is_the_file_record_without_the_body() {
    let app = App::start().await;
    let (status, _, json) = app
        .put_file("p", "k", "role=tpm&name=a.png", Some("image/png"), PNG)
        .await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    assert_eq!(
        keys(&json),
        [
            "createdAt",
            "ext",
            "key",
            "n",
            "name",
            "path",
            "project",
            "role",
            "sha256",
            "size",
            "type",
            "url",
            "workspace"
        ]
    );
    // workspace を書かなければ空。
    assert_eq!(json["workspace"], "");
    // 置いた時刻は、残す字（UTC・ミリ秒まで・24 字）。
    let stored: String = scalar(&app, "select created_at from files").await;
    assert_eq!(stored.len(), 24, "{stored}");
}

#[tokio::test]
async fn files_are_numbered_per_name_and_a_removed_number_is_never_used_again() {
    let app = App::start().await;
    let q = named("dot.png");
    assert_eq!(
        app.place_file("p", "SKILLS-14", &q, "image/png", PNG).await,
        "1"
    );
    assert_eq!(
        app.place_file("p", "SKILLS-14", &q, "image/png", PNG).await,
        "2"
    );
    // 名か拡張子か Key か project が違えば、別に数える。
    assert_eq!(
        app.place_file(
            "p",
            "SKILLS-14",
            &named("dot.pdf"),
            "application/pdf",
            b"%PDF-1.7"
        )
        .await,
        "1"
    );
    assert_eq!(
        app.place_file("p", "SKILLS-14", &named("other.png"), "image/png", PNG)
            .await,
        "1"
    );
    assert_eq!(app.place_file("p", "t-x", &q, "image/png", PNG).await, "1");
    assert_eq!(
        app.place_file("q", "SKILLS-14", &q, "image/png", PNG).await,
        "1"
    );

    let (status, _, json) = app
        .remove_files("project=p&key=SKILLS-14&name=dot.png&n=2")
        .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json, json!({ "removed": ["/p/SKILLS-14/files/dot-2.png"] }));
    let (status, headers, body) = app.get("/p/SKILLS-14/files/dot-2.png").await;
    assert_eq!(status, StatusCode::GONE);
    let message = refused(&headers, &body, "Gone");
    assert!(message.starts_with("その file は消した（"), "{message}");
    // 消した時刻は、残す字（UTC・ミリ秒まで・24 字）で、訳に出る。
    let removed_at: String = scalar(&app, "select removed_at from files_removed").await;
    assert_eq!(removed_at.len(), 24, "{removed_at}");
    assert!(message.contains(&removed_at), "{message}");
    assert_eq!(
        app.get("/p/SKILLS-14/files/dot-1.png").await.0,
        StatusCode::OK
    );
    // 一番大きい番号を消しても、次は 3。
    assert_eq!(
        app.place_file("p", "SKILLS-14", &q, "image/png", PNG).await,
        "3"
    );
    // 消した番号を名指しても入らない（409。何も替えない）。
    let (status, headers, json) = app
        .put_file(
            "p",
            "SKILLS-14",
            &format!("{q}&n=2"),
            Some("image/png"),
            PNG,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{json}");
    assert_eq!(error_type(&headers), "Conflict");
    assert_eq!(
        app.get("/p/SKILLS-14/files/dot-2.png").await.0,
        StatusCode::GONE
    );
    // 2 度目は消す物が無い。
    let (_, _, json) = app
        .remove_files("project=p&key=SKILLS-14&name=dot.png&n=2")
        .await;
    assert_eq!(json, json!({ "removed": [] }));
}

#[tokio::test]
async fn an_imported_file_keeps_its_number_and_time_and_a_taken_number_is_a_conflict() {
    let app = App::start().await;
    let q = "role=tpm&name=dot.png&n=3&createdAt=2026-11-20T01%3A02%3A03Z";
    let (status, _, json) = app.put_file("p", "k", q, Some("image/png"), PNG).await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    assert_eq!(json["n"], "3");
    assert_eq!(json["createdAt"], "2026-11-20T01:02:03Z");
    let stored: String = scalar(&app, "select created_at from files").await;
    assert_eq!(stored, "2026-11-20T01:02:03.000Z");
    // 種類の検査（415）は、番号の検査（409）より前。
    let (status, headers, json) = app
        .put_file("p", "k", q, Some("text/plain"), b"x".to_vec())
        .await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE, "{json}");
    assert_eq!(error_type(&headers), "UnsupportedMediaType");
    // 置いた番号は 409。何も替えない。
    let (status, headers, json) = app.put_file("p", "k", q, Some("image/png"), PNG).await;
    assert_eq!(status, StatusCode::CONFLICT, "{json}");
    assert_eq!(error_type(&headers), "Conflict");
    assert!(json["message"].as_str().unwrap().contains("番号"), "{json}");
    assert_eq!(app.count("files").await, 1);
    // 番号を任せると、取り込んだ番号の次になる。
    assert_eq!(
        app.place_file("p", "k", "role=tpm&name=dot.png", "image/png", PNG)
            .await,
        "4"
    );
}

#[tokio::test]
async fn bad_file_arguments_are_refused() {
    let app = App::start().await;
    let long = "a".repeat(256);
    let stem_65 = "a".repeat(65);
    for (project, key, query) in [
        // project・key: 英数字と . _ - で 255 字まで。. と .. と api（project）は駄目。
        ("api", "k", "role=tpm&name=a.png".to_owned()),
        (".", "k", "role=tpm&name=a.png".to_owned()),
        ("..", "k", "role=tpm&name=a.png".to_owned()),
        ("a%20b", "k", "role=tpm&name=a.png".to_owned()),
        (long.as_str(), "k", "role=tpm&name=a.png".to_owned()),
        ("p", "..", "role=tpm&name=a.png".to_owned()),
        ("p", ".", "role=tpm&name=a.png".to_owned()),
        // name: <語>.<拡張子>。語は英小文字か数字で始まり、英小文字・数字・- _ で 64 字まで。. は 1 つ。拡張子は英数字で 16 字まで。
        ("p", "k", "role=tpm".to_owned()),
        ("p", "k", "role=tpm&name=a".to_owned()),
        ("p", "k", "role=tpm&name=A.png".to_owned()),
        ("p", "k", "role=tpm&name=-a.png".to_owned()),
        ("p", "k", "role=tpm&name=_a.png".to_owned()),
        ("p", "k", "role=tpm&name=a.b.png".to_owned()),
        ("p", "k", "role=tpm&name=a-1.png.".to_owned()),
        ("p", "k", "role=tpm&name=.png".to_owned()),
        ("p", "k", "role=tpm&name=a.".to_owned()),
        ("p", "k", "role=tpm&name=a.p%2Fg".to_owned()),
        ("p", "k", format!("role=tpm&name={stem_65}.png")),
        ("p", "k", "role=tpm&name=a.abcdefghijklmnopq".to_owned()),
        // role: 英小文字で始まり、英小文字・数字・- で 32 字まで。一覧の UI のほかは要る。
        ("p", "k", "name=a.png".to_owned()),
        ("p", "k", "role=T&name=a.png".to_owned()),
        ("p", "k", "role=&name=a.png".to_owned()),
        // workspace: 英数字と : . _ - で 64 字まで。
        ("p", "k", "role=tpm&name=a.png&workspace=a%20b".to_owned()),
        // n: 頭に 0 の無い 1 以上の数の字。
        ("p", "k", "role=tpm&name=a.png&n=0".to_owned()),
        ("p", "k", "role=tpm&name=a.png&n=01".to_owned()),
        ("p", "k", "role=tpm&name=a.png&n=abc".to_owned()),
        // createdAt: UTC の RFC 3339 で、ミリ秒まで。
        (
            "p",
            "k",
            "role=tpm&name=a.png&createdAt=yesterday".to_owned(),
        ),
        (
            "p",
            "k",
            "role=tpm&name=a.png&createdAt=2026-11-20T10%3A02%3A03%2B09%3A00".to_owned(),
        ),
        (
            "p",
            "k",
            "role=tpm&name=a.png&createdAt=2026-11-20T01%3A02%3A03.1234Z".to_owned(),
        ),
        // 送り手の付随する id は、両方書くか、両方書かない。
        ("p", "k", "role=tpm&name=a.png&actorIssuer=tpm".to_owned()),
        (
            "p",
            "k",
            "role=tpm&name=a.png&actorRef=claude.session%3Aa".to_owned(),
        ),
    ] {
        let (status, headers, json) = app
            .put_file(project, key, &query, Some("image/png"), PNG)
            .await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "{project} {key} {query}: {json}"
        );
        assert!(
            matches!(
                error_type(&headers),
                "ValidationException" | "SerializationException"
            ),
            "{project} {key} {query}: {headers:?}"
        );
    }
    // 空の中身と、Content-Type の無い要求は 400（生成した層が断る。欄の path は /body・/contentType）。
    let (status, headers, json) = app
        .put_file(
            "p",
            "k",
            "role=tpm&name=a.png",
            Some("image/png"),
            b"".to_vec(),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
    assert_eq!(error_type(&headers), "ValidationException");
    assert!(
        json["message"].as_str().unwrap().contains("/body"),
        "{json}"
    );
    let (status, headers, json) = app
        .put_file("p", "k", "role=tpm&name=a.png", None, PNG)
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
    assert_eq!(error_type(&headers), "ValidationException");
    assert!(
        json["message"].as_str().unwrap().contains("/contentType"),
        "{json}"
    );
    // 一覧の sort の形の違いも 400。
    let (status, headers, _) = app.json("/api/files?sort=name").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_type(&headers), "ValidationException");
    assert_eq!(app.count("files").await, 0);
    // 語の 64 字と、拡張子の 16 字までの形は、400 にならない（受けない拡張子は 415）。
    let stem_64 = "a".repeat(64);
    app.place_file(
        "p",
        "k",
        &format!("role=tpm&name={stem_64}.png"),
        "image/png",
        PNG,
    )
    .await;
    let (status, _, json) = app
        .put_file(
            "p",
            "k",
            "role=tpm&name=a.abcdefghijklmnop",
            Some("image/png"),
            PNG,
        )
        .await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE, "{json}");
}

#[tokio::test]
async fn the_messages_say_what_is_wrong() {
    let app = App::start().await;
    for (project, key, query, content_type, body, status, words) in [
        (
            "api",
            "k",
            "role=tpm&name=a.png",
            "image/png",
            PNG,
            StatusCode::BAD_REQUEST,
            "api",
        ),
        (
            "p",
            "..",
            "role=tpm&name=a.png",
            "image/png",
            PNG,
            StatusCode::BAD_REQUEST,
            "key",
        ),
        (
            "p",
            "k",
            "name=a.png",
            "image/png",
            PNG,
            StatusCode::BAD_REQUEST,
            "role",
        ),
        (
            "p",
            "k",
            "role=tpm&name=A.png",
            "image/png",
            PNG,
            StatusCode::BAD_REQUEST,
            "/name",
        ),
        (
            "p",
            "k",
            "role=tpm&name=a.png&createdAt=2026-11-20T01%3A02%3A03.1234Z",
            "image/png",
            PNG,
            StatusCode::BAD_REQUEST,
            "ミリ秒",
        ),
        (
            "p",
            "k",
            "role=tpm&name=a.png&actorIssuer=tpm",
            "image/png",
            PNG,
            StatusCode::BAD_REQUEST,
            "両方",
        ),
        (
            "p",
            "k",
            "role=tpm&name=a.svg",
            "image/svg+xml",
            b"<svg/>",
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "png・jpg・jpeg・gif・webp・pdf・txt・log・md・csv・json",
        ),
        (
            "p",
            "k",
            "role=tpm&name=a.png",
            "image/jpeg",
            PNG,
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            ".png の Content-Type は image/png",
        ),
        (
            "p",
            "k",
            "role=tpm&name=a.png",
            "image/png",
            b"%PDF-1.7",
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "中身が .png の形でない",
        ),
    ] {
        let (got, headers, json) = app
            .put_file(project, key, query, Some(content_type), body.to_vec())
            .await;
        assert_eq!(got, status, "{query}: {json}");
        let kind = if status == StatusCode::BAD_REQUEST {
            "ValidationException"
        } else {
            "UnsupportedMediaType"
        };
        assert_eq!(error_type(&headers), kind, "{query}");
        assert!(
            json["message"].as_str().unwrap_or("").contains(words),
            "{query}: {json}"
        );
    }
    assert_eq!(app.count("files").await, 0);
    assert_eq!(app.count("senders").await, 0);
}

#[tokio::test]
async fn a_type_or_contents_that_do_not_match_the_extension_are_refused() {
    let app = App::start().await;
    for (name, content_type, bytes) in [
        // 中身が拡張子と合わない。
        ("a.png", "image/png", &b"%PDF-1.7 ..."[..]),
        ("a.jpg", "image/jpeg", PNG),
        ("a.gif", "image/gif", b"GIF90a"),
        ("a.webp", "image/webp", b"RIFF\0\0\0\0WAVEfmt "),
        ("a.webp", "image/webp", b"RIFF"),
        ("a.pdf", "application/pdf", PNG),
        ("a.txt", "text/plain", b"\xff\xfe\x00a"),
        ("a.txt", "text/plain", b"\xff\xfe"),
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
        ("a.png", "image/png-x", PNG),
    ] {
        let (status, headers, json) = app
            .put_file("p", "k", &named(name), Some(content_type), bytes.to_vec())
            .await;
        assert_eq!(
            status,
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "{name} {content_type}: {json}"
        );
        assert_eq!(error_type(&headers), "UnsupportedMediaType", "{name}");
    }
    assert_eq!(app.count("files").await, 0);
    // 合っていれば通る（Content-Type は ; の前を大小を問わずに比べる）。
    for (name, content_type, bytes) in [
        ("a.png", "image/png; x=y", PNG),
        ("a.jpg", "image/jpeg", &b"\xff\xd8\xff\xe0"[..]),
        ("a.jpeg", "IMAGE/JPEG", b"\xff\xd8\xff\xdb"),
        ("a.gif", "image/gif", b"GIF89a"),
        ("a.gif", "image/gif", b"GIF87a"),
        ("a.webp", "image/webp", b"RIFF\0\0\0\0WEBPVP8 "),
        ("a.pdf", "application/pdf", b"%PDF-1.7"),
        ("a.txt", "text/plain", "字の file\n".as_bytes()),
        ("a.log", "text/plain; charset=utf-8", "ログ".as_bytes()),
        ("a.md", "text/markdown", b"# x"),
        ("a.csv", "text/csv", b"a,b\n1,2\n"),
        ("a.json", "application/json", b"{}"),
    ] {
        app.place_file("p", "k", &named(name), content_type, bytes)
            .await;
    }
    // 返す時の Content-Type は表の値（jpg と jpeg は同じ）。
    for (path, want) in [
        ("/p/k/files/a-1.jpg", "image/jpeg"),
        ("/p/k/files/a-1.jpeg", "image/jpeg"),
        ("/p/k/files/a-1.webp", "image/webp"),
        ("/p/k/files/a-1.pdf", "application/pdf"),
        ("/p/k/files/a-1.txt", "text/plain; charset=utf-8"),
        ("/p/k/files/a-1.log", "text/plain; charset=utf-8"),
        ("/p/k/files/a-1.csv", "text/csv; charset=utf-8"),
        ("/p/k/files/a-1.json", "application/json; charset=utf-8"),
        ("/p/k/files/a-2.gif", "image/gif"),
    ] {
        let (status, headers, _) = app.get(path).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        assert_eq!(headers[header::CONTENT_TYPE], want, "{path}");
    }
}

/// 4 つを、1 日ずつ違う時刻で置く（今の tests/api.rs の file_filters_combine_and_sort_by_time と同じ見本）。
async fn four(app: &App) {
    for (project, key, query, day) in [
        ("p", "A-1", "name=screen.png&role=tpm&workspace=w:1", 1),
        (
            "p",
            "A-1",
            "name=shot.png&role=software-engineer&workspace=w:2",
            2,
        ),
        (
            "p",
            "B-2",
            "name=screen.png&role=software-engineer&workspace=w:3",
            3,
        ),
        (
            "q",
            "A-1",
            "name=screen.png&role=devops-engineer&workspace=w:2",
            4,
        ),
    ] {
        app.place_file(
            project,
            key,
            &format!("{query}&{}", at(day)),
            "image/png",
            PNG,
        )
        .await;
    }
}

const ALL: [&str; 4] = [
    "/q/A-1/files/screen-1.png",
    "/p/B-2/files/screen-1.png",
    "/p/A-1/files/shot-1.png",
    "/p/A-1/files/screen-1.png",
];

#[tokio::test]
async fn file_filters_combine_and_sort_by_time() {
    let app = App::start().await;
    four(&app).await;
    assert_eq!(app.file_paths("").await, ALL);
    assert_eq!(app.file_paths("sort=-createdAt").await, ALL);
    let mut oldest = ALL;
    oldest.reverse();
    assert_eq!(app.file_paths("sort=createdAt").await, oldest);
    assert_eq!(
        app.file_paths("role=software-engineer").await,
        ["/p/B-2/files/screen-1.png", "/p/A-1/files/shot-1.png"]
    );
    assert_eq!(
        app.file_paths("workspace=w:2&project=p").await,
        ["/p/A-1/files/shot-1.png"]
    );
    assert_eq!(
        app.file_paths("key=A-1&project=p").await,
        ["/p/A-1/files/shot-1.png", "/p/A-1/files/screen-1.png"]
    );
    // q は名（<語>.<拡張子>）に含まれる字（大小を問わない。前後の空白は落とし、空なら絞らない）。
    assert_eq!(app.file_paths("q=shot").await, ["/p/A-1/files/shot-1.png"]);
    assert_eq!(app.file_paths("q=SCREEN.PNG").await.len(), 3);
    assert_eq!(app.file_paths("q=%20shot%20").await.len(), 1);
    assert_eq!(app.file_paths("q=%20").await.len(), 4);
    assert!(app.file_paths("q=pdf").await.is_empty());
    assert_eq!(app.file_paths("maxResults=2").await, ALL[..2]);
    // 空の workspace も条件（workspace を書かずに置いた file）。
    assert!(app.file_paths("workspace=").await.is_empty());
    // ページの一覧には出ない。
    assert!(app.paths("").await.is_empty());

    // 一覧の記録には、中身も url も無い（送り手の欄も、書いていなければ無い）。
    let json = list(&app, "key=B-2").await;
    let file = &json["files"][0];
    assert_eq!(file["size"], PNG.len());
    assert_eq!(file["type"], "image/png");
    assert_eq!(
        keys(file),
        [
            "createdAt",
            "ext",
            "key",
            "n",
            "name",
            "path",
            "project",
            "role",
            "sha256",
            "size",
            "type",
            "workspace"
        ]
    );
    assert!(json.get("nextToken").is_none(), "{json}");
}

/// 置いた答えと一覧の行は、url のほかは同じ欄・同じ値。
#[tokio::test]
async fn the_listed_record_is_the_placed_one_without_the_url() {
    let app = App::start().await;
    let (_, _, mut placed) = app
        .put_file("p", "SKILLS-14", &named("dot.png"), Some("image/png"), PNG)
        .await;
    let json = list(&app, "project=p").await;
    placed.as_object_mut().unwrap().remove("url");
    assert_eq!(json["files"][0], placed);
}

/// 頁は createdAt と行の順で続ける。同じ時刻の行が頁の境に並んでも、抜けず重ならない。
#[tokio::test]
async fn file_pages_continue_by_the_next_token_without_gaps_or_overlaps() {
    let app = App::start().await;
    for i in 0..7 {
        let name = ["a.png", "b.png", "c.png"][i % 3];
        app.place_file(
            "p",
            "k",
            &format!("role=tpm&name={name}&{}", at(2)),
            "image/png",
            PNG,
        )
        .await;
    }
    for sort in ["sort=-createdAt", "sort=createdAt"] {
        let whole = app.file_paths(&format!("{sort}&maxResults=1000")).await;
        assert_eq!(whole.len(), 7);
        for max in [1, 2, 3] {
            let mut walked = Vec::new();
            let mut token: Option<String> = None;
            loop {
                let mut q = format!("{sort}&maxResults={max}");
                if let Some(t) = &token {
                    q.push_str(&format!("&nextToken={t}"));
                }
                let json = list(&app, &q).await;
                let paths = file_paths(&json);
                assert!(paths.len() <= max, "{q}: {json}");
                walked.extend(paths);
                match json["nextToken"].as_str() {
                    Some(t) => token = Some(t.to_owned()),
                    None => break,
                }
                assert!(walked.len() <= 7, "終わらない: {walked:?}");
            }
            assert_eq!(walked, whole, "{sort} maxResults={max}");
        }
    }
    // 同じ時刻の中は置いた順（新しい順なら後に置いた物が先）。
    assert_eq!(
        app.file_paths("maxResults=3").await,
        [
            "/p/k/files/a-3.png",
            "/p/k/files/c-2.png",
            "/p/k/files/b-2.png"
        ]
    );
}

/// nextToken は、同じ sort の file の一覧の答えの物だけ。崩れた物・ほかの sort の物・ページの検索の物は 400。
#[tokio::test]
async fn a_token_from_another_query_or_a_broken_one_is_refused() {
    let app = App::start().await;
    four(&app).await;
    let newest = list(&app, "maxResults=1").await;
    let token = newest["nextToken"].as_str().unwrap().to_owned();
    assert_eq!(
        app.file_paths(&format!("maxResults=1&nextToken={token}"))
            .await,
        ALL[1..2]
    );
    let hex = |s: &str| -> String { s.bytes().map(|b| format!("{b:02x}")).collect() };
    let pages = hex("pages\n-createdAt\n2026-11-01T00:00:00.000Z\n1");
    let short = hex("files\n-createdAt\n2026-11-01T00:00:00.000Z");
    let bad_id = hex("files\n-createdAt\n2026-11-01T00:00:00.000Z\nx");
    for query in [
        format!("sort=createdAt&nextToken={token}"),
        format!("nextToken={pages}"),
        format!("nextToken={short}"),
        format!("nextToken={bad_id}"),
        "nextToken=zz".to_owned(),
    ] {
        let (status, headers, json) = app.json(&format!("/api/files?{query}")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}: {json}");
        assert_eq!(error_type(&headers), "ValidationException", "{query}");
        assert!(
            json["message"].as_str().unwrap_or("").contains("nextToken"),
            "{query}: {json}"
        );
    }
    // file の一覧の印は、ページの一覧にも渡せない。
    let (status, _, _) = app.json(&format!("/api/pages?nextToken={token}")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn files_of_a_key_or_a_whole_project_are_removed_at_once() {
    let app = App::start().await;
    app.place_file("p", "SKILLS-14", &named("a.png"), "image/png", PNG)
        .await;
    app.place_file("p", "SKILLS-14", &named("b.txt"), "text/plain", b"b")
        .await;
    app.place_file("p", "t-x", "name=a.png&role=tpm", "image/png", PNG)
        .await;
    app.place_file("q", "t-x", "name=a.png&role=tpm", "image/png", PNG)
        .await;
    // ページは消えない。
    app.place("p", "SKILLS-14", "plan", SE, "<p>1</p>").await;

    let (status, _, json) = app.remove_files("project=p&key=SKILLS-14").await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(
        json,
        json!({ "removed": ["/p/SKILLS-14/files/a-1.png", "/p/SKILLS-14/files/b-1.txt"] })
    );
    assert_eq!(app.file_paths("project=p").await, ["/p/t-x/files/a-1.png"]);
    assert_eq!(app.paths("").await, ["/p/SKILLS-14/plan-1.html"]);

    // 名だけでも、project の中のその名をみな消す。
    let (_, _, json) = app.remove_files("project=p&name=a.png").await;
    assert_eq!(json, json!({ "removed": ["/p/t-x/files/a-1.png"] }));
    assert_eq!(app.file_paths("").await, ["/q/t-x/files/a-1.png"]);
    assert_eq!(
        app.place_file("p", "SKILLS-14", &named("a.png"), "image/png", PNG)
            .await,
        "2"
    );
    assert_eq!(app.count("files_removed").await, 3);
    assert_eq!(app.count("pages").await, 1);
    assert_eq!(app.count("removed").await, 0);
}

#[tokio::test]
async fn removing_files_needs_a_project_and_a_full_name_for_one_file() {
    let app = App::start().await;
    app.place_file("p", "SKILLS-14", &named("a.png"), "image/png", PNG)
        .await;
    for (query, message) in [
        ("key=SKILLS-14", None),
        ("project=..", Some("project")),
        ("project=.", Some("project")),
        ("project=p&key=..", Some("key")),
        (
            "project=p&name=a.png&n=1",
            Some("n を書くときは key と name も書く"),
        ),
        (
            "project=p&key=SKILLS-14&n=1",
            Some("n を書くときは key と name も書く"),
        ),
        ("project=p&key=SKILLS-14&name=a.png&n=0", Some("/n")),
        ("project=p&name=A.png", Some("/name")),
        ("project=p&name=a", Some("/name")),
    ] {
        let (status, headers, json) = app.remove_files(query).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}: {json}");
        assert_eq!(error_type(&headers), "ValidationException", "{query}");
        if let Some(message) = message {
            assert!(
                json["message"].as_str().unwrap_or("").contains(message),
                "{query}: {json}"
            );
        }
    }
    assert_eq!(app.file_paths("").await, ["/p/SKILLS-14/files/a-1.png"]);
}

/// 書く接続が空かなければ 503（置く・消す）、読む接続がみな塞がっていれば 503（探す・中身）。DB の誤りは 500
/// （答えは決まった字で、中の誤りの字を出さない。何も残さない・消さない）。
#[tokio::test]
async fn a_db_error_is_internal_and_a_busy_db_is_unavailable() {
    let app = App::start().await;
    app.place_file("p", "k", "role=tpm&name=a.png", "image/png", PNG)
        .await;
    let held = app.db.write().acquire().await.expect("書く接続");
    let (status, headers, json) = app
        .put_file("p", "k", "role=tpm&name=a.png", Some("image/png"), PNG)
        .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{json}");
    assert_eq!(error_type(&headers), "Unavailable");
    let (status, headers, json) = app.remove_files("project=p").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{json}");
    assert_eq!(error_type(&headers), "Unavailable");
    drop(held);
    let mut held = Vec::new();
    for _ in 0..pagetray_server::db::READS {
        held.push(app.db.read().acquire().await.expect("読む接続"));
    }
    let (status, headers, json) = app.json("/api/files").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{json}");
    assert_eq!(error_type(&headers), "Unavailable");
    let (status, headers, body) = app.get("/p/k/files/a-1.png").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    refused(&headers, &body, "Unavailable");
    drop(held);
    assert_eq!(app.count("files").await, 1);

    // 消した番号の表が無い: 置く（番号を決められない）・消す・無い file の中身は 500。
    app.execute("drop table files_removed").await;
    for (status, headers, json) in [
        app.put_file("p", "k", "role=tpm&name=a.png", Some("image/png"), PNG)
            .await,
        app.remove_files("project=p").await,
    ] {
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{json}");
        assert_eq!(error_type(&headers), "InternalError");
        let message = json["message"].as_str().unwrap_or("");
        assert!(!message.contains("files_removed"), "{message}");
    }
    let (status, headers, body) = app.get("/p/k/files/a-9.png").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let message = refused(&headers, &body, "InternalError");
    assert!(!message.contains("files_removed"), "{message}");
    assert_eq!(app.count("files").await, 1);
    // 置いてある file の中身は読める。
    assert_eq!(app.get("/p/k/files/a-1.png").await.0, StatusCode::OK);

    // file の表が無い: 探すも 500。
    app.execute("drop table files").await;
    let (status, headers, json) = app.json("/api/files").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{json}");
    assert_eq!(error_type(&headers), "InternalError");
}

#[tokio::test]
async fn files_survive_a_restart() {
    let mut app = App::start().await;
    app.place_file("p", "SKILLS-14", &named("a.png"), "image/png", PNG)
        .await;
    app.place_file("p", "SKILLS-14", &named("a.png"), "image/png", PNG)
        .await;
    app.remove_files("project=p&key=SKILLS-14&name=a.png&n=1")
        .await;
    app.reopen().await;
    let (status, _, body) = app.get("/p/SKILLS-14/files/a-2.png").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, PNG);
    assert_eq!(
        app.get("/p/SKILLS-14/files/a-1.png").await.0,
        StatusCode::GONE
    );
    assert_eq!(app.file_paths("").await, ["/p/SKILLS-14/files/a-2.png"]);
    assert_eq!(
        app.place_file("p", "SKILLS-14", &named("a.png"), "image/png", PNG)
            .await,
        "3"
    );
}

/// file の道を足しても、ページの URL と答えは前のまま。ページを消しても file は残り、file を消してもページは残る。
#[tokio::test]
async fn pages_and_files_live_side_by_side() {
    let app = App::start().await;
    let page = html("図のあるページ", "<img src=\"files/dot-1.png\">");
    app.place("p", "SKILLS-14", "result", SE, &page).await;
    app.place_file("p", "SKILLS-14", &named("dot.png"), "image/png", PNG)
        .await;
    // 種類 files のページ（`/p/SKILLS-14/files-1.html`）とぶつからない。
    app.place("p", "SKILLS-14", "files", SE, "<p>files という種類</p>")
        .await;
    assert_eq!(
        app.get("/p/SKILLS-14/result-1.html").await.2,
        page.as_bytes()
    );
    assert_eq!(
        app.get("/p/SKILLS-14/files-1.html").await.2,
        "<p>files という種類</p>".as_bytes()
    );
    assert_eq!(app.get("/p/SKILLS-14/files/dot-1.png").await.2, PNG);
    assert_eq!(
        app.paths("").await,
        ["/p/SKILLS-14/files-1.html", "/p/SKILLS-14/result-1.html"]
    );
    assert_eq!(app.file_paths("").await, ["/p/SKILLS-14/files/dot-1.png"]);
    let (_, headers, _) = app.get("/p/SKILLS-14/result-1.html").await;
    assert_eq!(
        headers[header::CONTENT_SECURITY_POLICY],
        "sandbox allow-scripts"
    );
    let (_, headers, _) = app.get("/p/SKILLS-14/files/dot-1.png").await;
    assert_eq!(headers[header::CONTENT_SECURITY_POLICY], "sandbox");
    assert_eq!(headers[header::REFERRER_POLICY], "no-referrer");
    let (status, _, _) = app.remove("project=p&key=SKILLS-14&kind=result").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        app.get("/p/SKILLS-14/files/dot-1.png").await.0,
        StatusCode::OK
    );
    let (status, _, _) = app.remove_files("project=p&key=SKILLS-14").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(app.get("/p/SKILLS-14/files-1.html").await.0, StatusCode::OK);
    assert_eq!(
        app.get("/p/SKILLS-14/files/dot-1.png").await.0,
        StatusCode::GONE
    );
}

/// `/api/` の下の file の中身の形の道は、本文の道に入らず、生成した層が答える。
#[tokio::test]
async fn paths_under_api_never_reach_the_file_route() {
    let app = App::start().await;
    let (status, headers, body) = app.get("/api/k/files/a-1.png").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error_type(&headers), "UnknownOperationException");
    assert_eq!(body, b"{}");
}

/// browser から来た書く要求（`Origin` 付き）の、通す・断るの組み合わせ（docs/src/next.md の「守りの層」）。
/// 通すのは、一覧の UI の form が file を上げる 1 つの形（CreateFile の道・同じ Host と port の http の Origin・
/// Sec-Fetch-Site が無いか same-origin）だけ。
#[tokio::test]
async fn only_the_list_ui_may_put_files_from_a_browser() {
    let app = App::start().await;
    app.place("p", "SKILLS-14", "plan", SE, "<p>x</p>").await;
    app.place_file("p", "SKILLS-14", &named("old.png"), "image/png", PNG)
        .await;
    let ui = format!("http://{HOST}");
    let put = files_uri("p", "SKILLS-14", &named("a.png"));
    let cases: Vec<(&str, String, &str, Option<&str>, StatusCode)> = vec![
        // 一覧の UI の form（同じ Host・port の Origin）。
        ("POST", put.clone(), &ui, None, StatusCode::CREATED),
        (
            "POST",
            put.clone(),
            &ui,
            Some("same-origin"),
            StatusCode::CREATED,
        ),
        // 本文の枠の中のページ（sandbox なので null）・ほかの port・ほかの名・https。
        ("POST", put.clone(), "null", None, StatusCode::FORBIDDEN),
        (
            "POST",
            put.clone(),
            "http://127.0.0.1:8800",
            None,
            StatusCode::FORBIDDEN,
        ),
        (
            "POST",
            put.clone(),
            "http://localhost:8801",
            None,
            StatusCode::FORBIDDEN,
        ),
        (
            "POST",
            put.clone(),
            "https://127.0.0.1:8801",
            None,
            StatusCode::FORBIDDEN,
        ),
        (
            "POST",
            put.clone(),
            "http://127.0.0.1:8801/",
            None,
            StatusCode::FORBIDDEN,
        ),
        (
            "POST",
            put.clone(),
            "https://evil.example",
            None,
            StatusCode::FORBIDDEN,
        ),
        // 同じ Origin でも、browser がほかの site からと言う物。
        (
            "POST",
            put.clone(),
            &ui,
            Some("cross-site"),
            StatusCode::FORBIDDEN,
        ),
        (
            "POST",
            put.clone(),
            &ui,
            Some("same-site"),
            StatusCode::FORBIDDEN,
        ),
        (
            "POST",
            put.clone(),
            &ui,
            Some("none"),
            StatusCode::FORBIDDEN,
        ),
        // 同じ Origin でも、ほかの道（ページを置く・種類 files のページ・道の後ろの足し・消す・返事）。
        (
            "POST",
            common::pages_uri("p", "SKILLS-14", "plan", SE),
            &ui,
            None,
            StatusCode::FORBIDDEN,
        ),
        (
            "POST",
            common::pages_uri("p", "SKILLS-14", "files", SE),
            &ui,
            None,
            StatusCode::FORBIDDEN,
        ),
        (
            "POST",
            format!("/api/projects/p/keys/SKILLS-14/files/x?{}", named("a.png")),
            &ui,
            None,
            StatusCode::FORBIDDEN,
        ),
        (
            "POST",
            format!("/api/projects/p/keys/SKILLS-14/files/?{}", named("a.png")),
            &ui,
            None,
            StatusCode::FORBIDDEN,
        ),
        ("PUT", put.clone(), &ui, None, StatusCode::FORBIDDEN),
        (
            "DELETE",
            "/api/files?project=p".to_owned(),
            &ui,
            None,
            StatusCode::FORBIDDEN,
        ),
        (
            "DELETE",
            "/api/pages?project=p".to_owned(),
            &ui,
            None,
            StatusCode::FORBIDDEN,
        ),
        (
            "DELETE",
            "/api/files?project=p".to_owned(),
            "null",
            None,
            StatusCode::FORBIDDEN,
        ),
    ];
    for (method, uri, origin, site, expected) in cases {
        let mut request = Request::builder()
            .method(method)
            .uri(&uri)
            .header(header::HOST, HOST)
            .header(header::ORIGIN, origin);
        if let Some(site) = site {
            request = request.header("sec-fetch-site", site);
        }
        let request = if uri.contains("/kinds/") {
            request
                .header(header::CONTENT_TYPE, "text/html")
                .body(Body::from("<p>x</p>"))
        } else {
            request
                .header(header::CONTENT_TYPE, "image/png")
                .body(Body::from(PNG))
        }
        .unwrap();
        let (status, headers, body) = app.send(request).await;
        assert_eq!(status, expected, "{method} {uri} Origin: {origin} {site:?}");
        if expected == StatusCode::FORBIDDEN {
            refused(&headers, &body, "Forbidden");
        }
    }
    // 断った物は、何も置かず、何も消していない。
    assert_eq!(app.paths("").await, ["/p/SKILLS-14/plan-1.html"]);
    assert_eq!(
        app.file_paths("").await,
        [
            "/p/SKILLS-14/files/a-2.png",
            "/p/SKILLS-14/files/a-1.png",
            "/p/SKILLS-14/files/old-1.png"
        ]
    );
}

/// 一覧の UI から上げた file は、引数に何が書いてあっても、役割 product-manager・workspace 空で記録する。
/// 番号と時刻も、置き場が決める（browser から取り込みはできない）。`Origin` の無い要求（CLI・roster）は前のまま。
#[tokio::test]
async fn files_from_the_list_ui_are_recorded_as_the_product_manager() {
    let app = App::start().await;
    let uri = files_uri(
        "p",
        "SKILLS-14",
        &format!("{}&n=7&createdAt=2020-01-01T00%3A00%3A00Z", named("a.png")),
    );
    let (status, _, json) = app
        .send_json(from_ui(
            &uri,
            HOST,
            &format!("http://{HOST}"),
            Some("same-origin"),
        ))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    assert_eq!(json["role"], "product-manager");
    assert_eq!(json["workspace"], "");
    assert_eq!(json["n"], "1");
    assert_ne!(json["createdAt"], "2020-01-01T00:00:00Z");
    let stored: String = scalar(&app, "select created_at from files").await;
    assert!(
        stored.starts_with("20") && !stored.starts_with("2020-"),
        "{stored}"
    );
    // 役割を書かなくても上げられる（UI は書かない）。ほかの Host と port でも、同じ Origin なら同じ。
    let uri = files_uri("p", "SKILLS-14", "name=b.png");
    let (status, _, json) = app
        .send_json(from_ui(
            &uri,
            "localhost:18801",
            "http://localhost:18801",
            None,
        ))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    assert_eq!(json["role"], "product-manager");
    assert_eq!(json["workspace"], "");
    assert_eq!(
        json["url"],
        "http://localhost:18801/p/SKILLS-14/files/b-1.png"
    );
    // UI から上げた物も、種類と中身は見る（415）。
    let request = Request::post(files_uri("p", "SKILLS-14", "name=c.png"))
        .header(header::HOST, HOST)
        .header(header::ORIGIN, format!("http://{HOST}"))
        .header(header::CONTENT_TYPE, "image/png")
        .body(Body::from("not a png"))
        .unwrap();
    let (status, headers, _) = app.send_json(request).await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(error_type(&headers), "UnsupportedMediaType");

    // Origin の無い要求は、引数の役割・workspace・番号・時刻のまま。役割が無ければ断る。
    let (status, _, json) = app
        .put_file(
            "p",
            "SKILLS-14",
            &format!("{}&n=7&{}", named("c.png"), at(1)),
            Some("image/png"),
            PNG,
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    assert_eq!(json["role"], "software-engineer");
    assert_eq!(json["workspace"], "workspace:1");
    assert_eq!(json["n"], "7");
    assert_eq!(json["createdAt"], "2026-11-01T00:00:00Z");
    let (status, headers, json) = app
        .put_file("p", "SKILLS-14", "name=c.png", Some("image/png"), PNG)
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
    assert_eq!(error_type(&headers), "ValidationException");
    assert!(json["message"].as_str().unwrap().contains("role"), "{json}");
    assert_eq!(app.count("files").await, 3);
}

// --- 索引 ---

/// file を探す SQL（`pagetray_server::search::file_plans()`）が、files を索引なしに端から読まない。
/// 条件の無い一覧と nextToken は時刻の索引（files_created）の順に読み、並べ替えの一時の表を作らない。
/// project（と key）は一意の索引で、送り手は files_sender で引く。
#[tokio::test]
async fn the_file_list_sql_reads_files_by_an_index() {
    let app = App::start().await;
    let plans = pagetray_server::search::file_plans();
    let names: Vec<&str> = plans.iter().map(|(name, _)| *name).collect();
    let searched = ["project", "project_key", "actor", "after", "after_oldest"];
    let ordered = ["plain", "oldest", "after", "after_oldest"];
    let others = ["role", "workspace", "q", "all"];
    for name in searched.iter().chain(&ordered).chain(&others) {
        assert!(
            names.contains(name),
            "{name} が file_plans に無い: {names:?}"
        );
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
        if searched.contains(name) {
            assert!(
                details.iter().any(|d| d.starts_with("SEARCH f USING")),
                "{name} が files を索引で引かない: {details:?}\n{sql}"
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
