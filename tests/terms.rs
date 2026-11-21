//! 語の決まり（docs/src/glossary.md の用語集）が、画面と文書に合っているか。

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
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

/// 用語集に書いた id と class が、画面（template）に在るか。名を変える時は、両方を一緒に変える。
#[test]
fn ids_in_the_glossary_exist_in_the_ui() {
    let glossary = read("docs/src/glossary.md");
    let template = read("templates/index.html");
    for id in ["toolbar", "filters", "list", "viewer", "fresh"] {
        assert!(
            glossary.contains(&format!("`#{id}`")),
            "用語集に #{id} が無い"
        );
        assert_eq!(
            template.matches(&format!(r#" id="{id}""#)).count(),
            1,
            "template に id=\"{id}\" が 1 つでない"
        );
    }
    for class in ["rows", "unread"] {
        assert!(
            glossary.contains(&format!("`.{class}`")),
            "用語集に .{class} が無い"
        );
        assert!(
            template.contains(&format!(".{class}")),
            "template に .{class} が無い"
        );
    }
    // 区画の読み上げ用の名は、用語集の語と同じ。
    for name in ["操作の帯", "絞り込みの列", "一覧の列", "本文の枠"] {
        assert!(
            glossary.contains(&format!("| {name}（")),
            "用語集に {name} が無い"
        );
        assert!(
            template.contains(&format!(r#"aria-label="{name}""#)),
            "template に aria-label=\"{name}\" が無い"
        );
    }
}

/// 用語集の「使わない言い方」が、文書・画面・code に戻って来ていないか。
/// 見ない所: 用語集そのもの（使わない言い方の欄）・CHANGELOG（前の語から何に替えたかを書く）・この file・英語の訳。
#[test]
fn retired_terms_stay_out() {
    // 字の並びとして、ほかの語の一部にならない物だけを見る（「口」1 字は「受け口」などに当たるので見ない）。
    let retired = ["口の約束", "口の版", "右の枠", "サイドメニュー"];
    let mut paths = vec![root().join("README.md")];
    for dir in ["docs/src", "src", "templates", "tests", "crates", "ci"] {
        files(&root().join(dir), &mut paths);
    }
    let skip = [
        root().join("docs/src/glossary.md"),
        root().join("tests/terms.rs"),
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
