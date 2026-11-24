//! 頁の印（nextToken）: 中身の字を改行で繋ぎ、16 進にした物。中は読まずに、前の答えの物をそのまま渡してもらう。
//! 頭の字は検索の種類で、ほかの検索の印と分ける（castlist の新しい server の crates/server/src/token.rs と同じ形）。

/// 崩れた印の訳。
pub const BROKEN: &str = "nextToken が読めない（前の答えの nextToken を、そのまま渡す）";

/// 字を改行で繋ぎ、16 進にする。
pub fn encode(parts: &[&str]) -> String {
    parts
        .join("\n")
        .bytes()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// 16 進を戻し、改行で分ける。崩れていれば（空の物も）None。
pub fn decode(token: &str) -> Option<Vec<String>> {
    if token.is_empty()
        || !token.len().is_multiple_of(2)
        || !token.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return None;
    }
    let bytes = (0..token.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&token[i..i + 2], 16))
        .collect::<Result<Vec<u8>, _>>()
        .ok()?;
    let text = String::from_utf8(bytes).ok()?;
    Some(text.split('\n').map(str::to_owned).collect())
}

/// ほかの検索の印の訳（`what` は検索の名。例「アクターの検索」）。
fn other(what: &str) -> String {
    format!("nextToken が読めない（{what}の nextToken ではない）")
}

/// 印を戻し、`kind` の検索の `n` 個の中身（頭の種類を含む）を返す。崩れた物と、種類が同じで数の違う物は崩れた印、
/// 種類の違う物はほかの検索の印（`what` は今の検索の名）の訳。
pub fn read(token: &str, kind: &str, n: usize, what: &str) -> Result<Vec<String>, String> {
    let parts = decode(token).ok_or(BROKEN)?;
    if parts[0] != kind {
        return Err(other(what));
    }
    if parts.len() != n {
        return Err(BROKEN.to_owned());
    }
    Ok(parts)
}

/// 一覧（ページと file）の並べ方の名（定義の `PageSort` の字）。
pub fn sort_name(oldest: bool) -> &'static str {
    if oldest { "createdAt" } else { "-createdAt" }
}

/// 一覧の次の頁の印: 検索の種類・並べ方・前の頁の最後の行の、置いた時刻（残す字）と id。
pub fn next(kind: &str, oldest: bool, created_at: &str, id: i64) -> String {
    encode(&[kind, sort_name(oldest), created_at, &id.to_string()])
}

/// 一覧の頁の印を読む（前の頁の最後の行の、置いた時刻と id）。崩れた物・ほかの検索の物・sort の違う物は、その訳。
/// ほかの条件は印に結ばない（castlist の ListActors と同じ）。
pub fn after(token: &str, kind: &str, what: &str, oldest: bool) -> Result<(String, i64), String> {
    let parts = read(token, kind, 4, what)?;
    let [_, sort, created_at, id] = &parts[..] else {
        return Err(BROKEN.to_owned());
    };
    if sort != sort_name(oldest) {
        return Err("nextToken は、同じ sort の答えの物を渡す".to_owned());
    }
    let id = id.parse().map_err(|_| BROKEN.to_owned())?;
    Ok((created_at.clone(), id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_reads_back_and_a_broken_or_other_one_is_refused() {
        let token = encode(&["pages", "-createdAt", "2026-11-24T00:00:00.000Z", "7"]);
        assert_eq!(
            read(&token, "pages", 4, "ページの検索"),
            Ok(vec![
                "pages".to_owned(),
                "-createdAt".to_owned(),
                "2026-11-24T00:00:00.000Z".to_owned(),
                "7".to_owned()
            ])
        );
        assert_eq!(
            read(&token, "files", 4, "file の検索"),
            Err(other("file の検索"))
        );
        // 種類が同じで、中身の数が違う物は崩れた印。
        for broken in [
            "zz",
            "abc",
            "ff",
            "",
            &encode(&["pages", "x"]),
            &encode(&["pages"]),
        ] {
            assert_eq!(
                read(broken, "pages", 4, "ページの検索"),
                Err(BROKEN.to_owned()),
                "{broken}"
            );
        }
    }
}
