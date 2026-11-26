//! 移行の file と、移行の表の合わない DB。出した移行の file は 1 byte も替えない。後の版が書いた DB は、移行を当てずに断る。
mod common;

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use common::{remove_db, temp_db};
use pagetray_server::db::{Db, OpenError};
use sha2::{Digest, Sha256, Sha384};
use sqlx::migrate::MigrateError;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

/// 出した版（1.0.0 から）が当てた移行の file と、その SHA-256。1 byte も替えない（替えると checksum が変わり、動いている DB を
/// sqlx の移行が「中身の違う移行」として断る）。直す時は新しい番号の file を足す。
/// 値は `git show v1.0.0:crates/server/migrations/<file> | shasum -a 256`。
const RELEASED: [(&str, &str); 1] = [(
    "0001_init.sql",
    "c70cb75ceb0c5da65701fcf6df1cf036dff939ad5c6a040d033ae3320fb7451e",
)];
/// 足したが、まだ出していない移行の file。出したら [`RELEASED`] に移す。
const UNRELEASED: [&str; 0] = [];

fn migrations_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

async fn sha256(path: &Path) -> String {
    hex(&Sha256::digest(tokio::fs::read(path).await.unwrap()))
}

/// folder の中の file の名。
async fn names(dir: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut entries = tokio::fs::read_dir(dir).await.unwrap();
    while let Some(entry) = entries.next_entry().await.unwrap() {
        found.push(entry.file_name().to_string_lossy().into_owned());
    }
    found.sort();
    found
}

/// この server の `0001_init.sql` の checksum（sqlx の SHA-384）の 16 進。
async fn ours() -> String {
    hex(&Sha384::digest(
        tokio::fs::read(migrations_dir().join("0001_init.sql"))
            .await
            .unwrap(),
    ))
}

/// 移行の表の行（版・名・checksum の 16 進）と表を 1 つ持つ DB を作る。
async fn ledger_db(tag: &str, rows: &[(i64, &str, String)]) -> PathBuf {
    let path = temp_db(tag);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true),
        )
        .await
        .expect("DB を作る");
    sqlx::query(
        "create table _sqlx_migrations (version bigint primary key, description text not null, \
         installed_on timestamp not null default current_timestamp, success boolean not null, \
         checksum blob not null, execution_time bigint not null)",
    )
    .execute(&pool)
    .await
    .unwrap();
    for (version, description, checksum) in rows {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "insert into _sqlx_migrations (version, description, success, checksum, execution_time) \
             values ({version}, '{description}', true, x'{checksum}', 1)"
        )))
        .execute(&pool)
        .await
        .unwrap();
    }
    for sql in [
        "create table pages (id integer primary key, project text not null, body blob not null)",
        "insert into pages (id, project, body) values (1, 'ai', x'00')",
    ] {
        sqlx::query(sql).execute(&pool).await.unwrap();
    }
    pool.close().await;
    path
}

/// DB の表の名と、移行の表の版。
async fn contents(path: &Path) -> (Vec<String>, Vec<i64>) {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(SqliteConnectOptions::new().filename(path))
        .await
        .expect("DB を開く");
    let tables =
        sqlx::query_scalar("select name from sqlite_master where type = 'table' order by name")
            .fetch_all(&pool)
            .await
            .unwrap();
    let versions = sqlx::query_scalar("select version from _sqlx_migrations order by version")
        .fetch_all(&pool)
        .await
        .unwrap();
    pool.close().await;
    (tables, versions)
}

/// 出した移行の file は 1 byte も替えず、migrations/ には、出した物と、まだ出していないと名指した物のほかを置かない
/// （次の file を足した時に、ここに足し忘れると落ちる）。
#[tokio::test]
async fn released_migrations_stay_byte_for_byte() {
    for (file, want) in RELEASED {
        assert_eq!(sha256(&migrations_dir().join(file)).await, want, "{file}");
    }
    for file in names(&migrations_dir()).await {
        assert!(
            RELEASED.iter().any(|(f, _)| *f == file) || UNRELEASED.contains(&file.as_str()),
            "{file}: tests/migrations.rs の RELEASED か UNRELEASED に足す"
        );
    }
}

/// 後の版（移行を足した版）が書いた DB は、書いた版で起こすよう言って断り、移行を 1 つも当てない
/// （この server の版 1 を当てていない DB でも、当てる前に断る）。
#[tokio::test]
async fn a_database_from_a_later_version_is_refused_without_applying_migrations() {
    let ours = ours().await;
    for rows in [
        vec![(1, "init", ours.clone()), (2, "later", "00".repeat(48))],
        vec![(2, "later", "00".repeat(48))],
    ] {
        let path = ledger_db("refuse-later", &rows).await;
        let before = contents(&path).await;
        let e = match Db::open(&path).await {
            Ok(_) => panic!("後の版の DB を開いた（{rows:?}）"),
            Err(e) => e,
        };
        assert!(
            matches!(&e, OpenError::Migrate(MigrateError::VersionMissing(2))),
            "{e}"
        );
        assert!(
            e.to_string().contains("この server より新しい版の DB"),
            "{e}"
        );
        assert!(e.to_string().contains("移行 2"), "{e}");
        assert_eq!(contents(&path).await, before, "{rows:?}");
        remove_db(&path);
    }
}

/// 中身の違う移行を当てた DB は、ふつうの移行の誤りの訳で断る（後の版の訳を出さない）。
#[tokio::test]
async fn a_database_with_a_modified_migration_is_refused_as_a_migration_failure() {
    let path = ledger_db("refuse-modified", &[(1, "init", "00".repeat(48))]).await;
    let e = match Db::open(&path).await {
        Ok(_) => panic!("中身の違う移行の DB を開いた"),
        Err(e) => e,
    };
    assert!(
        matches!(&e, OpenError::Migrate(MigrateError::VersionMismatch(1))),
        "{e}"
    );
    assert!(e.to_string().starts_with("DB の移行に落ちた"), "{e}");
    assert!(!e.to_string().contains("新しい版"), "{e}");
    remove_db(&path);
}

/// binary も、後の版が書いた DB（この server の移行をみな当てた後に、後の版の移行を 1 つ当てた物）では起きずに 1 で終わり、
/// 後の版の訳を stdout と記録の置き場（DB の隣の logs/ の server.log と、終わり方の印の runs.log）に残す。
#[tokio::test]
async fn the_binary_refuses_a_database_from_a_later_version() {
    let path = temp_db("refuse-later-binary");
    let db = Db::open(&path).await.expect("この server の DB を作る");
    let newest: i64 = sqlx::query_scalar("select max(version) from _sqlx_migrations")
        .fetch_one(db.write())
        .await
        .unwrap();
    let later = newest + 1;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "insert into _sqlx_migrations (version, description, success, checksum, execution_time) \
         values ({later}, 'later', true, x'{}', 1)",
        "00".repeat(48)
    )))
    .execute(db.write())
    .await
    .unwrap();
    db.close().await;

    let mut child = common::server::command(&path, "127.0.0.1:0")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("binary を起こす");
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() > deadline {
            child.kill().unwrap();
            panic!("後の版の DB で起きたまま");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(status.code(), Some(1), "{stdout}{stderr}");
    let reason = format!(
        "この server より新しい版の DB（この server の migrations に無い移行 {later} を当ててある）"
    );
    assert!(
        stdout.contains(&reason),
        "stdout: {stdout}\nstderr: {stderr}"
    );
    let logs = path.with_file_name("logs");
    for file in ["server.log", "runs.log"] {
        let text = std::fs::read_to_string(logs.join(file)).unwrap_or_default();
        assert!(text.contains(&reason), "{file}: {text}");
    }
    remove_db(&path);
}
