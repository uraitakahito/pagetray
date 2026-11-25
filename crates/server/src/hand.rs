//! 手の道（Smithy の定義の外）のうち、DB の写しと記録の物。今の server の `GET /api/backup`・`GET /api/logs` と同じ振る舞い
//! （castlist の新しい server の crates/server/src/hand.rs と同じ作り）。
//! - `GET /api/backup`: 動かしたまま、DB の揃ったバックアップを 1 つの SQLite の file で返す
//!   （`curl -fsS -o pages.db http://127.0.0.1:8801/api/backup`）。
//! - `GET /api/logs`: 残した記録（終わり方の印と、時刻つきの行）を、古い順の字で返す。記録の置き場が無ければ 404 `NotFound`
//!   （今の server と同じく 404）。
//!
//! 誤りは、生成した層と同じ restJson1 の形（`Unavailable`・`InternalError`・`NotFound`。[`crate::errors::reply`]）。
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use tokio_util::io::ReaderStream;

use crate::db::{BackupError, Db};
use crate::errors::{Failure, reply};
use crate::runlog::RunLog;

/// backup と logs の道。
pub fn routes(db: Arc<Db>, runlog: Option<RunLog>) -> Router {
    Router::new()
        .route("/api/backup", get(backup))
        .with_state(db)
        .route("/api/logs", get(move || logs(runlog.clone())))
}

/// バックアップを作って開いた file を送る（作り方は [`Db::backup`]）。守りの層の時間切れは送り始めるまで（写しを作る間）を
/// 数えるので、作るのに 5 秒を過ぎれば 503 になる（一時の file は残らない）。送る間は数えないので、大きな DB でも切らない。
async fn backup(State(db): State<Arc<Db>>) -> Response {
    let file = match db.backup().await {
        Ok(file) => Ok(tokio::fs::File::from_std(file)),
        Err(BackupError::Db(e)) => return Failure::db("backup", e).reply(),
        Err(BackupError::Io(e)) => Err(e),
    };
    match file {
        Ok(file) => (
            [
                (header::CONTENT_TYPE, "application/vnd.sqlite3"),
                (
                    header::CONTENT_DISPOSITION,
                    "attachment; filename=\"pages.db\"",
                ),
            ],
            Body::from_stream(ReaderStream::new(file)),
        )
            .into_response(),
        Err(e) => Failure::data("backup", &e).reply(),
    }
}

/// 印と記録を読む（塞ぐ読み方なので、tokio の糸の外で）。多くて 4 MiB と 64 KiB。
async fn logs(runlog: Option<RunLog>) -> Response {
    let Some(runlog) = runlog else {
        return reply(StatusCode::NOT_FOUND, "NotFound", "記録の置き場が無い");
    };
    match tokio::task::spawn_blocking(move || runlog.read_all()).await {
        Ok(text) => ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], text).into_response(),
        Err(e) => Failure::data("logs", &e).reply(),
    }
}
