//! 新着の push（SSE）。ページか file が置かれるたび・ページの返事の状態が変わるたびに、その記録を 1 件送る。

use crate::AppState;
use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use std::convert::Infallible;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::{Stream, StreamExt};

/// 置かれた物の知らせ（中身は記録の JSON）。
#[derive(Clone, Debug)]
pub enum Note {
    /// 名の無い event で送る（前からの受け手は、これだけを読む）。
    Page(String),
    /// `event: file` で送る。
    File(String),
    /// ページの返事の状態が変わった。`event: reply` で送る。
    Reply(String),
}

pub async fn stream(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    // 聞く側が遅れて取りこぼした分は捨てる（一覧を読み直せば出る）。
    let stream = BroadcastStream::new(state.events.subscribe())
        .filter_map(|message| message.ok())
        .map(|note| {
            Ok(match note {
                Note::Page(json) => Event::default().data(json),
                Note::File(json) => Event::default().event("file").data(json),
                Note::Reply(json) => Event::default().event("reply").data(json),
            })
        });
    Sse::new(stream).keep_alive(KeepAlive::default())
}
