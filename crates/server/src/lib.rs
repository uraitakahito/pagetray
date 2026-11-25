//! 新しい pagetray の server（AI-36）。
//!
//! 定義（model/）から作った骨（crates/pagetray-sdk/）に処理（handler）を入れ、守りの層（[`guard`]）を前に掛ける。
//! 手の道（Smithy の定義の外）のページの本文（[`pages::show`]）と file の中身（[`files::show`]）と新着の知らせ
//! （[`events::stream`]）と画面（[`ui`]）と backup・logs（[`hand`]）は、同じ Router に並べる。
//! 起こす binary（src/main.rs）は、今の server と同じく記録の file（[`runlog`]）を DB の隣の `logs/` に残す。
//! 操作: 置く（[`pages`]）・探す（[`list`]。SQL は [`search`]）・返事とレビューの印（[`marks`]）・消す（[`remove`]）・
//! file を置く・探す・消す（[`files`]）。
//! 形の決まりは docs/src/next.md。
pub mod castlist;
pub mod db;
mod errors;
mod events;
mod facets;
mod files;
mod generated;
mod guard;
mod hand;
mod list;
mod marks;
mod pages;
mod record;
mod remove;
pub mod runlog;
pub mod search;
mod text;
mod token;
mod ui;

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::middleware;
use axum::routing::get;
use pagetray_sdk::error::GetHealthError;
use pagetray_sdk::output::GetHealthOutput;
use pagetray_sdk::{Pagetray, PagetrayConfig};
use tokio_util::sync::CancellationToken;
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::Level;

use crate::db::Db;
use crate::events::Events;
use crate::generated::Generated;

/// 1 つの問い合わせにかけてよい長さ。過ぎれば 503 `Unavailable` で返す（castlist と同じ）。
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

/// 本文の上限（ページも file も。今の server と同じ）。超えれば守りの層が 413 `PayloadTooLarge`（[`guard::limit`]）。
pub const MAX_BODY: usize = 16 * 1024 * 1024;

/// 守りの層の設定。
#[derive(Clone)]
pub struct Config {
    /// 1 つの問い合わせにかけてよい長さ（既定は [`REQUEST_TIMEOUT`]。試験は短くする）。
    pub timeout: Duration,
    /// 送り手を聞く castlist（[`castlist::Castlist::from_env`]）。無ければ聞かずに、付随する id だけを残す。
    pub castlist: Option<castlist::Castlist>,
    /// 記録の置き場。あれば、残した記録を読む道 `GET /api/logs` が答える（無ければ 404 `NotFound`）。
    pub runlog: Option<runlog::RunLog>,
    /// 止める合図を受けた印（binary が付ける）。付いたら、新着の知らせの流れを閉じる。
    pub closing: CancellationToken,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            timeout: REQUEST_TIMEOUT,
            castlist: None,
            runlog: None,
            closing: CancellationToken::new(),
        }
    }
}

/// 動かす Router。
pub fn app(db: Arc<Db>) -> Router {
    app_with(db, Config::default())
}

/// 動かす Router。DB は、どの handler も同じ物を使う。
pub fn app_with(db: Arc<Db>, config: Config) -> Router {
    // 新着の知らせ。置く・返事・印の handler が送り、`GET /api/events` が流す。
    let events = Events::new(config.closing.clone());
    let generated = Pagetray::builder(PagetrayConfig::builder().build())
        .get_health(get_health)
        .create_page({
            let db = db.clone();
            let castlist = config.castlist.clone();
            let events = events.clone();
            move |input, host| {
                pages::create_page(db.clone(), castlist.clone(), events.clone(), input, host)
            }
        })
        .list_pages({
            let db = db.clone();
            move |input| list::list_pages(db.clone(), input)
        })
        .set_reply_state({
            let db = db.clone();
            let events = events.clone();
            move |input| marks::set_reply_state(db.clone(), events.clone(), input)
        })
        .add_review_marks({
            let db = db.clone();
            let events = events.clone();
            move |input| marks::add_review_marks(db.clone(), events.clone(), input)
        })
        .delete_pages({
            let db = db.clone();
            move |input| remove::delete_pages(db.clone(), input)
        })
        .create_file({
            let db = db.clone();
            let castlist = config.castlist.clone();
            let events = events.clone();
            move |input, host, ui| {
                files::create_file(
                    db.clone(),
                    castlist.clone(),
                    events.clone(),
                    input,
                    host,
                    ui,
                )
            }
        })
        .list_files({
            let db = db.clone();
            move |input| files::list_files(db.clone(), input)
        })
        .delete_files({
            let db = db.clone();
            move |input| files::delete_files(db.clone(), input)
        })
        .build()
        .expect("どの操作にも handler がある");
    let generated = Generated::new(generated);
    Router::new()
        // ページの本文（今の server と同じ URL）。`project` に `api` は置けないので、`/api/…` は本文の道に入れず、
        // みな生成した層に渡す（名指した道が先に当たる）。
        .route("/{project}/{key}/{file}", get(pages::show))
        // file の中身（今の server と同じ URL）。段の数がページの本文と違うので、種類 files のページ（`files-1.html`）とぶつからない。
        .route("/{project}/{key}/files/{file}", get(files::show))
        .with_state(db.clone())
        // 新着の知らせ（SSE。名指した道が、生成した層の `/api/…` より先に当たる）。
        .route("/api/events", get(events::stream).with_state(events))
        // 画面（`/`・`/{project}/`・`/{project}/{key}/`）。
        .merge(ui::routes(db.clone(), config.castlist))
        // backup と logs。
        .merge(hand::routes(db, config.runlog))
        .route_service("/api/{*rest}", generated.clone())
        .fallback_service(generated)
        .layer(middleware::from_fn(guard::limit))
        .layer(middleware::from_fn_with_state(
            config.timeout,
            guard::timeout,
        ))
        .layer(middleware::from_fn(guard::guard))
        // 問い合わせごとの 1 行（記録の file にだけ書く。src/runlog.rs の init）。guard が断った物も残る。今の server と同じ。
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        // 一番外に置く。guard が断った答えにも付く。
        .layer(middleware::from_fn(guard::stamp))
}

async fn get_health(
    _input: pagetray_sdk::input::GetHealthInput,
) -> Result<GetHealthOutput, GetHealthError> {
    Ok(GetHealthOutput {
        status: "ok".to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
    })
}

/// DB の字の時刻（RFC 3339）を、答えの時刻にする。読めなければ、その訳。
fn datetime(s: &str) -> Result<aws_smithy_types::DateTime, String> {
    time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339)
        .ok()
        .and_then(|t| aws_smithy_types::DateTime::from_nanos(t.unix_timestamp_nanos()).ok())
        .ok_or_else(|| format!("DB の時刻が読めない: {s}"))
}
