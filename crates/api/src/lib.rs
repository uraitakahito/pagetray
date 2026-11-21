//! pagetray の API の仕様（docs/src/api.md）のうち、サーバーと CLI が共に使う物。
//! API の版・header の名・引数の字の決まり・ページの path の形・ページの記録の型を、1 か所に置く。
//! ここを変える時は、docs/src/api.md の「API の版を上げる時・上げない時」を先に見る。

use serde::{Deserialize, Serialize};

/// API の版。API の形（URL・引数の名・答えの JSON の欄・code の意味）を、前からある呼ぶ側が困る形に変えた時だけ上げる
/// （決まりは docs/src/api.md）。
pub const API_VERSION: u32 = 1;
/// どの答えにも付く、API の版の header の名（小文字）。
pub const API_HEADER: &str = "pagetray-api";

/// 置く・探すの答えに出る、ページ 1 枚の記録（本文は入らない）。呼ぶ側が読む形。知らない欄は読み飛ばす。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageRecord {
    pub project: String,
    pub key: String,
    pub kind: String,
    pub n: i64,
    pub title: String,
    pub role: String,
    pub workspace: String,
    pub created_at: String,
    pub size: i64,
    /// サーバーの中の path（`/<project>/<key>/<kind>-<n>.html`）。
    pub path: String,
    /// 置いた答えにだけ付く、要求の `Host` で組んだ URL。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// ページの本文の path。
pub fn page_path(project: &str, key: &str, kind: &str, n: i64) -> String {
    format!("/{project}/{key}/{kind}-{n}.html")
}

/// URL の 1 段になる字（project・key）。project は tpm の状態の folder の名（project の path から作る）なので、
/// folder の名の長さの上限（255 字）まで受ける。
pub fn is_segment(s: &str) -> bool {
    (1..=255).contains(&s.len())
        && s != "."
        && s != ".."
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// 種類と役割の名の形。名の一覧は持たない（呼ぶ側が決める）。
pub fn is_name(s: &str) -> bool {
    (1..=32).contains(&s.len())
        && s.as_bytes()[0].is_ascii_lowercase()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

pub fn is_workspace(s: &str) -> bool {
    s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b':' | b'.' | b'_' | b'-'))
}

/// `2026-01-02T03:04:05Z` か、秒の小数（3 桁まで）が付いた形。字の順が時刻の順になる形だけを受ける。
pub fn is_timestamp(s: &str) -> bool {
    let b = s.as_bytes();
    let shape = |pattern: &str| {
        b.len() == pattern.len()
            && pattern.bytes().zip(b).all(|(p, c)| match p {
                b'0' => c.is_ascii_digit(),
                _ => p == *c,
            })
    };
    shape("0000-00-00T00:00:00Z")
        || shape("0000-00-00T00:00:00.0Z")
        || shape("0000-00-00T00:00:00.00Z")
        || shape("0000-00-00T00:00:00.000Z")
}

/// 置き場の中のページの path（`/<project>/<key>/<kind>-<n>.html`）の形か。
pub fn is_page_path(path: &str) -> bool {
    let mut parts = path.strip_prefix('/').unwrap_or("/").split('/');
    let (project, key, file) = (parts.next(), parts.next(), parts.next());
    parts.next().is_none()
        && project.is_some_and(is_segment)
        && key.is_some_and(is_segment)
        && file.and_then(parse_file).is_some()
}

/// `<kind>-<n>.html` を分ける。番号は、頭に 0 の無い 1 以上の数だけ（1 枚に URL は 1 つ）。
pub fn parse_file(file: &str) -> Option<(&str, i64)> {
    let (kind, n) = file.strip_suffix(".html")?.rsplit_once('-')?;
    let ok = is_name(kind) && !n.starts_with('0') && n.bytes().all(|b| b.is_ascii_digit());
    ok.then(|| n.parse().ok()).flatten().map(|n| (kind, n))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_lowercase_words() {
        for ok in ["brief", "issue", "software-engineer", "a1"] {
            assert!(is_name(ok), "{ok}");
        }
        for ng in ["", "Brief", "1a", "-a", "a_b", "a b", &"a".repeat(33)] {
            assert!(!is_name(ng), "{ng}");
        }
    }

    #[test]
    fn segments_cannot_escape_the_path() {
        for ok in [
            "-Users-me-projects-skills",
            "SKILLS-14",
            "t-d7hb",
            "a.b_c",
            &"a".repeat(255),
        ] {
            assert!(is_segment(ok), "{ok}");
        }
        for ng in ["", ".", "..", "a/b", "a b", "鍵", &"a".repeat(256)] {
            assert!(!is_segment(ng), "{ng}");
        }
    }

    #[test]
    fn workspaces_may_be_empty() {
        assert!(is_workspace(""));
        assert!(is_workspace("workspace:1000090098"));
        assert!(!is_workspace("a b"));
    }

    #[test]
    fn timestamps_are_utc_iso() {
        for ok in [
            "2026-11-21T04:45:22Z",
            "2026-11-21T04:45:22.4Z",
            "2026-11-21T04:45:22.472Z",
        ] {
            assert!(is_timestamp(ok), "{ok}");
        }
        for ng in [
            "2026-11-21 04:45:22",
            "2026-11-21T04:45:22+09:00",
            "2026-11-21T04:45:22.4721Z",
            "",
        ] {
            assert!(!is_timestamp(ng), "{ng}");
        }
    }

    #[test]
    fn page_paths_stay_inside_the_store() {
        assert!(is_page_path("/p/SKILLS-14/plan-1.html"));
        for ng in [
            "p/k/plan-1.html",
            "/p/plan-1.html",
            "/p/k/x/plan-1.html",
            "//evil.example/k/plan-1.html",
            "https://evil.example/p/k/plan-1.html",
            "/p/k/plan-1.html?x",
            "",
        ] {
            assert!(!is_page_path(ng), "{ng}");
        }
    }

    #[test]
    fn file_names_split_into_kind_and_number() {
        assert_eq!(parse_file("plan-1.html"), Some(("plan", 1)));
        assert_eq!(parse_file("my-kind-12.html"), Some(("my-kind", 12)));
        for ng in [
            "plan-1",
            "plan.html",
            "plan-01.html",
            "plan-0.html",
            "plan-.html",
            "Plan-1.html",
            "plan-+1.html",
            "plan-99999999999999999999.html",
        ] {
            assert_eq!(parse_file(ng), None, "{ng}");
        }
    }
}
