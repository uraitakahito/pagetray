//! 当てた移行の SQL が変わっていないかを見る。
//!
//! sqlx は、当てた SQL の SHA-384 を DB（`_sqlx_migrations`）に覚えていて、起きる時に突き合わせる。
//! 当てた SQL を 1 字でも変えると、動いている DB では起きなくなる。CI には動いている DB が無いので、ここで止める。
//! 表を変える時は、次の番号の file を足して、下の一覧にも 1 行足す（前からある行は変えない）。

/// 版ごとの SHA-384（`shasum -a 384 migrations/*.sql` と同じ値。動いている DB が覚えている値でもある）。
const APPLIED: &[(i64, &str)] = &[
    (
        1,
        "8d9d677ffca4bb5056117010e6cca50b00f6499f637a5ee20d9a8e1fd3d46588eede801fa0bb84394afc52eeffb4467c",
    ),
    (
        2,
        "f26b7def147cf7927993bacabb7961ce5e4fc683334a28f8032699f47040830a665dd27fd305bf0e614cd15e8bd46fcd",
    ),
    (
        3,
        "1a04612b3f7d791b0147fdcf10415349c52daa622038b7912167bdd8368ef046bfcf5f4d54abe47e4362cd9b561b8014",
    ),
    (
        4,
        "00ccb60233fe40d02081815f069812591c403c667f378a075d49b373c6370261a7e53f6c7fe74e769acce62197d6615e",
    ),
];

#[test]
fn applied_migrations_are_never_edited() {
    let now: Vec<(i64, String)> = sqlx::migrate!()
        .iter()
        .map(|m| {
            let hex = m.checksum.iter().map(|b| format!("{b:02x}")).collect();
            (m.version, hex)
        })
        .collect();
    let want: Vec<(i64, String)> = APPLIED.iter().map(|(v, h)| (*v, (*h).to_owned())).collect();
    assert_eq!(
        now, want,
        "移行の SQL が変わった。当てた SQL は変えず、次の番号の file を足す"
    );
}

/// 前の版（移行 3 番まで）の DB に、置かれたページがある所から起こす。4 番が当たり、前からのページは
/// そのまま残り、返事の状態はどれも空になる。
#[tokio::test]
async fn a_database_of_the_previous_version_is_migrated_and_keeps_its_pages() {
    use sqlx::migrate::Migrator;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    let path = std::env::temp_dir().join(format!("pagetray-migrate-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let all = sqlx::migrate!();
    let before = Migrator::with_migrations(
        all.iter()
            .filter(|m| m.version <= 3)
            .cloned()
            .collect::<Vec<_>>(),
    );
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true),
        )
        .await
        .unwrap();
    before.run(&pool).await.unwrap();
    sqlx::query(
        "insert into pages(project, key, kind, n, title, role, workspace, created_at, body)
         values('p', 'K-1', 'confirm', 1, '確認', 'software-engineer', '', '2026-11-22T00:00:00.000Z', x'3c703e783c2f703e')",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;

    let db = pagetray::db::Db::open(&path).await.unwrap();
    let pages = db
        .list(&pagetray::db::Filter {
            limit: 10,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].n, 1);
    assert_eq!((&pages[0].asked_at, &pages[0].answered_at), (&None, &None));
    let waiting = db
        .list(&pagetray::db::Filter {
            reply: Some(pagetray::db::Reply::Waiting),
            limit: 10,
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(waiting.is_empty());
    db.close().await;

    // 4 番を当てた DB を、3 番までしか知らない版で起こすと、断られる（前の版へは戻せない）。
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(SqliteConnectOptions::new().filename(&path))
        .await
        .unwrap();
    assert!(before.run(&pool).await.is_err());
    pool.close().await;
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
}
