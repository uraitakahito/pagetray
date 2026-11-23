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
    (
        5,
        "d9b883f0209a6f2280eb07d09f746cefbd994a0e5a9d47d5a2a30725928eebba28ea877228a0ab6a97c1257541e1aeeb",
    ),
    (
        6,
        "d116532ac7509c1bc8ac5108df6fd7c6a89f4e1b14d9dd4a3ab48bbe2ba43be64c08f9c77df0e6c9d10103bb632ff333",
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

/// 送り手の記録ができる前（移行 4 番まで）のページと file は、送り手が空のまま、前と同じく一覧に出る。
/// 送り手で絞ると、出ない。
#[tokio::test]
async fn pages_and_files_from_before_senders_list_without_a_sender() {
    use axum::body::Body;
    use axum::http::{Request, header};
    use http_body_util::BodyExt;
    use sqlx::migrate::Migrator;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use tower::ServiceExt;

    let path = std::env::temp_dir().join(format!(
        "pagetray-migrate-senders-{}.db",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let before = Migrator::with_migrations(
        sqlx::migrate!()
            .iter()
            .filter(|m| m.version <= 4)
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
    for sql in [
        "insert into pages(project, key, kind, n, title, role, workspace, created_at, body)
         values('p', 'K-1', 'plan', 1, '計画', 'tpm', '', '2026-11-22T00:00:00.000Z', x'3c703e783c2f703e')",
        "insert into files(project, key, name, ext, n, type, size, sha256, role, workspace, created_at, body)
         values('p', 'K-1', 'a', 'txt', 1, 'text/plain; charset=utf-8', 1, '', 'tpm', '', '2026-11-22T00:00:00.000Z', x'61')",
    ] {
        sqlx::query(sql).execute(&pool).await.unwrap();
    }
    pool.close().await;

    let db = pagetray::db::Db::open(&path).await.unwrap();
    let app = pagetray::router(pagetray::AppState::new(db.clone()));
    let get = |uri: &str| {
        let request = Request::builder()
            .uri(uri)
            .header(header::HOST, "127.0.0.1")
            .body(Body::empty())
            .unwrap();
        let app = app.clone();
        async move {
            let response = app.oneshot(request).await.unwrap();
            assert_eq!(response.status(), 200);
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()
        }
    };
    let pages = get("/api/pages").await;
    assert_eq!(pages["pages"].as_array().unwrap().len(), 1);
    assert!(pages["pages"][0].get("sender").is_none(), "{pages}");
    let files = get("/api/files").await;
    assert_eq!(files["files"].as_array().unwrap().len(), 1);
    assert!(files["files"][0].get("sender").is_none(), "{files}");
    assert!(
        get("/api/pages?actor=019a").await["pages"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        get("/api/files?actor=019a").await["files"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    db.close().await;
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
}
