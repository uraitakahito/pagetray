//! 語の決まり（docs/src/glossary.md の用語集）が、server の画面と文書に合っているか（template は repo の templates/ にあり、
//! ここでは server が組んだ画面を見る）。

mod common;

use std::path::{Path, PathBuf};

use axum::http::StatusCode;
use common::{App, PNG};

/// repo の頭（この crate は crates/server）。
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: &str) -> String {
    std::fs::read_to_string(root().join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// folder の下の file を、みな集める。
fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files(&path, out);
        } else {
            out.push(path);
        }
    }
}

/// 用語集に書いた id と class と区画の名が、server の Key の画面に在るか。名を変える時は、両方を一緒に変える。
#[tokio::test]
async fn ids_in_the_glossary_exist_in_the_ui() {
    let s = App::start().await;
    s.place("p", "K-1", "plan", "role=tpm", "<p>x</p>").await;
    s.place_file("p", "K-1", "name=a.png&role=tpm", "image/png", PNG)
        .await;
    let (status, _, body) = s.get("/p/K-1/?open=/p/K-1/plan-1.html").await;
    assert_eq!(status, StatusCode::OK);
    let page = String::from_utf8(body).unwrap();
    let glossary = read("docs/src/glossary.md");
    for id in [
        "toolbar",
        "filters",
        "list",
        "viewer",
        "fresh",
        "waiting",
        "review-waiting",
        "escalated",
        "files",
        "upload",
    ] {
        assert!(
            glossary.contains(&format!("`#{id}`")),
            "用語集に #{id} が無い"
        );
        assert_eq!(
            page.matches(&format!(r#" id="{id}""#)).count(),
            1,
            "画面に id=\"{id}\" が 1 つでない"
        );
    }
    for class in ["rows", "unread", "reply", "verdict", "links"] {
        assert!(
            glossary.contains(&format!("`.{class}`")),
            "用語集に .{class} が無い"
        );
        assert!(
            page.contains(&format!(".{class}")),
            "画面に .{class} が無い"
        );
    }
    // 区画の読み上げ用の名は、用語集の語と同じ。
    for name in [
        "操作の帯",
        "絞り込みの列",
        "一覧の列",
        "本文の枠",
        "file の枠",
        "上げる枠",
    ] {
        assert!(
            glossary.contains(&format!("| {name}（")),
            "用語集に {name} が無い"
        );
        assert!(
            page.contains(&format!(r#"aria-label="{name}""#)),
            "画面に aria-label=\"{name}\" が無い"
        );
    }
}

/// 用語集の「使わない言い方」が、文書・画面・code に戻って来ていないか。
/// 見ない所: 用語集そのもの・CHANGELOG・この試験・英語の訳。
#[test]
fn retired_terms_stay_out() {
    let retired = ["口の約束", "口の版", "右の枠", "サイドメニュー"];
    let mut paths = vec![root().join("README.md")];
    for dir in ["docs/src", "templates", "crates", "ci"] {
        files(&root().join(dir), &mut paths);
    }
    let skip = [
        root().join("docs/src/glossary.md"),
        root().join("crates/server/tests/terms.rs"),
    ];
    for path in paths.iter().filter(|p| !skip.contains(p)) {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue; // 字でない file
        };
        for term in retired {
            assert!(
                !text.contains(term),
                "{} に「{term}」がある（用語集の語に直す）",
                path.display()
            );
        }
    }
}
