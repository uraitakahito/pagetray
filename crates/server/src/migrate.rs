//! 今の server の DB（`GET /api/backup` の写し）を、新しい server の DB へ移す（`pagetray-next migrate --from <写し> --to <新しい DB>`）。
//!
//! - 移す元は、読むだけ（`mode=ro&immutable=1`）で開き、書かない。隣に `-wal`・`-shm` があれば、写しでないので断る。
//!   今の migrations の途中の版（0001〜0005 まで当てた物）も受ける: 無い表は空、後の版で足した列は空として読む。
//! - 移す先は、新しい migrations を当ててから、行が 1 つも無いことを見る（あれば断る）。
//! - 1 つの transaction の中で、親の表から順に insert だけで写す（外のキーと足すだけの trigger は効いたまま）。
//!   id・番号・送り手の行（castlist のアクターの id も）・見た物の順（rowid）・本文の byte は替えない。
//!   時刻の字は、新しい server の残す字（24 字の UTC。[`crate::db::stamp`]）に揃える。揃えられない字は写したまま残り、
//!   突き合わせが断る。探す索引（`pages_fts`）は、写した本文から作り直す。
//! - 写した後、同じ transaction の中で、表ごとの行の数と中身の digest・本文の SHA-256・索引の行・外のキー・
//!   新しい API の形・時刻の字を突き合わせ、1 つでも合わなければ何も足さずに戻す。
//!
//! この file の SQL は insert と select だけ。
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use sha2::{Digest, Sha256};
use sqlx::sqlite::{SqliteConnectOptions, SqliteConnection};
use sqlx::{AssertSqlSafe, Connection};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::db::{Db, OpenError, stamp};

/// 移す表の 1 つ。
struct Table {
    name: &'static str,
    /// 写す列（新しい DB の名と順。`rowid` は、見た物の順を残すために写す）。
    columns: &'static [&'static str],
    /// 時刻の列（24 字に揃える）。
    times: &'static [&'static str],
    /// 今の migrations の後の版で足した列（それより前の版の写しには無いので、空として読む）。
    later: &'static [&'static str],
    /// 行の順（突き合わせの digest と、写す順）。
    order: &'static str,
    /// 行を指す字（断った行を出す時の印）。
    label: &'static str,
}

/// 移す表（親が先）。表が無い（今の migrations の途中の版の）写しでは、空として読む。
const TABLES: &[Table] = &[
    Table {
        name: "senders",
        columns: &[
            "id",
            "actor_issuer",
            "actor_ref",
            "actor_id",
            "actor_name",
            "actor_kind",
            "resolved_at",
            "created_at",
        ],
        times: &["resolved_at", "created_at"],
        later: &[],
        order: "t.id",
        label: "t.id",
    },
    Table {
        name: "pages",
        columns: &[
            "id",
            "project",
            "key",
            "kind",
            "n",
            "title",
            "role",
            "workspace",
            "created_at",
            "body",
            "asked_at",
            "answered_at",
            "sender_id",
            "waits_on",
            "verdict",
        ],
        times: &["created_at", "asked_at", "answered_at"],
        // 0004（返事）・0005（送り手）・0006（レビュー）で足した列。
        later: &[
            "asked_at",
            "answered_at",
            "sender_id",
            "waits_on",
            "verdict",
        ],
        order: "t.id",
        label: "t.id",
    },
    Table {
        name: "removed",
        columns: &["project", "key", "kind", "n", "removed_at"],
        times: &["removed_at"],
        later: &[],
        order: "t.project, t.key, t.kind, t.n",
        label: "'/' || t.project || '/' || t.key || '/' || t.kind || '-' || t.n || '.html'",
    },
    Table {
        name: "page_targets",
        columns: &["rowid", "page_id", "target"],
        times: &[],
        later: &[],
        order: "t.rowid",
        label: "t.page_id",
    },
    Table {
        name: "files",
        columns: &[
            "id",
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
            "body",
            "sender_id",
        ],
        times: &["created_at"],
        // 0005（送り手）で足した列。
        later: &["sender_id"],
        order: "t.id",
        label: "t.id",
    },
    Table {
        name: "files_removed",
        columns: &["project", "key", "name", "ext", "n", "removed_at"],
        times: &["removed_at"],
        later: &[],
        order: "t.project, t.key, t.name, t.ext, t.n",
        label: "'/' || t.project || '/' || t.key || '/files/' || t.name || '-' || t.n || '.' || t.ext",
    },
];

/// 本文を持つ表（本文の SHA-256 を突き合わせる）。
const BODIES: &[&str] = &["pages", "files"];

/// 移した表の数と digest の突き合わせ（1 つの表）。本文の突き合わせ（[`Summary::bodies`]）も同じ形。
#[derive(Clone, Debug)]
pub struct TableCheck {
    pub table: &'static str,
    pub old_rows: i64,
    pub new_rows: i64,
    pub old_digest: String,
    pub new_digest: String,
}

impl TableCheck {
    pub fn ok(&self) -> bool {
        self.old_rows == self.new_rows && self.old_digest == self.new_digest
    }
}

/// 探す索引（`pages_fts`）の行: ページの数と、索引の行の数と、そのうちページを指す行の数（移す先だけを見る）。
#[derive(Clone, Debug, Default)]
pub struct IndexCheck {
    pub pages: i64,
    pub rows: i64,
    pub matched: i64,
}

impl IndexCheck {
    pub fn ok(&self) -> bool {
        self.pages == self.rows && self.rows == self.matched
    }
}

/// 時刻の列 1 つの照らし: 移す元で値のある行の数と、そのうち移す先で同じ時点（ms）の行の数。
#[derive(Clone, Debug)]
pub struct TimeCheck {
    pub table: &'static str,
    pub column: &'static str,
    pub old_rows: i64,
    pub same_rows: i64,
}

impl TimeCheck {
    pub fn ok(&self) -> bool {
        self.old_rows == self.same_rows
    }
}

/// 突き合わせの全部。
#[derive(Clone, Debug, Default)]
pub struct Summary {
    /// 表ごとの行の数と、本文の外の列の digest。
    pub tables: Vec<TableCheck>,
    /// 本文を持つ表ごとの行の数と、本文の SHA-256 を id の順に並べた物の SHA-256。
    pub bodies: Vec<TableCheck>,
    pub index: IndexCheck,
    /// 時刻の列ごとの、同じ時点の行の数。
    pub times: Vec<TimeCheck>,
    /// 移す先の `pragma foreign_key_check` に出た行（空なら崩れは無い）。
    pub broken: Vec<String>,
    /// 新しい API の形に合わない行（`<表> <行>: <列>…`。空なら無い）。
    pub bad_rows: Vec<String>,
    /// 24 字の UTC に揃えられない時刻（`<表>.<列> <行>: <字>`。空なら無い）。
    pub bad_times: Vec<String>,
}

impl Summary {
    /// どの表も数と digest が合い、本文も合い、索引がページと同じ数で、外のキーが崩れず、形と時刻に外れが無いか。
    pub fn ok(&self) -> bool {
        self.tables.iter().all(TableCheck::ok)
            && self.bodies.iter().all(TableCheck::ok)
            && self.index.ok()
            && self.broken.is_empty()
            && self.bad_rows.is_empty()
            && self.bad_times.is_empty()
    }

    /// 名で表の突き合わせを引く。
    pub fn table(&self, name: &str) -> Option<&TableCheck> {
        self.tables.iter().find(|t| t.table == name)
    }

    /// 表と列の名で、時刻の照らしを引く。
    pub fn time(&self, table: &str, column: &str) -> Option<&TimeCheck> {
        self.times
            .iter()
            .find(|t| t.table == table && t.column == column)
    }

    /// 名で本文の突き合わせを引く。
    pub fn body(&self, name: &str) -> Option<&TableCheck> {
        self.bodies.iter().find(|t| t.table == name)
    }
}

/// 1 行に 1 つ（digest は頭の 16 字）:
/// `<表> <今の行の数> → <新しい行の数> digest <今> → <新しい> ok|NG`・`body <表> … sha256 … ok|NG`・
/// `index pages_fts <ページの数> → <索引の行の数> ok|NG`・`foreign_key_check`・`rows`・`times`。
impl fmt::Display for Summary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mark = |ok: bool| if ok { "ok" } else { "NG" };
        for t in &self.tables {
            writeln!(
                f,
                "{} {} → {} digest {} → {} {}",
                t.table,
                t.old_rows,
                t.new_rows,
                &t.old_digest[..16],
                &t.new_digest[..16],
                mark(t.ok())
            )?;
        }
        for t in &self.bodies {
            writeln!(
                f,
                "body {} {} → {} sha256 {} → {} {}",
                t.table,
                t.old_rows,
                t.new_rows,
                &t.old_digest[..16],
                &t.new_digest[..16],
                mark(t.ok())
            )?;
        }
        writeln!(
            f,
            "index pages_fts {} → {} {}",
            self.index.pages,
            self.index.rows,
            mark(self.index.ok())
        )?;
        let list = |f: &mut fmt::Formatter<'_>, name: &str, why: &str, items: &[String]| {
            if items.is_empty() {
                writeln!(f, "{name} ok")
            } else {
                writeln!(f, "{name} NG（{why}）: {}", items.join("・"))
            }
        };
        list(f, "foreign_key_check", "外のキーの崩れ", &self.broken)?;
        list(f, "rows", "新しい API の形に合わない", &self.bad_rows)?;
        list(f, "times", "24 字の UTC に揃えられない", &self.bad_times)
    }
}

/// 移せなかった訳。
#[derive(Debug)]
pub enum MigrateError {
    /// 移す元が読めない・今の server の DB の写しでない。
    Source(String),
    /// 移す先が空でない・元と同じ file。
    Target(String),
    /// 移す先を開けない（今の server の DB・移行に落ちた・外のキーの崩れた行がある）。
    Open(OpenError),
    Db(sqlx::Error),
    /// 写した後の突き合わせが合わない（移す先には何も足していない）。
    Mismatch(Box<Summary>),
}

impl fmt::Display for MigrateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source(why) => write!(f, "移す元: {why}"),
            Self::Target(why) => write!(f, "移す先: {why}"),
            Self::Open(e) => write!(f, "移す先を開けない: {e}"),
            Self::Db(e) => write!(f, "DB: {e}"),
            Self::Mismatch(summary) => {
                write!(f, "突き合わせが合わないので、何も足さずに戻した\n{summary}")
            }
        }
    }
}

impl std::error::Error for MigrateError {}

impl From<sqlx::Error> for MigrateError {
    fn from(e: sqlx::Error) -> Self {
        Self::Db(e)
    }
}

/// 移す元に在る、表ごとの列（表が無ければ入らない）。
type Columns = BTreeMap<&'static str, Vec<String>>;

/// 今の server の DB の写し `from` を、新しい DB `to`（無ければ作る）へ移し、突き合わせを返す。
pub async fn migrate(from: &Path, to: &Path) -> Result<Summary, MigrateError> {
    let old = inspect_source(from).await?;
    if same_file(from, to).await {
        return Err(MigrateError::Target(format!(
            "移す元と同じ file: {}",
            to.display()
        )));
    }
    let db = Db::open(to).await.map_err(MigrateError::Open)?;
    let result = copy(&db, from, &old).await;
    db.close().await;
    result
}

/// 今の server の DB の写し `from` と、新しい DB `to` の中身を突き合わせる（どちらにも書かない）。
pub async fn verify(from: &Path, to: &Path) -> Result<Summary, MigrateError> {
    let old = inspect_source(from).await?;
    let mut conn = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(to)
            .read_only(true)
            .create_if_missing(false),
    )
    .await?;
    attach(&mut conn, from).await?;
    let summary = summarize(&mut conn, &old).await;
    detach(&mut conn).await;
    let _ = conn.close().await;
    summary
}

/// 移す元が、読める今の server の DB の写しかを見て、表ごとの列を返す（移す先は、まだ作らない）。
async fn inspect_source(from: &Path) -> Result<Columns, MigrateError> {
    match tokio::fs::metadata(from).await {
        Ok(m) if m.is_file() => {}
        Ok(_) => return Err(source(from, "file でない")),
        Err(e) => return Err(source(from, &format!("読めない（{e}）"))),
    }
    // GET /api/backup の写しには -wal も -shm も無い。あれば（空でも）動いている DB か、その残りなので断る。
    let name = from.file_name().unwrap_or_default().to_string_lossy();
    for suffix in ["-wal", "-shm"] {
        let beside = from.with_file_name(format!("{name}{suffix}"));
        if tokio::fs::try_exists(&beside).await.unwrap_or(true) {
            return Err(source(
                from,
                &format!(
                    "隣に {suffix} がある。動いている DB でなく、GET /api/backup の写しを渡す"
                ),
            ));
        }
    }
    let mut conn = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(from)
            .read_only(true)
            .immutable(true)
            .create_if_missing(false),
    )
    .await
    .map_err(|e| source(from, &format!("開けない（{e}）")))?;
    let found = source_columns(&mut conn).await;
    let _ = conn.close().await;
    match found {
        Ok(Ok(columns)) => Ok(columns),
        Ok(Err(why)) => Err(source(from, &why)),
        Err(e) => Err(source(from, &format!("SQLite の DB として読めない（{e}）"))),
    }
}

fn source(from: &Path, why: &str) -> MigrateError {
    MigrateError::Source(format!("{}: {why}", from.display()))
}

/// 2 つの path が同じ file を指すか（無い path は、どれとも同じでない）。
async fn same_file(a: &Path, b: &Path) -> bool {
    match (
        tokio::fs::canonicalize(a).await,
        tokio::fs::canonicalize(b).await,
    ) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// 移す元の表ごとの列を読む。`pages` が無ければ pagetray の DB でない。在る表に、後の版で足した物のほかの列が
/// 無ければ、今の server の DB の形でない（外の Err の中の訳）。
async fn source_columns(
    conn: &mut SqliteConnection,
) -> Result<Result<Columns, String>, sqlx::Error> {
    let mut found = Columns::new();
    for table in TABLES {
        let have: Vec<String> = sqlx::query_scalar("select name from pragma_table_info(?1)")
            .bind(table.name)
            .fetch_all(&mut *conn)
            .await?;
        if have.is_empty() {
            continue;
        }
        let missing: Vec<&str> = table
            .columns
            .iter()
            .copied()
            .filter(|c| *c != "rowid" && !table.later.contains(c) && !have.iter().any(|h| h == c))
            .collect();
        if !missing.is_empty() {
            return Ok(Err(format!(
                "今の server の DB の形でない（表 {} に列 {} が無い）",
                table.name,
                missing.join("・")
            )));
        }
        found.insert(table.name, have);
    }
    if !found.contains_key("pages") {
        return Ok(Err("pagetray の DB でない（表 pages が無い）".to_owned()));
    }
    Ok(Ok(found))
}

/// 移す元を、読むだけの `old` として付ける（URI の `mode=ro&immutable=1`）。
async fn attach(conn: &mut SqliteConnection, from: &Path) -> Result<(), MigrateError> {
    let path = from
        .to_str()
        .ok_or_else(|| source(from, "path が UTF-8 でない"))?;
    let encoded: String = path
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    sqlx::query("attach database ?1 as old")
        .bind(format!("file:{encoded}?mode=ro&immutable=1"))
        .execute(&mut *conn)
        .await
        .map_err(|e| source(from, &format!("付けられない（{e}）")))?;
    Ok(())
}

/// 移す元を外す。commit の後に外せなくても、移した結果（commit した物）は替えない: 落ちたことを記録に書くだけにする
/// （接続は閉じれば外れる。ここで誤りにすると、移し終えたのに 1 で終わり、2 度目の移しは「空でない」で断られる）。
async fn detach(conn: &mut SqliteConnection) {
    if let Err(e) = sqlx::query("detach database old").execute(&mut *conn).await {
        tracing::warn!("migrate: detach old failed: {e}");
    }
}

/// 空の移す先へ、1 つの transaction で写し、索引を作り直し、突き合わせが合えば commit する。
async fn copy(db: &Db, from: &Path, old: &Columns) -> Result<Summary, MigrateError> {
    let mut conn = db.write().acquire().await?;
    let rows: i64 = sqlx::query_scalar(
        "select (select count(*) from senders) + (select count(*) from pages)
              + (select count(*) from removed) + (select count(*) from page_targets)
              + (select count(*) from files) + (select count(*) from files_removed)
              + (select count(*) from pages_fts)",
    )
    .fetch_one(&mut *conn)
    .await?;
    if rows > 0 {
        return Err(MigrateError::Target(format!(
            "行が {rows} ある。空の DB（ページも file も送り手も無い物）にだけ移す"
        )));
    }
    attach(&mut conn, from).await?;
    let result = async {
        let mut tx = conn.begin().await?;
        for table in TABLES {
            if old.contains_key(table.name) {
                sqlx::query(AssertSqlSafe(copy_sql(table, old)))
                    .execute(&mut *tx)
                    .await?;
            }
        }
        reindex(&mut tx).await?;
        let summary = summarize(&mut tx, old).await?;
        if summary.ok() {
            tx.commit().await?;
            Ok(summary)
        } else {
            tx.rollback().await?;
            Err(MigrateError::Mismatch(Box::new(summary)))
        }
    }
    .await;
    detach(&mut conn).await;
    result
}

/// 移す元の列の式。無い列（後の版で足した物）は空。時刻の列は 24 字に揃える。
fn old_column(table: &Table, column: &str, old: &Columns) -> String {
    let have = column == "rowid"
        || old
            .get(table.name)
            .is_some_and(|c| c.iter().any(|c| c == column));
    match (have, table.times.contains(&column)) {
        (false, _) => "null".to_owned(),
        (true, true) => aligned(&format!("t.{column}")),
        (true, false) => format!("t.{column}"),
    }
}

/// 時刻の字を、新しい server の残す字（`2026-01-02T03:04:05.000Z`）にする SQL の式。今の server が受けた形
/// （UTC の `Z` で終わり、秒の下は 0〜3 桁）で、在る日と時刻の物だけを揃え、ほかは字のまま返す（突き合わせの
/// [`Summary::bad_times`] が断る）。SQLite の時刻の関数は、無い日（2 月 30 日）を次の月に回すので、秒までが
/// 元の字と同じ物だけを在る時刻とする。
fn aligned(c: &str) -> String {
    format!(
        "case when {c} glob '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T[0-9][0-9]:[0-9][0-9]:[0-9][0-9]*Z'
                and (length({c}) = 20
                     or (length({c}) between 22 and 24 and substr({c}, 20, 1) = '.'
                         and substr({c}, 21, length({c}) - 21) not glob '*[^0-9]*'))
                and strftime('%Y-%m-%dT%H:%M:%S', {c}) = substr({c}, 1, 19)
              then strftime('%Y-%m-%dT%H:%M:%fZ', {c}) else {c} end"
    )
}

/// 1 つの表を、移す元（old）から移す先（main）へ写す insert。
fn copy_sql(table: &Table, old: &Columns) -> String {
    let select: Vec<String> = table
        .columns
        .iter()
        .map(|c| old_column(table, c, old))
        .collect();
    format!(
        "insert into main.{}({}) select {} from old.{} t order by {}",
        table.name,
        table.columns.join(", "),
        select.join(", "),
        table.name,
        table.order
    )
}

/// 探す索引を、移した本文から作り直す（置く時と同じ字: 題は残した題、本文は tag などを除いた字）。
async fn reindex(conn: &mut SqliteConnection) -> Result<(), sqlx::Error> {
    let ids: Vec<i64> = sqlx::query_scalar("select id from main.pages order by id")
        .fetch_all(&mut *conn)
        .await?;
    for id in ids {
        let (title, body): (String, Vec<u8>) =
            sqlx::query_as("select title, body from main.pages where id = ?1")
                .bind(id)
                .fetch_one(&mut *conn)
                .await?;
        let text = crate::text::plain(&String::from_utf8_lossy(&body));
        sqlx::query("insert into main.pages_fts(rowid, title, text) values(?1, ?2, ?3)")
            .bind(id)
            .bind(title)
            .bind(text)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

/// 移す元（old）と移す先（main）を突き合わせる。
async fn summarize(conn: &mut SqliteConnection, old: &Columns) -> Result<Summary, MigrateError> {
    let mut summary = Summary::default();
    for table in TABLES {
        let (old_rows, old_digest) = if old.contains_key(table.name) {
            digest(conn, table, Some(old)).await?
        } else {
            empty()
        };
        let (new_rows, new_digest) = digest(conn, table, None).await?;
        summary.tables.push(TableCheck {
            table: table.name,
            old_rows,
            new_rows,
            old_digest,
            new_digest,
        });
    }
    for table in BODIES {
        let (old_rows, old_digest) = if old.contains_key(table) {
            bodies(conn, table, "old").await?
        } else {
            empty()
        };
        let (new_rows, new_digest) = bodies(conn, table, "main").await?;
        summary.bodies.push(TableCheck {
            table,
            old_rows,
            new_rows,
            old_digest,
            new_digest,
        });
    }
    let (pages, rows, matched): (i64, i64, i64) = sqlx::query_as(
        "select (select count(*) from main.pages), (select count(*) from main.pages_fts),
                (select count(*) from main.pages_fts where rowid in (select id from main.pages))",
    )
    .fetch_one(&mut *conn)
    .await?;
    summary.index = IndexCheck {
        pages,
        rows,
        matched,
    };
    // 新しい server の Db::open と同じ形（表・rowid・指す先）。
    let broken: Vec<(String, Option<i64>, String)> = sqlx::query_as(
        "select \"table\", rowid, parent from pragma_foreign_key_check(null, 'main')",
    )
    .fetch_all(&mut *conn)
    .await?;
    summary.broken = broken
        .into_iter()
        .map(|(table, rowid, parent)| {
            let rowid = rowid.map_or("-".to_owned(), |r| r.to_string());
            format!("{table} の rowid {rowid}（{parent} を指す）")
        })
        .collect();
    summary.bad_rows = bad_rows(conn).await?;
    summary.bad_times = bad_times(conn).await?;
    Ok(summary)
}

/// 1 つの表の行の数と、本文の外の列を 1 行 1 つの JSON の配列にして行の順に並べた字（改行で区切る）の SHA-256。
/// `old` に移す元の列を渡せば移す元（old）を、移す時と同じ式（無い列は空・時刻は揃えた字）で読む。無ければ移す先（main）。
async fn digest(
    conn: &mut SqliteConnection,
    table: &Table,
    old: Option<&Columns>,
) -> Result<(i64, String), sqlx::Error> {
    let select: Vec<String> = table
        .columns
        .iter()
        .filter(|c| **c != "body")
        .map(|c| match old {
            Some(old) => old_column(table, c, old),
            None => format!("t.{c}"),
        })
        .collect();
    let rows: Vec<String> = sqlx::query_scalar(AssertSqlSafe(format!(
        "select json_array({}) from {}.{} t order by {}",
        select.join(", "),
        if old.is_some() { "old" } else { "main" },
        table.name,
        table.order
    )))
    .fetch_all(&mut *conn)
    .await?;
    let mut hash = Sha256::new();
    for row in &rows {
        hash.update(row.as_bytes());
        hash.update(b"\n");
    }
    Ok((rows.len() as i64, hex(&hash.finalize())))
}

/// 移す元に無い表の突き合わせ（0 行と、空の字の SHA-256）。
fn empty() -> (i64, String) {
    (0, hex(&Sha256::new().finalize()))
}

/// 本文を持つ表の行の数と、`<id> <本文の SHA-256>` の行を id の順に並べた字の SHA-256（本文は 1 行ずつ読む）。
async fn bodies(
    conn: &mut SqliteConnection,
    table: &str,
    schema: &str,
) -> Result<(i64, String), sqlx::Error> {
    let ids: Vec<i64> = sqlx::query_scalar(AssertSqlSafe(format!(
        "select id from {schema}.{table} order by id"
    )))
    .fetch_all(&mut *conn)
    .await?;
    let mut hash = Sha256::new();
    for id in &ids {
        let body: Vec<u8> = sqlx::query_scalar(AssertSqlSafe(format!(
            "select body from {schema}.{table} where id = ?1"
        )))
        .bind(id)
        .fetch_one(&mut *conn)
        .await?;
        hash.update(format!("{id} {}\n", hex(&Sha256::digest(&body))).as_bytes());
    }
    Ok((ids.len() as i64, hex(&hash.finalize())))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// 移す先（main）の行のうち、新しい API の形に合わない物（表の順・行の順。`<表> <行>: <合わない列>`）。
/// 合わない行は、新しい API で読めず（道と URL の形に入らない）、送り直しも形の誤りで断られるので、移さない。
async fn bad_rows(conn: &mut SqliteConnection) -> Result<Vec<String>, sqlx::Error> {
    use crate::castlist::is_actor_id;
    use crate::files::{file_type, is_file_stem};
    use crate::marks::is_target;
    use crate::pages::{is_name, is_segment};

    let mut out = Vec::new();
    // 合わない列があれば 1 行（`<表> <行>: <列>・…`）。
    let mut report = |table: &str, label: &str, checks: &[(&str, bool)]| {
        let columns: Vec<&str> = checks
            .iter()
            .filter(|(_, ok)| !ok)
            .map(|(c, _)| *c)
            .collect();
        if !columns.is_empty() {
            out.push(format!("{table} {label}: {}", columns.join("・")));
        }
    };
    type Sender = (
        i64,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
    );
    let senders: Vec<Sender> = sqlx::query_as(
        "select id, actor_issuer, actor_ref, actor_id, actor_name, actor_kind from main.senders order by id",
    )
    .fetch_all(&mut *conn)
    .await?;
    for (id, issuer, actor_ref, actor_id, name, kind) in &senders {
        // castlist の答え（アクターの id・名・種類）は、揃って在るか、揃って無い。
        let answered = actor_id.is_some() || name.is_some() || kind.is_some();
        report(
            "senders",
            &id.to_string(),
            &[
                ("actor_issuer", is_issuer(issuer)),
                ("actor_ref", is_actor_ref(actor_ref)),
                (
                    "actor_id",
                    !answered || actor_id.as_deref().is_some_and(is_actor_id),
                ),
                (
                    "actor_name",
                    !answered
                        || name.as_deref().is_some_and(|n| {
                            (1..=128).contains(&n.chars().count())
                                && !n.chars().any(char::is_control)
                        }),
                ),
                (
                    "actor_kind",
                    !answered || matches!(kind.as_deref(), Some("ai" | "human")),
                ),
            ],
        );
    }
    let place = |project: &str, key: &str| {
        [
            ("project", is_segment(project) && project != "api"),
            ("key", is_segment(key)),
        ]
    };
    let pages: Vec<(i64, String, String, String, i64, String, String)> = sqlx::query_as(
        "select id, project, key, kind, n, role, workspace from main.pages order by id",
    )
    .fetch_all(&mut *conn)
    .await?;
    for (id, project, key, kind, n, role, workspace) in &pages {
        let [p, k] = place(project, key);
        report(
            "pages",
            &id.to_string(),
            &[
                p,
                k,
                ("kind", is_name(kind)),
                ("n", is_number(*n)),
                ("role", is_name(role)),
                ("workspace", is_workspace(workspace)),
            ],
        );
    }
    let removed: Vec<(String, String, String, i64)> = sqlx::query_as(
        "select project, key, kind, n from main.removed order by project, key, kind, n",
    )
    .fetch_all(&mut *conn)
    .await?;
    for (project, key, kind, n) in &removed {
        let [p, k] = place(project, key);
        report(
            "removed",
            &format!("/{project}/{key}/{kind}-{n}.html"),
            &[p, k, ("kind", is_name(kind)), ("n", is_number(*n))],
        );
    }
    let targets: Vec<(i64, String)> =
        sqlx::query_as("select page_id, target from main.page_targets order by rowid")
            .fetch_all(&mut *conn)
            .await?;
    for (page_id, target) in &targets {
        // 見た物は、列が target だけなので、列の名の替わりに字そのものを出す。
        if !is_target(target) {
            report("page_targets", &page_id.to_string(), &[(target, false)]);
        }
    }
    // id・project・key・name・ext・n・role・workspace。
    type File = (i64, String, String, String, String, i64, String, String);
    let files: Vec<File> = sqlx::query_as(
        "select id, project, key, name, ext, n, role, workspace from main.files order by id",
    )
    .fetch_all(&mut *conn)
    .await?;
    for (id, project, key, name, ext, n, role, workspace) in &files {
        let [p, k] = place(project, key);
        report(
            "files",
            &id.to_string(),
            &[
                p,
                k,
                ("name", is_file_stem(name)),
                ("ext", file_type(ext).is_some()),
                ("n", is_number(*n)),
                ("role", is_name(role)),
                ("workspace", is_workspace(workspace)),
            ],
        );
    }
    let files_removed: Vec<(String, String, String, String, i64)> = sqlx::query_as(
        "select project, key, name, ext, n from main.files_removed order by project, key, name, ext, n",
    )
    .fetch_all(&mut *conn)
    .await?;
    for (project, key, name, ext, n) in &files_removed {
        let [p, k] = place(project, key);
        report(
            "files_removed",
            &format!("/{project}/{key}/files/{name}-{n}.{ext}"),
            &[
                p,
                k,
                ("name", is_file_stem(name)),
                ("ext", file_type(ext).is_some()),
                ("n", is_number(*n)),
            ],
        );
    }
    Ok(out)
}

/// 移す先（main）の時刻のうち、残す字（24 字の UTC・0001〜9999 年）でない物（表の順・列の順・行の順。
/// `<表>.<列> <行>: <字>`）。移す時に揃えられなかった字が、ここに出る。
async fn bad_times(conn: &mut SqliteConnection) -> Result<Vec<String>, sqlx::Error> {
    let mut out = Vec::new();
    for table in TABLES {
        for column in table.times {
            let rows: Vec<(String, String)> = sqlx::query_as(AssertSqlSafe(format!(
                "select cast({} as text), t.{column} from main.{} t where t.{column} is not null order by {}",
                table.label, table.name, table.order
            )))
            .fetch_all(&mut *conn)
            .await?;
            out.extend(
                rows.into_iter()
                    .filter(|(_, t)| !is_stamp(t))
                    .map(|(label, t)| format!("{}.{column} {label}: {t}", table.name)),
            );
        }
    }
    Ok(out)
}

/// 新しい server の残す時刻の字か（[`stamp`] で書き直しても同じ字で、0001〜9999 年）。
fn is_stamp(s: &str) -> bool {
    OffsetDateTime::parse(s, &Rfc3339)
        .is_ok_and(|t| (1..=9999).contains(&t.year()) && stamp(t) == s)
}

/// ページと file の番号の形（定義の `PageNumber`: 頭に 0 の無い 1 以上の 18 桁まで）。
fn is_number(n: i64) -> bool {
    (1..=999_999_999_999_999_999).contains(&n)
}

/// workspace の形（定義の `Workspace`: 英数字と `:`・`.`・`_`・`-` で 64 字まで。空でもよい）。
fn is_workspace(s: &str) -> bool {
    s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b':' | b'.' | b'_' | b'-'))
}

/// 付随する id を出した物の形（定義の `Issuer`: 英小文字・数字・`-`・`.` で 1〜64 字）。
fn is_issuer(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'.'))
}

/// 付随する id の形（`ai.common#ActorRef`: `種類:値`。種類は [`is_issuer`] の形、値は制御字（Unicode の Cc）の無い 1〜256 字）。
fn is_actor_ref(s: &str) -> bool {
    s.split_once(':').is_some_and(|(kind, value)| {
        is_issuer(kind)
            && (1..=256).contains(&value.chars().count())
            && !value.chars().any(char::is_control)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamps_are_24_chars_of_utc() {
        assert!(is_stamp("2026-01-02T03:04:05.000Z"));
        for ng in [
            "2026-01-02T03:04:05Z",
            "2026-01-02T03:04:05.5Z",
            "2026-01-02T12:04:05.000+09:00",
            "0000-01-02T03:04:05.000Z",
            "",
        ] {
            assert!(!is_stamp(ng), "{ng}");
        }
    }

    #[test]
    fn actor_refs_take_any_value_without_control_chars() {
        assert!(is_actor_ref("claude.session:s 1/2:x"));
        for ng in ["claude.session", "Claude:x", "tpm:", "tpm:a\u{7f}b"] {
            assert!(!is_actor_ref(ng), "{ng}");
        }
    }
}
