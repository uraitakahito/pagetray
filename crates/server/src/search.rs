//! ページを探す（ListPages）の DB の所。読む接続の 1 つの取引の中で、1 頁を読む。
//!
//! - 条件は今の server（src/db.rs の list）と同じ。違うのは頁の続け方で、`created_at` と行の id の組で続ける（keyset）。
//!   同じ時刻の行が頁の境に並んでも、抜けず重ならない。辿る間に置いたページ（前の頁より新しい）は、新しい順の続きに出ない。
//! - SQL は書いた条件の欠片だけを繋ぎ、値は bind するだけ（[`plans`] の試験が、pages を索引で引くことを見る）。
use sqlx::{QueryBuilder, Sqlite};

use crate::db::{Db, PAGE_ROW, PageRow, Party, Reply};

/// 1 頁の既定。
pub const LIMIT: i64 = 100;

/// ページを探す条件（どれも AND）。形は定義と handler が見てある。
#[derive(Clone, Debug, Default)]
pub struct PageQuery {
    pub project: Option<String>,
    pub key: Option<String>,
    pub kind: Option<String>,
    pub role: Option<String>,
    /// 空も条件（workspace を書かずに置いたページ）。
    pub workspace: Option<String>,
    /// 探す語（[`search`] で、全文の索引の形か端から見る形に分ける）。
    pub q: Option<String>,
    /// 返事の状態と、その待ちの相手。
    pub reply: Option<(Reply, Party)>,
    pub verdict: Option<String>,
    /// 送り手（castlist のアクターの id）。
    pub actor: Option<String>,
    /// 古い順（無ければ新しい順）。
    pub oldest: bool,
    /// 前の頁の最後の行の、置いた時刻と id。
    pub after: Option<(String, i64)>,
    pub limit: i64,
}

/// 1 頁と、次の頁があるか。
pub struct Page {
    pub rows: Vec<PageRow>,
    pub more: bool,
}

/// 探す語を、全文の索引で探す形（どの語も 3 字以上の時。語ごとに引用符で囲んだ trigram の句）か、
/// 端から見る形（3 字に満たない語がある時。q 全体）に分ける。前後の空白を落として空なら、どちらも無い。
pub fn search(q: Option<&str>) -> (Option<String>, Option<String>) {
    let Some(q) = q.map(str::trim).filter(|q| !q.is_empty()) else {
        return (None, None);
    };
    if q.split_whitespace().all(|term| term.chars().count() >= 3) {
        let phrases: Vec<String> = q
            .split_whitespace()
            .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
            .collect();
        (Some(phrases.join(" ")), None)
    } else {
        (None, Some(q.to_owned()))
    }
}

impl PageQuery {
    fn builder(&self) -> QueryBuilder<Sqlite> {
        let mut q = QueryBuilder::new(PAGE_ROW);
        q.push(" where 1");
        for (column, value) in [
            ("project", &self.project),
            ("key", &self.key),
            ("kind", &self.kind),
            ("role", &self.role),
            ("workspace", &self.workspace),
            ("verdict", &self.verdict),
        ] {
            if let Some(value) = value {
                q.push(format!(" and p.{column} = "))
                    .push_bind(value.clone());
            }
        }
        let (fts, scan) = search(self.q.as_deref());
        if let Some(fts) = fts {
            q.push(" and p.id in (select rowid from pages_fts where pages_fts match ")
                .push_bind(fts)
                .push(")");
        }
        if let Some(scan) = scan {
            q.push(" and p.id in (select rowid from pages_fts where instr(title, ")
                .push_bind(scan.clone())
                .push(") > 0 or instr(text, ")
                .push_bind(scan)
                .push(") > 0)");
        }
        if let Some((reply, on)) = self.reply {
            // 返事待ちは、部分の索引（pages_waiting。返事待ちの行だけの物）から引く。条件をじかに書くと、planner は
            // 時刻の索引の順に端から読む方を選ぶ（返事待ちの少ない時に、1 頁を埋めるまで表を読む）。
            q.push(match reply {
                Reply::Waiting => {
                    " and p.id in (select id from pages where asked_at is not null and answered_at is null)"
                }
                Reply::Answered => " and p.answered_at is not null",
            });
            match on.column() {
                None => q.push(" and p.waits_on is null"),
                Some(party) => q.push(" and p.waits_on = ").push_bind(party),
            };
        }
        if let Some(actor) = &self.actor {
            q.push(" and p.sender_id in (select id from senders where actor_id = ")
                .push_bind(actor.clone())
                .push(")");
        }
        let (cmp, dir) = if self.oldest {
            (">", "asc")
        } else {
            ("<", "desc")
        };
        if let Some((created_at, id)) = &self.after {
            q.push(format!(" and (p.created_at, p.id) {cmp} ("))
                .push_bind(created_at.clone())
                .push(", ")
                .push_bind(*id)
                .push(")");
        }
        // 次の頁があるかを見るため、1 つ多く読む。
        q.push(format!(" order by p.created_at {dir}, p.id {dir} limit "))
            .push_bind(self.limit + 1);
        q
    }
}

impl Db {
    /// 条件に合うページの 1 頁（読む接続の 1 つの取引の中で読む）。
    pub async fn list(&self, query: &PageQuery) -> Result<Page, sqlx::Error> {
        let mut tx = self.read().begin().await?;
        let mut rows: Vec<PageRow> = query.builder().build_query_as().fetch_all(&mut *tx).await?;
        tx.commit().await?;
        let limit = usize::try_from(query.limit).unwrap_or(usize::MAX);
        let more = rows.len() > limit;
        rows.truncate(limit);
        Ok(Page { rows, more })
    }
}

/// 探す SQL（名と SQL）。試験が `explain query plan` で、pages を索引なしに端から読まないかを見る。
/// 条件を 1 つずつと、全部を重ねた物。値は bind するので、ここでは仮の値を入れる。
pub fn plans() -> Vec<(&'static str, String)> {
    fn x() -> Option<String> {
        Some("x".to_owned())
    }
    fn after() -> Option<(String, i64)> {
        Some(("2026-11-24T00:00:00.000Z".to_owned(), 1))
    }
    let sql = |f: fn(&mut PageQuery)| {
        let mut q = PageQuery {
            limit: LIMIT,
            ..PageQuery::default()
        };
        f(&mut q);
        q.builder().into_string()
    };
    vec![
        ("plain", sql(|_| {})),
        ("oldest", sql(|q| q.oldest = true)),
        ("project", sql(|q| q.project = x())),
        ("project_key", sql(|q| (q.project, q.key) = (x(), x()))),
        (
            "project_key_kind",
            sql(|q| (q.project, q.key, q.kind) = (x(), x(), x())),
        ),
        ("kind", sql(|q| q.kind = x())),
        ("role", sql(|q| q.role = x())),
        ("workspace", sql(|q| q.workspace = Some(String::new()))),
        ("verdict", sql(|q| q.verdict = x())),
        ("actor", sql(|q| q.actor = x())),
        (
            "waiting",
            sql(|q| q.reply = Some((Reply::Waiting, Party::Pm))),
        ),
        (
            "waiting_review",
            sql(|q| q.reply = Some((Reply::Waiting, Party::Review))),
        ),
        (
            "answered",
            sql(|q| q.reply = Some((Reply::Answered, Party::Pm))),
        ),
        ("fts", sql(|q| q.q = Some("絞り込み 並べ替え".to_owned()))),
        ("scan", sql(|q| q.q = Some("UI".to_owned()))),
        ("after", sql(|q| q.after = after())),
        (
            "after_oldest",
            sql(|q| (q.oldest, q.after) = (true, after())),
        ),
        (
            "all",
            sql(|q| {
                q.project = x();
                q.key = x();
                q.kind = x();
                q.role = x();
                q.workspace = x();
                q.q = Some("絞り込み".to_owned());
                q.reply = Some((Reply::Answered, Party::Tpm));
                q.verdict = x();
                q.actor = x();
                q.after = after();
            }),
        ),
    ]
}

/// file を探す SQL（名と SQL）。まだ作っていない（試験を先に書くための形だけ）。
pub fn file_plans() -> Vec<(&'static str, String)> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::search;

    #[test]
    fn long_terms_become_quoted_phrases() {
        assert_eq!(
            search(Some(" 絞り込み a\"b\"c ")),
            (Some("\"絞り込み\" \"a\"\"b\"\"c\"".into()), None)
        );
    }

    #[test]
    fn a_short_term_falls_back_to_a_scan() {
        assert_eq!(search(Some("一覧 UI")), (None, Some("一覧 UI".into())));
        assert_eq!(search(Some("  ")), (None, None));
        assert_eq!(search(None), (None, None));
    }
}
