//! CLI の試験の土台。同じ commit のサーバーを、試験ごとに一時の DB と空いている port で起こし、
//! build した `pagetray` をそこに向けて打つ。
#![allow(dead_code)]

use pagetray::{AppState, db::Db, router};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;

pub struct Server {
    pub url: String,
    db: PathBuf,
}

impl Server {
    pub fn start() -> Self {
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let db = std::env::temp_dir().join(format!(
            "pagetray-cli-test-{}-{}.db",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let (ready, port) = mpsc::channel();
        let path = db.clone();
        // 試験の process が終わるまで動かしておく。
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Runtime::new().unwrap();
            runtime.block_on(async move {
                let db = Db::open(&path).await.unwrap();
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                ready.send(listener.local_addr().unwrap().port()).unwrap();
                axum::serve(listener, router(AppState::new(db)))
                    .await
                    .unwrap();
            });
        });
        let url = format!("http://127.0.0.1:{}", port.recv().unwrap());
        Self { url, db }
    }

    /// `pagetray` を、このサーバーに向けて打つ。
    pub fn run(&self, args: &[&str]) -> Ran {
        run_with(&self.url, args, &[], None)
    }

    /// 口を直に呼んで 1 枚置き、置き場の中の path を返す（置く命令の試験でない所で、材料を置くため）。
    pub fn place(&self, query: &str, html: &str) -> String {
        let mut response = ureq::post(format!("{}/api/pages?{query}", self.url))
            .send(html)
            .unwrap();
        let text = response.body_mut().read_to_string().unwrap();
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        json["path"].as_str().unwrap().to_owned()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm"] {
            let mut name = self.db.clone().into_os_string();
            name.push(suffix);
            let _ = std::fs::remove_file(name);
        }
    }
}

/// 命令の結果。
pub struct Ran {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// 打つ binary。ふだんは、この試験と一緒に build した物。`$PAGETRAY_TEST_BIN` があれば、その file
/// （CI の job `cli` と release が、外に出すその binary を試すのに使う）。
pub fn binary() -> String {
    std::env::var("PAGETRAY_TEST_BIN").unwrap_or_else(|_| env!("CARGO_BIN_EXE_pagetray").to_owned())
}

/// `pagetray` を打つ。環境変数は受け継がず（手元の `PAGETRAY_*` や proxy に左右されないように）、名指した物だけを渡す。
pub fn run_with(url: &str, args: &[&str], env: &[(&str, &str)], stdin: Option<&[u8]>) -> Ran {
    let mut child = Command::new(binary())
        .args(args)
        .env_clear()
        .env("PAGETRAY_URL", url)
        .envs(env.iter().copied())
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Some(input) = stdin {
        child.stdin.take().unwrap().write_all(input).unwrap();
    }
    let Output {
        status,
        stdout,
        stderr,
    } = child.wait_with_output().unwrap();
    Ran {
        code: status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    }
}

/// だれも待ち受けていない URL（port を取って、すぐ放す）。
pub fn closed_url() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port())
}

/// どの要求にも、決まった答えを 1 つ返すだけのサーバー（口の版が違う置き場のまね）。
pub fn canned(response: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut request = [0; 4096];
            let _ = stream.read(&mut request);
            let _ = stream.write_all(response.as_bytes());
        }
    });
    url
}

pub fn html(title: &str, text: &str) -> String {
    format!(
        "<!doctype html><html><head><title>{title}</title></head><body><p>{text}</p></body></html>"
    )
}
