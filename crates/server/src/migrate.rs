//! 今の server の DB（`GET /api/backup` の写し）を、新しい server の DB へ移す（`pagetray-next migrate --from <写し> --to <新しい DB>`）。
//! まだ移さない（試験を先に書いた。次の commit で作る）。
use std::fmt;
use std::path::Path;

use crate::db::OpenError;

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

/// 突き合わせの全部。
#[derive(Clone, Debug, Default)]
pub struct Summary {
    pub tables: Vec<TableCheck>,
    pub bodies: Vec<TableCheck>,
    pub broken: Vec<String>,
    pub bad_rows: Vec<String>,
    pub bad_times: Vec<String>,
}

impl Summary {
    pub fn ok(&self) -> bool {
        false
    }

    pub fn table(&self, name: &str) -> Option<&TableCheck> {
        self.tables.iter().find(|t| t.table == name)
    }

    pub fn body(&self, name: &str) -> Option<&TableCheck> {
        self.bodies.iter().find(|t| t.table == name)
    }
}

impl fmt::Display for Summary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "まだ無い")
    }
}

/// 移せなかった訳。
#[derive(Debug)]
pub enum MigrateError {
    Source(String),
    Target(String),
    Open(OpenError),
    Db(sqlx::Error),
    Mismatch(Summary),
}

impl fmt::Display for MigrateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source(why) => write!(f, "移す元: {why}"),
            Self::Target(why) => write!(f, "移す先: {why}"),
            Self::Open(e) => write!(f, "移す先を開けない: {e}"),
            Self::Db(e) => write!(f, "DB: {e}"),
            Self::Mismatch(summary) => write!(f, "突き合わせが合わない\n{summary}"),
        }
    }
}

impl std::error::Error for MigrateError {}

/// 移す（まだ無い）。
pub async fn migrate(_from: &Path, _to: &Path) -> Result<Summary, MigrateError> {
    Err(MigrateError::Db(sqlx::Error::Protocol(
        "まだ無い".to_owned(),
    )))
}

/// 突き合わせる（まだ無い）。
pub async fn verify(_from: &Path, _to: &Path) -> Result<Summary, MigrateError> {
    Err(MigrateError::Db(sqlx::Error::Protocol(
        "まだ無い".to_owned(),
    )))
}
