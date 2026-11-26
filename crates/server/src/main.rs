//! pagetray の server を起こす binary（`pagetray-server`）。release の image（Containerfile）に /pagetray-server の名で入る。
//! e2e の試験と、手で動かして見るのにも使う。
//!
//! - `PAGETRAY_DB`（既定 `/data/pages.db`）を開き、`PAGETRAY_LISTEN`（既定 `0.0.0.0:8080`）で待ち受ける。
//! - castlist の設定（`PAGETRAY_CASTLIST_URL`・`PAGETRAY_CASTLIST_LINK_URL`）の形が違う時は、記録の置き場も DB も作らずに、
//!   stderr に訳を書いて 1 で終わる。
//! - 記録の置き場は DB の隣の `logs/`（[`pagetray_server::runlog`]）。起きたら初めに、前の回の終わり方の行（`previous: …`）を出す。
//! - 止める合図（SIGTERM・SIGINT）で新しい問い合わせを断り、新着の知らせの流れを閉じ、走っている物を終えてから閉じる
//!   （待つのは [`SHUTDOWN_WAIT`] まで）。
use std::future::IntoFuture;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use pagetray_server::castlist::Castlist;
use pagetray_server::db::Db;
use pagetray_server::runlog::{self, RunLog};
use pagetray_server::{Config, REQUEST_TIMEOUT, app_with};
use tokio::signal::unix::{SignalKind, signal};
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !args.is_empty() {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    let path = PathBuf::from(env_or("PAGETRAY_DB", "/data/pages.db"));
    // 形の違う castlist の設定は、記録の置き場も DB も作らずに断る（castlist の URL が無ければ、付随する id だけを残す）。
    let castlist = match Castlist::from_env() {
        Ok(castlist) => castlist,
        Err(e) => {
            eprintln!("pagetray: {e}");
            return ExitCode::FAILURE;
        }
    };
    // 記録の置き場は DB の隣の logs/。DB を開く前に用意する（開けなかった訳も残すため）。
    let log = RunLog::beside(&path);
    runlog::init(&log);
    let previous = log.previous();
    log.start(env!("CARGO_PKG_VERSION"));
    log.hook_panics();
    tracing::info!("{previous}");

    match serve(&path, castlist, &log).await {
        Ok(()) => {
            log.stopped();
            tracing::info!("pagetray stopped");
            ExitCode::SUCCESS
        }
        Err(e) => {
            // 誤りで終わった印を残す。残さないと、次の起動で SIGKILL と見分けられない。
            log.failed(&e.to_string());
            tracing::error!("pagetray: {e}");
            ExitCode::FAILURE
        }
    }
}

/// 使い方。引数は取らない。
const USAGE: &str = "使い方: pagetray-server（image の中では /pagetray-server。環境変数 PAGETRAY_DB・PAGETRAY_LISTEN などで起きる。引数は取らない）";

/// 起きて、止める合図まで受け続ける。
async fn serve(
    path: &Path,
    castlist: Option<Castlist>,
    log: &RunLog,
) -> Result<(), Box<dyn std::error::Error>> {
    let listen = env_or("PAGETRAY_LISTEN", "0.0.0.0:8080");
    // 開けない時（移行に落ちた・外のキーの崩れた行がある）は、訳を出して起きない。
    let db = Arc::new(Db::open(path).await?);
    let listener = tokio::net::TcpListener::bind(&listen).await?;
    tracing::info!(
        "pagetray {} listen={listen} db={}{}",
        env!("CARGO_PKG_VERSION"),
        path.display(),
        castlist.as_ref().map_or(String::new(), |c| format!(
            " castlist={} castlist_link={}",
            c.api(),
            c.link()
        ))
    );
    tokio::spawn(runlog::keep_alive(log.clone()));

    // container の中では PID 1 になる。PID 1 は、受け口を作らないと止める合図を無視する。
    // 合図が来たら新しい問い合わせを断り、新着の知らせの流れを閉じ（閉じないと、聞き手が切るまで待ち続ける）、
    // 走っている物を終えてから閉じる。
    let mut term = signal(SignalKind::terminate())?;
    let mut int = signal(SignalKind::interrupt())?;
    let closing = CancellationToken::new();
    let config = Config {
        timeout: REQUEST_TIMEOUT,
        castlist,
        runlog: Some(log.clone()),
        closing: closing.clone(),
    };
    let signalled = log.clone();
    let stopping = closing.clone();
    let serving = axum::serve(listener, app_with(db.clone(), config))
        .with_graceful_shutdown(async move {
            let name = tokio::select! {
                _ = term.recv() => "SIGTERM",
                _ = int.recv() => "SIGINT",
            };
            // 合図を受けた時に 1 行。走っている物を待つ間に切られても、合図を受けたことは残る。
            // 閉じ終えた印（stopped）は、呼んだ側が書く。
            signalled.signal(name);
            tracing::info!("signal {name}");
            closing.cancel();
        })
        .into_future();
    // 合図の後、走っている物を待つのは SHUTDOWN_WAIT まで（受け口を作ったので、2 度目の合図でも終わらない。
    // 遅い送り手・切らない client が居ても、container を止める命令の SIGKILL より前に、DB を閉じて印を残して終わる）。
    let served = tokio::select! {
        served = serving => served,
        () = async {
            stopping.cancelled().await;
            tokio::time::sleep(SHUTDOWN_WAIT).await;
        } => {
            tracing::warn!("stop wait exceeded wait={}s", SHUTDOWN_WAIT.as_secs());
            Ok(())
        }
    };
    // 受けるのに落ちた時も、DB は閉じてから訳を返す（WAL を畳む）。
    db.close().await;
    Ok(served?)
}

/// 止める合図の後、走っている問い合わせを待つ長さ。問い合わせの時間切れ（5 秒）と、container を止める命令が SIGKILL を
/// 送るまでの待ちより短くする。
const SHUTDOWN_WAIT: Duration = Duration::from_secs(3);

fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_owned())
}
