//! 出力の形。端末には見出しの付いた表、pipe には tab 区切り、`--json` には JSON。

use crate::client::Failure;
use serde::Serialize;
use serde_json::{Map, Value};

/// `page list --json` で選べる欄（API の「ページの記録」と、CLI が足す `url`）。
pub const PAGE_FIELDS: &[&str] = &[
    "project",
    "key",
    "kind",
    "n",
    "title",
    "role",
    "workspace",
    "created_at",
    "size",
    "path",
    "url",
];

/// `file list --json` で選べる欄（API の「file の記録」と、CLI が足す `url`）。
pub const FILE_FIELDS: &[&str] = &[
    "project",
    "key",
    "name",
    "ext",
    "n",
    "type",
    "size",
    "sha256",
    "role",
    "workspace",
    "created_at",
    "path",
    "url",
];

/// 行を並べる。`tty` なら見出しを付けて幅をそろえ、違えば tab 区切り（見出しは無い）。
pub fn table(header: &[&str], rows: &[Vec<String>], tty: bool) -> String {
    // 1 行 1 件を保つため、欄の中の改行と tab は空白にする。
    let clean = |cell: &String| cell.replace(['\t', '\n', '\r'], " ");
    let rows: Vec<Vec<String>> = rows.iter().map(|r| r.iter().map(clean).collect()).collect();
    if !tty {
        return rows.iter().map(|r| r.join("\t") + "\n").collect();
    }
    let all = || {
        std::iter::once(header.iter().map(|h| (*h).to_owned()).collect())
            .chain(rows.iter().cloned())
    };
    let width = |column: usize| {
        all()
            .map(|row: Vec<String>| row[column].chars().count())
            .max()
            .unwrap_or(0)
    };
    let widths: Vec<usize> = (0..header.len()).map(width).collect();
    all()
        .map(|row| {
            let line: Vec<String> = row
                .iter()
                .enumerate()
                .map(|(i, cell)| {
                    let pad = widths[i].saturating_sub(cell.chars().count());
                    format!("{cell}{}", " ".repeat(pad))
                })
                .collect();
            line.join("  ").trim_end().to_owned() + "\n"
        })
        .collect()
}

/// 記録を JSON の配列にする。`fields` が空ならどの欄も、`a,b` なら名指した欄（`known` のどれか）だけを、その順に出す。
pub fn records<T: Serialize>(items: &[T], known: &[&str], fields: &str) -> Result<String, Failure> {
    let wanted: Vec<&str> = fields
        .split(',')
        .map(str::trim)
        .filter(|f| !f.is_empty())
        .collect();
    if let Some(unknown) = wanted.iter().find(|f| !known.contains(f)) {
        return Err(Failure::usage(format!(
            "--json に無い欄: {unknown}（選べるのは {}）",
            known.join(",")
        )));
    }
    let values: Vec<Value> = items
        .iter()
        .map(|item| {
            let all = serde_json::to_value(item).unwrap_or_default();
            if wanted.is_empty() {
                return all;
            }
            let picked: Map<String, Value> = wanted
                .iter()
                .map(|f| ((*f).to_owned(), all[*f].clone()))
                .collect();
            Value::Object(picked)
        })
        .collect();
    Ok(pretty(&Value::Array(values)))
}

pub fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default() + "\n"
}

#[cfg(test)]
mod tests {
    use super::table;

    fn rows() -> Vec<Vec<String>> {
        vec![
            vec!["AI-22".to_owned(), "計画\tの\n題".to_owned()],
            vec!["t-8zuk".to_owned(), "x".to_owned()],
        ]
    }

    #[test]
    fn pipes_get_tab_separated_rows_without_a_header() {
        assert_eq!(
            table(&["KEY", "TITLE"], &rows(), false),
            "AI-22\t計画 の 題\nt-8zuk\tx\n"
        );
    }

    #[test]
    fn terminals_get_aligned_columns_with_a_header() {
        assert_eq!(
            table(&["KEY", "TITLE"], &rows(), true),
            "KEY     TITLE\nAI-22   計画 の 題\nt-8zuk  x\n"
        );
    }
}
