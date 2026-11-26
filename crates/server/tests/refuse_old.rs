//! 0.8.x までの server の DB（切り替えの前の volume）と、後の版が書いた DB を、この server は開かず、何も書かない。
//! 古い DB は、0.8.x の移行の表の形（版 1〜6。checksum は v0.8.0 の tag の migrations/ の SHA-384）を SQL で作る
//! （0.8.x の server の code は repo に無い。v0.8.0 の tag の src/ にある）。
mod common;

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use common::{remove_db, temp_db};
use pagetray_server::db::{Db, OpenError};
use sha2::{Digest, Sha256, Sha384};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

/// 移すための道を指す字（0.8.x の DB の時の [`OpenError::Current`] の訳）。
const HINT: &str = "v1.0.0 の image の migrate";

/// v0.8.0 の tag の `migrations/0001_init.sql`〜`0006_review.sql` の SHA-384（sqlx の checksum）。
/// 値は `git show v0.8.0:migrations/<file> | shasum -a 384`。
const V0_8_CHECKSUMS: [(i64, &str, &str); 6] = [
    (
        1,
        "init",
        "8d9d677ffca4bb5056117010e6cca50b00f6499f637a5ee20d9a8e1fd3d46588eede801fa0bb84394afc52eeffb4467c",
    ),
    (
        2,
        "removed",
        "f26b7def147cf7927993bacabb7961ce5e4fc683334a28f8032699f47040830a665dd27fd305bf0e614cd15e8bd46fcd",
    ),
    (
        3,
        "files",
        "1a04612b3f7d791b0147fdcf10415349c52daa622038b7912167bdd8368ef046bfcf5f4d54abe47e4362cd9b561b8014",
    ),
    (
        4,
        "reply",
        "00ccb60233fe40d02081815f069812591c403c667f378a075d49b373c6370261a7e53f6c7fe74e769acce62197d6615e",
    ),
    (
        5,
        "senders",
        "d9b883f0209a6f2280eb07d09f746cefbd994a0e5a9d47d5a2a30725928eebba28ea877228a0ab6a97c1257541e1aeeb",
    ),
    (
        6,
        "review",
        "d116532ac7509c1bc8ac5108df6fd7c6a89f4e1b14d9dd4a3ab48bbe2ba43be64c08f9c77df0e6c9d10103bb632ff333",
    ),
];

/// 出した版（1.0.0 から）が当てた移行の file と、その SHA-256。1 byte も替えない（替えると checksum が変わり、動いている DB を
/// [`pagetray_server::db::refuse_current`] が「別の版の DB」として断る）。直す時は新しい番号の file を足す。
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

/// 移行の表の行（版・名・checksum の 16 進）と表を 1 つ持つ DB を作る。`wal` なら WAL の形で閉じる
/// （0.8.x の server は DB を WAL で開いていたので、古い volume の file は WAL の形。写しの `VACUUM INTO` の file は WAL でない）。
async fn ledger_db(tag: &str, rows: &[(i64, &str, String)], wal: bool) -> PathBuf {
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
    if wal {
        let mode: String = sqlx::query_scalar("pragma journal_mode = wal")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(mode, "wal");
    }
    pool.close().await;
    // 閉じると -wal と -shm は消える（古い volume で止めた pagetray と同じ形）。
    assert_eq!(
        siblings(&path).await,
        Vec::<String>::new(),
        "{}",
        path.display()
    );
    path
}

async fn old_db(tag: &str, wal: bool) -> PathBuf {
    let rows: Vec<(i64, &str, String)> = V0_8_CHECKSUMS
        .iter()
        .map(|(v, d, c)| (*v, *d, (*c).to_owned()))
        .collect();
    ledger_db(tag, &rows, wal).await
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

/// 同じ folder に、`name` で始まる別の file（`-wal`・`-shm`・`-journal`）があるか。
async fn siblings(path: &Path) -> Vec<String> {
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    names(path.parent().unwrap())
        .await
        .into_iter()
        .filter(|n| n.starts_with(&name) && *n != name)
        .collect()
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
            "{file}: tests/refuse_old.rs の RELEASED か UNRELEASED に足す"
        );
    }
}

/// `Db::open` は訳（移す道）を出して断り、file を替えず、隣に file を作らない。WAL の形の file（古い volume）も、写しの形も同じ。
#[tokio::test]
async fn opening_an_old_database_is_refused_without_writing() {
    for wal in [false, true] {
        let path = old_db(&format!("refuse-open-{wal}"), wal).await;
        let before = sha256(&path).await;
        let e = match Db::open(&path).await {
            Ok(_) => panic!("0.8.x の DB を開いた（wal={wal}）"),
            Err(e) => e,
        };
        assert!(
            matches!(&e, OpenError::Current(v) if v == &vec![1, 2, 3, 4, 5, 6]),
            "wal={wal}: {e}"
        );
        assert!(e.to_string().contains("1・2・3・4・5・6"), "wal={wal}: {e}");
        assert!(e.to_string().contains(HINT), "wal={wal}: {e}");
        assert_eq!(sha256(&path).await, before, "wal={wal}");
        assert_eq!(siblings(&path).await, Vec::<String>::new(), "wal={wal}");
        remove_db(&path);
    }
}

/// 後の版（移行を足した版）が書いた DB は、移す道でなく、書いた版で起こすよう言って断る。
#[tokio::test]
async fn a_database_from_a_later_version_is_refused_with_its_own_reason() {
    let ours = Sha384::digest(
        tokio::fs::read(migrations_dir().join("0001_init.sql"))
            .await
            .unwrap(),
    );
    let rows = [(1, "init", hex(&ours)), (2, "later", "00".repeat(48))];
    let path = ledger_db("refuse-later", &rows, true).await;
    let before = sha256(&path).await;
    let e = match Db::open(&path).await {
        Ok(_) => panic!("後の版の DB を開いた"),
        Err(e) => e,
    };
    assert!(matches!(&e, OpenError::Current(v) if v == &vec![2]), "{e}");
    assert!(
        e.to_string().contains("この server より新しい版の DB"),
        "{e}"
    );
    assert!(!e.to_string().contains("migrate"), "{e}");
    assert_eq!(sha256(&path).await, before);
    assert_eq!(siblings(&path).await, Vec::<String>::new());
    remove_db(&path);
}

/// binary も、訳を出して 1 で終わり、DB の隣に記録の置き場（logs/）も作らない。
#[tokio::test]
async fn the_binary_refuses_an_old_database_and_leaves_no_logs() {
    for wal in [false, true] {
        let path = old_db(&format!("refuse-binary-{wal}"), wal).await;
        let before = sha256(&path).await;
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
                panic!("0.8.x の DB で起きたまま（wal={wal}）");
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        };
        let out = child.wait_with_output().unwrap();
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(status.code(), Some(1), "wal={wal}: {text}");
        assert!(text.contains(HINT), "wal={wal}: {text}");
        assert_eq!(sha256(&path).await, before, "wal={wal}");
        assert_eq!(siblings(&path).await, Vec::<String>::new(), "wal={wal}");
        assert!(
            !tokio::fs::try_exists(path.with_file_name("logs"))
                .await
                .unwrap(),
            "古い volume に記録の置き場を作った（wal={wal}）"
        );
        remove_db(&path);
    }
}

/// `pagetray-server migrate …` は、起きずに、移す命令のある所（v1.0.0 の image）を言って 2 で終わる。
#[test]
fn the_migrate_argument_points_to_the_v1_0_0_image() {
    let out = std::process::Command::new(common::server::binary())
        .args([
            "migrate",
            "--from",
            "/in/pages.db",
            "--to",
            "/data/pages.db",
        ])
        .output()
        .expect("binary を起こす");
    let text = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "{text}");
    assert!(
        text.contains("v1.0.0 の image（ghcr.io/uraitakahito/pagetray:v1.0.0）"),
        "{text}"
    );
}
