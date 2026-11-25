//! 新しい CLI の試験の土台。同じ commit の新しい server（pagetray-next の app）を、試験ごとに一時の DB と空いている port で
//! 起こし、build した `pagetray-cli-next` をそこに向けて打つ。試験の材料は、同じ Router に直に要求を通して置く。
#![allow(dead_code)]

use axum::Router;
use axum::body::Body;
use axum::http::{Request, header};
use http_body_util::BodyExt;
use pagetray_next::db::Db;
use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use tower::ServiceExt;

pub struct Server {
    pub url: String,
    app: Router,
    runtime: tokio::runtime::Runtime,
    dir: PathBuf,
}

impl Server {
    pub fn start() -> Self {
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "pagetray-cli-next-test-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let path = dir.join("pages.db");
        let (app, listener) = runtime.block_on(async {
            let db = Arc::new(Db::open(&path).await.unwrap());
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            (pagetray_next::app(db), listener)
        });
        let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        // 試験の間、待ち受け続ける（runtime を落とすと止まる）。
        let serve = axum::serve(listener, app.clone());
        runtime.spawn(async move { serve.await });
        Self {
            url,
            app,
            runtime,
            dir,
        }
    }

    /// `pagetray-cli-next` を、この server に向けて打つ。
    pub fn run(&self, args: &[&str]) -> Ran {
        run_with(&self.url, args, &[], None)
    }

    /// Router に要求を 1 つ通し、状態の数と、答えの本文（JSON に読めなければ Null）を返す。
    pub fn send(
        &self,
        method: &str,
        uri: &str,
        content_type: Option<&str>,
        body: Vec<u8>,
    ) -> (u16, Value) {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, "127.0.0.1");
        if let Some(t) = content_type {
            request = request.header(header::CONTENT_TYPE, t);
        }
        let request = request.body(Body::from(body)).unwrap();
        self.runtime.block_on(async {
            let response = self.app.clone().oneshot(request).await.unwrap();
            let status = response.status().as_u16();
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            (
                status,
                serde_json::from_slice(&bytes).unwrap_or(Value::Null),
            )
        })
    }

    /// API を直に呼んで 1 枚置き、置き場の中の path を返す（置く命令の試験でない所で、材料を置くため）。
    /// `query` は `role=…` などの引数（`&` でつなぐ）。
    pub fn place(&self, project: &str, key: &str, kind: &str, query: &str, html: &str) -> String {
        let uri = format!("/api/projects/{project}/keys/{key}/kinds/{kind}/pages?{query}");
        let (status, json) = self.send("POST", &uri, Some("text/html"), html.as_bytes().to_vec());
        assert_eq!(status, 201, "{json}");
        json["path"].as_str().unwrap().to_owned()
    }

    /// API を直に呼んで file を 1 つ置き、置き場の中の path を返す。
    pub fn place_file(
        &self,
        project: &str,
        key: &str,
        query: &str,
        content_type: &str,
        body: &[u8],
    ) -> String {
        let uri = format!("/api/projects/{project}/keys/{key}/files?{query}");
        let (status, json) = self.send("POST", &uri, Some(content_type), body.to_vec());
        assert_eq!(status, 201, "{json}");
        json["path"].as_str().unwrap().to_owned()
    }

    /// ページの返事の状態を、API を直に呼んで書く（`path` はページの path）。
    pub fn reply(&self, path: &str, state: &str) {
        let (project, key, file) = {
            let mut parts = path.trim_start_matches('/').splitn(3, '/');
            (
                parts.next().unwrap(),
                parts.next().unwrap(),
                parts.next().unwrap(),
            )
        };
        let (kind, n) = file
            .strip_suffix(".html")
            .and_then(|f| f.rsplit_once('-'))
            .unwrap();
        let uri = format!("/api/projects/{project}/keys/{key}/kinds/{kind}/pages/{n}/reply");
        let body = serde_json::json!({ "state": state }).to_string();
        let (status, json) = self.send("PUT", &uri, Some("application/json"), body.into_bytes());
        assert_eq!(status, 200, "{json}");
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// 中身の見方に合う、小さな PNG（頭の印だけ）。
pub const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";

/// 命令の結果。
pub struct Ran {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// 打つ binary（この試験と一緒に build した物）。
pub fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_pagetray-cli-next")
}

/// `pagetray-cli-next` を打つ。環境変数は受け継がず（手元の `PAGETRAY_*` や proxy に左右されないように）、名指した物だけを渡す。
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

/// どの要求にも、決まった答えを 1 つ返すだけの server。受けた要求の頭（要求の行と header。小文字にした物）を、
/// 受けた順に channel に送る。
pub fn canned(response: &'static str) -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let (heads, received) = mpsc::channel();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut head = String::new();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                head.push_str(&line.to_ascii_lowercase());
            }
            // 本文があれば読み捨てる。
            if let Some(length) = head
                .lines()
                .find_map(|l| l.strip_prefix("content-length:"))
                .and_then(|n| n.trim().parse::<u64>().ok())
            {
                let _ = std::io::copy(&mut reader.by_ref().take(length), &mut std::io::sink());
            }
            let _ = heads.send(head);
            let _ = stream.write_all(response.as_bytes());
        }
    });
    (url, received)
}

/// 要求を受けた順に、`answers` の答えを 1 つずつ返す server（`answers` を使い切れば閉じる）。受けた要求の行（小文字）を
/// 受けた順に channel に送る。答えは本文の長さ（`Content-Length`）と `Connection: close` を付けて返す。
pub fn scripted(answers: Vec<(&'static str, String)>) -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let (lines, received) = mpsc::channel();
    std::thread::spawn(move || {
        for (head, body) in answers {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut first = String::new();
            let _ = reader.read_line(&mut first);
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
            }
            let _ = lines.send(first.trim_end().to_ascii_lowercase());
            let _ = stream.write_all(
                format!(
                    "{head}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            );
        }
    });
    (url, received)
}

pub fn html(title: &str, text: &str) -> String {
    format!(
        "<!doctype html><html><head><title>{title}</title></head><body><p>{text}</p></body></html>"
    )
}

/// tab 区切りの出力を、行と欄に分ける。
pub fn rows(stdout: &str) -> Vec<Vec<&str>> {
    stdout.lines().map(|l| l.split('\t').collect()).collect()
}
