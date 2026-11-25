//! 出力の形。端末には見出しの付いた表、pipe には tab 区切り、`--json` には JSON。
//! JSON の記録は、API の答えと同じ欄の名（camelCase）と字に、頼んだ先（`--url`）で組んだ `url` を足した物。

use crate::client::Failure;
use aws_smithy_types::DateTime;
use aws_smithy_types::date_time::Format;
use pagetray_client::types::Sender;
use serde_json::{Map, Value, json};

/// `page list --json` で選べる欄（API の「ページの記録」と、CLI が足す `url`）。
pub const PAGE_FIELDS: &[&str] = &[
    "project",
    "key",
    "kind",
    "n",
    "title",
    "role",
    "workspace",
    "createdAt",
    "size",
    "path",
    "url",
    "askedAt",
    "answeredAt",
    "waitsOn",
    "verdict",
    "targets",
    "sender",
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
    "createdAt",
    "path",
    "url",
    "sender",
];

/// 答えの時刻を、API の答えと同じ字にする（RFC 3339 の UTC）。
pub fn time(t: &DateTime) -> String {
    t.fmt(Format::DateTime).unwrap_or_default()
}

/// ページの記録（`Page`・置く答え・返事の答え。どれも同じ名の欄を持つ）を、JSON の記録にする。
/// 返事と印の欄（`askedAt` など）は、置く答えに無いので、[`page_state`] で足す。
macro_rules! page_record {
    ($page:expr, $base:expr) => {{
        let p = $page;
        let mut m = serde_json::Map::new();
        m.insert("project".into(), p.project().into());
        m.insert("key".into(), p.key().into());
        m.insert("kind".into(), p.kind().into());
        m.insert("n".into(), p.n().into());
        m.insert("title".into(), p.title().into());
        m.insert("role".into(), p.role().into());
        m.insert("workspace".into(), p.workspace().into());
        m.insert(
            "createdAt".into(),
            $crate::output::time(p.created_at()).into(),
        );
        m.insert("size".into(), p.size().into());
        m.insert("path".into(), p.path().into());
        m.insert("url".into(), format!("{}{}", $base, p.path()).into());
        if let Some(sender) = p.sender() {
            m.insert("sender".into(), $crate::output::sender(sender));
        }
        m
    }};
}
pub(crate) use page_record;

/// ページの返事と印の欄（値のある物だけ）を、記録に足す。
macro_rules! page_state {
    ($record:expr, $page:expr) => {{
        let (m, p) = (&mut $record, $page);
        if let Some(t) = p.asked_at() {
            m.insert("askedAt".into(), $crate::output::time(t).into());
        }
        if let Some(t) = p.answered_at() {
            m.insert("answeredAt".into(), $crate::output::time(t).into());
        }
        if let Some(on) = p.waits_on() {
            m.insert("waitsOn".into(), on.into());
        }
        if let Some(verdict) = p.verdict() {
            m.insert("verdict".into(), verdict.into());
        }
        if !p.targets().is_empty() {
            m.insert("targets".into(), p.targets().into());
        }
    }};
}
pub(crate) use page_state;

/// file の記録（`File`・置く答え）を、JSON の記録にする。
macro_rules! file_record {
    ($file:expr, $base:expr) => {{
        let f = $file;
        let mut m = serde_json::Map::new();
        m.insert("project".into(), f.project().into());
        m.insert("key".into(), f.key().into());
        m.insert("name".into(), f.name().into());
        m.insert("ext".into(), f.ext().into());
        m.insert("n".into(), f.n().into());
        m.insert("type".into(), f.r#type().into());
        m.insert("size".into(), f.size().into());
        m.insert("sha256".into(), f.sha256().into());
        m.insert("role".into(), f.role().into());
        m.insert("workspace".into(), f.workspace().into());
        m.insert(
            "createdAt".into(),
            $crate::output::time(f.created_at()).into(),
        );
        m.insert("path".into(), f.path().into());
        m.insert("url".into(), format!("{}{}", $base, f.path()).into());
        if let Some(sender) = f.sender() {
            m.insert("sender".into(), $crate::output::sender(sender));
        }
        m
    }};
}
pub(crate) use file_record;

/// 送り手の欄（castlist の答えがあれば `actor` と `resolvedAt` も）。
pub fn sender(sender: &Sender) -> Value {
    let mut m = Map::new();
    m.insert("issuer".into(), sender.issuer().into());
    m.insert("ref".into(), sender.r#ref().into());
    if let Some(actor) = sender.actor() {
        m.insert(
            "actor".into(),
            json!({"id": actor.id(), "name": actor.name(), "kind": actor.kind()}),
        );
    }
    if let Some(t) = sender.resolved_at() {
        m.insert("resolvedAt".into(), time(t).into());
    }
    Value::Object(m)
}

/// 記録の字の欄（無ければ空）。
pub fn text<'a>(record: &'a Map<String, Value>, field: &str) -> &'a str {
    record.get(field).and_then(Value::as_str).unwrap_or("")
}

/// 一覧に出す、返事の状態の字（返事を求めていないページは `-`）。Product Manager のほかを待つ物は
/// `waiting:review` のように相手を添える。
pub fn reply_of(record: &Map<String, Value>) -> String {
    let state = match (record.get("askedAt"), record.get("answeredAt")) {
        (_, Some(_)) => "answered",
        (Some(_), None) => "waiting",
        (None, None) => return "-".to_owned(),
    };
    match record.get("waitsOn").and_then(Value::as_str) {
        Some(on) => format!("{state}:{on}"),
        None => state.to_owned(),
    }
}

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

/// `--json` の欄の名指し（`a,b`）を見る。空ならどの欄も。知らない欄は使い方の誤り。
pub fn fields<'a>(fields: &'a str, known: &[&str]) -> Result<Vec<&'a str>, Failure> {
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
    Ok(wanted)
}

/// 記録を JSON の配列にする。`wanted` が空ならどの欄も、あれば名指した欄だけを、その順に出す（値の無い欄は null）。
pub fn records(items: Vec<Map<String, Value>>, wanted: &[&str]) -> String {
    let values: Vec<Value> = items
        .into_iter()
        .map(|all| {
            if wanted.is_empty() {
                return Value::Object(all);
            }
            let picked: Map<String, Value> = wanted
                .iter()
                .map(|f| ((*f).to_owned(), all.get(*f).cloned().unwrap_or(Value::Null)))
                .collect();
            Value::Object(picked)
        })
        .collect();
    pretty(&Value::Array(values))
}

pub fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default() + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn times_are_written_like_the_api() {
        assert_eq!(
            time(&DateTime::from_secs(1_795_478_400)),
            "2026-11-24T00:00:00Z"
        );
        assert_eq!(
            time(&DateTime::from_millis(1_795_478_400_500)),
            "2026-11-24T00:00:00.5Z"
        );
    }

    #[test]
    fn the_reply_column_names_the_party_but_pm() {
        let record = |v: Value| v.as_object().unwrap().clone();
        assert_eq!(reply_of(&record(json!({}))), "-");
        assert_eq!(reply_of(&record(json!({"askedAt": "t"}))), "waiting");
        assert_eq!(
            reply_of(&record(
                json!({"askedAt": "t", "answeredAt": "t", "waitsOn": "tpm"})
            )),
            "answered:tpm"
        );
    }
}
