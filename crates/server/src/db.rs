//! 接続・移行・SQL。SQL は動く時に組む形（`query`・`query_as`）で書く（`.sqlx/` は要らない）。
//!
//! - 書く接続は 1 本だけ。番号（今の最大 ＋ 1）を取る所が 1 列に並ぶ。
//! - 読む接続は 4 本で、読むだけ。WAL なので、書いている間も待たずに読める。
//! - 接続の決まりと、今の server の DB を断る所は、castlist の新しい server（crates/server/src/db.rs）と同じ。
//! - 返事・印・消すは、書く接続の 1 つの取引で、見る・書く・答えの記録を読むをする（間に別の書く物が入らない）。
//! - file はページと同じ作り（番号は project・key・語・拡張子ごと。消した番号は files_removed）。
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteConnectOptions, SqliteConnection, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{Connection, FromRow, SqlitePool};
use time::OffsetDateTime;

/// 書く接続を待つ長さ。過ぎれば「混んでいる」で返す。問い合わせの時間切れ（[`crate::REQUEST_TIMEOUT`]）より短くし、
/// 混んでいる時は「混んでいる」の 503 が、時間切れの 503 より先に答える。
pub const WRITE_WAIT: Duration = Duration::from_secs(2);
/// 読む接続を待つ長さ。過ぎれば「混んでいる」で返す（時間切れより短い）。
pub const READ_WAIT: Duration = Duration::from_secs(3);
/// 読む接続の本数。
pub const READS: u32 = 4;

/// 新しい server の移行（crates/server/migrations/）。
static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// [`Db::backup`] の失敗。
#[derive(Debug)]
pub enum BackupError {
    /// 写しを作れない（混んでいる・DB の誤り）。
    Db(sqlx::Error),
    /// 作った写しを開けない。
    Io(std::io::Error),
}

pub struct Db {
    read: SqlitePool,
    /// 書く接続は 1 本だけ。
    write: SqlitePool,
    path: PathBuf,
}

/// 置くページの中身（形は定義と handler が見てある）。
pub struct NewPage<'a> {
    pub project: &'a str,
    pub key: &'a str,
    pub kind: &'a str,
    pub role: &'a str,
    pub workspace: &'a str,
    pub title: &'a str,
    /// 全文の索引に入れる字。
    pub text: &'a str,
    pub body: &'a [u8],
    /// 取り込みの時だけ。無ければ server が決める。
    pub n: Option<i64>,
    /// 取り込みの時だけ（[`stamp`] の字）。無ければ今。
    pub created_at: Option<&'a str>,
    /// 送り手。あれば senders に 1 行足し、ページから指す（同じ取引）。
    pub sender: Option<&'a NewSender>,
}

/// 足す送り手の行: 付随する id と、castlist の答え（castlist に聞かなかった時は無い）。
pub struct NewSender {
    pub issuer: String,
    pub actor_ref: String,
    pub answer: Option<crate::castlist::Answer>,
}

/// 置いたページの記録（DB の字のまま）。
#[derive(Debug)]
pub struct Placed {
    pub n: i64,
    pub created_at: String,
    pub size: i64,
}

/// ページの記録の行（送り手の列は senders を left join した物。見た物は改行でつないだ物）。
#[derive(Clone, Debug, FromRow)]
pub struct PageRow {
    pub id: i64,
    pub project: String,
    pub key: String,
    pub kind: String,
    pub n: i64,
    pub title: String,
    pub role: String,
    pub workspace: String,
    pub created_at: String,
    pub size: i64,
    pub asked_at: Option<String>,
    pub answered_at: Option<String>,
    pub waits_on: Option<String>,
    pub verdict: Option<String>,
    pub targets: Option<String>,
    pub actor_issuer: Option<String>,
    pub actor_ref: Option<String>,
    pub actor_id: Option<String>,
    pub actor_name: Option<String>,
    pub actor_kind: Option<String>,
    pub resolved_at: Option<String>,
}

/// [`PageRow`] を読む select（`pages p` と `senders s`。後ろに where を繋ぐ）。
pub const PAGE_ROW: &str = "select p.id, p.project, p.key, p.kind, p.n, p.title, p.role, p.workspace, p.created_at,
        length(p.body) as size, p.asked_at, p.answered_at, p.waits_on, p.verdict,
        (select group_concat(t.target, char(10) order by t.rowid) from page_targets t where t.page_id = p.id) as targets,
        s.actor_issuer, s.actor_ref, s.actor_id, s.actor_name, s.actor_kind, s.resolved_at
 from pages p left join senders s on s.id = p.sender_id";

/// 置く file の中身（形・種類・中身は定義と handler が見てある）。
pub struct NewFile<'a> {
    pub project: &'a str,
    pub key: &'a str,
    /// 名の語。
    pub name: &'a str,
    pub ext: &'a str,
    /// 返す時の Content-Type（拡張子から決まる）。
    pub content_type: &'a str,
    /// 中身の SHA-256（16 進の小文字）。
    pub sha256: &'a str,
    pub role: &'a str,
    pub workspace: &'a str,
    pub body: &'a [u8],
    /// 取り込みの時だけ。無ければ server が決める。
    pub n: Option<i64>,
    /// 取り込みの時だけ（[`stamp`] の字）。無ければ今。
    pub created_at: Option<&'a str>,
    /// 送り手。あれば senders に 1 行足し、file から指す（同じ取引）。
    pub sender: Option<&'a NewSender>,
}

/// file の記録の行（中身は入らない。送り手の列は senders を left join した物）。
#[derive(Clone, Debug, FromRow)]
pub struct FileRow {
    pub id: i64,
    pub project: String,
    pub key: String,
    pub name: String,
    pub ext: String,
    pub n: i64,
    pub content_type: String,
    pub size: i64,
    pub sha256: String,
    pub role: String,
    pub workspace: String,
    pub created_at: String,
    pub actor_issuer: Option<String>,
    pub actor_ref: Option<String>,
    pub actor_id: Option<String>,
    pub actor_name: Option<String>,
    pub actor_kind: Option<String>,
    pub resolved_at: Option<String>,
}

/// [`FileRow`] を読む select（`files f` と `senders s`。後ろに where を繋ぐ）。
pub const FILE_ROW: &str =
    "select f.id, f.project, f.key, f.name, f.ext, f.n, f.type as content_type, f.size, f.sha256,
        f.role, f.workspace, f.created_at,
        s.actor_issuer, s.actor_ref, s.actor_id, s.actor_name, s.actor_kind, s.resolved_at
 from files f left join senders s on s.id = f.sender_id";

/// 名指した file（project・Key・語・拡張子・番号）。
pub struct FileKey<'a> {
    pub project: &'a str,
    pub key: &'a str,
    pub name: &'a str,
    pub ext: &'a str,
    pub n: i64,
}

/// 消す file。`project` は要る。ほかは、書けば絞る（名は語と拡張子の組）。
pub struct FileTarget<'a> {
    pub project: &'a str,
    pub key: Option<&'a str>,
    pub name: Option<(&'a str, &'a str)>,
    pub n: Option<i64>,
}

/// 消した file（project・Key・語・拡張子・番号）。
pub type RemovedFile = (String, String, String, String, i64);

/// 返事の状態。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reply {
    /// 返事待ち（求めて、まだ答えていない）。
    Waiting,
    /// 答えた。
    Answered,
}

impl Reply {
    /// 定義の字（`waiting`・`answered`。形は定義が見てある）。
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "waiting" => Some(Reply::Waiting),
            "answered" => Some(Reply::Answered),
            _ => None,
        }
    }

    /// 定義の字。
    pub fn name(self) -> &'static str {
        match self {
            Reply::Waiting => "waiting",
            Reply::Answered => "answered",
        }
    }
}

/// 返事の待ちの相手。DB では Product Manager を空で持つ（今の server と同じ）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Party {
    #[default]
    Pm,
    Review,
    Tpm,
}

impl Party {
    /// 定義の字（`pm`・`review`・`tpm`。形は定義が見てある）。
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "pm" => Some(Party::Pm),
            "review" => Some(Party::Review),
            "tpm" => Some(Party::Tpm),
            _ => None,
        }
    }

    /// DB の `waits_on` の値（Product Manager は空）。
    pub fn column(self) -> Option<&'static str> {
        match self {
            Party::Pm => None,
            Party::Review => Some("review"),
            Party::Tpm => Some("tpm"),
        }
    }

    /// 定義の字。
    pub fn name(self) -> &'static str {
        match self {
            Party::Pm => "pm",
            Party::Review => "review",
            Party::Tpm => "tpm",
        }
    }

    /// DB の `waits_on` の値から読む（空と知らない字は Product Manager）。
    pub fn of(column: Option<&str>) -> Self {
        column.and_then(Self::parse).unwrap_or_default()
    }
}

/// 名指したページ（project・Key・種類・番号）。
pub struct PageKey<'a> {
    pub project: &'a str,
    pub key: &'a str,
    pub kind: &'a str,
    pub n: i64,
}

/// 返事の状態か印を書いた結果。
pub enum Marked {
    /// 書いた（替わらなかった時も）。書いた後の記録。
    Done(Box<PageRow>),
    /// そのページは無い（消したページも）。
    Missing,
    /// 返事: 待ちの相手が `on` と違う。印: 足すと見た物が上限を超える。どちらも何も替えていない。
    Refused,
}

/// 消すページ。`project` は要る。ほかは、書けば絞る。
pub struct Target<'a> {
    pub project: &'a str,
    pub key: Option<&'a str>,
    pub kind: Option<&'a str>,
    pub n: Option<i64>,
}

/// 消したページ（project・Key・種類・番号）。
pub type Removed = (String, String, String, i64);

/// 書けない訳。
#[derive(Debug)]
pub enum WriteError {
    /// 書く接続が空かない（混んでいる）。
    Busy,
    /// 名指した番号は、置いたか消した番号。
    Taken,
    Db(sqlx::Error),
}

impl From<sqlx::Error> for WriteError {
    fn from(e: sqlx::Error) -> Self {
        match e {
            sqlx::Error::PoolTimedOut => WriteError::Busy,
            e if e
                .as_database_error()
                .is_some_and(|d| d.is_unique_violation()) =>
            {
                WriteError::Taken
            }
            e => WriteError::Db(e),
        }
    }
}

/// DB を開けない訳。
#[derive(Debug)]
pub enum OpenError {
    Db(sqlx::Error),
    Migrate(sqlx::migrate::MigrateError),
    /// 外のキーの崩れた行がある（表と rowid と、指す先の表）。
    ForeignKey(Vec<String>),
    /// 今の server（か別の版）の移行を当てた DB（新しい migrations に無い・中身の違う移行の版）。何も書かずに断る。
    Current(Vec<i64>),
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpenError::Db(e) => write!(f, "DB を開けない: {e}"),
            OpenError::Migrate(e) => write!(f, "DB の移行に落ちた: {e}"),
            OpenError::ForeignKey(rows) => write!(
                f,
                "外のキーの崩れた行があるので開かない: {}",
                rows.join("・")
            ),
            OpenError::Current(versions) => write!(
                f,
                "今の server の DB（新しい server の migrations に無い・中身の違う移行 {} を当ててある）。開かずに止める。\
                 今の DB は、写しを取って新しい volume へ移してから起こす: pagetray-next migrate --from <今の DB の写し> --to <新しい DB>\
                 （image では <image> migrate --from <写し> --to /data/pages.db。/data には新しい volume を付ける）",
                versions
                    .iter()
                    .map(i64::to_string)
                    .collect::<Vec<_>>()
                    .join("・")
            ),
        }
    }
}

impl std::error::Error for OpenError {}

// --- 時刻 ---

/// pagetray の残す時刻の字（UTC・ミリ秒まで・いつも 24 字なので、字の順が時刻の順になる）。
pub fn stamp(t: OffsetDateTime) -> String {
    let t = t.to_offset(time::UtcOffset::UTC);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        t.year().clamp(0, 9999),
        u8::from(t.month()),
        t.day(),
        t.hour(),
        t.minute(),
        t.second(),
        t.millisecond()
    )
}

/// 今の pagetray の時刻（[`stamp`] の字）。
pub fn now() -> String {
    stamp(OffsetDateTime::now_utc())
}

/// 送り手の行を 1 つ足し、その id を返す（足すだけ。置くたびに 1 行で、まとめない）。ページや file と同じ取引の中で呼ぶ。
async fn add_sender(tx: &mut SqliteConnection, sender: &NewSender) -> Result<i64, sqlx::Error> {
    let answer = sender.answer.as_ref();
    sqlx::query_scalar(
        "insert into senders(actor_issuer, actor_ref, actor_id, actor_name, actor_kind, resolved_at, created_at)
         values(?1, ?2, ?3, ?4, ?5, ?6, ?7)
         returning id",
    )
    .bind(&sender.issuer)
    .bind(&sender.actor_ref)
    .bind(answer.map(|a| a.actor.id.as_str()))
    .bind(answer.map(|a| a.actor.name.as_str()))
    .bind(answer.map(|a| a.actor.kind.as_str()))
    .bind(answer.map(|a| a.as_of.as_str()))
    .bind(now())
    .fetch_one(tx)
    .await
}

/// 取引の中で、ページ 1 枚の記録を読む（送り手と見た物も入れる。update の returning では引けないため）。
async fn page_row(tx: &mut SqliteConnection, id: i64) -> Result<PageRow, sqlx::Error> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!("{PAGE_ROW} where p.id = ?1")))
        .bind(id)
        .fetch_one(tx)
        .await
}

// --- 開く ---

/// `path` が、今の server（か別の版）の移行を当てた DB なら [`OpenError::Current`] で断る。読むだけの接続
/// （`read_only`）で開き、DB の中身は書き替えない（WAL の DB は -wal も読む）。file が無い・移行の表が無い時は通す（開く時に作る）。
/// 移行の表の版のうち、新しい migrations に無い物か、中身の写し（checksum）が違う物があれば断る
/// （今の server の DB は 0002〜0006 が無く、0001 も中身が違う。sqlx の移行に任せると、断る前に DB を WAL にする）。
pub async fn refuse_current(path: &Path) -> Result<(), OpenError> {
    if !tokio::fs::try_exists(path).await.unwrap_or(false) {
        return Ok(());
    }
    let mut conn =
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(path).read_only(true))
            .await
            .map_err(OpenError::Db)?;
    let applied: Result<Vec<(i64, Vec<u8>)>, sqlx::Error> = async {
        let has: i64 = sqlx::query_scalar(
            "select count(*) from sqlite_master where type = 'table' and name = '_sqlx_migrations'",
        )
        .fetch_one(&mut conn)
        .await?;
        if has == 0 {
            return Ok(Vec::new());
        }
        sqlx::query_as("select version, checksum from _sqlx_migrations order by version")
            .fetch_all(&mut conn)
            .await
    }
    .await;
    let _ = conn.close().await;
    let foreign: Vec<i64> = applied
        .map_err(OpenError::Db)?
        .into_iter()
        .filter(|(version, checksum)| {
            !MIGRATOR
                .iter()
                .any(|m| m.version == *version && m.checksum.as_ref() == checksum.as_slice())
        })
        .map(|(version, _)| version)
        .collect();
    if foreign.is_empty() {
        Ok(())
    } else {
        Err(OpenError::Current(foreign))
    }
}

impl Db {
    /// 開き、移行し、外のキーの崩れが無いかを見る。崩れがあれば開かない。
    /// 今の server の DB は、何も書かずに断る（[`refuse_current`]）。
    pub async fn open(path: &Path) -> Result<Self, OpenError> {
        refuse_current(path).await?;
        // どの接続にも: 外のキーを効かせ、置き換え（REPLACE）が行を消す時も trigger を動かす。
        // 一時の物は memory に置く（動かす image には /tmp が無い）。
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5))
            .pragma("recursive_triggers", "ON")
            .pragma("temp_store", "MEMORY");
        let write = SqlitePoolOptions::new()
            .max_connections(1)
            .acquire_timeout(WRITE_WAIT)
            .connect_with(options.clone())
            .await
            .map_err(OpenError::Db)?;
        MIGRATOR.run(&write).await.map_err(OpenError::Migrate)?;
        let broken: Vec<(String, Option<i64>, String)> =
            sqlx::query_as("select \"table\", rowid, parent from pragma_foreign_key_check")
                .fetch_all(&write)
                .await
                .map_err(OpenError::Db)?;
        if !broken.is_empty() {
            write.close().await;
            return Err(OpenError::ForeignKey(
                broken
                    .into_iter()
                    .map(|(table, rowid, parent)| {
                        let rowid = rowid.map_or("-".to_owned(), |r| r.to_string());
                        format!("{table} の rowid {rowid}（{parent} を指す）")
                    })
                    .collect(),
            ));
        }
        let read = SqlitePoolOptions::new()
            .max_connections(READS)
            .acquire_timeout(READ_WAIT)
            .connect_with(options.read_only(true))
            .await
            .map_err(OpenError::Db)?;
        Ok(Self {
            read,
            write,
            path: path.to_owned(),
        })
    }

    /// 読む接続（[`READS`] 本・読むだけ）。
    pub fn read(&self) -> &SqlitePool {
        &self.read
    }

    /// 書く接続（1 本だけ）。
    pub fn write(&self) -> &SqlitePool {
        &self.write
    }

    /// DB の file。
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub async fn close(&self) {
        self.read.close().await;
        self.write.close().await;
    }

    /// 揃ったバックアップを、DB の隣の一時の file（`<DB>.backup-<pid>-<番号>`）に `VACUUM INTO` で作って開き、名を消してから
    /// 開いた file を返す（今の server と同じ。開いた後は名が要らず、消しても読み終えるまで中身は残る）。
    /// 読む接続で作る（今の server は書く接続。書く物を、写しを作る間ずっと待たせない）。
    /// 作る・開く・名を消すは、別の task でする。呼んだ側が待つのをやめても（守りの層の時間切れ・client が切った）、
    /// 終わりまで走って名を消すので、一時の file が DB の隣に残らない。
    pub async fn backup(&self) -> Result<std::fs::File, BackupError> {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let mut name = self.path.clone().into_os_string();
        name.push(format!(
            ".backup-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let out = PathBuf::from(name);
        let read = self.read.clone();
        let task = tokio::spawn(async move {
            // VACUUM INTO は、在る file には書かない。
            let _ = tokio::fs::remove_file(&out).await;
            let made = sqlx::query("vacuum into ?1")
                .bind(out.to_string_lossy().into_owned())
                .execute(&read)
                .await;
            if let Err(e) = made {
                // 途中まで書いた file が残っていれば消す。
                let _ = tokio::fs::remove_file(&out).await;
                return Err(BackupError::Db(e));
            }
            let file = std::fs::File::open(&out);
            let _ = tokio::fs::remove_file(&out).await;
            file.map_err(BackupError::Io)
        });
        task.await
            .map_err(|e| BackupError::Io(std::io::Error::other(e)))?
    }

    /// ページを 1 枚置き、全文の索引にも入れる（1 つの transaction）。
    /// 番号を名指せば、置いた番号・消した番号なら [`WriteError::Taken`]（何も替えない）。
    pub async fn put_page(&self, page: &NewPage<'_>) -> Result<Placed, WriteError> {
        let mut tx = self.write.begin().await?;
        // 消した番号も、もう使われている番号として扱う。
        if let Some(n) = page.n {
            let removed: Option<i64> = sqlx::query_scalar(
                "select 1 from removed where project = ?1 and key = ?2 and kind = ?3 and n = ?4",
            )
            .bind(page.project)
            .bind(page.key)
            .bind(page.kind)
            .bind(n)
            .fetch_optional(&mut *tx)
            .await?;
            if removed.is_some() {
                return Err(WriteError::Taken);
            }
        }
        let created_at = page.created_at.map_or_else(now, str::to_owned);
        let sender_id = match page.sender {
            Some(sender) => Some(add_sender(&mut tx, sender).await?),
            None => None,
        };
        let (id, n): (i64, i64) = sqlx::query_as(
            "insert into pages(project, key, kind, n, title, role, workspace, created_at, body, sender_id)
             values(?1, ?2, ?3,
                    coalesce(?4, (select coalesce(max(n), 0) + 1 from
                                    (select n from pages where project = ?1 and key = ?2 and kind = ?3
                                     union all
                                     select n from removed where project = ?1 and key = ?2 and kind = ?3))),
                    ?5, ?6, ?7, ?8, ?9, ?10)
             returning id, n",
        )
        .bind(page.project)
        .bind(page.key)
        .bind(page.kind)
        .bind(page.n)
        .bind(page.title)
        .bind(page.role)
        .bind(page.workspace)
        .bind(&created_at)
        .bind(page.body)
        .bind(sender_id)
        .fetch_one(&mut *tx)
        .await?;
        sqlx::query("insert into pages_fts(rowid, title, text) values(?1, ?2, ?3)")
            .bind(id)
            .bind(page.title)
            .bind(page.text)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(Placed {
            n,
            created_at,
            size: page.body.len() as i64,
        })
    }

    /// ページ 1 枚の返事の状態を書く（docs/src/next.md の「返事」）。同じ状態を 2 度書いても、時刻は初めの物を保つ。
    /// - 返事待ち: 待ちの相手を `on` にし、答えた時刻を空にする。答えた後にもう 1 度求めた時は、求めた時刻を今にする。
    ///   今ほかの相手の返事待ちなら、何も替えない（[`Marked::Refused`]）。
    /// - 答えた: 答えた時刻を入れる（求めた時刻が空なら、同じ時刻を入れる）。待ちの相手が `on` と違えば何も替えない
    ///   （[`Marked::Refused`]）。Product Manager のほかの相手は、返事待ちにしたページだけを答えたにできる。
    pub async fn set_reply(
        &self,
        page: &PageKey<'_>,
        reply: Reply,
        on: Party,
    ) -> Result<Marked, WriteError> {
        let answered = reply == Reply::Answered;
        let mut tx = self.write.begin().await?;
        let Some((id, waits_on, asked_at, answered_at)) =
            sqlx::query_as::<_, (i64, Option<String>, Option<String>, Option<String>)>(
                "select id, waits_on, asked_at, answered_at from pages
                 where project = ?1 and key = ?2 and kind = ?3 and n = ?4",
            )
            .bind(page.project)
            .bind(page.key)
            .bind(page.kind)
            .bind(page.n)
            .fetch_optional(&mut *tx)
            .await?
        else {
            return Ok(Marked::Missing);
        };
        let party = Party::of(waits_on.as_deref());
        let waiting = asked_at.is_some() && answered_at.is_none();
        // 答えた: 相手が違う・相手が Product Manager でないのに求めていない。返事待ち: 今ほかの相手を待っている
        // （待ちを黙って移さない。答えた後の求め直しは、相手を替えてよい）。
        let refused = if answered {
            party != on || (on != Party::Pm && asked_at.is_none())
        } else {
            waiting && party != on
        };
        if refused {
            return Ok(Marked::Refused);
        }
        sqlx::query(
            "update pages
             set asked_at = case when asked_at is not null and (?2 or answered_at is null)
                                 then asked_at else ?4 end,
                 answered_at = case when ?2 then coalesce(answered_at, ?4) end,
                 waits_on = case when ?2 then waits_on else ?3 end
             where id = ?1",
        )
        .bind(id)
        .bind(answered)
        .bind(on.column())
        .bind(now())
        .execute(&mut *tx)
        .await?;
        let row = page_row(&mut tx, id).await?;
        tx.commit().await?;
        Ok(Marked::Done(Box::new(row)))
    }

    /// レビューの印を書く: 判定（書けば替える）と見た物（足すだけ。同じ物は 1 つ）。
    /// 足した後の見た物が `max` を超えるなら、何も替えない（[`Marked::Refused`]）。
    /// 判定を書けるページの種類と、見た物の形は、呼ぶ側が見る。
    pub async fn add_marks(
        &self,
        page: &PageKey<'_>,
        verdict: Option<&str>,
        targets: &[String],
        max: i64,
    ) -> Result<Marked, WriteError> {
        let mut tx = self.write.begin().await?;
        let Some(id) = sqlx::query_scalar::<_, i64>(
            "update pages set verdict = coalesce(?5, verdict)
             where project = ?1 and key = ?2 and kind = ?3 and n = ?4
             returning id",
        )
        .bind(page.project)
        .bind(page.key)
        .bind(page.kind)
        .bind(page.n)
        .bind(verdict)
        .fetch_optional(&mut *tx)
        .await?
        else {
            return Ok(Marked::Missing);
        };
        for target in targets {
            sqlx::query(
                "insert into page_targets(page_id, target) values(?1, ?2) on conflict do nothing",
            )
            .bind(id)
            .bind(target)
            .execute(&mut *tx)
            .await?;
        }
        let count: i64 = sqlx::query_scalar("select count(*) from page_targets where page_id = ?1")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
        if count > max {
            // 取引を commit せずに落とすと、判定も見た物も替わらない。
            return Ok(Marked::Refused);
        }
        let row = page_row(&mut tx, id).await?;
        tx.commit().await?;
        Ok(Marked::Done(Box::new(row)))
    }

    /// 合うページを消し（全文の索引の行も。見た物は外のキーで一緒に消える）、番号を消した時刻と removed に残す。
    /// 消したページを、置いた順（id の順）に返す。送り手の行は残る。
    pub async fn remove(&self, target: &Target<'_>) -> Result<Vec<Removed>, WriteError> {
        const MATCHES: &str = "project = ?1 and (?2 is null or key = ?2)
             and (?3 is null or kind = ?3) and (?4 is null or n = ?4)";
        let mut tx = self.write.begin().await?;
        // 消す物を、置いた順（id の順）に先に読む（insert の returning の行の順は、SQLite が決めないので頼らない）。
        let removed: Vec<Removed> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "select project, key, kind, n from pages where {MATCHES} order by id"
        )))
        .bind(target.project)
        .bind(target.key)
        .bind(target.kind)
        .bind(target.n)
        .fetch_all(&mut *tx)
        .await?;
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "insert into removed(project, key, kind, n, removed_at)
             select project, key, kind, n, ?5 from pages where {MATCHES}"
        )))
        .bind(target.project)
        .bind(target.key)
        .bind(target.kind)
        .bind(target.n)
        .bind(now())
        .execute(&mut *tx)
        .await?;
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "delete from pages_fts where rowid in (select id from pages where {MATCHES})"
        )))
        .bind(target.project)
        .bind(target.key)
        .bind(target.kind)
        .bind(target.n)
        .execute(&mut *tx)
        .await?;
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "delete from pages where {MATCHES}"
        )))
        .bind(target.project)
        .bind(target.key)
        .bind(target.kind)
        .bind(target.n)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(removed)
    }

    /// file を 1 つ置く（1 つの transaction）。番号はページと同じく「置いた番号と消した番号の最大 + 1」（project・key・語・
    /// 拡張子ごと）。番号を名指せば、置いた番号・消した番号なら [`WriteError::Taken`]（何も替えない）。
    /// 置いた file の記録（送り手も）を、同じ取引の中で読んで返す。
    pub async fn put_file(&self, file: &NewFile<'_>) -> Result<FileRow, WriteError> {
        let mut tx = self.write.begin().await?;
        if let Some(n) = file.n {
            let removed: Option<i64> = sqlx::query_scalar(
                "select 1 from files_removed
                 where project = ?1 and key = ?2 and name = ?3 and ext = ?4 and n = ?5",
            )
            .bind(file.project)
            .bind(file.key)
            .bind(file.name)
            .bind(file.ext)
            .bind(n)
            .fetch_optional(&mut *tx)
            .await?;
            if removed.is_some() {
                return Err(WriteError::Taken);
            }
        }
        let created_at = file.created_at.map_or_else(now, str::to_owned);
        let sender_id = match file.sender {
            Some(sender) => Some(add_sender(&mut tx, sender).await?),
            None => None,
        };
        let id: i64 = sqlx::query_scalar(
            "insert into files(project, key, name, ext, n, type, size, sha256, role, workspace, created_at, body, sender_id)
             values(?1, ?2, ?3, ?4,
                    coalesce(?5, (select coalesce(max(n), 0) + 1 from
                                    (select n from files where project = ?1 and key = ?2 and name = ?3 and ext = ?4
                                     union all
                                     select n from files_removed
                                     where project = ?1 and key = ?2 and name = ?3 and ext = ?4))),
                    ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
             returning id",
        )
        .bind(file.project)
        .bind(file.key)
        .bind(file.name)
        .bind(file.ext)
        .bind(file.n)
        .bind(file.content_type)
        .bind(file.body.len() as i64)
        .bind(file.sha256)
        .bind(file.role)
        .bind(file.workspace)
        .bind(&created_at)
        .bind(file.body)
        .bind(sender_id)
        .fetch_one(&mut *tx)
        .await?;
        let row = sqlx::query_as(sqlx::AssertSqlSafe(format!("{FILE_ROW} where f.id = ?1")))
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(row)
    }

    /// 合う file を消し、番号を消した時刻と files_removed に残す。消した file を、置いた順（id の順）に返す。
    /// 送り手の行は残る。ページは見ない。
    pub async fn remove_files(
        &self,
        target: &FileTarget<'_>,
    ) -> Result<Vec<RemovedFile>, WriteError> {
        const MATCHES: &str = "project = ?1 and (?2 is null or key = ?2)
             and (?3 is null or (name = ?3 and ext = ?4)) and (?5 is null or n = ?5)";
        let (name, ext) = target.name.unzip();
        let mut tx = self.write.begin().await?;
        // 消す物を、置いた順（id の順）に先に読む（insert の returning の行の順は、SQLite が決めないので頼らない。ページの remove と同じ）。
        let removed: Vec<RemovedFile> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "select project, key, name, ext, n from files where {MATCHES} order by id"
        )))
        .bind(target.project)
        .bind(target.key)
        .bind(name)
        .bind(ext)
        .bind(target.n)
        .fetch_all(&mut *tx)
        .await?;
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "insert into files_removed(project, key, name, ext, n, removed_at)
             select project, key, name, ext, n, ?6 from files where {MATCHES}"
        )))
        .bind(target.project)
        .bind(target.key)
        .bind(name)
        .bind(ext)
        .bind(target.n)
        .bind(now())
        .execute(&mut *tx)
        .await?;
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "delete from files where {MATCHES}"
        )))
        .bind(target.project)
        .bind(target.key)
        .bind(name)
        .bind(ext)
        .bind(target.n)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(removed)
    }

    /// file の Content-Type と中身。無ければ None。
    pub async fn file_body(
        &self,
        file: &FileKey<'_>,
    ) -> Result<Option<(String, Vec<u8>)>, sqlx::Error> {
        sqlx::query_as(
            "select type, body from files
             where project = ?1 and key = ?2 and name = ?3 and ext = ?4 and n = ?5",
        )
        .bind(file.project)
        .bind(file.key)
        .bind(file.name)
        .bind(file.ext)
        .bind(file.n)
        .fetch_optional(&self.read)
        .await
    }

    /// 消した file なら、消した時刻。
    pub async fn file_removed_at(&self, file: &FileKey<'_>) -> Result<Option<String>, sqlx::Error> {
        sqlx::query_scalar(
            "select removed_at from files_removed
             where project = ?1 and key = ?2 and name = ?3 and ext = ?4 and n = ?5",
        )
        .bind(file.project)
        .bind(file.key)
        .bind(file.name)
        .bind(file.ext)
        .bind(file.n)
        .fetch_optional(&self.read)
        .await
    }

    /// ページの本文。無ければ None。
    pub async fn body(
        &self,
        project: &str,
        key: &str,
        kind: &str,
        n: i64,
    ) -> Result<Option<Vec<u8>>, sqlx::Error> {
        sqlx::query_scalar(
            "select body from pages where project = ?1 and key = ?2 and kind = ?3 and n = ?4",
        )
        .bind(project)
        .bind(key)
        .bind(kind)
        .bind(n)
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
        sqlx::query_scalar(
            "select removed_at from removed where project = ?1 and key = ?2 and kind = ?3 and n = ?4",
        )
        .bind(project)
        .bind(key)
        .bind(kind)
        .bind(n)
        .fetch_optional(&self.read)
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::stamp;
    use time::OffsetDateTime;
    use time::format_description::well_known::Rfc3339;

    #[test]
    fn stamp_is_utc_with_milliseconds_and_24_chars() {
        let t = OffsetDateTime::parse("2026-11-24T09:00:00.123456+09:00", &Rfc3339).unwrap();
        assert_eq!(stamp(t), "2026-11-24T00:00:00.123Z");
        let t = OffsetDateTime::parse("2026-11-24T00:00:05Z", &Rfc3339).unwrap();
        assert_eq!(stamp(t), "2026-11-24T00:00:05.000Z");
    }
}
