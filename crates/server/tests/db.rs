//! DB の決まりの試験: 移行（今の migrations と同じ作り）・足すだけの送り手・外のキー・接続の設定・開かない DB。
//! 書く接続と読む接続を直に使う。
mod common;

use std::path::Path;

use common::{App, remove_db, temp_db};
use pagetray_next::db::{Db, OpenError};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Connection, SqlitePool};

/// 今の server の migrations（repo の頭の migrations/。0001〜0006）。
fn current_migrations() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../migrations")
}

/// 移行の後の作りを、比べられる字の並びにする（表・列・外のキー・索引・strict・check・trigger と索引の SQL・全文検索の表）。
async fn schema(pool: &SqlitePool) -> Vec<String> {
    let mut out: Vec<String> = sqlx::query_scalar(
        "select type || ' ' || name || ' on ' || tbl_name from sqlite_schema
         where name not like 'sqlite_%' and name != '_sqlx_migrations' order by type, name",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    let tables: Vec<(String, String)> = sqlx::query_as(
        "select name, sql from sqlite_schema where type = 'table' and name not like 'sqlite_%'
         and name != '_sqlx_migrations' order by name",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    let per_table = [
        "select 'column ' || cid || ' ' || name || ' ' || type || ' notnull=' || \"notnull\" || ' default=' || coalesce(dflt_value, '-') || ' pk=' || pk || ' hidden=' || hidden from pragma_table_xinfo(?1) order by cid",
        "select 'fk ' || id || ' ' || seq || ' ' || \"table\" || ' ' || \"from\" || '->' || coalesce(\"to\", '-') || ' update=' || on_update || ' delete=' || on_delete || ' match=' || \"match\" from pragma_foreign_key_list(?1) order by id, seq",
        "select 'index ' || l.name || ' unique=' || l.\"unique\" || ' origin=' || l.origin || ' partial=' || l.partial || ' (' || (select group_concat(name, ',') from pragma_index_info(l.name)) || ')' from pragma_index_list(?1) l order by l.name",
        "select 'list strict=' || \"strict\" || ' type=' || type || ' ncol=' || ncol from pragma_table_list where name = ?1",
    ];
    for (table, sql) in tables {
        out.push(format!("== {table}"));
        for query in per_table {
            let rows: Vec<String> = sqlx::query_scalar(query)
                .bind(&table)
                .fetch_all(pool)
                .await
                .unwrap_or_else(|e| panic!("{table}: {e}"));
            out.extend(rows);
        }
        out.extend(checks(&sql).into_iter().map(|c| format!("check {c}")));
        if sql.to_lowercase().starts_with("create virtual table") {
            out.push(normalize(&sql));
        }
    }
    let sqls: Vec<(String, String)> = sqlx::query_as(
        "select name, sql from sqlite_schema where type in ('trigger', 'index') and sql is not null order by name",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    out.extend(
        sqls.into_iter()
            .map(|(n, s)| format!("{n}: {}", normalize(&s))),
    );
    out
}

/// 空白を 1 つにまとめ、小文字にする。
fn normalize(sql: &str) -> String {
    sql.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
        .replace("( ", "(")
        .replace(" )", ")")
}

/// 表の SQL の中の `check (…)` の中身（入れ子の括弧も）。註（`--` の行の残り）は除く。
fn checks(sql: &str) -> Vec<String> {
    let sql: String = sql
        .lines()
        .map(|l| l.split("--").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join(" ");
    let lower = sql.to_lowercase();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = lower[from..].find("check") {
        let start = from + i + "check".len();
        let Some(open) = lower[start..].find('(').map(|j| start + j) else {
            break;
        };
        let mut depth = 0;
        let mut end = open;
        for (k, c) in lower[open..].char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = open + k;
                        break;
                    }
                }
                _ => {}
            }
        }
        out.push(normalize(&lower[open + 1..end]));
        from = end;
    }
    out.sort();
    out
}

async fn fresh_pool(path: &Path) -> SqlitePool {
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(path)
                .create_if_missing(true),
        )
        .await
        .unwrap()
}

/// 新しい migrations の 1 つが、今の server の 0001〜0006 を当てた後と同じ作りを作る。
#[tokio::test]
async fn the_one_migration_makes_the_same_schema_as_the_current_six() {
    let old_path = temp_db("old-schema");
    let old = fresh_pool(&old_path).await;
    let migrator = sqlx::migrate::Migrator::new(current_migrations())
        .await
        .unwrap();
    assert_eq!(migrator.iter().count(), 6, "今の migrations は 6 つ");
    migrator.run(&old).await.unwrap();
    let app = App::start().await;
    let want = schema(&old).await;
    let got = schema(app.db.read()).await;
    old.close().await;
    remove_db(&old_path);
    assert!(
        want.iter().any(|l| l.starts_with("check ")),
        "check を読めていない: {want:?}"
    );
    assert!(
        want.iter()
            .any(|l| l.contains("pages_waiting:") && l.contains("where")),
        "{want:?}"
    );
    assert_eq!(got, want);
    let versions: Vec<i64> = sqlx::query_scalar("select version from _sqlx_migrations")
        .fetch_all(app.db.read())
        .await
        .unwrap();
    assert_eq!(versions, vec![1]);
}

#[tokio::test]
async fn senders_refuse_update_and_delete() {
    let app = App::start().await;
    app.execute(
        "insert into senders(actor_issuer, actor_ref, created_at) values('tpm', 'claude.session:s-1', '2026-11-24T00:00:00.000Z')",
    )
    .await;
    for sql in [
        "update senders set actor_ref = 'x' where id = 1",
        "delete from senders where id = 1",
        "insert or replace into senders select * from senders where id = 1",
        "replace into senders select * from senders where id = 1",
    ] {
        let refused = sqlx::query(sql)
            .execute(app.db.write())
            .await
            .expect_err(&format!("断らない: {sql}"));
        assert!(
            refused.to_string().contains("append-only: senders"),
            "{sql}: {refused}"
        );
    }
    assert_eq!(app.count("senders").await, 1);
}

#[tokio::test]
async fn foreign_keys_refuse_missing_parents_and_targets_go_with_their_page() {
    let app = App::start().await;
    app.place("p", "k", "review", "role=tpm", "<p>1</p>").await;
    for sql in [
        "update pages set sender_id = 99 where id = 1",
        "insert into files(project, key, name, ext, n, type, size, sha256, role, workspace, created_at, body, sender_id)
         values('p', 'k', 'a', 'png', 1, 'image/png', 1, 'x', 'tpm', '', '2026-11-24T00:00:00.000Z', x'00', 99)",
        "insert into page_targets(page_id, target) values(99, '/p/k/plan-1.html')",
    ] {
        let refused = sqlx::query(sql)
            .execute(app.db.write())
            .await
            .expect_err(&format!("断らない: {sql}"));
        assert!(refused.to_string().contains("FOREIGN KEY"), "{sql}: {refused}");
    }
    // 判定と待ちの相手は、決まった字だけ。
    for sql in [
        "update pages set verdict = 'ok' where id = 1",
        "update pages set waits_on = 'pm' where id = 1",
    ] {
        let refused = sqlx::query(sql)
            .execute(app.db.write())
            .await
            .expect_err(&format!("断らない: {sql}"));
        assert!(refused.to_string().contains("CHECK"), "{sql}: {refused}");
    }
    app.execute("insert into page_targets(page_id, target) values(1, 'https://example.com/pr/1')")
        .await;
    app.execute("delete from pages where id = 1").await;
    assert_eq!(app.count("page_targets").await, 0);
}

#[tokio::test]
async fn every_connection_has_foreign_keys_wal_busy_timeout_and_recursive_triggers() {
    let app = App::start().await;
    let mut held = Vec::new();
    for _ in 0..pagetray_next::db::READS {
        held.push(app.db.read().acquire().await.expect("読む接続"));
    }
    held.push(app.db.write().acquire().await.expect("書く接続"));
    for conn in &mut held {
        let mut got = Vec::new();
        for pragma in [
            "foreign_keys",
            "recursive_triggers",
            "busy_timeout",
            "temp_store",
        ] {
            let value: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!("pragma {pragma}")))
                .fetch_one(&mut **conn)
                .await
                .unwrap();
            got.push(value);
        }
        let journal: String = sqlx::query_scalar("pragma journal_mode")
            .fetch_one(&mut **conn)
            .await
            .unwrap();
        // temp_store の 2 は MEMORY。
        assert_eq!((got, journal.as_str()), (vec![1, 1, 5000, 2], "wal"));
    }
    // 読む接続は書けない。
    let refused = sqlx::query(
        "insert into senders(actor_issuer, actor_ref, created_at) values('a', 'b:c', 'd')",
    )
    .execute(&mut *held[0])
    .await
    .expect_err("読む接続で書けた");
    assert!(refused.to_string().contains("readonly"), "{refused}");
}

#[tokio::test]
async fn a_db_with_broken_foreign_keys_is_not_opened() {
    let path = temp_db("broken");
    let db = Db::open(&path).await.unwrap();
    db.close().await;
    // 外のキーを切った接続で、無い送り手を指すページを書く。
    let mut conn =
        sqlx::SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&path))
            .await
            .unwrap();
    sqlx::query("pragma foreign_keys = off")
        .execute(&mut conn)
        .await
        .unwrap();
    sqlx::query(
        "insert into pages(project, key, kind, n, title, role, workspace, created_at, body, sender_id)
         values('p', 'k', 'plan', 1, '', 'tpm', '', '2026-11-24T00:00:00.000Z', x'00', 99)",
    )
    .execute(&mut conn)
    .await
    .unwrap();
    conn.close().await.unwrap();
    let refused = Db::open(&path).await.err().expect("開いた");
    assert!(
        matches!(&refused, OpenError::ForeignKey(rows) if rows.len() == 1),
        "{refused}"
    );
    assert!(refused.to_string().contains("pages"), "{refused}");
    remove_db(&path);
}

#[tokio::test]
async fn opening_again_keeps_the_rows_and_does_not_migrate_twice() {
    let app = App::start().await;
    app.place("p", "k", "plan", "role=tpm", "<p>1</p>").await;
    app.db.close().await;
    let db = Db::open(&app.path).await.unwrap();
    let pages: i64 = sqlx::query_scalar("select count(*) from pages")
        .fetch_one(db.read())
        .await
        .unwrap();
    let migrations: i64 = sqlx::query_scalar("select count(*) from _sqlx_migrations")
        .fetch_one(db.read())
        .await
        .unwrap();
    assert_eq!((pages, migrations), (1, 1));
    db.close().await;
}

/// 今の server の DB（0001〜0006 を当てた物）は、何も書かずに断る。
#[tokio::test]
async fn the_current_servers_db_is_refused_without_writing() {
    let path = temp_db("current");
    let old = fresh_pool(&path).await;
    sqlx::migrate::Migrator::new(current_migrations())
        .await
        .unwrap()
        .run(&old)
        .await
        .unwrap();
    sqlx::query(
        "insert into pages(project, key, kind, n, title, role, workspace, created_at, body)
         values('p', 'k', 'plan', 1, '', 'tpm', '', '2026-11-24T00:00:00Z', x'00')",
    )
    .execute(&old)
    .await
    .unwrap();
    old.close().await;
    let before = std::fs::read(&path).unwrap();
    let refused = Db::open(&path)
        .await
        .err()
        .expect("今の server の DB を開いた");
    assert!(
        matches!(&refused, OpenError::Current(v) if v == &vec![1, 2, 3, 4, 5, 6]),
        "{refused}"
    );
    assert!(
        refused.to_string().contains("今の server の DB"),
        "{refused}"
    );
    // 中身も journal の形も替えていない（WAL にしていない）。
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let mut wal = path.clone().into_os_string();
    wal.push("-wal");
    assert!(!Path::new(&wal).exists(), "-wal ができた");
    remove_db(&path);
}

/// 移行の表の無い SQLite の file（空の DB）と、無い file は開く（作る）。
#[tokio::test]
async fn an_empty_db_or_a_missing_file_is_opened() {
    let path = temp_db("empty");
    let empty = fresh_pool(&path).await;
    sqlx::query("create table other(x)")
        .execute(&empty)
        .await
        .unwrap();
    empty.close().await;
    let db = Db::open(&path).await.expect("空の DB を開けない");
    db.close().await;
    remove_db(&path);
    let path = temp_db("missing");
    let db = Db::open(&path).await.expect("無い file を開けない");
    db.close().await;
    remove_db(&path);
}
