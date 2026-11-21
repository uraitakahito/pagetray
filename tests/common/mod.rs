//! 試験の土台。試験ごとに一時の DB を作り、道筋を直に呼ぶ。
#![allow(dead_code)]

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode, header};
use http_body_util::BodyExt;
use pagetray::{AppState, db::Db, router};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use tower::ServiceExt;

pub const SE: &str = "project=p&key=SKILLS-14&role=software-engineer&workspace=workspace:1";

/// 1×1 の PNG（本物の形）。
pub const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x06\x00\x00\x00\x1f\x15\xc4\x89\x00\x00\x00\rIDATx\x9cc\xf8\xcf\xc0\xf0\x1f\x00\x05\x00\x01\xff\x89\x99=\x1d\x00\x00\x00\x00IEND\xaeB`\x82";

pub struct Server {
    pub app: Router,
    pub db: Db,
    pub path: PathBuf,
}

impl Server {
    pub async fn start() -> Self {
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "pagetray-test-{}-{}.db",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let db = Db::open(&path).await.unwrap();
        let app = router(AppState::new(db.clone()));
        Self { app, db, path }
    }

    pub async fn send(&self, request: Request<Body>) -> (StatusCode, HeaderMap, Vec<u8>) {
        let response = self.app.clone().oneshot(request).await.unwrap();
        let (parts, body) = response.into_parts();
        let bytes = body.collect().await.unwrap().to_bytes().to_vec();
        (parts.status, parts.headers, bytes)
    }

    pub async fn get(&self, uri: &str) -> (StatusCode, HeaderMap, Vec<u8>) {
        self.send(request("GET", uri, Body::empty())).await
    }

    pub async fn json(&self, uri: &str) -> Value {
        let (status, _, body) = self.get(uri).await;
        assert_eq!(status, StatusCode::OK, "{uri}");
        serde_json::from_slice(&body).unwrap()
    }

    pub async fn put(&self, query: &str, html: &str) -> (StatusCode, Value) {
        let uri = format!("/api/pages?{query}");
        let (status, _, body) = self
            .send(request("POST", &uri, Body::from(html.to_owned())))
            .await;
        (status, serde_json::from_slice(&body).unwrap())
    }

    /// 消して、消した path の一覧を返す。
    pub async fn remove(&self, query: &str) -> (StatusCode, Value) {
        let uri = format!("/api/pages?{query}");
        let (status, _, body) = self.send(request("DELETE", &uri, Body::empty())).await;
        (status, serde_json::from_slice(&body).unwrap())
    }

    /// file を置く。`content_type` が None なら header を付けない。
    pub async fn put_file(
        &self,
        query: &str,
        content_type: Option<&str>,
        bytes: &[u8],
    ) -> (StatusCode, HeaderMap, Value) {
        let mut request = request(
            "POST",
            &format!("/api/files?{query}"),
            Body::from(bytes.to_vec()),
        );
        if let Some(content_type) = content_type {
            request
                .headers_mut()
                .insert(header::CONTENT_TYPE, content_type.parse().unwrap());
        }
        let (status, headers, body) = self.send(request).await;
        // 引数が足りない時は、axum が JSON でない文で断る。
        let json = serde_json::from_slice(&body).unwrap_or(Value::Null);
        (status, headers, json)
    }

    /// file を置いて、番号を返す。
    pub async fn place_file(&self, query: &str, content_type: &str, bytes: &[u8]) -> i64 {
        let (status, _, json) = self.put_file(query, Some(content_type), bytes).await;
        assert_eq!(status, StatusCode::CREATED, "{json}");
        json["n"].as_i64().unwrap()
    }

    pub async fn file_paths(&self, query: &str) -> Vec<String> {
        let json = self.json(&format!("/api/files?{query}")).await;
        json["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["path"].as_str().unwrap().to_owned())
            .collect()
    }

    pub async fn remove_files(&self, query: &str) -> (StatusCode, Value) {
        let uri = format!("/api/files?{query}");
        let (status, _, body) = self.send(request("DELETE", &uri, Body::empty())).await;
        (status, serde_json::from_slice(&body).unwrap())
    }

    pub async fn text(&self, uri: &str) -> String {
        let (status, _, body) = self.get(uri).await;
        assert_eq!(status, StatusCode::OK, "{uri}");
        String::from_utf8(body).unwrap()
    }

    /// 置いて、番号を返す。
    pub async fn place(&self, query: &str, html: &str) -> i64 {
        let (status, json) = self.put(query, html).await;
        assert_eq!(status, StatusCode::CREATED, "{json}");
        json["n"].as_i64().unwrap()
    }

    pub async fn paths(&self, query: &str) -> Vec<String> {
        let json = self.json(&format!("/api/pages?{query}")).await;
        json["pages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["path"].as_str().unwrap().to_owned())
            .collect()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm"] {
            let mut name = self.path.clone().into_os_string();
            name.push(suffix);
            let _ = std::fs::remove_file(name);
        }
    }
}

pub fn request(method: &str, uri: &str, body: Body) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::HOST, "127.0.0.1:8801")
        .body(body)
        .unwrap()
}

pub fn html(title: &str, text: &str) -> String {
    format!(
        "<!doctype html><html><head><title>{title}</title><style>p{{margin:0}}</style></head><body><p>{text}</p></body></html>"
    )
}
