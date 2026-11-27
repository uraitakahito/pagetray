//! 語の決まり（docs/src/glossary.md の用語集）が、文書と code に合っているか。

use std::path::{Path, PathBuf};

/// repo の頭（この crate は crates/server）。
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
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

/// 用語集の「使わない言い方」が、文書・code に戻って来ていないか。
/// 見ない所: 用語集そのもの・CHANGELOG・この試験・英語の訳。
#[test]
fn retired_terms_stay_out() {
    let retired = ["口の約束", "口の版", "右の枠", "サイドメニュー"];
    let mut paths = vec![root().join("README.md")];
    for dir in ["docs/src", "crates", "ci"] {
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
