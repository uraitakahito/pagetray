//! 新着の知らせ（SSE。`GET /api/events`。手の道）。ページか file を置くたび・返事の状態か印を書くたび（替わらなかった時も）に、
//! その記録を 1 件送る。消した時は送らない（0.8.x の server と同じ）。
//!
//! - 中身は、API の答えと同じ camelCase の記録（ページは `ai.pagetray#Page`、file は `ai.pagetray#File`。`url` は入らない）。
//!   0.8.x の server は snake_case の記録だった（画面の script が読むのは `path`・`project`・`key` だけで、名は同じ）。
//! - event の名: ページは名の無い event（前からの受け手は、これだけを読む）、file は `file`、返事は `reply`、印は `review`。
//! - 聞いている者が無くても失敗にしない。遅れて取りこぼした聞き手は、捨てた分を飛ばして続ける（[`CAPACITY`] 件を覚える。
//!   画面は、一覧を読み直せば出る）。keep-alive は axum の既定（15 秒ごとの注釈の行）。
//! - 止める合図を受けたら（[`Events::new`] の印）、どの流れも閉じる。閉じないと、止める時に走っている物を終えるのを待つ所
//!   （src/main.rs）が、聞き手が切るまで待ち続ける。
use std::convert::Infallible;

use aws_smithy_types::DateTime;
use aws_smithy_types::date_time::Format;
use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use pagetray_sdk::model::{File, Page, Sender};
use serde_json::{Map, Value, json};
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::{Stream, StreamExt};
use tokio_util::sync::CancellationToken;

/// 聞き手ごとに覚えておく知らせの数。
pub const CAPACITY: usize = 64;

/// 知らせの種類と、書いた後の記録。
pub enum Note {
    /// ページを置いた。名の無い event。
    Page(Page),
    /// 返事の状態を書いた（替わらなかった時も）。`event: reply`。
    Reply(Page),
    /// レビューの印を書いた（替わらなかった時も）。`event: review`。
    Review(Page),
    /// file を置いた。`event: file`。
    File(File),
}

/// 送る 1 件（event の名と、data の JSON の字）。
#[derive(Clone, Debug)]
struct Message {
    name: Option<&'static str>,
    data: String,
}

/// 知らせを送る所（どの handler も同じ物を使う。複製しても同じ channel）。
#[derive(Clone)]
pub struct Events {
    sender: broadcast::Sender<Message>,
    /// 止める合図を受けた印。付いたら、どの流れも閉じる。
    closing: CancellationToken,
}

impl Events {
    /// `closing` が付いたら、どの流れも閉じる。
    pub fn new(closing: CancellationToken) -> Self {
        Self {
            sender: broadcast::channel(CAPACITY).0,
            closing,
        }
    }

    /// 知らせを送る。聞き手が無くても失敗にしない。記録を JSON にできなければ（時刻が答えに書けない）、記録に書いて送らない。
    pub fn send(&self, note: Note) {
        let (name, path, data) = match &note {
            Note::Page(page) => (None, &page.path, page_json(page)),
            Note::Reply(page) => (Some("reply"), &page.path, page_json(page)),
            Note::Review(page) => (Some("review"), &page.path, page_json(page)),
            Note::File(file) => (Some("file"), &file.path, file_json(file)),
        };
        match data {
            Ok(data) => {
                let listeners = self
                    .sender
                    .send(Message {
                        name,
                        data: data.to_string(),
                    })
                    .unwrap_or(0);
                tracing::debug!(event = name.unwrap_or("page"), path = %path, listeners, "新着の知らせ");
            }
            Err(e) => tracing::warn!(path = %path, "新着の知らせを送れない: {e}"),
        }
    }
}

/// `GET /api/events`。聞き始めた後の知らせを、1 件ずつ流す。
pub async fn stream(
    State(events): State<Events>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    // 遅れて取りこぼした分（Lagged）は捨てて続ける。止める合図を受けたら閉じる。
    let stream = BroadcastStream::new(events.sender.subscribe())
        .filter_map(Result::ok)
        .map(|message| {
            let event = Event::default().data(message.data);
            Ok(match message.name {
                Some(name) => event.event(name),
                None => event,
            })
        });
    // tokio-stream の StreamExt には take_until が無いので、futures-util の物を名指して呼ぶ（名が重なるため）。
    let stream = futures_util::StreamExt::take_until(stream, events.closing.cancelled_owned());
    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// 答えの時刻の字（生成した層の答えと同じ `date-time` の形）。
fn time(t: &DateTime) -> Result<String, String> {
    t.fmt(Format::DateTime)
        .map_err(|e| format!("時刻を書けない: {e}"))
}

/// いつもある欄。
fn fields<const N: usize>(pairs: [(&str, Value); N]) -> Map<String, Value> {
    pairs
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect()
}

/// 値のある時だけ、欄を足す。
fn put(object: &mut Map<String, Value>, name: &str, value: Option<Value>) {
    if let Some(value) = value {
        object.insert(name.to_owned(), value);
    }
}

/// 送り手の欄（生成した層の `Sender` の答えと同じ形）。
fn sender_json(sender: &Sender) -> Result<Value, String> {
    let mut object = fields([
        ("issuer", json!(sender.issuer)),
        ("ref", json!(sender.r#ref)),
    ]);
    put(
        &mut object,
        "actor",
        sender
            .actor
            .as_ref()
            .map(|a| json!({ "id": a.id, "name": a.name, "kind": a.kind })),
    );
    put(
        &mut object,
        "resolvedAt",
        sender
            .resolved_at
            .as_ref()
            .map(time)
            .transpose()?
            .map(Value::from),
    );
    Ok(Value::Object(object))
}

/// ページの記録の JSON（生成した層の `Page` の答えと同じ欄と字。ListPages の行と同じ）。
fn page_json(page: &Page) -> Result<Value, String> {
    let mut object = fields([
        ("project", json!(page.project)),
        ("key", json!(page.key)),
        ("kind", json!(page.kind)),
        ("n", json!(page.n)),
        ("title", json!(page.title)),
        ("role", json!(page.role)),
        ("workspace", json!(page.workspace)),
        ("createdAt", json!(time(&page.created_at)?)),
        ("size", json!(page.size)),
        ("path", json!(page.path)),
    ]);
    put(
        &mut object,
        "sender",
        page.sender.as_ref().map(sender_json).transpose()?,
    );
    put(
        &mut object,
        "askedAt",
        page.asked_at
            .as_ref()
            .map(time)
            .transpose()?
            .map(Value::from),
    );
    put(
        &mut object,
        "answeredAt",
        page.answered_at
            .as_ref()
            .map(time)
            .transpose()?
            .map(Value::from),
    );
    put(
        &mut object,
        "waitsOn",
        page.waits_on.clone().map(Value::from),
    );
    put(
        &mut object,
        "verdict",
        page.verdict.clone().map(Value::from),
    );
    put(
        &mut object,
        "targets",
        page.targets.clone().map(Value::from),
    );
    Ok(Value::Object(object))
}

/// file の記録の JSON（生成した層の `File` の答えと同じ欄と字。ListFiles の行と同じ）。
fn file_json(file: &File) -> Result<Value, String> {
    let mut object = fields([
        ("project", json!(file.project)),
        ("key", json!(file.key)),
        ("name", json!(file.name)),
        ("ext", json!(file.ext)),
        ("n", json!(file.n)),
        ("type", json!(file.r#type)),
        ("size", json!(file.size)),
        ("sha256", json!(file.sha256)),
        ("role", json!(file.role)),
        ("workspace", json!(file.workspace)),
        ("createdAt", json!(time(&file.created_at)?)),
        ("path", json!(file.path)),
    ]);
    put(
        &mut object,
        "sender",
        file.sender.as_ref().map(sender_json).transpose()?,
    );
    Ok(Value::Object(object))
}
