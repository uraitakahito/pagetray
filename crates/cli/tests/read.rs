//! 読む命令（status・page list・page view・file list・file view・browse・completion）を、同じ commit のサーバーに向けて打つ。

mod common;

use common::{PNG, Server, canned, closed_url, html, run_with};

const SE: &str = "project=p&key=AI-22&role=software-engineer&workspace=workspace:1";

#[test]
fn status_reports_both_versions() {
    let s = Server::start();
    let ran = s.run(&["status"]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    let version = env!("CARGO_PKG_VERSION");
    assert!(
        ran.stdout.contains(&format!("サーバーの版: {version}")),
        "{}",
        ran.stdout
    );
    assert!(ran.stdout.contains("API の版: 1"), "{}", ran.stdout);

    let json: serde_json::Value =
        serde_json::from_str(&s.run(&["status", "--json"]).stdout).unwrap();
    assert_eq!(json["url"], s.url);
    assert_eq!(json["server"], version);
    assert_eq!(json["api"], 1);
    assert_eq!(json["cli"], version);
    assert_eq!(json["cli_api"], 1);
}

#[test]
fn a_store_that_is_down_exits_3() {
    let ran = run_with(&closed_url(), &["status"], &[], None);
    assert_eq!(ran.code, 3);
    assert!(ran.stderr.contains("応えない"), "{}", ran.stderr);
    assert_eq!(ran.stdout, "");
}

#[test]
fn another_api_version_exits_4_without_using_the_answer() {
    let url = canned(
        "HTTP/1.1 200 OK\r\nPagetray-Api: 2\r\nContent-Type: application/json\r\nContent-Length: 12\r\nConnection: close\r\n\r\n{\"pages\":[]}",
    );
    let ran = run_with(&url, &["page", "list"], &[], None);
    assert_eq!(ran.code, 4);
    assert!(ran.stderr.contains("API の版は 2"), "{}", ran.stderr);
    assert_eq!(ran.stdout, "");
}

#[test]
fn an_answer_without_the_header_is_an_older_store_and_passes() {
    let url = canned(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 12\r\nConnection: close\r\n\r\n{\"pages\":[]}",
    );
    let ran = run_with(&url, &["page", "list", "--json"], &[], None);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    assert_eq!(ran.stdout, "[]\n");
}

#[test]
fn usage_errors_exit_2() {
    let s = Server::start();
    assert_eq!(s.run(&["nope"]).code, 2);
    assert_eq!(s.run(&["page", "view", "p/AI-22"]).code, 2);
    assert_eq!(s.run(&["page", "list", "--json=nope"]).code, 2);
    assert_eq!(
        run_with("https://127.0.0.1:1", &["status"], &[], None).code,
        2
    );
}

#[test]
fn list_is_tab_separated_newest_first_when_piped() {
    let s = Server::start();
    s.place(&format!("{SE}&kind=plan"), &html("計画", "一"));
    s.place(&format!("{SE}&kind=result"), &html("結果", "二"));
    s.place(
        "project=q&key=t-x&kind=plan&role=tpm",
        &html("別の project", "三"),
    );

    let ran = s.run(&["page", "list"]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    let rows: Vec<Vec<&str>> = ran
        .stdout
        .lines()
        .map(|l| l.split('\t').collect())
        .collect();
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
    assert_eq!(rows[2][1..4], ["p", "AI-22", "plan-1"]);

    // project で絞ると、project の欄は出ない。
    let ran = s.run(&["page", "list", "--project", "p", "--oldest"]);
    let rows: Vec<Vec<&str>> = ran
        .stdout
        .lines()
        .map(|l| l.split('\t').collect())
        .collect();
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
    s.place(&format!("{SE}&kind=plan"), &html("計画", "りんご"));
    s.place(&format!("{SE}&kind=result"), &html("結果", "みかん"));
    s.place(
        "project=p&key=AI-21&kind=plan&role=tpm",
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
    assert_eq!(
        keys(&["--after", "2999-01-01T00:00:00Z"]),
        Vec::<String>::new()
    );
    assert_eq!(keys(&["--before", "2999-01-01T00:00:00Z"]).len(), 3);
}

#[test]
fn list_json_carries_the_record_and_the_url() {
    let s = Server::start();
    s.place(&format!("{SE}&kind=plan"), &html("計画", "一"));
    let ran = s.run(&["page", "list", "--json"]);
    let json: serde_json::Value = serde_json::from_str(&ran.stdout).unwrap();
    let page = &json[0];
    assert_eq!(page["project"], "p");
    assert_eq!(page["n"], 1);
    assert_eq!(page["title"], "計画");
    assert_eq!(page["workspace"], "workspace:1");
    assert_eq!(page["path"], "/p/AI-22/plan-1.html");
    assert_eq!(page["url"], format!("{}/p/AI-22/plan-1.html", s.url));
    for field in ["created_at", "size", "role", "key", "kind"] {
        assert!(!page[field].is_null(), "{field}");
    }
}

#[test]
fn project_names_may_start_with_a_hyphen_and_come_from_the_environment() {
    let s = Server::start();
    s.place(
        "project=-srv-me-x&key=AI-22&kind=plan&role=tpm",
        &html("計画", "一"),
    );
    s.place("project=p&key=AI-22&kind=plan&role=tpm", &html("別", "二"));
    assert_eq!(
        s.run(&["page", "list", "--project", "-srv-me-x"])
            .stdout
            .lines()
            .count(),
        1
    );
    assert_eq!(
        s.run(&["page", "list", "-p", "-srv-me-x"])
            .stdout
            .lines()
            .count(),
        1
    );
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
    let path = s.place(&format!("{SE}&kind=plan"), &body);
    for page in [
        format!("{}{path}", s.url),
        "p/AI-22/plan-1".to_owned(),
        "http://127.0.0.1:8800/p/AI-22/plan-1.html".to_owned(),
    ] {
        let ran = s.run(&["page", "view", &page]);
        assert_eq!(ran.code, 0, "{}", ran.stderr);
        assert_eq!(ran.stdout, body, "{page}");
    }

    let file = std::env::temp_dir().join(format!("pagetray-cli-view-{}.html", std::process::id()));
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
    s.place(&format!("{SE}&kind=plan"), &html("計画", "一"));
    let ran = s.run(&["page", "view", "p/AI-22/plan-9"]);
    assert_eq!(ran.code, 1);
    assert!(
        ran.stderr.contains("（404）: そのページは無い"),
        "{}",
        ran.stderr
    );

    ureq::delete(format!(
        "{}/api/pages?project=p&key=AI-22&kind=plan&n=1",
        s.url
    ))
    .call()
    .unwrap();
    let ran = s.run(&["page", "view", "p/AI-22/plan-1"]);
    assert_eq!(ran.code, 1);
    assert!(
        ran.stderr.contains("（410）: そのページは消した"),
        "{}",
        ran.stderr
    );
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
    let book = std::fs::read_to_string(concat!(
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
            book.contains(&format!("`pagetray {command}")),
            "docs/src/cli.md に pagetray {command} が無い"
        );
    }
    for group in ["page", "file"] {
        let sub = commands(&[group, "--help"]);
        assert!(sub.contains(&"list".to_owned()), "{sub:?}");
        for command in sub {
            assert!(
                book.contains(&format!("`pagetray {group} {command}")),
                "docs/src/cli.md に pagetray {group} {command} が無い"
            );
        }
    }
}

const FILES: &str = "project=p&key=AI-24&role=software-engineer&workspace=workspace:1";

#[test]
fn file_list_shows_the_files_newest_first() {
    let s = Server::start();
    s.place_file(&format!("{FILES}&name=screen.png"), "image/png", PNG);
    s.place_file(
        &format!("{FILES}&name=note.txt"),
        "text/plain",
        "メモ\n".as_bytes(),
    );
    s.place_file("project=q&key=K&name=screen.png&role=tpm", "image/png", PNG);

    let ran = s.run(&["file", "list"]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    let rows: Vec<Vec<&str>> = ran
        .stdout
        .lines()
        .map(|l| l.split('\t').collect())
        .collect();
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
    let rows: Vec<Vec<&str>> = ran
        .stdout
        .lines()
        .map(|l| l.split('\t').collect())
        .collect();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0][1..3], ["AI-24", "screen-1.png"]);

    let ran = s.run(&["file", "list", "-q", "note", "--json"]);
    let json: serde_json::Value = serde_json::from_str(&ran.stdout).unwrap();
    let file = &json[0];
    assert_eq!(json.as_array().unwrap().len(), 1);
    assert_eq!(file["name"], "note");
    assert_eq!(file["ext"], "txt");
    assert_eq!(file["type"], "text/plain; charset=utf-8");
    assert_eq!(file["workspace"], "workspace:1");
    assert_eq!(file["path"], "/p/AI-24/files/note-1.txt");
    assert_eq!(file["url"], format!("{}/p/AI-24/files/note-1.txt", s.url));
    for field in ["created_at", "size", "sha256", "role", "n"] {
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
    let path = s.place_file(&format!("{FILES}&name=screen.png"), "image/png", PNG);
    s.place_file(
        &format!("{FILES}&name=note.md"),
        "text/markdown",
        "# 見出し\n".as_bytes(),
    );
    let out =
        std::env::temp_dir().join(format!("pagetray-cli-file-view-{}.png", std::process::id()));
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
    assert!(
        ran.stderr.contains("（404）: その file は無い"),
        "{}",
        ran.stderr
    );
    for ng in [
        "p/AI-24/note-1.md",
        "p/AI-24/files/note.md",
        "p/AI-24/files/x-1.svg",
    ] {
        assert_eq!(s.run(&["file", "view", ng]).code, 2, "{ng}");
    }
}
