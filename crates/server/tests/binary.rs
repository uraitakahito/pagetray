//! 起こす binary（`pagetray-server`）。`PAGETRAY_DB`・`PAGETRAY_LISTEN` で起き、castlist の設定（`PAGETRAY_CASTLIST_URL`・
//! `PAGETRAY_CASTLIST_LINK_URL`）の形が違えば、DB を開く前に断って止まる。記録・止め方・起きた行・backup は
//! tests/server.rs と tests/runlog.rs（本物の binary を起こす道具は tests/common/server.rs）。

mod common;

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};

const BIN: &str = env!("CARGO_BIN_EXE_pagetray-server");

/// 空いている port。
fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

/// `GET path` を 1 度送り、答えの全部（頭と本文）を字で返す。つながらなければ None。
async fn fetch(port: u16, path: &str) -> Option<String> {
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .ok()?;
    let request =
        format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await.ok()?;
    let mut answer = Vec::new();
    stream.read_to_end(&mut answer).await.ok()?;
    Some(String::from_utf8_lossy(&answer).into_owned())
}

#[tokio::test]
async fn it_serves_the_api_from_the_env_settings_and_no_screen() {
    let db = common::temp_db("bin");
    let port = free_port();
    let mut child = Command::new(BIN)
        .env("PAGETRAY_DB", &db)
        .env("PAGETRAY_LISTEN", format!("127.0.0.1:{port}"))
        .env_remove("PAGETRAY_CASTLIST_URL")
        // 画面の link の頭は、2.0.0 で画面と一緒に無くした。形の違う値が書いてあっても、読まずに起きる。
        .env("PAGETRAY_CASTLIST_LINK_URL", "not a url")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("起こせない");
    let started = Instant::now();
    let health = loop {
        if let Some(answer) = fetch(port, "/healthz").await {
            break answer;
        }
        if started.elapsed() > Duration::from_secs(10) || child.try_wait().unwrap().is_some() {
            let _ = child.kill();
            panic!("10 秒で待ち受けない（{:?}）", child.wait());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    let screen = fetch(port, "/").await.unwrap_or_default();
    let _ = child.kill();
    let _ = child.wait();
    common::remove_db(&db);
    assert!(health.starts_with("HTTP/1.1 200"), "{health}");
    assert!(health.contains(r#""status":"ok""#), "{health}");
    // 画面は無い（2.0.0 で外した。画面は callboard が持つ）。
    assert!(screen.starts_with("HTTP/1.1 404"), "{screen}");
    assert!(!screen.contains("<title>"), "{screen}");
}

#[test]
fn a_bad_castlist_url_stops_it_before_the_db_is_opened() {
    for (name, value) in [
        ("PAGETRAY_CASTLIST_URL", "https://127.0.0.1:8802"),
        ("PAGETRAY_CASTLIST_URL", "http://127.0.0.1:8802/api"),
    ] {
        let db = common::temp_db("bin-bad");
        let out = Command::new(BIN)
            .env("PAGETRAY_DB", &db)
            .env("PAGETRAY_LISTEN", format!("127.0.0.1:{}", free_port()))
            .env_remove("PAGETRAY_CASTLIST_URL")
            .env_remove("PAGETRAY_CASTLIST_LINK_URL")
            .env(name, value)
            .output()
            .expect("起こせない");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "{name}={value}: {stderr}");
        assert!(stderr.contains(name), "{name}={value}: {stderr}");
        assert!(!db.exists(), "{name}={value}: DB を作った");
        common::remove_db(&db);
    }
}
