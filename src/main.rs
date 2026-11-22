use pagetray::runlog::{self, RunLog};
use pagetray::{AppState, castlist::Castlist, db::Db, router};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use tokio::signal::unix::{SignalKind, signal};

#[tokio::main]
async fn main() -> ExitCode {
    let path = PathBuf::from(env_or("PAGETRAY_DB", "/data/pages.db"));
    // 記録の置き場は DB の隣の logs/。DB を開く前に用意する（開けなかった訳も残すため）。
    let log = RunLog::beside(&path);
    runlog::init(&log);
    let previous = log.previous();
    log.start(env!("CARGO_PKG_VERSION"));
    log.hook_panics();
    tracing::info!("{previous}");

    match serve(&path, &log).await {
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

/// 起きて、止める合図まで受け続ける。
async fn serve(path: &Path, log: &RunLog) -> Result<(), Box<dyn std::error::Error>> {
    let listen = env_or("PAGETRAY_LISTEN", "0.0.0.0:8080");
    // 送り手を聞く castlist と、画面の castlist への link の頭。castlist の URL が無ければ、付随する id だけを残す（未確認）。
    let castlist = Castlist::configure(
        std::env::var("PAGETRAY_CASTLIST_URL").ok().as_deref(),
        std::env::var("PAGETRAY_CASTLIST_LINK_URL").ok().as_deref(),
    )?;
    let db = Db::open(path).await?;
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
    let state = AppState::new(db.clone()).with_runlog(log.clone());
    let state = match castlist {
        Some(castlist) => state.with_castlist(castlist),
        None => state,
    };
    tokio::spawn(runlog::keep_alive(log.clone()));

    // container の中では PID 1 になる。PID 1 は、受け口を作らないと止める合図を無視する。
    let mut term = signal(SignalKind::terminate())?;
    let mut int = signal(SignalKind::interrupt())?;
    let stopped_by = tokio::select! {
        served = axum::serve(listener, router(state)) => { served?; None }
        _ = term.recv() => Some("SIGTERM"),
        _ = int.recv() => Some("SIGINT"),
    };
    if let Some(name) = stopped_by {
        // 合図を受けた時に 1 行。閉じ終えた印（stopped）は、呼んだ側が書く。
        log.signal(name);
        tracing::info!("signal {name}");
    }
    db.close().await;
    Ok(())
}

fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_owned())
}
