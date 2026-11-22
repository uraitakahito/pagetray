//! pagetray の API の仕様（docs/src/api.md）のうち、サーバーと CLI が共に使う物。
//! API の版・header の名・引数の字の決まり・ページと file の path の形・記録の型・受ける file の種類を、1 か所に置く。
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
    /// 返事待ちにした時刻。返事を求めていなければ無い。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asked_at: Option<String>,
    /// 答えた時刻。まだ答えていなければ無い。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answered_at: Option<String>,
    /// 送り手。置いた時に付随する id を書いていなければ無い（0.6.0 から）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender: Option<SenderRecord>,
}

/// ページと file の送り手。置いた時の付随する id と、その時に castlist が答えたアクター。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SenderRecord {
    /// 付随する id を出した物（`tpm` など）。
    pub issuer: String,
    /// 付随する id（`<種類>:<値>`）。
    #[serde(rename = "ref")]
    pub actor_ref: String,
    /// castlist が答えたアクター。castlist に聞かなかった（置き場に castlist の URL が無い）時は null（未確認）。
    pub actor: Option<ActorRecord>,
    /// castlist が答えた時刻。聞かなかった時は無い。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<String>,
}

/// castlist が答えた、送り手のアクター（本当の id・名・種類）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorRecord {
    pub id: String,
    pub name: String,
    /// `ai` か `human`。
    pub kind: String,
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

/// 付随する id を出した物の名（`actor_issuer`）。英小文字・数字・`-` `.` で 1〜64 字（castlist の名の形）。
pub fn is_issuer(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'.'))
}

/// 付随する id（`actor_ref`）: `<種類>:<値>`。種類は [`is_issuer`] と同じ形。値は英数字と `:` `.` `_` `-` で
/// 1〜256 字（初めの `:` で分けるので、値に `:` があってよい）。
pub fn is_actor_ref(s: &str) -> bool {
    s.split_once(':').is_some_and(|(kind, value)| {
        is_issuer(kind)
            && (1..=256).contains(&value.len())
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b':' | b'.' | b'_' | b'-'))
    })
}

/// castlist のアクターの id の形（UUID など）。英数字と `-` で 1〜64 字。URL の 1 段にそのまま入れられる。
pub fn is_actor_id(s: &str) -> bool {
    (1..=64).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
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
    parse_page_path(path).is_some()
}

/// ページの path（`/<project>/<key>/<kind>-<n>.html`）を、project・Key・種類・番号に分ける。
pub fn parse_page_path(path: &str) -> Option<(&str, &str, &str, i64)> {
    let mut parts = path.strip_prefix('/')?.split('/');
    let (project, key, file) = (parts.next()?, parts.next()?, parts.next()?);
    let (kind, n) = parse_file(file)?;
    (parts.next().is_none() && is_segment(project) && is_segment(key))
        .then_some((project, key, kind, n))
}

/// `<kind>-<n>.html` を分ける。番号は、頭に 0 の無い 1 以上の数だけ（1 枚に URL は 1 つ）。
pub fn parse_file(file: &str) -> Option<(&str, i64)> {
    let (kind, n) = file.strip_suffix(".html")?.rsplit_once('-')?;
    let ok = is_name(kind) && !n.starts_with('0') && n.bytes().all(|b| b.is_ascii_digit());
    ok.then(|| n.parse().ok()).flatten().map(|n| (kind, n))
}

/// 1 つの file の上限（ページの上限と同じ）。
pub const MAX_FILE: usize = 16 * 1024 * 1024;

/// 置く・探すの答えに出る、file 1 つの記録（中身は入らない）。呼ぶ側が読む形。知らない欄は読み飛ばす。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRecord {
    pub project: String,
    pub key: String,
    /// 名の語（`screen`）。
    pub name: String,
    /// 拡張子（`png`）。
    pub ext: String,
    pub n: i64,
    /// 返す時の Content-Type（[`FILE_TYPES`] の値）。
    pub r#type: String,
    pub size: i64,
    /// 中身の SHA-256（16 進の小文字）。
    pub sha256: String,
    pub role: String,
    pub workspace: String,
    pub created_at: String,
    /// サーバーの中の path（`/<project>/<key>/files/<name>-<n>.<ext>`）。
    pub path: String,
    /// 置いた答えにだけ付く、要求の `Host` で組んだ URL。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// 送り手。置いた時に付随する id を書いていなければ無い（0.6.0 から）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender: Option<SenderRecord>,
}

/// 中身の見方。拡張子と合わなければ受けない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sniff {
    Png,
    Jpeg,
    Gif,
    Webp,
    Pdf,
    /// UTF-8 として読めて、NUL を含まない。
    Text,
}

/// 受ける file の種類（拡張子・返す時の Content-Type・中身の見方）。ここに無い拡張子は受けない。
pub const FILE_TYPES: &[(&str, &str, Sniff)] = &[
    ("png", "image/png", Sniff::Png),
    ("jpg", "image/jpeg", Sniff::Jpeg),
    ("jpeg", "image/jpeg", Sniff::Jpeg),
    ("gif", "image/gif", Sniff::Gif),
    ("webp", "image/webp", Sniff::Webp),
    ("pdf", "application/pdf", Sniff::Pdf),
    ("txt", "text/plain; charset=utf-8", Sniff::Text),
    ("log", "text/plain; charset=utf-8", Sniff::Text),
    ("md", "text/markdown; charset=utf-8", Sniff::Text),
    ("csv", "text/csv; charset=utf-8", Sniff::Text),
    ("json", "application/json; charset=utf-8", Sniff::Text),
];

/// 拡張子の Content-Type と中身の見方。受けない拡張子なら None。
pub fn file_type(ext: &str) -> Option<(&'static str, Sniff)> {
    FILE_TYPES
        .iter()
        .find(|(e, _, _)| *e == ext)
        .map(|&(_, mime, sniff)| (mime, sniff))
}

/// 中身が、その見方に合うか。
pub fn sniff(kind: Sniff, bytes: &[u8]) -> bool {
    match kind {
        Sniff::Png => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        Sniff::Jpeg => bytes.starts_with(b"\xff\xd8\xff"),
        Sniff::Gif => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        Sniff::Webp => bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"),
        Sniff::Pdf => bytes.starts_with(b"%PDF-"),
        Sniff::Text => !bytes.contains(&0) && std::str::from_utf8(bytes).is_ok(),
    }
}

/// file の名の語の形: 英小文字・数字・`-`・`_` で 1〜64 字。英小文字か数字で始まる。
pub fn is_file_stem(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && matches!(s.as_bytes()[0], b'a'..=b'z' | b'0'..=b'9')
        && s.bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_'))
}

/// 置く時の名（`<語>.<拡張子>`）を、語と拡張子に分ける。拡張子は英数字だけ（受けるかは [`file_type`] が見る）。
pub fn split_file_name(name: &str) -> Option<(&str, &str)> {
    let (stem, ext) = name.split_once('.')?;
    let ext_ok = (1..=16).contains(&ext.len()) && ext.bytes().all(|b| b.is_ascii_alphanumeric());
    (is_file_stem(stem) && ext_ok).then_some((stem, ext))
}

/// file の中身の path。
pub fn file_path(project: &str, key: &str, name: &str, ext: &str, n: i64) -> String {
    format!("/{project}/{key}/files/{name}-{n}.{ext}")
}

/// `<語>-<n>.<拡張子>` を分ける。番号は、頭に 0 の無い 1 以上の数だけ。拡張子は受ける物だけ。
pub fn parse_file_name(file: &str) -> Option<(&str, &str, i64)> {
    let (rest, ext) = file.split_once('.')?;
    let (name, n) = rest.rsplit_once('-')?;
    let ok = is_file_stem(name)
        && file_type(ext).is_some()
        && !n.starts_with('0')
        && !n.is_empty()
        && n.bytes().all(|b| b.is_ascii_digit());
    ok.then(|| n.parse().ok()).flatten().map(|n| (name, ext, n))
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
    fn actor_refs_are_a_kind_and_a_value() {
        for ok in [
            "claude.session:5f0c2a8e-1",
            "tpm.person:pm",
            "cmux.workspace.ref:workspace:3",
            &format!("a:{}", "x".repeat(256)),
        ] {
            assert!(is_actor_ref(ok), "{ok}");
        }
        for ng in [
            "",
            "claude.session",
            "claude.session:",
            ":a",
            "Claude:a",
            "a:b c",
            "a:b/c",
            "a:字",
            &format!("a:{}", "x".repeat(257)),
        ] {
            assert!(!is_actor_ref(ng), "{ng}");
        }
        assert!(is_issuer("tpm") && is_issuer("a.b-1"));
        assert!(!is_issuer("") && !is_issuer("TPM") && !is_issuer("a_b"));
        assert!(is_actor_id("019a0000-0000-7000-8000-00000000000a"));
        assert!(!is_actor_id("../x") && !is_actor_id("") && !is_actor_id("a b"));
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

    #[test]
    fn file_stems_are_lowercase_words() {
        for ok in ["screen", "a", "0", "screen-shot_2", &"a".repeat(64)] {
            assert!(is_file_stem(ok), "{ok}");
        }
        for ng in [
            "",
            "-a",
            "_a",
            "Screen",
            "a.b",
            "a b",
            "画像",
            &"a".repeat(65),
        ] {
            assert!(!is_file_stem(ng), "{ng}");
        }
        assert_eq!(split_file_name("screen.png"), Some(("screen", "png")));
        assert_eq!(split_file_name("screen.svg"), Some(("screen", "svg")));
        assert_eq!(split_file_name("screen.PNG"), Some(("screen", "PNG")));
        for ng in [
            "screen",
            "screen.",
            ".png",
            "a.b.png",
            "Screen.png",
            "a.p/g",
        ] {
            assert_eq!(split_file_name(ng), None, "{ng}");
        }
    }

    #[test]
    fn file_names_split_into_stem_number_and_ext() {
        assert_eq!(parse_file_name("screen-1.png"), Some(("screen", "png", 1)));
        assert_eq!(
            parse_file_name("my-shot_2-12.jpeg"),
            Some(("my-shot_2", "jpeg", 12))
        );
        assert_eq!(
            file_path("p", "K-1", "screen", "png", 2),
            "/p/K-1/files/screen-2.png"
        );
        for ng in [
            "screen.png",
            "screen-0.png",
            "screen-01.png",
            "screen-.png",
            "screen-1",
            "screen-1.svg",
            "screen-1.PNG",
            "Screen-1.png",
            "-1.png",
            "screen-1.png.png",
            "screen-99999999999999999999.png",
        ] {
            assert_eq!(parse_file_name(ng), None, "{ng}");
        }
    }

    #[test]
    fn only_listed_types_are_taken() {
        assert_eq!(file_type("jpg"), file_type("jpeg"));
        assert_eq!(file_type("md").unwrap().0, "text/markdown; charset=utf-8");
        for ng in ["svg", "html", "mp4", "PNG", "exe", ""] {
            assert_eq!(file_type(ng), None, "{ng}");
        }
    }

    #[test]
    fn contents_are_sniffed() {
        assert!(sniff(Sniff::Png, b"\x89PNG\r\n\x1a\nrest"));
        assert!(!sniff(Sniff::Png, b"%PDF-1.7"));
        assert!(sniff(Sniff::Jpeg, b"\xff\xd8\xff\xe0"));
        assert!(sniff(Sniff::Gif, b"GIF89a..."));
        assert!(!sniff(Sniff::Gif, b"GIF90a..."));
        assert!(sniff(Sniff::Webp, b"RIFF\0\0\0\0WEBPVP8 "));
        assert!(!sniff(Sniff::Webp, b"RIFF\0\0\0\0WAVE"));
        assert!(!sniff(Sniff::Webp, b"RIFF"));
        assert!(sniff(Sniff::Pdf, b"%PDF-1.7"));
        assert!(sniff(Sniff::Text, "字の file\n".as_bytes()));
        assert!(!sniff(Sniff::Text, b"a\0b"));
        assert!(!sniff(Sniff::Text, b"\xff\xfe"));
    }
}
