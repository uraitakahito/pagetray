//! 読む命令（status・page list・page view・file list・file view・browse・completion）を、同じ commit の server に向けて打つ。

mod common;

use common::{PNG, Server, canned, closed_url, html, rows, run_with};

const SE: &str = "role=software-engineer&workspace=workspace:1";

#[test]
fn status_reports_the_server_version() {
    let s = Server::start();
    let ran = s.run(&["status"]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    let version = env!("CARGO_PKG_VERSION");
    assert!(
        ran.stdout.contains(&format!("サーバーの版: {version}")),
        "{}",
        ran.stdout
    );
    assert!(
        ran.stdout.contains(&format!("pagetray の版: {version}")),
        "{}",
        ran.stdout
    );

    let ran = s.run(&["status", "--json"]);
    let json: serde_json::Value = serde_json::from_str(&ran.stdout).unwrap();
    assert_eq!(
        json,
        serde_json::json!({"url": s.url, "server": version, "cli": version})
    );
}

#[test]
fn a_store_that_is_down_exits_3() {
    for args in [
        &["status"][..],
        &["page", "list"],
        &["page", "view", "p/AI-22/plan-1"],
    ] {
        let ran = run_with(&closed_url(), args, &[], None);
        assert_eq!(ran.code, 3, "{args:?}: {}", ran.stderr);
        assert!(ran.stderr.contains("応えない"), "{}", ran.stderr);
        assert_eq!(ran.stdout, "");
    }
}

#[test]
fn a_store_that_does_not_know_the_operation_exits_4() {
    // 生成した層は、知らない道に 404 UnknownOperationException を返す（定義より古い server）。
    let (url, _) = canned(
        "HTTP/1.1 404 Not Found\r\nX-Amzn-Errortype: UnknownOperationException\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
    );
    let ran = run_with(&url, &["page", "list"], &[], None);
    assert_eq!(ran.code, 4, "{}", ran.stderr);
    assert!(ran.stderr.contains("知らない"), "{}", ran.stderr);
    assert_eq!(ran.stdout, "");
}

#[test]
fn requests_name_127_0_0_1_as_the_host_and_skip_the_proxy() {
    // 置き場は Host が 127.0.0.1・localhost でない要求を断る。つなぐ先の名がどうでも、Host は 127.0.0.1（と --url の port）。
    let (url, heads) = canned(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 33\r\n\r\n{\"status\":\"ok\",\"version\":\"9.9.9\"}",
    );
    // 環境変数の proxy は使わない（使えば、待ち受けの無い proxy につなごうとして code 3 になる）。
    let proxy = closed_url();
    let env = [
        ("HTTP_PROXY", proxy.as_str()),
        ("http_proxy", proxy.as_str()),
        ("ALL_PROXY", proxy.as_str()),
    ];
    let ran = run_with(&url, &["status"], &env, None);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    let head = heads.recv().unwrap();
    assert!(head.starts_with("get /healthz http/1.1\r\n"), "{head}");
    let port = url.rsplit(':').next().unwrap();
    assert!(
        head.contains(&format!("\r\nhost: 127.0.0.1:{port}\r\n")),
        "{head}"
    );
    assert!(!head.contains("origin:"), "{head}");
    assert!(ran.stdout.contains("サーバーの版: 9.9.9"), "{}", ran.stdout);
}

#[test]
fn usage_errors_exit_2() {
    let s = Server::start();
    for args in [
        &["nope"][..],
        &["page", "view", "p/AI-22"],
        &["page", "list", "--json=nope"],
        &["page", "list", "--json=created_at"],
        &["page", "list", "-L", "0"],
        &["page", "list", "-L", "1001"],
        &["page", "list", "--before", "2026-01-01T00:00:00Z"],
    ] {
        assert_eq!(s.run(args).code, 2, "{args:?}");
    }
    assert_eq!(
        run_with("https://127.0.0.1:1", &["status"], &[], None).code,
        2
    );
}

#[test]
fn list_is_tab_separated_newest_first_when_piped() {
    let s = Server::start();
    s.place("p", "AI-22", "plan", SE, &html("計画", "一"));
    s.place("p", "AI-22", "result", SE, &html("結果", "二"));
    s.place("q", "t-x", "plan", "role=tpm", &html("別の project", "三"));

    let ran = s.run(&["page", "list"]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    let rows = rows(&ran.stdout);
    assert_eq!(rows.len(), 3);
    // CREATED・PROJECT・KEY・PAGE・ROLE・REPLY・URL・TITLE（返事を求めていないページの REPLY は -）
    assert_eq!(
        rows[0][1..],
        [
            "q",
            "t-x",
            "plan-1",
            "tpm",
            "-",
            &format!("{}/q/t-x/plan-1.html", s.url),
            "別の project"
        ]
    );
    // 置いた時刻は、API の答えと同じ UTC の字。
    assert!(rows[0][0].ends_with('Z'), "{}", rows[0][0]);
    assert_eq!(rows[2][1..4], ["p", "AI-22", "plan-1"]);

    // project で絞ると、project の欄は出ない。
    let ran = s.run(&["page", "list", "--project", "p", "--oldest"]);
    let rows = common::rows(&ran.stdout);
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0][1..],
        [
            "AI-22",
            "plan-1",
            "software-engineer",
            "-",
            &format!("{}/p/AI-22/plan-1.html", s.url),
            "計画"
        ]
    );
}

#[test]
fn list_filters_reach_the_store() {
    let s = Server::start();
    s.place("p", "AI-22", "plan", SE, &html("計画", "りんご"));
    s.place("p", "AI-22", "result", SE, &html("結果", "みかん"));
    s.place(
        "p",
        "AI-21",
        "plan",
        "role=tpm",
        &html("別の Key", "りんご"),
    );
    let keys = |args: &[&str]| -> Vec<String> {
        let ran = s.run(&[&["page", "list", "--json=key,kind"], args].concat());
        assert_eq!(ran.code, 0, "{}", ran.stderr);
        let json: serde_json::Value = serde_json::from_str(&ran.stdout).unwrap();
        json.as_array()
            .unwrap()
            .iter()
            .map(|p| {
                format!(
                    "{}/{}",
                    p["key"].as_str().unwrap(),
                    p["kind"].as_str().unwrap()
                )
            })
            .collect()
    };
    assert_eq!(keys(&["--key", "AI-22"]), ["AI-22/result", "AI-22/plan"]);
    assert_eq!(keys(&["--kind", "plan", "--role", "tpm"]), ["AI-21/plan"]);
    assert_eq!(
        keys(&["--workspace", "workspace:1", "-q", "みかん"]),
        ["AI-22/result"]
    );
    assert_eq!(keys(&["-q", "りんご", "-L", "1"]), ["AI-21/plan"]);
    // 置き場が断る条件は、置き場の訳で code 1。
    let ran = s.run(&["page", "list", "--kind", "Plan"]);
    assert_eq!(ran.code, 1, "{}", ran.stderr);
    assert!(
        ran.stderr.contains("（400 ValidationException）"),
        "{}",
        ran.stderr
    );
}

#[test]
fn list_tells_how_to_get_the_rest_and_follows_the_next_token() {
    let s = Server::start();
    for kind in ["a", "b", "c"] {
        s.place("p", "AI-22", kind, "role=tpm", &html(kind, "x"));
    }
    let ran = s.run(&["page", "list", "-L", "2", "--json=kind"]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    let json: serde_json::Value = serde_json::from_str(&ran.stdout).unwrap();
    assert_eq!(json, serde_json::json!([{"kind": "c"}, {"kind": "b"}]));
    // 続きの印は stderr に出す（stdout は頼んだ物だけ）。
    let token = ran
        .stderr
        .split_whitespace()
        .skip_while(|w| *w != "--next-token")
        .nth(1)
        .unwrap_or_else(|| panic!("{}", ran.stderr))
        .to_owned();
    let ran = s.run(&[
        "page",
        "list",
        "-L",
        "2",
        "--json=kind",
        "--next-token",
        &token,
    ]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    let json: serde_json::Value = serde_json::from_str(&ran.stdout).unwrap();
    assert_eq!(json, serde_json::json!([{"kind": "a"}]));
    // 終わりの頁には、続きの印が無い。
    assert!(!ran.stderr.contains("--next-token"), "{}", ran.stderr);
    // 崩れた印は、置き場が断る。
    let ran = s.run(&["page", "list", "--next-token", "nope"]);
    assert_eq!(ran.code, 1, "{}", ran.stderr);
    assert!(ran.stderr.contains("nextToken"), "{}", ran.stderr);
}

#[test]
fn list_json_carries_the_record_of_the_api_and_the_url() {
    let s = Server::start();
    s.place("p", "AI-22", "plan", SE, &html("計画", "一"));
    let ran = s.run(&["page", "list", "--json"]);
    let json: serde_json::Value = serde_json::from_str(&ran.stdout).unwrap();
    let page = &json[0];
    assert_eq!(page["project"], "p");
    // 番号は、API の答えと同じく字。
    assert_eq!(page["n"], "1");
    assert_eq!(page["title"], "計画");
    assert_eq!(page["workspace"], "workspace:1");
    assert_eq!(page["path"], "/p/AI-22/plan-1.html");
    assert_eq!(page["url"], format!("{}/p/AI-22/plan-1.html", s.url));
    // 欄の名は API の答えと同じ camelCase。
    let created = page["createdAt"].as_str().unwrap();
    assert!(created.ends_with('Z'), "{created}");
    assert!(page["created_at"].is_null());
    for field in ["size", "role", "key", "kind"] {
        assert!(!page[field].is_null(), "{field}");
    }
    // 値の無い欄は出さない。
    for field in ["askedAt", "answeredAt", "waitsOn", "verdict", "sender"] {
        assert!(page.get(field).is_none(), "{field}: {page}");
    }
}

#[test]
fn project_names_may_start_with_a_hyphen_and_come_from_the_environment() {
    let s = Server::start();
    s.place(
        "-srv-me-x",
        "AI-22",
        "plan",
        "role=tpm",
        &html("計画", "一"),
    );
    s.place("p", "AI-22", "plan", "role=tpm", &html("別", "二"));
    for args in [
        &["page", "list", "--project", "-srv-me-x"][..],
        &["page", "list", "-p", "-srv-me-x"],
    ] {
        assert_eq!(s.run(args).stdout.lines().count(), 1, "{args:?}");
    }
    let ran = run_with(
        &s.url,
        &["page", "list"],
        &[("PAGETRAY_PROJECT", "-srv-me-x")],
        None,
    );
    assert_eq!(ran.stdout.lines().count(), 1, "{}", ran.stderr);
    let ran = s.run(&["page", "view", "-srv-me-x/AI-22/plan-1"]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
}

#[test]
fn view_prints_the_body_as_placed() {
    let s = Server::start();
    let body = html("計画", "本文そのまま");
    let path = s.place("p", "AI-22", "plan", SE, &body);
    for page in [
        format!("{}{path}", s.url),
        "p/AI-22/plan-1".to_owned(),
        "http://127.0.0.1:8800/p/AI-22/plan-1.html".to_owned(),
    ] {
        let ran = s.run(&["page", "view", &page]);
        assert_eq!(ran.code, 0, "{}", ran.stderr);
        assert_eq!(ran.stdout, body, "{page}");
    }

    let file = std::env::temp_dir().join(format!("pagetray-view-{}.html", std::process::id()));
    let ran = s.run(&[
        "page",
        "view",
        "p/AI-22/plan-1",
        "-o",
        file.to_str().unwrap(),
    ]);
    assert_eq!((ran.code, ran.stdout.as_str()), (0, ""), "{}", ran.stderr);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), body);
    std::fs::remove_file(file).unwrap();
}

#[test]
fn missing_and_removed_pages_exit_1_with_the_stores_reason() {
    let s = Server::start();
    s.place("p", "AI-22", "plan", SE, &html("計画", "一"));
    let ran = s.run(&["page", "view", "p/AI-22/plan-9"]);
    assert_eq!(ran.code, 1);
    assert!(
        ran.stderr.contains("（404 NotFound）: そのページは無い"),
        "{}",
        ran.stderr
    );

    let (status, _) = s.send(
        "DELETE",
        "/api/pages?project=p&key=AI-22&kind=plan&n=1",
        None,
        Vec::new(),
    );
    assert_eq!(status, 200);
    let ran = s.run(&["page", "view", "p/AI-22/plan-1"]);
    assert_eq!(ran.code, 1);
    assert!(ran.stderr.contains("（410 Gone）: "), "{}", ran.stderr);
    assert_eq!(ran.stdout, "");
}

#[test]
fn browse_and_web_hand_the_url_to_the_browser_command() {
    let s = Server::start();
    assert_eq!(s.run(&["browse", "-n"]).stdout, format!("{}/\n", s.url));
    assert_eq!(
        s.run(&["browse", "--no-browser", "-srv-me-x"]).stdout,
        format!("{}/-srv-me-x/\n", s.url)
    );
    assert_eq!(
        s.run(&["browse", "-n", "p/AI-22"]).stdout,
        format!("{}/p/AI-22/\n", s.url)
    );
    assert_eq!(s.run(&["browse", "-n", "p/AI-22/plan-1"]).code, 2);

    // $BROWSER の命令に URL が渡る（echo が、渡された URL を出す）。
    let echo = [("BROWSER", "echo"), ("PATH", "/bin:/usr/bin")];
    let ran = run_with(&s.url, &["browse", "p"], &echo, None);
    assert_eq!((ran.code, ran.stdout), (0, format!("{}/p/\n", s.url)));
    let ran = run_with(
        &s.url,
        &["page", "view", "--web", "p/AI-22/plan-1"],
        &echo,
        None,
    );
    assert_eq!(
        (ran.code, ran.stdout),
        (0, format!("{}/p/AI-22/plan-1.html\n", s.url))
    );
}

#[test]
fn completion_scripts_name_the_command() {
    let s = Server::start();
    for shell in ["bash", "zsh", "fish"] {
        let ran = s.run(&["completion", shell]);
        assert_eq!(ran.code, 0, "{shell}");
        assert!(ran.stdout.contains("pagetray"), "{shell}");
    }
    assert_eq!(
        s.run(&["--version"]).stdout,
        format!("pagetray {}\n", env!("CARGO_PKG_VERSION"))
    );
}

/// 文書（docs/src/cli.md）に、どの命令の名もあるか。命令を足した時に、文書を忘れると落ちる。
#[test]
fn every_command_is_in_the_book() {
    let section = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/src/cli.md"
    ))
    .unwrap();
    let s = Server::start();
    let commands = |args: &[&str]| -> Vec<String> {
        let help = s.run(args).stdout;
        help.lines()
            .skip_while(|line| *line != "Commands:")
            .skip(1)
            .take_while(|line| line.starts_with("  "))
            .filter_map(|line| line.split_whitespace().next().map(str::to_owned))
            .collect()
    };
    let top = commands(&["--help"]);
    assert!(top.contains(&"status".to_owned()), "{top:?}");
    for command in top
        .iter()
        .filter(|c| !matches!(c.as_str(), "page" | "file"))
    {
        assert!(
            section.contains(&format!("`pagetray {command}")),
            "docs/src/cli.md に pagetray {command} が無い"
        );
    }
    for group in ["page", "file"] {
        let sub = commands(&[group, "--help"]);
        assert!(sub.contains(&"list".to_owned()), "{sub:?}");
        for command in sub {
            assert!(
                section.contains(&format!("`pagetray {group} {command}")),
                "docs/src/cli.md に pagetray {group} {command} が無い"
            );
        }
    }
}

const FILES: &str = "role=software-engineer&workspace=workspace:1";

#[test]
fn file_list_shows_the_files_newest_first() {
    let s = Server::start();
    s.place_file(
        "p",
        "AI-24",
        &format!("{FILES}&name=screen.png"),
        "image/png",
        PNG,
    );
    s.place_file(
        "p",
        "AI-24",
        &format!("{FILES}&name=note.txt"),
        "text/plain",
        "メモ\n".as_bytes(),
    );
    s.place_file("q", "K", "name=screen.png&role=tpm", "image/png", PNG);

    let ran = s.run(&["file", "list"]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    let rows = rows(&ran.stdout);
    assert_eq!(rows.len(), 3);
    // CREATED・PROJECT・KEY・FILE・SIZE・ROLE・URL
    assert_eq!(
        rows[0][1..],
        [
            "q",
            "K",
            "screen-1.png",
            &PNG.len().to_string(),
            "tpm",
            &format!("{}/q/K/files/screen-1.png", s.url),
        ]
    );
    // project で絞ると、project の欄は出ない。
    let ran = s.run(&["file", "list", "--project", "p", "--oldest"]);
    let rows = common::rows(&ran.stdout);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0][1..3], ["AI-24", "screen-1.png"]);

    let ran = s.run(&["file", "list", "-q", "note", "--json"]);
    let json: serde_json::Value = serde_json::from_str(&ran.stdout).unwrap();
    let file = &json[0];
    assert_eq!(json.as_array().unwrap().len(), 1);
    assert_eq!(file["name"], "note");
    assert_eq!(file["ext"], "txt");
    assert_eq!(file["n"], "1");
    assert_eq!(file["type"], "text/plain; charset=utf-8");
    assert_eq!(file["workspace"], "workspace:1");
    assert_eq!(file["path"], "/p/AI-24/files/note-1.txt");
    assert_eq!(file["url"], format!("{}/p/AI-24/files/note-1.txt", s.url));
    for field in ["createdAt", "size", "sha256", "role"] {
        assert!(!file[field].is_null(), "{field}");
    }
    let ran = s.run(&["file", "list", "--json=name,url", "--key", "K"]);
    let json: serde_json::Value = serde_json::from_str(&ran.stdout).unwrap();
    assert_eq!(
        json,
        serde_json::json!([{"name": "screen", "url": format!("{}/q/K/files/screen-1.png", s.url)}])
    );
    assert_eq!(s.run(&["file", "list", "--json=title"]).code, 2);
}

#[test]
fn file_view_writes_the_bytes_as_placed() {
    let s = Server::start();
    let path = s.place_file(
        "p",
        "AI-24",
        &format!("{FILES}&name=screen.png"),
        "image/png",
        PNG,
    );
    s.place_file(
        "p",
        "AI-24",
        &format!("{FILES}&name=note.md"),
        "text/markdown",
        "# 見出し\n".as_bytes(),
    );
    let out = std::env::temp_dir().join(format!("pagetray-file-view-{}.png", std::process::id()));
    for file in [
        format!("{}{path}", s.url),
        "p/AI-24/files/screen-1.png".to_owned(),
        "http://127.0.0.1:8800/p/AI-24/files/screen-1.png".to_owned(),
    ] {
        let ran = s.run(&["file", "view", &file, "-o", out.to_str().unwrap()]);
        assert_eq!((ran.code, ran.stdout.as_str()), (0, ""), "{}", ran.stderr);
        assert_eq!(std::fs::read(&out).unwrap(), PNG, "{file}");
    }
    std::fs::remove_file(out).unwrap();
    // pipe には、そのまま出す。
    let ran = s.run(&["file", "view", "p/AI-24/files/note-1.md"]);
    assert_eq!((ran.code, ran.stdout.as_str()), (0, "# 見出し\n"));

    let ran = s.run(&["file", "view", "p/AI-24/files/note-9.md"]);
    assert_eq!(ran.code, 1);
    assert!(ran.stderr.contains("（404 NotFound）: "), "{}", ran.stderr);
    for ng in [
        "p/AI-24/note-1.md",
        "p/AI-24/files/note.md",
        "p/AI-24/files/x-1.svg",
    ] {
        assert_eq!(s.run(&["file", "view", ng]).code, 2, "{ng}");
    }
}
