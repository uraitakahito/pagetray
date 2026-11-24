//! 接続・移行・SQL。SQL は動く時に組む形（`query`・`query_as`）で書く（`.sqlx/` は要らない）。
//!
//! - 書く接続は 1 本だけ。番号（今の最大 ＋ 1）を取る所が 1 列に並ぶ。
//! - 読む接続は 4 本で、読むだけ。WAL なので、書いている間も待たずに読める。
//! - 接続の決まりと、今の server の DB を断る所は、castlist の新しい server（crates/server/src/db.rs）と同じ。
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use sqlx::SqlitePool;
use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
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
}

/// 置いたページの記録（DB の字のまま）。
#[derive(Debug)]
pub struct Placed {
    pub n: i64,
    pub created_at: String,
    pub size: i64,
}

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
                "今の server の DB（新しい server の migrations に無い・中身の違う移行 {} を当ててある）。開かずに止める",
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

// --- 開く ---

/// `path` が、今の server（か別の版）の移行を当てた DB なら [`OpenError::Current`] で断る。読むだけの接続
/// （`read_only`）で開き、DB の中身は書き替えない（WAL の DB は -wal も読む）。file が無い・移行の表が無い時は通す（開く時に作る）。
/// 移行の表の版のうち、新しい migrations に無い物か、中身の写し（checksum）が違う物があれば断る
/// （今の server の DB は 0002〜0006 が無く、0001 も中身が違う。sqlx の移行に任せると、断る前に DB を WAL にする）。
pub async fn refuse_current(_path: &Path) -> Result<(), OpenError> {
    // 試験を先に書く commit の仮の物（次の commit で作る）。
    Ok(())
}

impl Db {
    /// 開き、移行し、外のキーの崩れが無いかを見る。崩れがあれば開かない。
    /// 今の server の DB は、何も書かずに断る（[`refuse_current`]）。
    pub async fn open(path: &Path) -> Result<Self, OpenError> {
        // 試験を先に書く commit の仮の物（次の commit で作る）。
        refuse_current(path).await?;
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true);
        let write = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options.clone())
            .await
            .map_err(OpenError::Db)?;
        MIGRATOR.run(&write).await.map_err(OpenError::Migrate)?;
        let read = SqlitePoolOptions::new()
            .max_connections(READS)
            .connect_with(options)
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

    /// ページを 1 枚置き、全文の索引にも入れる（1 つの transaction）。
    /// 番号を名指せば、置いた番号・消した番号なら [`WriteError::Taken`]（何も替えない）。
    pub async fn put_page(&self, _page: &NewPage<'_>) -> Result<Placed, WriteError> {
        // 試験を先に書く commit の仮の物（次の commit で作る）。
        Err(WriteError::Db(sqlx::Error::Protocol(
            "まだ作っていない".to_owned(),
        )))
    }

    /// ページの本文。無ければ None。
    pub async fn body(
        &self,
        _project: &str,
        _key: &str,
        _kind: &str,
        _n: i64,
    ) -> Result<Option<Vec<u8>>, sqlx::Error> {
        Ok(None)
    }

    /// 消したページなら、消した時刻。
    pub async fn removed_at(
        &self,
        _project: &str,
        _key: &str,
        _kind: &str,
        _n: i64,
    ) -> Result<Option<String>, sqlx::Error> {
        Ok(None)
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
