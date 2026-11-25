//! 今の server の DB を新しい DB へ移す（`pagetray-next migrate`）。移す元の DB は、同じ workspace の今の server
//! （repo の頭の crate `pagetray`）の Router に今の API で書き、`GET /api/backup` で取った写し。今の migrations の途中の版
//! （0001〜0005 まで当てた物）の DB は、その版まで当てて SQL で行を置き、`VACUUM INTO` で写しにする。
//! 移した後は、新しい API で読み、今の API で読んだ物と比べる。
mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use common::{App, HOST, PNG, html, temp_db};
use http_body_util::BodyExt;
use pagetray_next::db::{Db, OpenError};
use pagetray_next::migrate::{self, MigrateError};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tower::ServiceExt;

/// 移す表（親が先）。
const TABLES: [&str; 6] = [
    "senders",
    "pages",
    "removed",
    "page_targets",
    "files",
    "files_removed",
];
/// 本文を持つ表。
const BODIES: [&str; 2] = ["pages", "files"];
/// castlist のアクターの id（小文字の UUID）。
const ACTOR: &str = "01a20a45-c62c-76fa-9f5f-4a4d49204ab8";
/// 字の UTF-8 でない本文（題と索引の字は、読めない byte を置き換えて取る）。
const NOT_UTF8: &[u8] = b"<title>bytes</title><p>\xff\xfe raw</p>";

// --- 今の server ---

/// 今の server の DB と Router（今の API）。
struct Old {
    app: Router,
    path: PathBuf,
}

impl Drop for Old {
    fn drop(&mut self) {
        common::remove_db(&self.path);
    }
}

impl Old {
    async fn start() -> Self {
        let path = temp_db("old");
        let db = pagetray::db::Db::open(&path).await.expect("今の Db::open");
        let app = pagetray::router(pagetray::AppState::new(db));
        Self { app, path }
    }

    async fn send(
        &self,
        method: Method,
        uri: &str,
        content_type: Option<&str>,
        body: Vec<u8>,
    ) -> (StatusCode, Vec<u8>) {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, HOST);
        if let Some(t) = content_type {
            builder = builder.header(header::CONTENT_TYPE, t);
        }
        let response = self
            .app
            .clone()
            .oneshot(builder.body(Body::from(body)).unwrap())
            .await
            .expect("oneshot");
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, bytes.to_vec())
    }

    /// 書いて、2xx であることを見て、答えの JSON を返す。
    async fn write(
        &self,
        method: Method,
        uri: &str,
        content_type: Option<&str>,
        body: impl Into<Vec<u8>>,
    ) -> Value {
        let (status, bytes) = self.send(method, uri, content_type, body.into()).await;
        let answer: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        assert!(status.is_success(), "今の {uri}: {status} {answer}");
        answer
    }

    /// ページを置き、path を返す。
    async fn put_page(&self, query: &str, body: impl Into<Vec<u8>>) -> String {
        let answer = self
            .write(
                Method::POST,
                &format!("/api/pages?{query}"),
                Some("text/html"),
                body,
            )
            .await;
        answer["path"].as_str().expect("path").to_owned()
    }

    async fn put_file(&self, query: &str, content_type: &str, body: &[u8]) -> String {
        let answer = self
            .write(
                Method::POST,
                &format!("/api/files?{query}"),
                Some(content_type),
                body.to_vec(),
            )
            .await;
        answer["path"].as_str().expect("path").to_owned()
    }

    async fn get(&self, uri: &str) -> (StatusCode, Vec<u8>) {
        self.send(Method::GET, uri, None, Vec::new()).await
    }

    async fn json(&self, uri: &str) -> Value {
        let (status, bytes) = self.get(uri).await;
        let answer: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        assert_eq!(status, StatusCode::OK, "今の {uri}: {answer}");
        answer
    }

    /// `GET /api/backup` の写しを `to` に置く。
    async fn backup(&self, to: &Path) {
        let (status, bytes) = self.get("/api/backup").await;
        assert_eq!(status, StatusCode::OK, "今の backup");
        tokio::fs::write(to, bytes).await.expect("写しを置く");
    }

    /// 今の API では書けない行を、DB に直に書く（別の接続。WAL なので、Router の接続と並べて書ける）。
    async fn execute(&self, sql: &str) {
        let pool = pool(&self.path).await;
        sqlx::query(sqlx::AssertSqlSafe(sql.to_owned()))
            .execute(&pool)
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e}"));
        pool.close().await;
    }
}

async fn pool(path: &Path) -> SqlitePool {
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(path)
                .create_if_missing(true),
        )
        .await
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// 今の server に、どの表にも行のある見本を書く: 秒の下の桁の違う置いた時刻（0・1・2 桁と、server が付けた 3 桁）・
/// 返事待ちと答えた・レビューの判定と見た物・消したページと file・付随する id だけの送り手と、castlist の答えのある送り手・
/// UTF-8 でない本文。
async fn seed(old: &Old) {
    const PLAN: &str = "project=ai&key=AI-36&kind=plan&role=tpm";
    old.put_page(
        &format!("{PLAN}&workspace=w:1&created_at=2026-01-02T03:04:05Z"),
        html("移す計画", "<p>全文の索引を作り直す</p>"),
    )
    .await;
    old.put_page(
        &format!("{PLAN}&created_at=2026-01-02T03:04:05.5Z"),
        html("消す計画", "<p>消す</p>"),
    )
    .await;
    old.put_page(
        &format!(
            "{PLAN}&created_at=2026-01-02T03:04:05.25Z&actor_issuer=tpm&actor_ref=claude.session:s-1"
        ),
        html("三つ目", "<p>送り手の付いた計画</p>"),
    )
    .await;
    old.put_page(
        "project=ai&key=AI-36&kind=review&role=reviewer&actor_issuer=tpm&actor_ref=claude.session:s-2",
        html("レビュー", "<p>見た</p>"),
    )
    .await;
    old.put_page("project=ai&key=AI-37&kind=result&role=tpm", NOT_UTF8)
        .await;
    for query in [
        "project=ai&key=AI-36&kind=plan&n=1&state=waiting",
        "project=ai&key=AI-36&kind=plan&n=1&state=answered",
        "project=ai&key=AI-36&kind=plan&n=3&state=waiting&on=review",
    ] {
        old.write(
            Method::POST,
            &format!("/api/pages/reply?{query}"),
            None,
            Vec::new(),
        )
        .await;
    }
    old.write(
        Method::POST,
        "/api/pages/review?project=ai&key=AI-36&kind=review&n=1",
        Some("application/json"),
        json!({
            "verdict": "concerns",
            "targets": ["/ai/AI-36/plan-3.html", "https://github.com/uraitakahito/pagetray/pull/57", "/ai/AI-36/plan-1.html"],
        })
        .to_string(),
    )
    .await;
    old.write(
        Method::DELETE,
        "/api/pages?project=ai&key=AI-36&kind=plan&n=2",
        None,
        Vec::new(),
    )
    .await;
    const FILE: &str = "project=ai&key=AI-36&role=tpm";
    old.put_file(
        &format!("{FILE}&name=dot.png&created_at=2026-01-02T03:04:06Z"),
        "image/png",
        PNG,
    )
    .await;
    old.put_file(
        &format!("{FILE}&name=dot.png&actor_issuer=tpm&actor_ref=claude.session:s-1"),
        "image/png",
        PNG,
    )
    .await;
    old.put_file(
        &format!("{FILE}&name=notes.md&workspace=w:2"),
        "text/markdown",
        "# メモ\n".as_bytes(),
    )
    .await;
    old.write(
        Method::DELETE,
        "/api/files?project=ai&key=AI-36&name=dot.png&n=1",
        None,
        Vec::new(),
    )
    .await;
    // castlist の答えのある送り手（今の server は castlist に聞いて書く。試験では直に置き、UTF-8 でない本文のページから指す）。
    old.execute(&format!(
        "insert into senders(actor_issuer, actor_ref, actor_id, actor_name, actor_kind, resolved_at, created_at)
         values('tpm', 'claude.session:s-9', '{ACTOR}', 'se.9', 'ai', '2026-01-02T03:04:07Z', '2026-01-02T03:04:07.1Z')"
    ))
    .await;
    old.execute("update pages set sender_id = (select max(id) from senders) where kind = 'result'")
        .await;
}

/// 見本を書いた今の server と、その写しの path。
async fn backup() -> (Old, PathBuf) {
    let old = Old::start().await;
    seed(&old).await;
    let path = temp_db("backup");
    old.backup(&path).await;
    (old, path)
}

/// 今の migrations（repo の頭の migrations/。0001〜0006）。
fn current_migrations() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../migrations")
}

/// 今の migrations を `version` まで当てた DB に、その版の表に合う行を SQL で置き、`VACUUM INTO` の写しにする。
async fn backup_at(version: i64) -> PathBuf {
    let path = temp_db(&format!("v{version}"));
    let pool = pool(&path).await;
    sqlx::migrate::Migrator::new(current_migrations())
        .await
        .unwrap()
        .run_to(version, &pool)
        .await
        .unwrap();
    let mut sqls = vec![
        // 全文の索引の行は置かない（移す時に本文から作り直すかを見る）。
        "insert into pages(id, project, key, kind, n, title, role, workspace, created_at, body) values
           (3, 'ai', 'AI-1', 'plan', 1, '古い版', 'tpm', '', '2026-01-02T03:04:05Z', cast('<title>古い版</title><p>途中の版の DB</p>' as blob)),
           (7, 'ai', 'AI-1', 'plan', 3, '次', 'tpm', 'w:1', '2026-01-02T03:04:06.5Z', cast('<p>次</p>' as blob))"
            .to_owned(),
    ];
    if version >= 2 {
        sqls.push(
            "insert into removed values('ai', 'AI-1', 'plan', 5, '2026-01-02T03:04:07Z')"
                .to_owned(),
        );
    }
    if version >= 3 {
        sqls.push(format!(
            "insert into files(id, project, key, name, ext, n, type, size, sha256, role, workspace, created_at, body)
             values(4, 'ai', 'AI-1', 'dot', 'png', 2, 'image/png', {}, '{}', 'tpm', '', '2026-01-02T03:04:08Z', x'{}')",
            PNG.len(),
            hex(&Sha256::digest(PNG)),
            hex(PNG)
        ));
        sqls.push(
            "insert into files_removed values('ai', 'AI-1', 'dot', 'png', 1, '2026-01-02T03:04:09.25Z')"
                .to_owned(),
        );
    }
    if version >= 4 {
        sqls.push(
            "update pages set asked_at = '2026-01-02T03:04:10Z', answered_at = '2026-01-02T03:04:11.5Z' where n = 1"
                .to_owned(),
        );
    }
    if version >= 5 {
        sqls.push(format!(
            "insert into senders(id, actor_issuer, actor_ref, actor_id, actor_name, actor_kind, resolved_at, created_at)
             values(2, 'tpm', 'claude.session:s-1', '{ACTOR}', 'se.1', 'ai', '2026-01-02T03:04:12Z', '2026-01-02T03:04:12Z')"
        ));
        sqls.push("update pages set sender_id = 2 where n = 3".to_owned());
        sqls.push("update files set sender_id = 2".to_owned());
    }
    for sql in &sqls {
        sqlx::query(sqlx::AssertSqlSafe(sql.clone()))
            .execute(&pool)
            .await
            .unwrap_or_else(|e| panic!("v{version}: {sql}: {e}"));
    }
    let copy = temp_db(&format!("v{version}-backup"));
    sqlx::query("vacuum into ?1")
        .bind(copy.to_string_lossy().into_owned())
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    common::remove_db(&path);
    copy
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// --- 新しい server ---

/// 移した DB を、新しい server で開く。
async fn open_new(path: &Path) -> App {
    let db = Arc::new(Db::open(path).await.expect("新しい Db::open"));
    let app = pagetray_next::app(db.clone());
    App {
        app,
        db,
        path: path.to_owned(),
    }
}

/// 移す（library の関数）。突き合わせが合うことも見る。
async fn migrated(from: &Path) -> (PathBuf, migrate::Summary) {
    let to = temp_db("new");
    let summary = migrate::migrate(from, &to)
        .await
        .unwrap_or_else(|e| panic!("移す: {e}"));
    assert!(summary.ok(), "{summary}");
    (to, summary)
}

async fn new_json(app: &App, uri: &str) -> Value {
    let (status, _, answer) = app.json(uri).await;
    assert_eq!(status, StatusCode::OK, "新しい {uri}: {answer}");
    answer
}

// --- 比べる形 ---

fn camel(key: &str) -> String {
    let mut out = String::new();
    let mut up = false;
    for c in key.chars() {
        if c == '_' {
            up = true;
        } else if up {
            out.extend(c.to_uppercase());
            up = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// 今の答えと新しい答えを、同じ形にして比べる: 欄の名は camelCase（file の `type` は `contentType`）、null の欄は無い物、
/// 番号 `n` は字、時刻の字は残す字（24 字の UTC）に直した物、頁の印は外す。
fn norm(v: &Value) -> Value {
    match v {
        Value::Object(map) => {
            let mut out = Map::new();
            for (k, v) in map {
                let k = match camel(k).as_str() {
                    "type" => "contentType".to_owned(),
                    k => k.to_owned(),
                };
                if v.is_null() || k == "nextToken" {
                    continue;
                }
                let v = match (k.as_str(), v) {
                    ("n", Value::Number(n)) => Value::String(n.to_string()),
                    _ => norm(v),
                };
                out.insert(k, v);
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(norm).collect()),
        Value::String(s) => match OffsetDateTime::parse(s, &Rfc3339) {
            Ok(t) => Value::String(pagetray_next::db::stamp(t)),
            Err(_) => v.clone(),
        },
        _ => v.clone(),
    }
}

/// 一覧の答えの行を、path の順に並べ替えた物（今の server は、秒の下の桁の違う時刻を字の順に並べるので、
/// 並びは新しい server と違いうる。並びは別に見る）。
fn by_path(list: &Value, field: &str) -> Vec<Value> {
    let mut rows: Vec<Value> = norm(list)[field]
        .as_array()
        .unwrap_or_else(|| panic!("{list}"))
        .clone();
    rows.sort_by_key(|r| r["path"].as_str().unwrap_or_default().to_owned());
    rows
}

fn paths(list: &Value, field: &str) -> Vec<String> {
    list[field]
        .as_array()
        .unwrap_or_else(|| panic!("{list}"))
        .iter()
        .map(|r| r["path"].as_str().unwrap().to_owned())
        .collect()
}

/// 今の API で読んだ物と、移した後に新しい API で読んだ物が同じか（ページと file の記録・本文の byte・消した物の 410）。
async fn same_reads(old: &Old, new: &App) {
    let old_pages = old.json("/api/pages?sort=created_at&limit=1000").await;
    let new_pages = new_json(new, "/api/pages?sort=createdAt&maxResults=1000").await;
    assert_eq!(
        by_path(&new_pages, "pages"),
        by_path(&old_pages, "pages"),
        "ListPages"
    );
    let old_files = old.json("/api/files?sort=created_at&limit=1000").await;
    let new_files = new_json(new, "/api/files?sort=createdAt&maxResults=1000").await;
    assert_eq!(
        by_path(&new_files, "files"),
        by_path(&old_files, "files"),
        "ListFiles"
    );
    for path in paths(&old_pages, "pages")
        .into_iter()
        .chain(paths(&old_files, "files"))
    {
        let (status, want) = old.get(&path).await;
        assert_eq!(status, StatusCode::OK, "今の {path}");
        let (status, _, got) = new.get(&path).await;
        assert_eq!(status, StatusCode::OK, "新しい {path}");
        assert!(got == want, "{path} の本文の byte が違う");
    }
    for gone in ["/ai/AI-36/plan-2.html", "/ai/AI-36/files/dot-1.png"] {
        assert_eq!(old.get(gone).await.0, StatusCode::GONE, "今の {gone}");
        assert_eq!(new.get(gone).await.0, StatusCode::GONE, "新しい {gone}");
    }
}

/// 1 列の値を読む SQL（DB の file を、読むだけで開く）。
async fn scalars<T>(path: &Path, sql: &str) -> Vec<T>
where
    T: for<'r> sqlx::Decode<'r, sqlx::Sqlite> + sqlx::Type<sqlx::Sqlite> + Send + Unpin,
{
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(SqliteConnectOptions::new().filename(path).read_only(true))
        .await
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let rows = sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
        .fetch_all(&pool)
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
    pool.close().await;
    rows
}

fn sha256(path: &Path) -> String {
    hex(&Sha256::digest(std::fs::read(path).unwrap()))
}

/// 同じ folder に、`name` で始まる別の file（`-wal`・`-shm`・`-journal`）があるか。
fn siblings(path: &Path) -> Vec<String> {
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    std::fs::read_dir(path.parent().unwrap())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(&name) && *n != name)
        .collect()
}

/// 時刻の列（表・列）。
const TIMES: [(&str, &str); 8] = [
    ("senders", "created_at"),
    ("senders", "resolved_at"),
    ("pages", "created_at"),
    ("pages", "asked_at"),
    ("pages", "answered_at"),
    ("removed", "removed_at"),
    ("files", "created_at"),
    ("files_removed", "removed_at"),
];

// --- 試験 ---

/// 今の DB の写しを移すと、ページも file も送り手も返事とレビューの印も、今の API で読んだ物と同じに新しい API で読め、
/// 本文は同じ byte で、消したページと file は 410 のまま。
#[tokio::test]
async fn a_backup_moves_whole_and_reads_the_same_through_the_new_api() {
    let (old, from) = backup().await;
    let (to, summary) = migrated(&from).await;
    for table in TABLES {
        let t = summary
            .table(table)
            .unwrap_or_else(|| panic!("{table} の突き合わせが無い"));
        assert!(t.ok(), "{t:?}");
    }
    let rows = |table: &str| summary.table(table).unwrap().new_rows;
    assert_eq!(
        TABLES.map(rows),
        [4, 4, 1, 3, 2, 1],
        "表ごとの行の数: {summary}"
    );
    for table in BODIES {
        let b = summary
            .body(table)
            .unwrap_or_else(|| panic!("{table} の本文の突き合わせが無い"));
        assert!(b.ok(), "{b:?}");
    }
    assert_eq!(summary.body("pages").unwrap().new_rows, 4);
    assert_eq!(summary.body("files").unwrap().new_rows, 2);
    let new = open_new(&to).await;
    same_reads(&old, &new).await;

    // 中身の幾つかを、新しい答えの字で直に見る。
    let list = new_json(&new, "/api/pages?sort=createdAt&maxResults=1000").await;
    let plan1 = &list["pages"][0];
    assert_eq!(plan1["path"], "/ai/AI-36/plan-1.html", "{list}");
    assert_eq!(plan1["createdAt"], "2026-01-02T03:04:05Z", "{plan1}");
    assert_eq!(plan1["workspace"], "w:1");
    assert!(plan1["askedAt"].is_string() && plan1["answeredAt"].is_string());
    let plan3 = &list["pages"][1];
    assert_eq!(plan3["path"], "/ai/AI-36/plan-3.html", "{list}");
    assert_eq!(plan3["createdAt"], "2026-01-02T03:04:05.25Z");
    assert_eq!(plan3["waitsOn"], "review");
    assert_eq!(plan3["sender"]["ref"], "claude.session:s-1");
    let review = list["pages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["kind"] == "review")
        .unwrap();
    assert_eq!(review["verdict"], "concerns");
    assert_eq!(
        review["targets"],
        json!([
            "/ai/AI-36/plan-3.html",
            "https://github.com/uraitakahito/pagetray/pull/57",
            "/ai/AI-36/plan-1.html"
        ]),
        "見た物は書いた順"
    );
    let result = list["pages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["kind"] == "result")
        .unwrap();
    assert_eq!(
        result["sender"]["actor"],
        json!({ "id": ACTOR, "name": "se.9", "kind": "ai" })
    );
    assert_eq!(result["sender"]["resolvedAt"], "2026-01-02T03:04:07Z");
    assert_eq!(result["title"], "bytes");
    let (_, _, body) = new.get("/ai/AI-37/result-1.html").await;
    assert_eq!(body, NOT_UTF8);

    // 探す索引は本文から作り直してある（題と本文の字で引ける）。
    for (q, want) in [
        ("索引を作り直す", "/ai/AI-36/plan-1.html"),
        ("送り手の付いた", "/ai/AI-36/plan-3.html"),
        ("bytes", "/ai/AI-37/result-1.html"),
    ] {
        assert_eq!(new.paths(&format!("q={q}")).await, [want], "q={q}");
    }
    assert!(new.paths("q=消す").await.is_empty(), "消したページの字");
    drop(new);
    common::remove_db(&from);
}

/// 残す時刻の字は、どの表のどの列も 24 字の UTC（ミリ秒まで）に揃え、時刻の順と字の順が同じになる。
/// 今の server の、秒の下の桁の違う字（`…:05Z`・`…:05.5Z`）を字で並べると狂う順は、移した後は時刻の順になる。
#[tokio::test]
async fn created_at_and_every_time_are_aligned_to_24_chars() {
    let (old, from) = backup().await;
    let (to, _) = migrated(&from).await;
    for (table, column) in TIMES {
        let got: Vec<String> = scalars(
            &to,
            &format!("select {column} from {table} where {column} is not null order by rowid"),
        )
        .await;
        assert!(!got.is_empty(), "{table}.{column}");
        for t in &got {
            let parsed = OffsetDateTime::parse(t, &Rfc3339).unwrap();
            assert_eq!(*t, pagetray_next::db::stamp(parsed), "{table}.{column}");
        }
    }
    assert_eq!(
        scalars::<String>(
            &to,
            "select created_at from pages where id in (1, 3) order by id"
        )
        .await,
        ["2026-01-02T03:04:05.000Z", "2026-01-02T03:04:05.250Z"]
    );
    assert_eq!(
        scalars::<String>(
            &to,
            "select resolved_at || ' ' || created_at from senders where actor_id is not null"
        )
        .await,
        ["2026-01-02T03:04:07.000Z 2026-01-02T03:04:07.100Z"]
    );
    // 今の server は字の順（plan-3 の `.25Z` が plan-1 の `Z` より前）、新しい server は時刻の順。
    let old_order = paths(
        &old.json("/api/pages?project=ai&key=AI-36&kind=plan&sort=created_at")
            .await,
        "pages",
    );
    assert_eq!(
        old_order,
        ["/ai/AI-36/plan-3.html", "/ai/AI-36/plan-1.html"]
    );
    let new = open_new(&to).await;
    assert_eq!(
        new.paths("project=ai&key=AI-36&kind=plan&sort=createdAt")
            .await,
        ["/ai/AI-36/plan-1.html", "/ai/AI-36/plan-3.html"]
    );
    drop(new);
    common::remove_db(&from);
}

/// id の値（ページ・file・送り手）と送り手との結び・castlist のアクターの id・見た物の順・消した番号は、写しのまま残る。
/// 移した後に置くと、番号は消した番号も飛ばして次になる（古い URL が別のページを指さない）。
#[tokio::test]
async fn ids_senders_and_removed_numbers_stay() {
    let (_old, from) = backup().await;
    let (to, _) = migrated(&from).await;
    for sql in [
        "select id || ' ' || coalesce(sender_id, '-') || ' ' || project || '/' || key || '/' || kind || '-' || n from pages order by id",
        "select id || ' ' || coalesce(sender_id, '-') || ' ' || name || '-' || n || '.' || ext || ' ' || sha256 || ' ' || size from files order by id",
        "select id || ' ' || actor_issuer || ' ' || actor_ref || ' ' || coalesce(actor_id, '-') || ' ' || coalesce(actor_name, '-') || ' ' || coalesce(actor_kind, '-') from senders order by id",
        "select page_id || ' ' || target from page_targets order by rowid",
        "select project || '/' || key || '/' || kind || '-' || n from removed order by 1",
        "select project || '/' || key || '/' || name || '-' || n || '.' || ext from files_removed order by 1",
        "select title from pages order by id",
    ] {
        let old: Vec<String> = scalars(&from, sql).await;
        let new: Vec<String> = scalars(&to, sql).await;
        assert!(!old.is_empty(), "{sql}");
        assert_eq!(new, old, "{sql}");
    }
    let actors: Vec<String> = scalars(
        &to,
        "select actor_id from senders where actor_id is not null",
    )
    .await;
    assert_eq!(actors, [ACTOR]);

    let new = open_new(&to).await;
    let n = new
        .place(
            "ai",
            "AI-36",
            "plan",
            "role=tpm",
            &html("次", "<p>移した後</p>"),
        )
        .await;
    assert_eq!(n, "4", "消した 2 も、置いた 3 も飛ばす");
    let (status, _, answer) = new
        .put("ai", "AI-36", "plan", "role=tpm&n=2", "<p>x</p>")
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "消した番号: {answer}");
    let n = new
        .place_file("ai", "AI-36", "role=tpm&name=dot.png", "image/png", PNG)
        .await;
    assert_eq!(n, "3", "消した 1 も、置いた 2 も飛ばす");
    let broken = new.foreign_key_violations().await;
    assert_eq!(broken, 0);
    drop(new);
    common::remove_db(&from);
}

/// 今の migrations の途中の版（0001〜0005 まで当てた物）の写しも、無い表は空・無い列は空として移し、新しい API で読める。
#[tokio::test]
async fn backups_of_every_earlier_migration_move() {
    for version in 1..=5 {
        let from = backup_at(version).await;
        let (to, summary) = migrated(&from).await;
        let rows = |table: &str| summary.table(table).unwrap().new_rows;
        let want = [
            i64::from(version >= 5),
            2,
            i64::from(version >= 2),
            0,
            i64::from(version >= 3),
            i64::from(version >= 3),
        ];
        assert_eq!(TABLES.map(rows), want, "v{version}: {summary}");
        let new = open_new(&to).await;
        let list = new_json(&new, "/api/pages?sort=createdAt").await;
        assert_eq!(
            paths(&list, "pages"),
            ["/ai/AI-1/plan-1.html", "/ai/AI-1/plan-3.html"],
            "v{version}"
        );
        let first = &list["pages"][0];
        assert_eq!(first["createdAt"], "2026-01-02T03:04:05Z", "v{version}");
        assert_eq!(first["title"], "古い版");
        assert_eq!(
            first.get("askedAt").is_some(),
            version >= 4,
            "v{version}: {first}"
        );
        assert_eq!(
            list["pages"][1].get("sender").is_some(),
            version >= 5,
            "v{version}"
        );
        let (status, _, body) = new.get("/ai/AI-1/plan-1.html").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, "<title>古い版</title><p>途中の版の DB</p>".as_bytes());
        // 索引は、写しに行が無くても本文から作る。
        assert_eq!(
            new.paths("q=途中の版").await,
            ["/ai/AI-1/plan-1.html"],
            "v{version}"
        );
        let gone = new.get("/ai/AI-1/plan-5.html").await.0;
        let next = new
            .place("ai", "AI-1", "plan", "role=tpm", "<p>次</p>")
            .await;
        if version >= 2 {
            assert_eq!((gone, next.as_str()), (StatusCode::GONE, "6"), "v{version}");
        } else {
            assert_eq!(
                (gone, next.as_str()),
                (StatusCode::NOT_FOUND, "4"),
                "v{version}"
            );
        }
        if version >= 3 {
            let (status, _, body) = new.get("/ai/AI-1/files/dot-2.png").await;
            assert_eq!(
                (status, body.as_slice()),
                (StatusCode::OK, PNG),
                "v{version}"
            );
            assert_eq!(
                new.get("/ai/AI-1/files/dot-1.png").await.0,
                StatusCode::GONE
            );
        }
        drop(new);
        common::remove_db(&from);
    }
}

/// 移した後も、外のキーは崩れず、送り手は足すだけの決まり（trigger）が効き、ページを消すと見た物も消える。
#[tokio::test]
async fn after_moving_the_rules_of_the_new_db_hold() {
    let (_old, from) = backup().await;
    let (to, _) = migrated(&from).await;
    let new = open_new(&to).await;
    assert_eq!(new.foreign_key_violations().await, 0);
    for sql in ["update senders set actor_name = 'x'", "delete from senders"] {
        let e = sqlx::query(sqlx::AssertSqlSafe(sql))
            .execute(new.db.write())
            .await
            .expect_err(sql);
        assert!(e.to_string().contains("append-only"), "{sql}: {e}");
    }
    let (status, _, answer) = new.remove("project=ai&key=AI-36&kind=review").await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(new.count("page_targets").await, 0);
    assert_eq!(
        new.count("senders").await,
        4,
        "ページを消しても送り手は残る"
    );
    drop(new);
    common::remove_db(&from);
}

/// 移す元の file には書かない（中身の写しが同じで、隣に -wal・-shm も作らない）。
#[tokio::test]
async fn the_source_file_is_only_read() {
    let (_old, from) = backup().await;
    let before = sha256(&from);
    let (to, _) = migrated(&from).await;
    assert_eq!(sha256(&from), before);
    assert_eq!(siblings(&from), Vec::<String>::new());
    common::remove_db(&from);
    common::remove_db(&to);
}

/// 移す先にページか file が 1 つでもあれば断り、何も足さない。移し終えた先・元と同じ file・今の server の DB も断る。
#[tokio::test]
async fn a_target_that_is_not_empty_is_refused() {
    let (_old, from) = backup().await;
    let new = App::start().await;
    new.place("ai", "AI-1", "plan", "role=tpm", "<p>先に在る</p>")
        .await;
    let to = new.path.clone();
    new.db.close().await;
    let e = migrate::migrate(&from, &to)
        .await
        .expect_err("空でない移す先");
    assert!(matches!(e, MigrateError::Target(_)), "{e}");
    assert_eq!(scalars::<i64>(&to, "select count(*) from pages").await, [1]);
    assert_eq!(scalars::<i64>(&to, "select count(*) from files").await, [0]);
    // 移し終えた先に、もう 1 度移すのも断る。
    let (done, _) = migrated(&from).await;
    let e = migrate::migrate(&from, &done)
        .await
        .expect_err("移し終えた先");
    assert!(matches!(e, MigrateError::Target(_)), "{e}");
    assert_eq!(
        scalars::<i64>(&done, "select count(*) from pages").await,
        [4]
    );
    // 元と同じ file も断る。
    let e = migrate::migrate(&from, &from).await.expect_err("同じ file");
    assert!(matches!(e, MigrateError::Target(_)), "{e}");
    // 今の server の DB（の写し）を移す先にしても、新しい server が開かずに断る。
    let current = temp_db("current-target");
    tokio::fs::copy(&from, &current).await.unwrap();
    let before = sha256(&current);
    let e = migrate::migrate(&from, &current)
        .await
        .expect_err("今の server の DB");
    assert!(
        matches!(e, MigrateError::Open(OpenError::Current(_))),
        "{e}"
    );
    assert_eq!(sha256(&current), before);
    drop(new);
    common::remove_db(&from);
    common::remove_db(&done);
    common::remove_db(&current);
}

/// 移す元が無い・SQLite の file でない・pagetray の DB でない・隣に -wal か -shm がある（大きさは問わない）時は断り、
/// 移す先を作らない。
#[tokio::test]
async fn a_missing_or_garbage_source_is_refused() {
    let dir = temp_db("garbage");
    let missing = dir.with_file_name("missing.db");
    let garbage = dir.with_file_name("garbage.db");
    tokio::fs::write(
        &garbage,
        b"not a sqlite database at all, just bytes".repeat(200),
    )
    .await
    .unwrap();
    let other = dir.with_file_name("other.db");
    let p = pool(&other).await;
    sqlx::query("create table other(x)")
        .execute(&p)
        .await
        .unwrap();
    p.close().await;
    let (_old, from) = backup().await;
    let mut sources = vec![missing, garbage, other];
    for (name, suffix, bytes) in [
        ("with-wal.db", "-wal", &b"x"[..]),
        ("empty-wal.db", "-wal", b""),
        ("with-shm.db", "-shm", b""),
    ] {
        let path = dir.with_file_name(name);
        tokio::fs::copy(&from, &path).await.unwrap();
        tokio::fs::write(dir.with_file_name(format!("{name}{suffix}")), bytes)
            .await
            .unwrap();
        sources.push(path);
    }
    for source in &sources {
        let to = dir.with_file_name("new.db");
        let e = migrate::migrate(source, &to)
            .await
            .expect_err(&source.display().to_string());
        assert!(
            matches!(e, MigrateError::Source(_)),
            "{}: {e}",
            source.display()
        );
        assert!(!to.exists(), "{}: 移す先を作った", source.display());
    }
    common::remove_db(&dir);
    common::remove_db(&from);
}

/// 新しい API の形（URL の段・種類と役割の名・workspace・file の語と拡張子・番号・送り手の付随する id と castlist の
/// アクター・見た物）に合わない行があれば、表と行を出して何も足さずに戻す（新しい API では読めない・URL にならない行のため）。
#[tokio::test]
async fn rows_that_do_not_fit_the_new_api_are_refused() {
    let old = Old::start().await;
    seed(&old).await;
    for sql in [
        "insert into senders(id, actor_issuer, actor_ref, actor_id, actor_name, actor_kind, resolved_at, created_at)
         values(90, 'tpm', 'claude.session:s-90', 'ABC-1', 'se.90', 'ai', '2026-01-02T03:04:05Z', '2026-01-02T03:04:05Z')",
        "insert into senders(id, actor_issuer, actor_ref, created_at) values(91, 'TPM', 'claude.session:s-91', '2026-01-02T03:04:05Z')",
        "insert into pages(id, project, key, kind, n, title, role, workspace, created_at, body)
         values(90, 'api', 'K', 'plan', 1, '', 'tpm', '', '2026-01-02T03:04:05Z', x'00')",
        "insert into pages(id, project, key, kind, n, title, role, workspace, created_at, body)
         values(91, 'ai', 'K', 'Plan', 1, '', 'tpm', 'w 1', '2026-01-02T03:04:05Z', x'00')",
        "insert into page_targets(page_id, target) values(1, 'http://example.com/x')",
        "insert into removed values('ai', 'K', 'plan', 0, '2026-01-02T03:04:05Z')",
        "insert into files(id, project, key, name, ext, n, type, size, sha256, role, workspace, created_at, body)
         values(90, 'ai', 'K', 'run', 'exe', 1, 'application/octet-stream', 1, 'x', 'tpm', '', '2026-01-02T03:04:05Z', x'00')",
        "insert into files_removed values('ai', '..', 'dot', 'png', 1, '2026-01-02T03:04:05Z')",
    ] {
        old.execute(sql).await;
    }
    let from = temp_db("backup-bad-rows");
    old.backup(&from).await;
    let to = temp_db("new-bad-rows");
    let e = migrate::migrate(&from, &to)
        .await
        .expect_err("形の合わない行");
    let MigrateError::Mismatch(summary) = &e else {
        panic!("突き合わせの NG でない: {e}");
    };
    assert_eq!(
        summary.bad_rows,
        [
            "senders 90: actor_id",
            "senders 91: actor_issuer",
            "pages 90: project",
            "pages 91: kind・workspace",
            "removed /ai/K/plan-0.html: n",
            "page_targets 1: http://example.com/x",
            "files 90: ext",
            "files_removed /ai/../files/dot-1.png: key",
        ],
        "{summary}"
    );
    assert!(summary.tables.iter().all(|t| t.ok()), "{summary}");
    assert!(summary.bad_times.is_empty(), "{summary}");
    assert!(e.to_string().contains("pages 90: project"), "{e}");
    // 何も足していない。
    for table in TABLES {
        assert_eq!(
            scalars::<i64>(&to, &format!("select count(*) from {table}")).await,
            [0],
            "{table}"
        );
    }
    common::remove_db(&from);
    common::remove_db(&to);
}

/// 24 字の UTC に揃えられない時刻の字（UTC の RFC 3339 の形でない・時差の付いた・無い日・ミリ秒より細かい）があれば、
/// 表と列と行と字を出して何も足さずに戻す（字を推して直すと、置いた時刻が替わる）。
#[tokio::test]
async fn times_that_cannot_be_aligned_are_refused() {
    let old = Old::start().await;
    seed(&old).await;
    for (id, created_at) in [
        (90, "2026-11-24 00:00:00"),
        (91, "2026-11-24T00:00:00+09:00"),
        (92, "2026-02-30T00:00:00Z"),
        (93, "2026-11-24T00:00:00.1234Z"),
    ] {
        old.execute(&format!(
            "insert into pages(id, project, key, kind, n, title, role, workspace, created_at, body)
             values({id}, 'ai', 'K', 'plan', {id}, '', 'tpm', '', '{created_at}', x'00')"
        ))
        .await;
    }
    old.execute("insert into removed values('ai', 'K', 'plan', 1, 'yesterday')")
        .await;
    let from = temp_db("backup-bad-times");
    old.backup(&from).await;
    let to = temp_db("new-bad-times");
    let e = migrate::migrate(&from, &to)
        .await
        .expect_err("揃えられない時刻");
    let MigrateError::Mismatch(summary) = &e else {
        panic!("突き合わせの NG でない: {e}");
    };
    assert_eq!(
        summary.bad_times,
        [
            "pages.created_at 90: 2026-11-24 00:00:00",
            "pages.created_at 91: 2026-11-24T00:00:00+09:00",
            "pages.created_at 92: 2026-02-30T00:00:00Z",
            "pages.created_at 93: 2026-11-24T00:00:00.1234Z",
            "removed.removed_at /ai/K/plan-1.html: yesterday",
        ],
        "{summary}"
    );
    assert!(summary.bad_rows.is_empty(), "{summary}");
    assert!(summary.tables.iter().all(|t| t.ok()), "{summary}");
    assert_eq!(scalars::<i64>(&to, "select count(*) from pages").await, [0]);
    common::remove_db(&from);
    common::remove_db(&to);
}

/// 突き合わせは、表の中身と本文の違いを見つける（別の写しと比べると、違う表だけが NG）。
#[tokio::test]
async fn verify_finds_a_table_that_differs() {
    let (old, from) = backup().await;
    let (to, _) = migrated(&from).await;
    old.put_page("project=ai&key=AI-36&kind=plan&role=tpm", "<p>後から</p>")
        .await;
    let other = temp_db("backup-other");
    old.backup(&other).await;
    let summary = migrate::verify(&other, &to).await.expect("突き合わせる");
    assert!(!summary.ok(), "{summary}");
    let pages = summary.table("pages").unwrap();
    assert_eq!((pages.old_rows, pages.new_rows), (5, 4));
    assert!(!pages.ok());
    assert!(!summary.body("pages").unwrap().ok());
    for table in [
        "senders",
        "removed",
        "page_targets",
        "files",
        "files_removed",
    ] {
        assert!(summary.table(table).unwrap().ok(), "{table}");
    }
    assert!(summary.body("files").unwrap().ok());
    assert!(migrate::verify(&from, &to).await.unwrap().ok());
    common::remove_db(&from);
    common::remove_db(&to);
    common::remove_db(&other);
}

/// binary の `pagetray-next migrate --from … --to …`: 移して表ごとと本文の突き合わせを出し、0 で終わる。
/// 移し終えた先へもう 1 度移せば、訳を出して 1 で終わる。引数の誤りは 2。どれも記録の置き場を作らない。
#[tokio::test]
async fn the_binary_migrates_and_prints_the_checks() {
    let (_old, from) = backup().await;
    let to = temp_db("new-bin");
    let run = |args: &[&Path]| {
        Command::new(common::server::binary())
            .arg("migrate")
            .arg("--from")
            .arg(args[0])
            .arg("--to")
            .arg(args[1])
            .output()
            .expect("binary を起こす")
    };
    let out = run(&[&from, &to]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stdout}\n{stderr}");
    for table in TABLES {
        let line = stdout
            .lines()
            .find(|l| l.starts_with(&format!("{table} ")))
            .unwrap_or_else(|| panic!("{table} の行が無い: {stdout}"));
        assert!(line.contains(" digest ") && line.ends_with(" ok"), "{line}");
    }
    for table in BODIES {
        let line = stdout
            .lines()
            .find(|l| l.starts_with(&format!("body {table} ")))
            .unwrap_or_else(|| panic!("{table} の本文の行が無い: {stdout}"));
        assert!(line.contains(" sha256 ") && line.ends_with(" ok"), "{line}");
    }
    assert!(stdout.contains("pages 4 → 4 digest "), "{stdout}");
    assert!(stdout.contains("body files 2 → 2 sha256 "), "{stdout}");
    assert!(
        !to.with_file_name("logs").exists(),
        "移すだけで記録の置き場を作った"
    );

    let out = run(&[&from, &to]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("移す先"), "{stderr}");

    for args in [
        &["migrate", "--from"][..],
        &["migrate", "--from", "a", "--to", "b", "--force"],
        &["migrate", "--from", "a", "--from", "b", "--to", "c"],
        &["serve"],
    ] {
        let out = Command::new(common::server::binary())
            .args(args)
            .env_remove("PAGETRAY_DB")
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "{args:?}");
    }
    assert!(!Path::new("b").exists() && !Path::new("c").exists());

    let new = open_new(&to).await;
    assert_eq!(new.count("pages").await, 4);
    drop(new);
    common::remove_db(&from);
}

/// 新しい server が今の server の DB を断る訳は、移す命令を指す。
#[tokio::test]
async fn refusing_the_current_db_points_to_migrate() {
    let (_old, from) = backup().await;
    let e = match Db::open(&from).await {
        Ok(_) => panic!("今の server の DB を開いた"),
        Err(e) => e,
    };
    assert!(e.to_string().contains("pagetray-next migrate"), "{e}");
    common::remove_db(&from);
}
