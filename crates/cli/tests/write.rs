//! 書く命令（page put・page delete・page watch・file put・file delete・backup・api）を、同じ commit のサーバーに向けて打つ。

mod common;

use common::{PNG, Server, closed_url, html, run_with};
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// 試験ごとの、一時の file の path。
fn temp(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("pagetray-cli-{}-{name}", std::process::id()))
}

fn put_args<'a>(file: &'a str, kind: &'a str) -> Vec<&'a str> {
    vec![
        "page",
        "put",
        file,
        "--project",
        "p",
        "--key",
        "AI-22",
        "--kind",
        kind,
    ]
}

#[test]
fn put_prints_the_url_and_the_store_numbers_the_pages() {
    let s = Server::start();
    let file = temp("put.html");
    let body = html("計画", "置いた本文");
    std::fs::write(&file, &body).unwrap();
    let file = file.to_str().unwrap();

    let ran = s.run(&put_args(file, "plan"));
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    assert_eq!(ran.stdout, format!("{}/p/AI-22/plan-1.html\n", s.url));
    // 置き直すと、次の番号になる。前の版は残る。
    assert_eq!(
        s.run(&put_args(file, "plan")).stdout,
        format!("{}/p/AI-22/plan-2.html\n", s.url)
    );
    assert_eq!(s.run(&["page", "view", "p/AI-22/plan-1"]).stdout, body);

    // 役割の既定は human。--json は記録を出す。
    let ran = s.run(
        &[
            &put_args(file, "result")[..],
            &["--workspace", "workspace:9", "--json"],
        ]
        .concat(),
    );
    let json: serde_json::Value = serde_json::from_str(&ran.stdout).unwrap();
    assert_eq!(json["role"], "human");
    assert_eq!(json["workspace"], "workspace:9");
    assert_eq!(json["n"], 1);
    assert_eq!(json["title"], "計画");
    assert_eq!(json["url"], format!("{}/p/AI-22/result-1.html", s.url));
    std::fs::remove_file(file).unwrap();
}

#[test]
fn put_reads_stdin_and_takes_role_and_project_from_the_environment() {
    let s = Server::start();
    let env = [("PAGETRAY_PROJECT", "-srv-me-x"), ("PAGETRAY_ROLE", "tpm")];
    let ran = run_with(
        &s.url,
        &[
            "page", "put", "-", "--key", "t-x", "--kind", "brief", "--json",
        ],
        &env,
        Some(html("依頼書", "stdin から").as_bytes()),
    );
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    let json: serde_json::Value = serde_json::from_str(&ran.stdout).unwrap();
    assert_eq!(json["path"], "/-srv-me-x/t-x/brief-1.html");
    assert_eq!(json["role"], "tpm");
}

#[test]
fn put_without_a_project_or_with_a_refused_page_places_nothing() {
    let s = Server::start();
    let file = temp("refused.html");
    std::fs::write(&file, html("x", "y")).unwrap();
    let file = file.to_str().unwrap();
    // project が無ければ、使い方の誤り（今いる folder から勝手に作らない）。
    assert_eq!(
        s.run(&["page", "put", file, "--key", "AI-22", "--kind", "plan"])
            .code,
        2
    );
    // 置き場が断る（種類の字の決まり）。
    let ran = s.run(&put_args(file, "Plan"));
    assert_eq!(ran.code, 1);
    assert!(ran.stderr.contains("（400）: kind は"), "{}", ran.stderr);
    // 無い file。
    assert_eq!(s.run(&put_args("/nonexistent/x.html", "plan")).code, 2);
    assert_eq!(s.run(&["page", "list"]).stdout, "");
    std::fs::remove_file(file).unwrap();
}

/// 3 枚（plan-1・plan-2・result-1）と、別の Key の 1 枚を置く。
fn four_pages(s: &Server) {
    for kind in ["plan", "plan", "result"] {
        s.place(
            &format!("project=p&key=AI-22&kind={kind}&role=tpm"),
            &html("消す", "x"),
        );
    }
    s.place("project=p&key=AI-21&kind=plan&role=tpm", &html("残す", "y"));
}

fn count(s: &Server) -> usize {
    s.run(&["page", "list"]).stdout.lines().count()
}

#[test]
fn delete_only_lists_until_yes_is_given() {
    let s = Server::start();
    four_pages(&s);
    let url = format!("{}/p/AI-22/plan-1.html", s.url);

    let ran = s.run(&["page", "delete", &url]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    assert!(ran.stdout.contains("1 枚"), "{}", ran.stdout);
    assert!(ran.stdout.contains("まだ消していない"), "{}", ran.stdout);
    assert!(ran.stdout.contains(&url), "{}", ran.stdout);
    assert!(!ran.stdout.contains("plan-2"), "{}", ran.stdout);
    assert_eq!(count(&s), 4);

    let ran = s.run(&["page", "delete", &url, "--yes"]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    assert_eq!(ran.stdout, format!("消した（1 枚）:\n{url}\n"));
    assert_eq!(count(&s), 3);
    // 消した URL は 410。番号は 2 度使わない。
    assert!(s.run(&["page", "view", &url]).stderr.contains("（410）"));
    assert_eq!(
        s.place("project=p&key=AI-22&kind=plan&role=tpm", "<p>3</p>"),
        "/p/AI-22/plan-3.html"
    );
    // もう無い物を名指すと、消す物は無い。
    assert!(
        s.run(&["page", "delete", &url, "--yes"])
            .stdout
            .contains("消すページは無い")
    );
}

#[test]
fn delete_by_key_kind_and_project() {
    let s = Server::start();
    four_pages(&s);
    let ran = s.run(&[
        "page",
        "delete",
        "--project",
        "p",
        "--key",
        "AI-22",
        "--kind",
        "plan",
        "--yes",
    ]);
    assert!(ran.stdout.starts_with("消した（2 枚）"), "{}", ran.stdout);
    assert_eq!(count(&s), 2);
    let ran = s.run(&[
        "page",
        "delete",
        "p/AI-22/result-1",
        "p/AI-21/plan-1",
        "--yes",
    ]);
    assert!(ran.stdout.starts_with("消した（2 枚）"), "{}", ran.stdout);
    assert_eq!(count(&s), 0);

    four_pages(&s);
    assert!(
        s.run(&["page", "delete", "--project", "p"])
            .stdout
            .contains("4 枚")
    );
    assert!(
        s.run(&["page", "delete", "--project", "p", "--yes"])
            .stdout
            .starts_with("消した（4 枚）")
    );
    assert_eq!(count(&s), 0);
}

#[test]
fn delete_needs_a_target_named_on_the_command_line() {
    let s = Server::start();
    four_pages(&s);
    assert_eq!(s.run(&["page", "delete", "--yes"]).code, 2);
    // project を環境変数で渡しても、消す先にはならない。
    let ran = run_with(
        &s.url,
        &["page", "delete", "--yes"],
        &[("PAGETRAY_PROJECT", "p")],
        None,
    );
    assert_eq!(ran.code, 2, "{}", ran.stdout);
    assert_eq!(
        s.run(&["page", "delete", "--key", "AI-22", "--yes"]).code,
        2
    );
    assert_eq!(
        s.run(&[
            "page",
            "delete",
            "p/AI-22/plan-1",
            "--project",
            "p",
            "--yes"
        ])
        .code,
        2
    );
    assert_eq!(count(&s), 4);
    // `-` で始まる project の名は、URL で書くか `--` の後ろに書く（旗と読み違えない）。
    s.place("project=-srv-me-x&key=K&kind=plan&role=tpm", "<p>1</p>");
    s.place("project=-srv-me-x&key=K&kind=plan&role=tpm", "<p>2</p>");
    assert_eq!(
        s.run(&["page", "delete", "-srv-me-x/K/plan-1", "--yes"])
            .code,
        2
    );
    let url = format!("{}/-srv-me-x/K/plan-1.html", s.url);
    assert!(
        s.run(&["page", "delete", &url, "--yes"])
            .stdout
            .starts_with("消した（1 枚）")
    );
    let ran = s.run(&["page", "delete", "--yes", "--", "-srv-me-x/K/plan-2"]);
    assert!(ran.stdout.starts_with("消した（1 枚）"), "{}", ran.stderr);
}

#[test]
fn no_delete_in_the_environment_stops_every_way_of_deleting() {
    let s = Server::start();
    four_pages(&s);
    let env = [("PAGETRAY_NO_DELETE", "1")];
    for args in [
        vec!["page", "delete", "p/AI-22/plan-1", "--yes"],
        vec!["page", "delete", "--project", "p", "--yes"],
        vec!["page", "delete", "p/AI-22/plan-1"],
        vec!["api", "-X", "DELETE", "/api/pages", "-f", "project=p"],
        vec!["api", "--method", "delete", "/api/pages", "-f", "project=p"],
    ] {
        let ran = run_with(&s.url, &args, &env, None);
        assert_eq!(ran.code, 1, "{args:?}");
        assert!(ran.stderr.contains("PAGETRAY_NO_DELETE"), "{}", ran.stderr);
        assert_eq!(ran.stdout, "", "{args:?}");
    }
    assert_eq!(count(&s), 4);
    // 読む・置くは止まらない。空と 0 は、付いていない扱い。
    assert_eq!(run_with(&s.url, &["page", "list"], &env, None).code, 0);
    let ran = run_with(
        &s.url,
        &["page", "delete", "p/AI-22/plan-1", "--yes"],
        &[("PAGETRAY_NO_DELETE", "0")],
        None,
    );
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    assert_eq!(count(&s), 3);
}

#[test]
fn backup_writes_a_sqlite_file_and_leaves_no_partial_file() {
    let s = Server::start();
    four_pages(&s);
    let file = temp("backup.db");
    let ran = s.run(&["backup", "-o", file.to_str().unwrap()]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    let bytes = std::fs::read(&file).unwrap();
    assert!(bytes.starts_with(b"SQLite format 3\0"));
    assert!(
        ran.stdout.contains(&format!("{} bytes", bytes.len())),
        "{}",
        ran.stdout
    );
    std::fs::remove_file(&file).unwrap();

    // 書けない先。途中の file を残さない。
    let ran = s.run(&["backup", "-o", "/nonexistent/dir/backup.db"]);
    assert_eq!(ran.code, 1);
    // 置き場が応えなければ、file を作らない。
    let ran = run_with(
        &common::closed_url(),
        &["backup", "-o", file.to_str().unwrap()],
        &[],
        None,
    );
    assert_eq!(ran.code, 3);
    let leftovers: Vec<_> = std::fs::read_dir(std::env::temp_dir())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with(&format!("pagetray-cli-{}-backup.db", std::process::id()))
        })
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn api_passes_the_answer_through() {
    let s = Server::start();
    four_pages(&s);
    let ran = s.run(&["api", "/api/version"]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    let json: serde_json::Value = serde_json::from_str(&ran.stdout).unwrap();
    assert_eq!(json["api"], 1);

    let ran = s.run(&["api", "/api/pages", "-f", "project=p", "-f", "key=AI-21"]);
    let json: serde_json::Value = serde_json::from_str(&ran.stdout).unwrap();
    assert_eq!(json["pages"].as_array().unwrap().len(), 1);

    // 断られた答えは、本文を出した上で code 1。
    let ran = s.run(&["api", "/api/pages", "-f", "sort=nope"]);
    assert_eq!(ran.code, 1);
    assert!(ran.stdout.contains("\"error\""), "{}", ran.stdout);
    assert!(ran.stderr.contains("400"), "{}", ran.stderr);

    // --input があれば POST。
    let ran = run_with(
        &s.url,
        &[
            "api",
            "/api/pages",
            "--input",
            "-",
            "-f",
            "project=p",
            "-f",
            "key=K",
            "-f",
            "kind=plan",
            "-f",
            "role=tpm",
        ],
        &[],
        Some(b"<title>api</title>"),
    );
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    assert!(
        ran.stdout.contains("\"path\":\"/p/K/plan-1.html\""),
        "{}",
        ran.stdout
    );

    let ran = s.run(&[
        "api",
        "-X",
        "DELETE",
        "/api/pages",
        "-f",
        "project=p",
        "-f",
        "key=K",
    ]);
    assert_eq!(ran.stdout.trim(), r#"{"removed":["/p/K/plan-1.html"]}"#);
    assert_eq!(s.run(&["api", "api/pages"]).code, 2);
    assert_eq!(s.run(&["api", "/api/pages", "-f", "nope"]).code, 2);
}

#[test]
fn watch_prints_a_line_for_each_page_placed() {
    let s = Server::start();
    let mut child = Command::new(common::binary())
        .args(["page", "watch", "--json"])
        .env_clear()
        .env("PAGETRAY_URL", &s.url)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (line_tx, line_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = BufReader::new(stdout).read_line(&mut line);
        let _ = line_tx.send(line);
    });
    // 待ち始める前に置いたページは流れない。流れるまで、少しずつ置く。
    let started = Instant::now();
    let line = loop {
        s.place("project=p&key=AI-22&kind=plan&role=tpm", &html("新着", "x"));
        if let Ok(line) = line_rx.recv_timeout(Duration::from_millis(100)) {
            break line;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "新着が流れない"
        );
    };
    child.kill().unwrap();
    child.wait().unwrap();
    let json: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(json["key"], "AI-22");
    assert_eq!(json["title"], "新着");
    assert!(
        json["url"]
            .as_str()
            .unwrap()
            .starts_with(&format!("{}/p/AI-22/plan-", s.url))
    );
}

fn file_put<'a>(file: &'a str, more: &[&'a str]) -> Vec<&'a str> {
    [
        &["file", "put", file, "--project", "p", "--key", "AI-24"][..],
        more,
    ]
    .concat()
}

fn file_count(s: &Server) -> usize {
    s.run(&["file", "list"]).stdout.lines().count()
}

/// 試験の秘密の形は、source にそのまま書かない（source の走査に当たるため）。頭を 2 つに分けて組む。
fn fake_token() -> String {
    format!("{}{}{}", "gh", "p_", "x".repeat(36))
}

#[test]
fn file_put_names_the_file_and_prints_the_url_and_the_reference() {
    let s = Server::start();
    let file = temp("My Notes (1).TXT");
    std::fs::write(&file, "メモ\n").unwrap();
    let file = file.to_str().unwrap();

    let ran = s.run(&file_put(file, &[]));
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    let name = format!("pagetray-cli-{}-my-notes-1", std::process::id());
    assert_eq!(
        ran.stdout,
        format!("{}/p/AI-24/files/{name}-1.txt\nfiles/{name}-1.txt\n", s.url)
    );
    // 同じ名で置き直すと、次の番号。--name で名を替える。
    assert!(
        s.run(&file_put(file, &[]))
            .stdout
            .ends_with(&format!("files/{name}-2.txt\n"))
    );
    let ran = s.run(&file_put(
        file,
        &["--name", "memo.md", "--json", "--role", "tpm"],
    ));
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    let json: serde_json::Value = serde_json::from_str(&ran.stdout).unwrap();
    assert_eq!(json["path"], "/p/AI-24/files/memo-1.md");
    assert_eq!(json["type"], "text/markdown; charset=utf-8");
    assert_eq!(json["role"], "tpm");
    assert_eq!(json["url"], format!("{}/p/AI-24/files/memo-1.md", s.url));
    assert_eq!(
        s.run(&["file", "view", "p/AI-24/files/memo-1.md"]).stdout,
        "メモ\n"
    );
    std::fs::remove_file(file).unwrap();
}

#[test]
fn file_put_needs_reviewed_for_images_and_pdf() {
    let s = Server::start();
    let png = temp("screen.png");
    std::fs::write(&png, PNG).unwrap();
    let pdf = temp("doc.pdf");
    std::fs::write(&pdf, b"%PDF-1.7\n%%EOF\n").unwrap();
    for file in [&png, &pdf] {
        let file = file.to_str().unwrap();
        let ran = s.run(&file_put(file, &[]));
        assert_eq!(ran.code, 1, "{file}");
        assert!(ran.stderr.contains("--reviewed"), "{}", ran.stderr);
        assert!(ran.stderr.contains("何も送っていない"), "{}", ran.stderr);
        assert_eq!(ran.stdout, "");
    }
    assert_eq!(file_count(&s), 0);
    for file in [&png, &pdf] {
        let ran = s.run(&file_put(file.to_str().unwrap(), &["--reviewed"]));
        assert_eq!(ran.code, 0, "{}", ran.stderr);
    }
    assert_eq!(file_count(&s), 2);
    std::fs::remove_file(png).unwrap();
    std::fs::remove_file(pdf).unwrap();
}

#[test]
fn file_put_stops_on_secret_shapes_unless_allowed() {
    let s = Server::start();
    let file = temp("leak.log");
    let token = fake_token();
    std::fs::write(&file, format!("一\n二\ntoken: {token}\n四\n")).unwrap();
    let file = file.to_str().unwrap();

    let ran = s.run(&file_put(file, &[]));
    assert_eq!(ran.code, 1, "{}", ran.stderr);
    assert!(ran.stderr.contains("3 行目"), "{}", ran.stderr);
    assert!(ran.stderr.contains("--allow-secret"), "{}", ran.stderr);
    // 当たった字そのものは出さない。
    assert!(!ran.stderr.contains(&token), "{}", ran.stderr);
    assert_eq!(file_count(&s), 0);

    let ran = s.run(&file_put(file, &["--allow-secret"]));
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    assert!(ran.stderr.contains("--allow-secret"), "{}", ran.stderr);
    assert!(ran.stderr.contains("3 行目"), "{}", ran.stderr);
    assert_eq!(file_count(&s), 1);
    std::fs::remove_file(file).unwrap();
}

#[test]
fn file_put_refuses_bad_files_without_reaching_the_store() {
    // 置き場の無い URL に向けて打つ。送ろうとすれば code 3 になる。
    let url = closed_url();
    let svg = temp("pic.svg");
    std::fs::write(&svg, "<svg/>").unwrap();
    let fake_png = temp("fake.png");
    std::fs::write(&fake_png, "png でない").unwrap();
    let binary_txt = temp("bin.txt");
    std::fs::write(&binary_txt, PNG).unwrap();
    let empty = temp("empty.txt");
    std::fs::write(&empty, "").unwrap();
    let no_ext = temp("noext");
    std::fs::write(&no_ext, "x").unwrap();
    let cases: Vec<(Vec<&str>, &str)> = vec![
        (file_put(svg.to_str().unwrap(), &["--reviewed"]), "置けない"),
        (
            file_put(fake_png.to_str().unwrap(), &["--reviewed"]),
            "の形でない",
        ),
        (file_put(binary_txt.to_str().unwrap(), &[]), "の形でない"),
        (file_put(empty.to_str().unwrap(), &[]), "空"),
        (file_put(no_ext.to_str().unwrap(), &[]), "拡張子"),
        (
            file_put(no_ext.to_str().unwrap(), &["--name", "Bad.txt"]),
            "--name",
        ),
        (file_put("/nonexistent/x.txt", &[]), "読めない"),
    ];
    for (args, says) in cases {
        let ran = run_with(&url, &args, &[], None);
        assert_eq!(ran.code, 2, "{args:?}: {}", ran.stderr);
        assert!(ran.stderr.contains(says), "{args:?}: {}", ran.stderr);
    }
    for file in [svg, fake_png, binary_txt, empty, no_ext] {
        std::fs::remove_file(file).unwrap();
    }
}

#[test]
fn file_put_warns_about_exif_in_jpeg() {
    let s = Server::start();
    let exif = b"\xff\xd8\xff\xe1\x00\x0eExif\0\0MM\0\x2a\0\0\xff\xda\x00\x02";
    let plain = b"\xff\xd8\xff\xe0\x00\x04\0\0\xff\xda\x00\x02";
    let file = temp("photo.jpg");
    std::fs::write(&file, exif).unwrap();
    let ran = s.run(&file_put(file.to_str().unwrap(), &["--reviewed"]));
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    assert!(ran.stderr.contains("EXIF"), "{}", ran.stderr);
    assert!(ran.stderr.contains("位置情報"), "{}", ran.stderr);
    // 止めはしないが、--reviewed が無い時も知らせる（見る時に気づけるように）。
    let ran = s.run(&file_put(file.to_str().unwrap(), &[]));
    assert_eq!(ran.code, 1);
    assert!(ran.stderr.contains("EXIF"), "{}", ran.stderr);
    std::fs::write(&file, plain).unwrap();
    let ran = s.run(&file_put(file.to_str().unwrap(), &["--reviewed"]));
    assert_eq!((ran.code, ran.stderr.as_str()), (0, ""));
    assert_eq!(file_count(&s), 2);
    std::fs::remove_file(file).unwrap();
}

/// 3 つ（AI-24 の screen-1・screen-2・note-1）と、別の Key の 1 つを置く。
fn four_files(s: &Server) {
    let q = "project=p&key=AI-24&role=tpm";
    s.place_file(&format!("{q}&name=screen.png"), "image/png", PNG);
    s.place_file(&format!("{q}&name=screen.png"), "image/png", PNG);
    s.place_file(&format!("{q}&name=note.txt"), "text/plain", b"x");
    s.place_file(
        "project=p&key=AI-23&name=screen.png&role=tpm",
        "image/png",
        PNG,
    );
}

#[test]
fn file_delete_only_lists_until_yes_is_given() {
    let s = Server::start();
    four_files(&s);
    let url = format!("{}/p/AI-24/files/screen-1.png", s.url);

    let ran = s.run(&["file", "delete", &url]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    assert!(ran.stdout.contains("1 個"), "{}", ran.stdout);
    assert!(ran.stdout.contains("まだ消していない"), "{}", ran.stdout);
    assert!(ran.stdout.contains(&url), "{}", ran.stdout);
    assert!(!ran.stdout.contains("screen-2"), "{}", ran.stdout);
    assert_eq!(file_count(&s), 4);

    let ran = s.run(&["file", "delete", &url, "--yes"]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    assert_eq!(ran.stdout, format!("消した（1 個）:\n{url}\n"));
    assert_eq!(file_count(&s), 3);
    assert!(s.run(&["file", "view", &url]).stderr.contains("（410）"));
    assert!(
        s.run(&["file", "delete", &url, "--yes"])
            .stdout
            .contains("消す file は無い")
    );

    // 名で選ぶと、その名のどの番号も。ほかの名と Key は残る。
    let by_name = [
        "file",
        "delete",
        "-p",
        "p",
        "-k",
        "AI-24",
        "--name",
        "screen.png",
    ];
    assert!(s.run(&by_name).stdout.contains("1 個"));
    let ran = s.run(&[&by_name[..], &["--yes"]].concat());
    assert!(ran.stdout.starts_with("消した（1 個）"), "{}", ran.stdout);
    assert_eq!(file_count(&s), 2);
    let ran = s.run(&["file", "delete", "--project", "p", "--yes"]);
    assert!(ran.stdout.starts_with("消した（2 個）"), "{}", ran.stdout);
    assert_eq!(file_count(&s), 0);
}

#[test]
fn file_delete_needs_a_target_and_stops_on_no_delete() {
    let s = Server::start();
    four_files(&s);
    for args in [
        vec!["file", "delete", "--yes"],
        vec!["file", "delete", "--key", "AI-24", "--yes"],
        vec!["file", "delete", "-p", "p", "--name", "screen.png", "--yes"],
        vec![
            "file",
            "delete",
            "p/AI-24/files/screen-1.png",
            "-p",
            "p",
            "--yes",
        ],
        vec!["file", "delete", "p/AI-24/screen-1.png", "--yes"],
    ] {
        assert_eq!(s.run(&args).code, 2, "{args:?}");
    }
    let env = [("PAGETRAY_NO_DELETE", "1")];
    for args in [
        vec!["file", "delete", "p/AI-24/files/screen-1.png", "--yes"],
        vec!["file", "delete", "--project", "p", "--yes"],
        vec!["file", "delete", "--project", "p"],
        vec!["api", "-X", "DELETE", "/api/files", "-f", "project=p"],
    ] {
        let ran = run_with(&s.url, &args, &env, None);
        assert_eq!(ran.code, 1, "{args:?}");
        assert!(ran.stderr.contains("PAGETRAY_NO_DELETE"), "{}", ran.stderr);
        assert_eq!(ran.stdout, "", "{args:?}");
    }
    assert_eq!(file_count(&s), 4);
}
