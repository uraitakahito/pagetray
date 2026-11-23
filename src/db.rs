//! 接続・移行・SQL。SQL は `query!` が build の時に検査する（`.sqlx/` の file を使う）。

use pagetray_api::{ActorRecord, FileRecord, SenderRecord, file_path};
use serde::Serialize;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{SqliteConnection, SqlitePool};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// 読む接続の数。
const READS: u32 = 4;

#[derive(Clone)]
pub struct Db {
    read: SqlitePool,
    /// 書く接続は 1 本だけ。番号（今の最大 ＋ 1）を取る所が 1 列に並ぶ。
    write: SqlitePool,
    path: PathBuf,
}

pub struct NewPage<'a> {
    pub project: &'a str,
    pub key: &'a str,
    pub kind: &'a str,
    pub role: &'a str,
    pub workspace: &'a str,
    pub title: &'a str,
    pub text: &'a str,
    pub body: &'a [u8],
    /// 取り込みの時だけ。無ければサーバーが決める。
    pub n: Option<i64>,
    pub created_at: Option<&'a str>,
    /// 送り手。あれば senders に 1 行足し、ページから指す（同じ取引）。
    pub sender: Option<&'a SenderRecord>,
}

#[derive(Debug, Serialize)]
pub struct PageMeta {
    pub project: String,
    pub key: String,
    pub kind: String,
    pub n: i64,
    pub title: String,
    pub role: String,
    pub workspace: String,
    pub created_at: String,
    pub size: i64,
    /// 返事待ちにした時刻。返事を求めていなければ無い。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asked_at: Option<String>,
    /// 答えた時刻。まだ答えていなければ無い。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answered_at: Option<String>,
    /// 誰の返事を待つか（`review`・`tpm`）。Product Manager を待つ・返事を求めていないページには無い。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub waits_on: Option<String>,
    /// レビューの判定。判定を書いていなければ無い。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verdict: Option<String>,
    /// 見た物・見てほしい物（書いた順）。無ければ欄が無い。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<String>,
    /// 送り手。置いた時に付随する id が無ければ無い。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sender: Option<SenderRecord>,
}

/// SQL から読んだページの行（送り手の列は senders を left join した物）。
struct PageRow {
    project: String,
    key: String,
    kind: String,
    n: i64,
    title: String,
    role: String,
    workspace: String,
    created_at: String,
    size: i64,
    asked_at: Option<String>,
    answered_at: Option<String>,
    waits_on: Option<String>,
    verdict: Option<String>,
    /// 見た物を改行でつないだ物（無ければ空）。
    targets: Option<String>,
    sender_issuer: Option<String>,
    sender_ref: Option<String>,
    actor_id: Option<String>,
    actor_name: Option<String>,
    actor_kind: Option<String>,
    resolved_at: Option<String>,
}

impl From<PageRow> for PageMeta {
    fn from(r: PageRow) -> Self {
        Self {
            sender: sender(
                r.sender_issuer,
                r.sender_ref,
                r.actor_id,
                r.actor_name,
                r.actor_kind,
                r.resolved_at,
            ),
            project: r.project,
            key: r.key,
            kind: r.kind,
            n: r.n,
            title: r.title,
            role: r.role,
            workspace: r.workspace,
            created_at: r.created_at,
            size: r.size,
            asked_at: r.asked_at,
            answered_at: r.answered_at,
            waits_on: r.waits_on,
            verdict: r.verdict,
            targets: r
                .targets
                .map(|t| t.split('\n').map(str::to_owned).collect())
                .unwrap_or_default(),
        }
    }
}

/// 取引の中で、ページ 1 枚の記録を読む（送り手と見た物も入れる。update の returning では引けないため）。
async fn page_by_id(tx: &mut SqliteConnection, id: i64) -> Result<PageMeta, sqlx::Error> {
    sqlx::query_as!(
        PageRow,
        r#"select p.project, p.key, p.kind, p.n, p.title, p.role, p.workspace, p.created_at,
                  length(p.body) as "size!: i64", p.asked_at, p.answered_at, p.waits_on, p.verdict,
                  (select group_concat(target, char(10)) from
                     (select target from page_targets where page_id = p.id order by rowid))
                    as "targets?: String",
                  s.actor_issuer as "sender_issuer?: String", s.actor_ref as "sender_ref?: String",
                  s.actor_id as "actor_id?: String", s.actor_name as "actor_name?: String",
                  s.actor_kind as "actor_kind?: String", s.resolved_at as "resolved_at?: String"
           from pages p left join senders s on s.id = p.sender_id
           where p.id = ?1"#,
        id,
    )
    .fetch_one(tx)
    .await
    .map(PageMeta::from)
}

/// senders の列から、送り手の記録を組む。付随する id が無ければ（送り手の無い行）無い。
/// アクターの列が揃っていなければ、castlist に聞いていない（未確認）。
fn sender(
    issuer: Option<String>,
    actor_ref: Option<String>,
    id: Option<String>,
    name: Option<String>,
    kind: Option<String>,
    resolved_at: Option<String>,
) -> Option<SenderRecord> {
    let actor = match (id, name, kind) {
        (Some(id), Some(name), Some(kind)) => Some(ActorRecord { id, name, kind }),
        _ => None,
    };
    Some(SenderRecord {
        issuer: issuer?,
        actor_ref: actor_ref?,
        actor,
        resolved_at,
    })
}

/// 送り手があれば senders に 1 行足し、その id を返す。ページや file と同じ取引の中で呼ぶ。
async fn add_sender(
    tx: &mut SqliteConnection,
    s: Option<&SenderRecord>,
) -> Result<Option<i64>, sqlx::Error> {
    let Some(s) = s else {
        return Ok(None);
    };
    let actor = s.actor.as_ref();
    let (id, name, kind) = (
        actor.map(|a| a.id.as_str()),
        actor.map(|a| a.name.as_str()),
        actor.map(|a| a.kind.as_str()),
    );
    sqlx::query_scalar!(
        r#"insert into senders(actor_issuer, actor_ref, actor_id, actor_name, actor_kind, resolved_at,
                               created_at)
           values(?1, ?2, ?3, ?4, ?5, ?6, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
           returning id as "id!: i64""#,
        s.issuer,
        s.actor_ref,
        id,
        name,
        kind,
        s.resolved_at,
    )
    .fetch_one(tx)
    .await
    .map(Some)
}

/// ページの返事の状態。返事待ち（求めて、まだ答えていない）か、答えたか。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reply {
    Waiting,
    Answered,
}

impl Reply {
    /// API の引数と SQL で使う名。
    pub fn name(self) -> &'static str {
        match self {
            Reply::Waiting => "waiting",
            Reply::Answered => "answered",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "waiting" => Some(Reply::Waiting),
            "answered" => Some(Reply::Answered),
            _ => None,
        }
    }
}

/// 誰の返事を待つか。DB では Product Manager を空で持つ（前からあるページと同じ）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WaitsOn {
    #[default]
    Pm,
    Review,
    Tpm,
}

impl WaitsOn {
    /// API の引数（`on`）の名。
    pub fn name(self) -> &'static str {
        match self {
            WaitsOn::Pm => "pm",
            WaitsOn::Review => "review",
            WaitsOn::Tpm => "tpm",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "pm" => Some(WaitsOn::Pm),
            "review" => Some(WaitsOn::Review),
            "tpm" => Some(WaitsOn::Tpm),
            _ => None,
        }
    }

    /// DB の `waits_on` の値（Product Manager は空）。
    fn column(self) -> Option<&'static str> {
        (self != WaitsOn::Pm).then(|| self.name())
    }

    /// DB の `waits_on` の値から読む。
    pub fn of(column: Option<&str>) -> Self {
        column.and_then(Self::parse).unwrap_or_default()
    }
}

/// 返事の状態を書いた結果。
pub enum ReplyOutcome {
    Done(Box<PageMeta>),
    /// そのページは無い。
    Missing,
    /// 答えた相手が、そのページの待ちの相手と違う（何も替えていない）。
    Conflict,
}

#[derive(Default)]
pub struct Filter {
    pub project: Option<String>,
    pub key: Option<String>,
    pub kind: Option<String>,
    pub role: Option<String>,
    pub workspace: Option<String>,
    pub q: Option<String>,
    /// 返事の状態で絞る。
    pub reply: Option<Reply>,
    /// 返事の状態で絞る時の、待ちの相手。
    pub on: WaitsOn,
    /// レビューの判定で絞る。
    pub verdict: Option<String>,
    /// 送り手（castlist のアクターの id）で絞る。
    pub actor: Option<String>,
    pub before: Option<String>,
    pub after: Option<String>,
    pub oldest_first: bool,
    pub limit: i64,
}

/// 消すページ。project は必ず。
pub struct Target {
    pub project: String,
    pub key: Option<String>,
    pub kind: Option<String>,
    pub n: Option<i64>,
}

pub struct NewFile<'a> {
    pub project: &'a str,
    pub key: &'a str,
    pub name: &'a str,
    pub ext: &'a str,
    pub r#type: &'a str,
    pub sha256: &'a str,
    pub role: &'a str,
    pub workspace: &'a str,
    pub body: &'a [u8],
    /// 取り込みの時だけ。無ければサーバーが決める。
    pub n: Option<i64>,
    pub created_at: Option<&'a str>,
    /// 送り手。あれば senders に 1 行足し、file から指す（同じ取引）。
    pub sender: Option<&'a SenderRecord>,
}

/// file を探す時の絞り込み。
#[derive(Default)]
pub struct FileFilter {
    pub project: Option<String>,
    pub key: Option<String>,
    pub role: Option<String>,
    pub workspace: Option<String>,
    /// 名（`<語>.<拡張子>`）に含まれる字。
    pub q: Option<String>,
    /// 送り手（castlist のアクターの id）で絞る。
    pub actor: Option<String>,
    pub before: Option<String>,
    pub after: Option<String>,
    pub oldest_first: bool,
    pub limit: i64,
}

/// 消す file。project は必ず。name と ext は、両方あるか両方無いか。
pub struct FileTarget {
    pub project: String,
    pub key: Option<String>,
    pub name: Option<String>,
    pub ext: Option<String>,
    pub n: Option<i64>,
}

/// 絞り込みの候補 1 つ（値・件数・添える字）。
#[derive(Debug)]
pub struct Count {
    pub value: String,
    pub count: i64,
    pub note: String,
}

/// 絞り込みの候補。DB にある値から作る。
#[derive(Debug)]
pub struct Facets {
    /// 送り手（値は castlist のアクターの id、添える字はその名）。castlist に聞けた送り手だけ。
    pub senders: Vec<Count>,
    pub roles: Vec<Count>,
    pub workspaces: Vec<Count>,
    pub kinds: Vec<Count>,
    pub keys: Vec<Count>,
    pub projects: Vec<Count>,
    /// Product Manager の返事待ちのページの数。
    pub waiting: i64,
    /// Product Manager が答えたページの数。
    pub answered: i64,
    /// レビュー待ちのページ（レビューの依頼書）の数。
    pub review_waiting: i64,
    /// TPM 担当に上げて、まだ決まっていないページ（レビューのページ）の数。
    pub escalated: i64,
    /// レビューの判定ごとの数。
    pub verdicts: Vec<Count>,
}

/// 見られたページから見た、そのページを見たレビュー 1 つ。
#[derive(Debug)]
pub struct ReviewOf {
    /// 見られたページの path。
    pub target: String,
    pub project: String,
    pub key: String,
    pub n: i64,
    pub verdict: Option<String>,
}

#[derive(Debug)]
pub enum PutError {
    /// 指定した番号は、もう使われている。
    Taken,
    Db(sqlx::Error),
}

impl Db {
    pub async fn open(path: &Path) -> Result<Self, sqlx::Error> {
        // 外部キーと trigger の決まりは接続ごとに効くので、読む接続にも書く接続にも、はっきり付ける。
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .foreign_keys(true)
            .pragma("recursive_triggers", "ON");
        let write = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options.clone())
            .await?;
        sqlx::migrate!().run(&write).await?;
        // 外部キーの合わない行があれば起きない（外部キーを切った接続で書かれた DB を、そのまま使わない）。
        let broken = sqlx::query("pragma foreign_key_check")
            .fetch_all(&write)
            .await?;
        if !broken.is_empty() {
            write.close().await;
            return Err(sqlx::Error::Protocol(format!(
                "外部キーの合わない行が {} ある（PRAGMA foreign_key_check）。起きない",
                broken.len()
            )));
        }
        let read = SqlitePoolOptions::new()
            .max_connections(READS)
            .connect_with(options.read_only(true))
            .await?;
        Ok(Self {
            read,
            write,
            path: path.to_owned(),
        })
    }

    pub async fn close(&self) {
        self.read.close().await;
        self.write.close().await;
    }

    pub async fn put(&self, page: NewPage<'_>) -> Result<PageMeta, PutError> {
        let mut tx = self.write.begin().await.map_err(PutError::Db)?;
        // 消した番号も、もう使われている番号として扱う。
        if let Some(n) = page.n {
            let removed = sqlx::query_scalar!(
                "select 1 from removed where project = ?1 and key = ?2 and kind = ?3 and n = ?4",
                page.project,
                page.key,
                page.kind,
                n,
            )
            .fetch_optional(&mut *tx)
            .await
            .map_err(PutError::Db)?;
            if removed.is_some() {
                return Err(PutError::Taken);
            }
        }
        let sender_id = add_sender(&mut tx, page.sender)
            .await
            .map_err(PutError::Db)?;
        let row = sqlx::query!(
            r#"insert into pages(project, key, kind, n, title, role, workspace, created_at, body, sender_id)
               values(?1, ?2, ?3,
                      coalesce(?4, (select coalesce(max(n), 0) + 1 from
                                      (select n from pages where project = ?1 and key = ?2 and kind = ?3
                                       union all
                                       select n from removed where project = ?1 and key = ?2 and kind = ?3))),
                      ?5, ?6, ?7,
                      coalesce(?8, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                      ?9, ?10)
               returning id as "id!: i64", n as "n!: i64", created_at as "created_at!: String""#,
            page.project,
            page.key,
            page.kind,
            page.n,
            page.title,
            page.role,
            page.workspace,
            page.created_at,
            page.body,
            sender_id,
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| match e.as_database_error() {
            Some(d) if d.is_unique_violation() => PutError::Taken,
            _ => PutError::Db(e),
        })?;
        sqlx::query!(
            "insert into pages_fts(rowid, title, text) values(?1, ?2, ?3)",
            row.id,
            page.title,
            page.text,
        )
        .execute(&mut *tx)
        .await
        .map_err(PutError::Db)?;
        tx.commit().await.map_err(PutError::Db)?;
        Ok(PageMeta {
            project: page.project.to_owned(),
            key: page.key.to_owned(),
            kind: page.kind.to_owned(),
            n: row.n,
            title: page.title.to_owned(),
            role: page.role.to_owned(),
            workspace: page.workspace.to_owned(),
            created_at: row.created_at,
            size: page.body.len() as i64,
            asked_at: None,
            answered_at: None,
            waits_on: None,
            verdict: None,
            targets: Vec::new(),
            sender: page.sender.cloned(),
        })
    }

    /// ページ 1 枚の返事の状態を書く。同じ状態を 2 度書いても、時刻は初めの物を保つ。
    /// - 返事待ち: 待ちの相手を `on` にし、答えた時刻を空にする。答えた後にもう 1 度求めた時は、求めた時刻を今にする。
    /// - 答えた: 答えた時刻を入れる（求めた時刻が空なら、同じ時刻を入れる）。待ちの相手が `on` と違えば何も替えない
    ///   （[`ReplyOutcome::Conflict`]）。Product Manager のほかの相手は、返事待ちにしたページだけを答えたにできる。
    pub async fn set_reply(
        &self,
        project: &str,
        key: &str,
        kind: &str,
        n: i64,
        reply: Reply,
        on: WaitsOn,
    ) -> Result<ReplyOutcome, sqlx::Error> {
        let answered = reply == Reply::Answered;
        let mut tx = self.write.begin().await?;
        let Some(now) = sqlx::query!(
            r#"select id as "id!: i64", waits_on, asked_at from pages
               where project = ?1 and key = ?2 and kind = ?3 and n = ?4"#,
            project,
            key,
            kind,
            n,
        )
        .fetch_optional(&mut *tx)
        .await?
        else {
            return Ok(ReplyOutcome::Missing);
        };
        if answered
            && (WaitsOn::of(now.waits_on.as_deref()) != on
                || (on != WaitsOn::Pm && now.asked_at.is_none()))
        {
            return Ok(ReplyOutcome::Conflict);
        }
        let waits_on = on.column();
        sqlx::query!(
            r#"update pages
               set asked_at = case when asked_at is not null and (?2 or answered_at is null)
                                   then asked_at
                                   else strftime('%Y-%m-%dT%H:%M:%fZ', 'now') end,
                   answered_at = case when ?2
                                      then coalesce(answered_at, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                                 end,
                   waits_on = case when ?2 then waits_on else ?3 end
               where id = ?1"#,
            now.id,
            answered,
            waits_on,
        )
        .execute(&mut *tx)
        .await?;
        let meta = page_by_id(&mut tx, now.id).await?;
        tx.commit().await?;
        Ok(ReplyOutcome::Done(Box::new(meta)))
    }

    /// レビューの印を書く: 判定（書けば替える）と見た物（足すだけ。同じ物は 1 つ）。無いページなら None。
    /// 判定を書けるページの種類と、見た物の形は、呼ぶ側（API）が見る。
    pub async fn set_review(
        &self,
        project: &str,
        key: &str,
        kind: &str,
        n: i64,
        verdict: Option<&str>,
        targets: &[String],
    ) -> Result<Option<PageMeta>, sqlx::Error> {
        let mut tx = self.write.begin().await?;
        let Some(id) = sqlx::query_scalar!(
            r#"update pages set verdict = coalesce(?5, verdict)
               where project = ?1 and key = ?2 and kind = ?3 and n = ?4
               returning id as "id!: i64""#,
            project,
            key,
            kind,
            n,
            verdict,
        )
        .fetch_optional(&mut *tx)
        .await?
        else {
            return Ok(None);
        };
        for target in targets {
            sqlx::query!(
                "insert into page_targets(page_id, target) values(?1, ?2) on conflict do nothing",
                id,
                target,
            )
            .execute(&mut *tx)
            .await?;
        }
        let meta = page_by_id(&mut tx, id).await?;
        tx.commit().await?;
        Ok(Some(meta))
    }

    /// 見られたページ（path）ごとに、そのページを見たレビューのページ（kind が review。古い順）。
    pub async fn reviews_of(&self, paths: &[String]) -> Result<Vec<ReviewOf>, sqlx::Error> {
        if paths.is_empty() {
            return Ok(Vec::new());
        }
        let paths = serde_json::to_string(paths).unwrap_or_default();
        sqlx::query_as!(
            ReviewOf,
            r#"select t.target as "target!: String", p.project as "project!: String",
                      p.key as "key!: String", p.n as "n!: i64", p.verdict
               from page_targets t join pages p on p.id = t.page_id
               where p.kind = 'review' and t.target in (select value from json_each(?1))
               order by p.created_at, p.id"#,
            paths,
        )
        .fetch_all(&self.read)
        .await
    }

    pub async fn body(
        &self,
        project: &str,
        key: &str,
        kind: &str,
        n: i64,
    ) -> Result<Option<Vec<u8>>, sqlx::Error> {
        sqlx::query_scalar!(
            "select body from pages where project = ?1 and key = ?2 and kind = ?3 and n = ?4",
            project,
            key,
            kind,
            n,
        )
        .fetch_optional(&self.read)
        .await
    }

    /// 消したページなら、消した時刻。
    pub async fn removed_at(
        &self,
        project: &str,
        key: &str,
        kind: &str,
        n: i64,
    ) -> Result<Option<String>, sqlx::Error> {
        sqlx::query_scalar!(
            "select removed_at from removed where project = ?1 and key = ?2 and kind = ?3 and n = ?4",
            project,
            key,
            kind,
            n,
        )
        .fetch_optional(&self.read)
        .await
    }

    /// 合うページを消し（全文の索引の行も）、番号を removed に残す。消したページの path を返す。
    /// key・kind・n は、無ければ問わない（project の中を、Key ごと・種類ごとにまとめて消せる）。
    pub async fn remove(&self, t: &Target) -> Result<Vec<String>, sqlx::Error> {
        let mut tx = self.write.begin().await?;
        let rows = sqlx::query!(
            r#"insert into removed(project, key, kind, n, removed_at)
               select project, key, kind, n, strftime('%Y-%m-%dT%H:%M:%fZ', 'now') from pages
               where project = ?1 and (?2 is null or key = ?2)
                 and (?3 is null or kind = ?3) and (?4 is null or n = ?4)
               order by id
               returning project as "project!: String", key as "key!: String",
                         kind as "kind!: String", n as "n!: i64""#,
            t.project,
            t.key,
            t.kind,
            t.n,
        )
        .fetch_all(&mut *tx)
        .await?;
        sqlx::query!(
            r#"delete from pages_fts where rowid in
                 (select id from pages where project = ?1 and (?2 is null or key = ?2)
                    and (?3 is null or kind = ?3) and (?4 is null or n = ?4))"#,
            t.project,
            t.key,
            t.kind,
            t.n,
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query!(
            r#"delete from pages where project = ?1 and (?2 is null or key = ?2)
                 and (?3 is null or kind = ?3) and (?4 is null or n = ?4)"#,
            t.project,
            t.key,
            t.kind,
            t.n,
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(rows
            .into_iter()
            .map(|r| crate::api::page_path(&r.project, &r.key, &r.kind, r.n))
            .collect())
    }

    pub async fn list(&self, f: &Filter) -> Result<Vec<PageMeta>, sqlx::Error> {
        let (fts, scan) = search(f.q.as_deref());
        let reply = f.reply.map(Reply::name);
        let on = f.on.name();
        let rows = sqlx::query_as!(
            PageRow,
            r#"select p.project, p.key, p.kind, p.n, p.title, p.role, p.workspace, p.created_at,
                      length(p.body) as "size!: i64", p.asked_at, p.answered_at, p.waits_on, p.verdict,
                      (select group_concat(target, char(10)) from
                         (select target from page_targets where page_id = p.id order by rowid))
                        as "targets?: String",
                      s.actor_issuer as "sender_issuer?: String", s.actor_ref as "sender_ref?: String",
                      s.actor_id as "actor_id?: String", s.actor_name as "actor_name?: String",
                      s.actor_kind as "actor_kind?: String", s.resolved_at as "resolved_at?: String"
               from pages p left join senders s on s.id = p.sender_id
               where (?1 is null or p.project = ?1)
                 and (?2 is null or p.key = ?2)
                 and (?3 is null or p.kind = ?3)
                 and (?4 is null or p.role = ?4)
                 and (?5 is null or p.workspace = ?5)
                 and (?6 is null or p.id in (select rowid from pages_fts where pages_fts match ?6))
                 and (?7 is null or p.id in (select rowid from pages_fts
                                             where instr(title, ?7) > 0 or instr(text, ?7) > 0))
                 and (?8 is null or p.created_at < ?8)
                 and (?9 is null or p.created_at > ?9)
                 and (?12 is null
                      or (coalesce(p.waits_on, 'pm') = ?14
                          and ((?12 = 'waiting' and p.asked_at is not null and p.answered_at is null)
                               or (?12 = 'answered' and p.answered_at is not null))))
                 and (?13 is null or p.sender_id in (select id from senders where actor_id = ?13))
                 and (?15 is null or p.verdict = ?15)
               order by case when ?10 then p.created_at end asc,
                        case when ?10 then p.id end asc,
                        p.created_at desc, p.id desc
               limit ?11"#,
            f.project,
            f.key,
            f.kind,
            f.role,
            f.workspace,
            fts,
            scan,
            f.before,
            f.after,
            f.oldest_first,
            f.limit,
            reply,
            f.actor,
            on,
            f.verdict,
        )
        .fetch_all(&self.read)
        .await?;
        Ok(rows.into_iter().map(PageMeta::from).collect())
    }

    /// file を置く。番号はページと同じく「置いてある分と消した分の最大 ＋ 1」。
    pub async fn put_file(&self, file: NewFile<'_>) -> Result<FileRecord, PutError> {
        let mut tx = self.write.begin().await.map_err(PutError::Db)?;
        // 消した番号も、もう使われている番号として扱う。
        if let Some(n) = file.n {
            let removed = sqlx::query_scalar!(
                r#"select 1 from files_removed
                   where project = ?1 and key = ?2 and name = ?3 and ext = ?4 and n = ?5"#,
                file.project,
                file.key,
                file.name,
                file.ext,
                n,
            )
            .fetch_optional(&mut *tx)
            .await
            .map_err(PutError::Db)?;
            if removed.is_some() {
                return Err(PutError::Taken);
            }
        }
        let size = file.body.len() as i64;
        let sender_id = add_sender(&mut tx, file.sender)
            .await
            .map_err(PutError::Db)?;
        let row = sqlx::query!(
            r#"insert into files(project, key, name, ext, n, type, size, sha256, role, workspace,
                                 created_at, body, sender_id)
               values(?1, ?2, ?3, ?4,
                      coalesce(?5, (select coalesce(max(n), 0) + 1 from
                                      (select n from files
                                       where project = ?1 and key = ?2 and name = ?3 and ext = ?4
                                       union all
                                       select n from files_removed
                                       where project = ?1 and key = ?2 and name = ?3 and ext = ?4))),
                      ?6, ?7, ?8, ?9, ?10,
                      coalesce(?11, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                      ?12, ?13)
               returning n as "n!: i64", created_at as "created_at!: String""#,
            file.project,
            file.key,
            file.name,
            file.ext,
            file.n,
            file.r#type,
            size,
            file.sha256,
            file.role,
            file.workspace,
            file.created_at,
            file.body,
            sender_id,
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| match e.as_database_error() {
            Some(d) if d.is_unique_violation() => PutError::Taken,
            _ => PutError::Db(e),
        })?;
        tx.commit().await.map_err(PutError::Db)?;
        Ok(FileRecord {
            project: file.project.to_owned(),
            key: file.key.to_owned(),
            name: file.name.to_owned(),
            ext: file.ext.to_owned(),
            n: row.n,
            r#type: file.r#type.to_owned(),
            size,
            sha256: file.sha256.to_owned(),
            role: file.role.to_owned(),
            workspace: file.workspace.to_owned(),
            created_at: row.created_at,
            path: file_path(file.project, file.key, file.name, file.ext, row.n),
            url: None,
            sender: file.sender.cloned(),
        })
    }

    /// file の Content-Type と中身。
    pub async fn file_body(
        &self,
        project: &str,
        key: &str,
        name: &str,
        ext: &str,
        n: i64,
    ) -> Result<Option<(String, Vec<u8>)>, sqlx::Error> {
        let row = sqlx::query!(
            r#"select type, body from files
               where project = ?1 and key = ?2 and name = ?3 and ext = ?4 and n = ?5"#,
            project,
            key,
            name,
            ext,
            n,
        )
        .fetch_optional(&self.read)
        .await?;
        Ok(row.map(|r| (r.r#type, r.body)))
    }

    /// 消した file なら、消した時刻。
    pub async fn file_removed_at(
        &self,
        project: &str,
        key: &str,
        name: &str,
        ext: &str,
        n: i64,
    ) -> Result<Option<String>, sqlx::Error> {
        sqlx::query_scalar!(
            r#"select removed_at from files_removed
               where project = ?1 and key = ?2 and name = ?3 and ext = ?4 and n = ?5"#,
            project,
            key,
            name,
            ext,
            n,
        )
        .fetch_optional(&self.read)
        .await
    }

    /// 合う file を消し、番号を files_removed に残す。消した file の path を返す。
    pub async fn remove_files(&self, t: &FileTarget) -> Result<Vec<String>, sqlx::Error> {
        let mut tx = self.write.begin().await?;
        let rows = sqlx::query!(
            r#"insert into files_removed(project, key, name, ext, n, removed_at)
               select project, key, name, ext, n, strftime('%Y-%m-%dT%H:%M:%fZ', 'now') from files
               where project = ?1 and (?2 is null or key = ?2)
                 and (?3 is null or (name = ?3 and ext = ?4)) and (?5 is null or n = ?5)
               order by id
               returning project as "project!: String", key as "key!: String",
                         name as "name!: String", ext as "ext!: String", n as "n!: i64""#,
            t.project,
            t.key,
            t.name,
            t.ext,
            t.n,
        )
        .fetch_all(&mut *tx)
        .await?;
        sqlx::query!(
            r#"delete from files where project = ?1 and (?2 is null or key = ?2)
                 and (?3 is null or (name = ?3 and ext = ?4)) and (?5 is null or n = ?5)"#,
            t.project,
            t.key,
            t.name,
            t.ext,
            t.n,
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(rows
            .into_iter()
            .map(|r| file_path(&r.project, &r.key, &r.name, &r.ext, r.n))
            .collect())
    }

    pub async fn list_files(&self, f: &FileFilter) -> Result<Vec<FileRecord>, sqlx::Error> {
        let q = f.q.as_deref().map(str::trim).filter(|q| !q.is_empty());
        let rows = sqlx::query!(
            r#"select f.project, f.key, f.name, f.ext, f.n, f.type, f.size, f.sha256, f.role,
                      f.workspace, f.created_at,
                      s.actor_issuer as "sender_issuer?: String", s.actor_ref as "sender_ref?: String",
                      s.actor_id as "actor_id?: String", s.actor_name as "actor_name?: String",
                      s.actor_kind as "actor_kind?: String", s.resolved_at as "resolved_at?: String"
               from files f left join senders s on s.id = f.sender_id
               where (?1 is null or f.project = ?1)
                 and (?2 is null or f.key = ?2)
                 and (?3 is null or f.role = ?3)
                 and (?4 is null or f.workspace = ?4)
                 and (?5 is null or instr(f.name || '.' || f.ext, lower(?5)) > 0)
                 and (?6 is null or f.created_at < ?6)
                 and (?7 is null or f.created_at > ?7)
                 and (?10 is null or f.sender_id in (select id from senders where actor_id = ?10))
               order by case when ?8 then f.created_at end asc,
                        case when ?8 then f.id end asc,
                        f.created_at desc, f.id desc
               limit ?9"#,
            f.project,
            f.key,
            f.role,
            f.workspace,
            q,
            f.before,
            f.after,
            f.oldest_first,
            f.limit,
            f.actor,
        )
        .fetch_all(&self.read)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| FileRecord {
                sender: sender(
                    r.sender_issuer,
                    r.sender_ref,
                    r.actor_id,
                    r.actor_name,
                    r.actor_kind,
                    r.resolved_at,
                ),
                path: file_path(&r.project, &r.key, &r.name, &r.ext, r.n),
                project: r.project,
                key: r.key,
                name: r.name,
                ext: r.ext,
                n: r.n,
                r#type: r.r#type,
                size: r.size,
                sha256: r.sha256,
                role: r.role,
                workspace: r.workspace,
                created_at: r.created_at,
                url: None,
            })
            .collect())
    }

    pub async fn facets(&self) -> Result<Facets, sqlx::Error> {
        let db = &self.read;
        let senders = sqlx::query_as!(
            Count,
            r#"select s.actor_id as "value!: String", count(*) as "count!: i64",
                      max(s.actor_name) as "note!: String"
               from pages p join senders s on s.id = p.sender_id
               where s.actor_id is not null
               group by s.actor_id order by 2 desc, 3 limit 100"#
        );
        let roles = sqlx::query_as!(
            Count,
            r#"select role as "value!: String", count(*) as "count!: i64", '' as "note!: String"
               from pages group by role order by 2 desc, 1"#
        );
        // workspace の ID だけでは何の居場所かわからないので、その ID で置かれた Key を添える。
        let workspaces = sqlx::query_as!(
            Count,
            r#"select workspace as "value!: String", count(*) as "count!: i64",
                      case when workspace = '' then ''
                           else replace(group_concat(distinct key), ',', ', ') end as "note!: String"
               from pages group by workspace order by max(created_at) desc limit 100"#
        );
        let kinds = sqlx::query_as!(
            Count,
            r#"select kind as "value!: String", count(*) as "count!: i64", '' as "note!: String"
               from pages group by kind order by 2 desc, 1"#
        );
        let keys = sqlx::query_as!(
            Count,
            r#"select key as "value!: String", count(*) as "count!: i64", '' as "note!: String"
               from pages group by key order by max(created_at) desc limit 100"#
        );
        let projects = sqlx::query_as!(
            Count,
            r#"select project as "value!: String", count(*) as "count!: i64", '' as "note!: String"
               from pages group by project order by 2 desc, 1"#
        );
        // 返事の数は、待ちの相手ごと（Product Manager の数に、レビューと上げた件を混ぜない）。
        let replies = sqlx::query!(
            r#"select count(*) filter (where waits_on is null and asked_at is not null and answered_at is null)
                        as "waiting!: i64",
                      count(answered_at) filter (where waits_on is null) as "answered!: i64",
                      count(*) filter (where waits_on = 'review' and asked_at is not null
                                         and answered_at is null) as "review_waiting!: i64",
                      count(*) filter (where waits_on = 'tpm' and asked_at is not null
                                         and answered_at is null) as "escalated!: i64"
               from pages"#
        );
        let verdicts = sqlx::query_as!(
            Count,
            r#"select verdict as "value!: String", count(*) as "count!: i64", '' as "note!: String"
               from pages where verdict is not null group by verdict order by 2 desc, 1"#
        );
        let (senders, roles, workspaces, kinds, keys, projects, replies, verdicts) = tokio::try_join!(
            senders.fetch_all(db),
            roles.fetch_all(db),
            workspaces.fetch_all(db),
            kinds.fetch_all(db),
            keys.fetch_all(db),
            projects.fetch_all(db),
            replies.fetch_one(db),
            verdicts.fetch_all(db),
        )?;
        Ok(Facets {
            senders,
            roles,
            workspaces,
            kinds,
            keys,
            projects,
            waiting: replies.waiting,
            answered: replies.answered,
            review_waiting: replies.review_waiting,
            escalated: replies.escalated,
            verdicts,
        })
    }

    /// 動かしたまま、DB の写しを 1 つの file に作る。呼んだ側が読み終えたら消す。
    pub async fn backup(&self) -> Result<PathBuf, sqlx::Error> {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let mut name = self.path.clone().into_os_string();
        name.push(format!(
            ".backup-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let out = PathBuf::from(name);
        let _ = std::fs::remove_file(&out);
        let target = out.to_string_lossy().into_owned();
        sqlx::query!("vacuum into ?1", target)
            .execute(&self.write)
            .await?;
        Ok(out)
    }
}

/// 探す語を、全文の索引で探す形（3 字以上の語だけの時）か、端から見る形（短い語がある時）に分ける。
fn search(q: Option<&str>) -> (Option<String>, Option<String>) {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 試験ごとの一時の DB。終わったら消す。
    struct Temp(PathBuf);

    impl Temp {
        fn new(name: &str) -> Self {
            static SEQ: AtomicU64 = AtomicU64::new(0);
            Self(std::env::temp_dir().join(format!(
                "pagetray-db-{name}-{}-{}.db",
                std::process::id(),
                SEQ.fetch_add(1, Ordering::Relaxed)
            )))
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            for suffix in ["", "-wal", "-shm"] {
                let _ = std::fs::remove_file(format!("{}{suffix}", self.0.display()));
            }
        }
    }

    const SENDER: &str = "insert into senders(actor_issuer, actor_ref, created_at)
                          values('tpm', 'claude.session:a', '2026-11-22T00:00:00.000Z')";

    #[tokio::test]
    async fn senders_are_append_only() {
        let temp = Temp::new("append");
        let db = Db::open(&temp.0).await.unwrap();
        let _ = sqlx::query(SENDER).execute(&db.write).await;
        for sql in ["update senders set actor_name = 'x'", "delete from senders"] {
            let err = sqlx::query(sql).execute(&db.write).await.unwrap_err();
            assert!(
                err.to_string().contains("append-only: senders"),
                "{sql}: {err}"
            );
        }
        db.close().await;
    }

    #[tokio::test]
    async fn a_page_cannot_point_to_a_missing_sender() {
        let temp = Temp::new("fk");
        let db = Db::open(&temp.0).await.unwrap();
        for sql in [
            "insert into pages(project, key, kind, n, title, role, workspace, created_at, body, sender_id)
             values('p', 'K', 'plan', 1, '', 'tpm', '', '2026-11-22T00:00:00.000Z', x'00', 999)",
            "insert into files(project, key, name, ext, n, type, size, sha256, role, workspace, created_at, body, sender_id)
             values('p', 'K', 'a', 'txt', 1, 'text/plain', 1, '', 'tpm', '', '2026-11-22T00:00:00.000Z', x'00', 999)",
        ] {
            let err = sqlx::query(sql).execute(&db.write).await.unwrap_err();
            assert!(err.to_string().contains("FOREIGN KEY constraint failed"), "{err}");
        }
        db.close().await;
    }

    /// 読む接続をみな同時に借り、書く接続と合わせて、どれも外部キーと trigger の決まりが効いているか。
    #[tokio::test]
    async fn every_connection_enforces_foreign_keys_and_recursive_triggers() {
        let temp = Temp::new("pragma");
        let db = Db::open(&temp.0).await.unwrap();
        let mut held = Vec::new();
        for _ in 0..READS {
            held.push(db.read.acquire().await.unwrap());
        }
        held.push(db.write.acquire().await.unwrap());
        assert_eq!(held.len() as u32, READS + 1);
        for conn in &mut held {
            for pragma in ["pragma foreign_keys", "pragma recursive_triggers"] {
                let on: i64 = sqlx::query_scalar(pragma)
                    .fetch_one(&mut **conn)
                    .await
                    .unwrap();
                assert_eq!(on, 1, "{pragma}");
            }
        }
        drop(held);
        db.close().await;
    }

    /// 外部キーの合わない行がある DB では、起きない。
    #[tokio::test]
    async fn opening_refuses_a_database_with_broken_foreign_keys() {
        let temp = Temp::new("check");
        Db::open(&temp.0).await.unwrap().close().await;
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(&temp.0)
                    .foreign_keys(false),
            )
            .await
            .unwrap();
        sqlx::query(
            "insert into pages(project, key, kind, n, title, role, workspace, created_at, body, sender_id)
             values('p', 'K', 'plan', 1, '', 'tpm', '', '2026-11-22T00:00:00.000Z', x'00', 999)",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool.close().await;
        let opened = Db::open(&temp.0).await;
        assert!(opened.is_err(), "外部キーの合わない DB で起きた");
    }

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
