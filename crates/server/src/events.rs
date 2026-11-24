//! 新着の知らせ（SSE。`GET /api/events`）に送る所。SSE は次の PR で足す（docs/src/next.md の「段 4 の分け方」）。
//! 今は何も送らない。SSE の PR で、ここを broadcast の channel に送る形にする（今の server の src/events.rs と同じく、
//! 聞いている者が無くても失敗にしない）。呼ぶ所（返事と印を書いた後・file を置いた後。消した時は送らない）は、もう置いてある。

use crate::db::{FileRow, PageRow};

/// 知らせの種類。中身は、書いた後の行（送る形は SSE の PR で決める）。
pub enum Note<'a> {
    /// 返事の状態を書いた（替わらなかった時も）。今の server の `event: reply`。
    Reply(&'a PageRow),
    /// レビューの印を書いた（替わらなかった時も）。今の server の `event: review`。
    Review(&'a PageRow),
    /// file を置いた。今の server の `event: file`。
    File(&'a FileRow),
}

/// 知らせを送る（今は記録に 1 行書くだけ）。
pub fn send(note: Note<'_>) {
    let (event, path) = match note {
        Note::Reply(row) => ("reply", page_path(row)),
        Note::Review(row) => ("review", page_path(row)),
        Note::File(row) => (
            "file",
            crate::files::file_path(&row.project, &row.key, &row.name, &row.ext, row.n),
        ),
    };
    tracing::debug!(
        event,
        path = %path,
        "新着の知らせ（SSE はまだ無いので送らない）"
    );
}

fn page_path(row: &PageRow) -> String {
    crate::pages::page_path(&row.project, &row.key, &row.kind, row.n)
}
