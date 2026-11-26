//! 置き場の path と file の名の形（もとは v0.8.0 の tag の crates/api。CLI だけが使うので、ここに移した）。

/// URL の 1 段になる字（project・key）。project は roster の状態の folder の名（project の path から作る）なので、
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
