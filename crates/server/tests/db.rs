//! DB の決まりの試験: 足すだけの送り手・外のキー・接続の設定・開かない DB（移行の表の合わない DB は tests/migrations.rs）。
//! 書く接続と読む接続を直に使う。
mod common;

use std::path::Path;

use common::{App, remove_db, temp_db};
use pagetray_server::db::{Db, OpenError};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Connection, SqlitePool};

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
    for _ in 0..pagetray_server::db::READS {
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
