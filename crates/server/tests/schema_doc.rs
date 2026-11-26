//! 本の章「DB の作り」（docs/src/db.md）が、本物の DB の作りと合っているか。
//!
//! 一時の DB に `crates/server/migrations/` を当て、表・列・型・空を許すか・主キーと一意と索引を SQLite から読み、章と比べる。
//! 章の、表ごとの節の形は次のとおり（ここが読む所。意味の文は読まない）:
//!
//! ```text
//! ## `<表の名>` — <説明>
//! | `<列>` | <型（無ければ —）> | <不可 か 可> | <意味> |
//! - 主キー: `(<列>, …)`
//! - 一意: `(<列>, …)`
//! - 索引: `<索引の名>` `(<列>, …)`
//! ```
//!
//! 図（```` ```mermaid ```` の塊）の結びの線に出る表の名と、用語集（docs/src/glossary.md）に表の名があるかも見る。

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::str::FromStr;

/// 表 1 つの作り。本物から読んだ物と、章から読んだ物を、同じ形にして比べる。
#[derive(Debug, Default, PartialEq)]
struct Table {
    /// 列の名・型・空を許すか（`不可`・`可`）。列の順。
    columns: Vec<(String, String, String)>,
    /// 主キーの列（無ければ空）。
    primary_key: Vec<String>,
    /// 一意の列の組。
    unique: BTreeSet<Vec<String>>,
    /// 索引の名と列。
    indexes: BTreeSet<(String, Vec<String>)>,
}

/// repo の頭（この crate は crates/server）からの path の file。
fn read(path: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

async fn migrated() -> SqlitePool {
    // 接続ごとに別の DB になるので、接続は 1 本だけ。
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(SqliteConnectOptions::from_str("sqlite::memory:").unwrap())
        .await
        .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    pool
}

async fn index_columns(db: &SqlitePool, index: &str) -> Vec<String> {
    sqlx::query("select name from pragma_index_info(?1) order by seqno")
        .bind(index)
        .fetch_all(db)
        .await
        .unwrap()
        .iter()
        .map(|r| r.get("name"))
        .collect()
}

/// 本物の表。章に書く物（`tables`）と、名だけを書く物（sqlx の表・仮想の表の影の表。`others`）に分ける。
async fn actual(db: &SqlitePool) -> (BTreeMap<String, Table>, Vec<String>) {
    let rows = sqlx::query(
        "select name, sql from sqlite_schema where type = 'table' and name not like 'sqlite!_%' escape '!' order by name",
    )
    .fetch_all(db)
    .await
    .unwrap();
    let all: Vec<(String, String)> = rows.iter().map(|r| (r.get("name"), r.get("sql"))).collect();
    let virtuals: Vec<&str> = all
        .iter()
        .filter(|(_, sql)| sql.to_lowercase().starts_with("create virtual table"))
        .map(|(name, _)| name.as_str())
        .collect();
    let mut tables = BTreeMap::new();
    let mut others = Vec::new();
    for (name, _) in &all {
        let shadow = virtuals.iter().any(|v| name.starts_with(&format!("{v}_")));
        if shadow || name.starts_with("_sqlx_") {
            others.push(name.clone());
            continue;
        }
        let mut table = Table::default();
        let mut pk = Vec::new();
        for r in sqlx::query(
            r#"select name, type, "notnull", pk from pragma_table_info(?1) order by cid"#,
        )
        .bind(name)
        .fetch_all(db)
        .await
        .unwrap()
        {
            let column: String = r.get("name");
            let ty: String = r.get("type");
            let not_null: i64 = r.get("notnull");
            let pk_seq: i64 = r.get("pk");
            // strict の表では、主キーの列は空にならない。
            let empty = if not_null == 1 || pk_seq > 0 {
                "不可"
            } else {
                "可"
            };
            let ty = if ty.is_empty() {
                "—".to_owned()
            } else {
                ty.to_lowercase()
            };
            table.columns.push((column.clone(), ty, empty.to_owned()));
            if pk_seq > 0 {
                pk.push((pk_seq, column));
            }
        }
        pk.sort();
        table.primary_key = pk.into_iter().map(|(_, c)| c).collect();
        for r in sqlx::query("select name, origin from pragma_index_list(?1)")
            .bind(name)
            .fetch_all(db)
            .await
            .unwrap()
        {
            let index: String = r.get("name");
            let origin: String = r.get("origin");
            let columns = index_columns(db, &index).await;
            match origin.as_str() {
                "u" => {
                    table.unique.insert(columns);
                }
                "c" => {
                    table.indexes.insert((index, columns));
                }
                _ => {} // pk: 主キーの列は pragma_table_info から取った
            }
        }
        tables.insert(name.clone(), table);
    }
    (tables, others)
}

/// `` `(a, b)` `` → `[a, b]`。
fn column_list(s: &str) -> Option<Vec<String>> {
    let inner = s.trim().strip_prefix("`(")?.strip_suffix(")`")?;
    Some(inner.split(',').map(|c| c.trim().to_owned()).collect())
}

/// 章の、表ごとの節。
fn documented(chapter: &str, errors: &mut Vec<String>) -> BTreeMap<String, Table> {
    let mut tables = BTreeMap::new();
    let mut current: Option<String> = None;
    for line in chapter.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            current = heading
                .strip_prefix('`')
                .and_then(|h| h.split_once('`'))
                .map(|(name, _)| name.to_owned());
            if let Some(name) = &current
                && tables.insert(name.clone(), Table::default()).is_some()
            {
                errors.push(format!("章に表 {name} の節が 2 つある"));
            }
            continue;
        }
        let Some(table) = current.as_ref().and_then(|n| tables.get_mut(n)) else {
            continue;
        };
        let name = current.as_deref().unwrap();
        if let Some(row) = line.strip_prefix("| `") {
            let cells: Vec<&str> = row.split('|').map(str::trim).collect();
            let column = cells[0].trim_end_matches('`').to_owned();
            match (cells.get(1), cells.get(2)) {
                (Some(ty), Some(empty)) => {
                    table
                        .columns
                        .push((column, ty.to_lowercase(), (*empty).to_owned()))
                }
                _ => errors.push(format!(
                    "章の {name} の列 {column} の行に、型か空の欄が無い"
                )),
            }
        } else if let Some(rest) = line.strip_prefix("- 主キー: ") {
            match column_list(rest) {
                Some(c) => table.primary_key = c,
                None => errors.push(format!("章の {name} の「主キー:」の行が読めない: {line}")),
            }
        } else if let Some(rest) = line.strip_prefix("- 一意: ") {
            match column_list(rest) {
                Some(c) => {
                    table.unique.insert(c);
                }
                None => errors.push(format!("章の {name} の「一意:」の行が読めない: {line}")),
            }
        } else if let Some(rest) = line.strip_prefix("- 索引: ") {
            let parsed = rest
                .strip_prefix('`')
                .and_then(|r| r.split_once("` "))
                .and_then(|(index, cols)| Some((index.to_owned(), column_list(cols)?)));
            match parsed {
                Some(i) => {
                    table.indexes.insert(i);
                }
                None => errors.push(format!("章の {name} の「索引:」の行が読めない: {line}")),
            }
        }
    }
    tables
}

/// 図の結びの線（`a ||--o{ b : "…"`）に出る表の名。
fn diagram_tables(chapter: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut inside = false;
    for line in chapter.lines() {
        let line = line.trim();
        if line.starts_with("```") {
            inside = line == "```mermaid";
            continue;
        }
        if !inside {
            continue;
        }
        let Some((relation, _)) = line.split_once(" : ") else {
            continue;
        };
        let words: Vec<&str> = relation.split_whitespace().collect();
        if let [left, _, right] = words[..] {
            names.insert(left.to_owned());
            names.insert(right.to_owned());
        }
    }
    names
}

fn compare(name: &str, want: &Table, doc: &Table, errors: &mut Vec<String>) {
    let want_cols: Vec<&str> = want.columns.iter().map(|c| c.0.as_str()).collect();
    let doc_cols: Vec<&str> = doc.columns.iter().map(|c| c.0.as_str()).collect();
    for c in want_cols.iter().filter(|c| !doc_cols.contains(c)) {
        errors.push(format!("{name} の列 {c} が章に無い"));
    }
    for c in doc_cols.iter().filter(|c| !want_cols.contains(c)) {
        errors.push(format!("章の {name} の列 {c} は、本物の DB に無い"));
    }
    if want_cols.iter().all(|c| doc_cols.contains(c))
        && want_cols.len() == doc_cols.len()
        && want_cols != doc_cols
    {
        errors.push(format!(
            "{name} の列の並びが違う: 本物は {want_cols:?}、章は {doc_cols:?}"
        ));
    }
    for (w, d) in want
        .columns
        .iter()
        .zip(&doc.columns)
        .filter(|(w, d)| w.0 == d.0)
    {
        if w.1 != d.1 {
            errors.push(format!(
                "{name} の列 {} の型が違う: 本物は {}、章は {}",
                w.0, w.1, d.1
            ));
        }
        if w.2 != d.2 {
            errors.push(format!(
                "{name} の列 {} の空が違う: 本物は {}、章は {}",
                w.0, w.2, d.2
            ));
        }
    }
    if want.primary_key != doc.primary_key {
        errors.push(format!(
            "{name} の主キーが違う: 本物は {:?}、章は {:?}",
            want.primary_key, doc.primary_key
        ));
    }
    if want.unique != doc.unique {
        errors.push(format!(
            "{name} の一意が違う: 本物は {:?}、章は {:?}",
            want.unique, doc.unique
        ));
    }
    if want.indexes != doc.indexes {
        errors.push(format!(
            "{name} の索引が違う: 本物は {:?}、章は {:?}",
            want.indexes, doc.indexes
        ));
    }
}

#[tokio::test]
async fn the_db_chapter_matches_the_migrated_schema() {
    let chapter = read("docs/src/db.md");
    let glossary = read("docs/src/glossary.md");
    let db = migrated().await;
    let (want, others) = actual(&db).await;
    let mut errors = Vec::new();
    let doc = documented(&chapter, &mut errors);

    for (name, table) in &want {
        match doc.get(name) {
            Some(d) => compare(name, table, d, &mut errors),
            None => errors.push(format!(
                "表 {name} の節が章に無い（見出し ## `{name}` — …）"
            )),
        }
        if !glossary.contains(&format!("`{name}`")) {
            errors.push(format!("表 {name} が用語集の code の名に無い"));
        }
    }
    for name in doc.keys().filter(|n| !want.contains_key(*n)) {
        errors.push(format!("章の表 {name} は、本物の DB に無い"));
    }
    for name in &others {
        if !chapter.contains(&format!("`{name}`")) {
            errors.push(format!(
                "表 {name} の名が章に無い（「ほかの表」か、仮想の表の節に書く）"
            ));
        }
    }
    let drawn = diagram_tables(&chapter);
    let names: BTreeSet<String> = want.keys().cloned().collect();
    if drawn != names {
        errors.push(format!("図の表が違う: 本物は {names:?}、図は {drawn:?}"));
    }

    assert!(
        errors.is_empty(),
        "docs/src/db.md が本物の DB と合っていない（表を変えたら章も直す）:\n- {}",
        errors.join("\n- ")
    );
}
