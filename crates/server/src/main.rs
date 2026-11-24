//! 新しい pagetray の server を起こす binary（`pagetray-next`。AI-36）。e2e の試験と、手で動かして見るのに使う。
//!
//! - `PAGETRAY_DB`（既定 `/data/pages.db`）を開き、`PAGETRAY_LISTEN`（既定 `0.0.0.0:8080`）で待ち受ける（今の server と同じ）。
//! - castlist の設定（`PAGETRAY_CASTLIST_URL`・`PAGETRAY_CASTLIST_LINK_URL`）は、DB を開く前に見て、形が違えば止まる。
//! - まだ最小の物: 記録（stdout と回す file）・止める合図での閉じ方・backup と logs の道は、次の PR で足す
//!   （docs/src/next.md の「段 4 の分け方」）。誤りは stderr に 1 行書く。
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use pagetray_next::castlist::Castlist;
use pagetray_next::db::Db;

#[tokio::main]
async fn main() -> ExitCode {
    match serve().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("pagetray-next: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn serve() -> Result<(), Box<dyn std::error::Error>> {
    let path = PathBuf::from(env_or("PAGETRAY_DB", "/data/pages.db"));
    let listen = env_or("PAGETRAY_LISTEN", "0.0.0.0:8080");
    // 形の違う castlist の URL は、DB を開く前に断る（castlist の URL が無ければ、付随する id だけを残す）。
    let castlist = Castlist::from_env()?;
    let db = Arc::new(Db::open(&path).await?);
    let listener = tokio::net::TcpListener::bind(&listen).await?;
    let config = pagetray_next::Config {
        castlist,
        ..Default::default()
    };
    axum::serve(listener, pagetray_next::app_with(db.clone(), config)).await?;
    db.close().await;
    Ok(())
}

fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_owned())
}
