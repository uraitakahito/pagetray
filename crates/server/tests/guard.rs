//! 守りの層の振る舞い。生成した道の前でも、手の道（ページの本文）の前でも効く。
//! - 断る答えは、生成した層の誤りと同じ restJson1 の形（header `X-Amzn-Errortype` と、本文の JSON の `message`）。
//! - Host と Origin（0.8.x の server と同じ）・HEAD（GET と同じに答え、本文を捨てる）・頭の印・時間切れ。
mod common;

use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use common::{App, HOST, pages_uri, refused};

fn with_host(method: &str, path: &str, host: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, host)
        .header(header::CONTENT_TYPE, "text/html")
        .body(Body::from("<p>x</p>"))
        .unwrap()
}

#[tokio::test]
async fn a_foreign_host_gets_the_forbidden_error_shape_on_every_kind_of_path() {
    let app = App::start().await;
    app.place("p", "k", "plan", "role=tpm", "<p>1</p>").await;
    let put = pages_uri("p", "k", "plan", "role=tpm");
    // 生成した道・ページの本文・知らない道のどれも、同じ形で断る。
    for (method, path) in [
        ("GET", "/healthz"),
        ("POST", put.as_str()),
        ("GET", "/p/k/plan-1.html"),
        ("GET", "/no/such"),
    ] {
        for host in [
            "evil.example",
            "evil.example:8801",
            "127.0.0.2:8801",
            "localhost.evil.example",
        ] {
            let (status, headers, body) = app.send(with_host(method, path, host)).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{method} {path} {host}");
            let message = refused(&headers, &body, "Forbidden");
            assert!(message.contains("Host"), "{message}");
            // 断った答えにも頭の印が付く（一番外）。
            assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
            assert_eq!(headers[header::REFERRER_POLICY], "no-referrer");
        }
    }
    // Host の無い要求も断る。
    let request = Request::get("/healthz").body(Body::empty()).unwrap();
    assert_eq!(app.send(request).await.0, StatusCode::FORBIDDEN);
    // 断った物は置いていない。
    assert_eq!(app.count("pages").await, 1);
}

#[tokio::test]
async fn local_hosts_with_any_port_are_accepted() {
    let app = App::start().await;
    for host in ["127.0.0.1", "127.0.0.1:9999", "localhost", "localhost:8801"] {
        let (status, _, _) = app.send(with_host("GET", "/healthz", host)).await;
        assert_eq!(status, StatusCode::OK, "{host}");
    }
}

#[tokio::test]
async fn a_write_with_an_origin_is_refused_even_from_the_same_origin() {
    let app = App::start().await;
    for origin in [
        format!("http://{HOST}"),
        "null".to_owned(),
        "https://evil.example".to_owned(),
    ] {
        let request = Request::post(pages_uri("p", "k", "plan", "role=tpm"))
            .header(header::HOST, HOST)
            .header(header::ORIGIN, origin.as_str())
            .header(header::CONTENT_TYPE, "text/html")
            .body(Body::from("<p>x</p>"))
            .unwrap();
        let (status, headers, body) = app.send(request).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{origin}");
        refused(&headers, &body, "Forbidden");
    }
    assert_eq!(app.count("pages").await, 0);
    // 読む要求は、Origin が付いていても通す（一覧の UI が本文を枠に出す）。
    app.place("p", "k", "plan", "role=tpm", "<p>1</p>").await;
    let request = Request::get("/p/k/plan-1.html")
        .header(header::HOST, HOST)
        .header(header::ORIGIN, format!("http://{HOST}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(app.send(request).await.0, StatusCode::OK);
}

#[tokio::test]
async fn head_answers_like_get_without_a_body() {
    let app = App::start().await;
    app.place("p", "k", "plan", "role=tpm", "<p>1</p>").await;
    for path in ["/healthz", "/p/k/plan-1.html", "/p/k/plan-2.html"] {
        let (get_status, get_headers, _) = app.get(path).await;
        let request = Request::head(path)
            .header(header::HOST, HOST)
            .body(Body::empty())
            .unwrap();
        let (status, headers, body) = app.send(request).await;
        assert_eq!(status, get_status, "{path}");
        assert_eq!(
            headers[header::CONTENT_TYPE],
            get_headers[header::CONTENT_TYPE],
            "{path}"
        );
        // HEAD で大きさを見る client のため、Content-Length も GET と同じ（review-17）。
        assert_eq!(
            headers.get(header::CONTENT_LENGTH),
            get_headers.get(header::CONTENT_LENGTH),
            "{path}"
        );
        assert!(body.is_empty(), "{path}");
    }
}

#[tokio::test]
async fn every_answer_is_stamped_and_the_old_api_header_is_gone() {
    let app = App::start().await;
    app.place("p", "k", "plan", "role=tpm", "<p>1</p>").await;
    for path in [
        "/healthz",
        "/p/k/plan-1.html",
        "/p/k/plan-9.html",
        "/api/nope",
    ] {
        let (_, headers, _) = app.get(path).await;
        assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff", "{path}");
        assert_eq!(headers[header::REFERRER_POLICY], "no-referrer", "{path}");
        assert!(headers.get("pagetray-api").is_none(), "{path}");
    }
}

/// 書く接続を握ったままにすると、置く要求は書く接続を待ち（WRITE_WAIT）、問い合わせの時間切れより前に「混んでいる」の 503。
#[tokio::test]
async fn a_busy_writer_is_unavailable_before_the_request_timeout() {
    assert!(pagetray_server::db::WRITE_WAIT < pagetray_server::REQUEST_TIMEOUT);
    assert!(pagetray_server::db::READ_WAIT < pagetray_server::REQUEST_TIMEOUT);
    let app = App::start().await;
    let held = app.db.write().acquire().await.expect("書く接続");
    let started = Instant::now();
    let (status, headers, json) = app.put("p", "k", "plan", "role=tpm", "<p>x</p>").await;
    let took = started.elapsed();
    drop(held);
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{json}");
    assert_eq!(common::error_type(&headers), "Unavailable");
    assert!(
        json["message"].as_str().unwrap().contains("混んでいる"),
        "{json}"
    );
    assert!(took >= pagetray_server::db::WRITE_WAIT, "{took:?}");
    assert!(took < pagetray_server::REQUEST_TIMEOUT, "{took:?}");
    assert_eq!(app.count("pages").await, 0);
}

/// 時間切れの長さを短くすると、それより遅い答えは時間切れの 503 になる。
#[tokio::test]
async fn a_slow_answer_times_out_with_unavailable() {
    let app = App::start_with(pagetray_server::Config {
        timeout: Duration::from_millis(200),
        ..Default::default()
    })
    .await;
    let held = app.db.write().acquire().await.expect("書く接続");
    let (status, headers, json) = app.put("p", "k", "plan", "role=tpm", "<p>x</p>").await;
    drop(held);
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{json}");
    assert_eq!(common::error_type(&headers), "Unavailable");
    assert!(
        json["message"].as_str().unwrap().contains("200 ms"),
        "{json}"
    );
}

/// 中身を `parts` 個の塊に分けて流す本文（大きさを前に言わない。`Content-Length` の無い chunked の要求と同じ）。
struct Chunks(Vec<bytes::Bytes>);

impl http_body::Body for Chunks {
    type Data = bytes::Bytes;
    type Error = std::convert::Infallible;

    fn poll_frame(
        mut self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<http_body::Frame<bytes::Bytes>, Self::Error>>> {
        std::task::Poll::Ready(self.0.pop().map(|b| Ok(http_body::Frame::data(b))))
    }
}

fn chunked(bytes: Vec<u8>, parts: usize) -> Body {
    let size = bytes.len().div_ceil(parts);
    let mut chunks: Vec<bytes::Bytes> = bytes
        .chunks(size)
        .map(bytes::Bytes::copy_from_slice)
        .collect();
    chunks.reverse();
    Body::new(Chunks(chunks))
}

/// 本文の上限は 16 MiB（ページも file も。0.8.x の server と同じ）。超えれば、守りの層が 413 `PayloadTooLarge`
/// （restJson1 の形）で断り、何も置かない。`Content-Length` の付いた要求も、大きさを言わずに流す要求も同じ。
/// ちょうど 16 MiB は通る。
#[tokio::test]
async fn a_body_over_16_mib_is_too_large_for_pages_and_files() {
    assert_eq!(pagetray_server::MAX_BODY, 16 * 1024 * 1024);
    let app = App::start().await;
    let page = |len: usize| vec![b'a'; len];
    let file = |len: usize| {
        let mut bytes = common::PNG.to_vec();
        bytes.resize(len, 0);
        bytes
    };
    let max = pagetray_server::MAX_BODY;
    let page_uri = pages_uri("p", "k", "plan", "role=tpm");
    let file_uri = common::files_uri("p", "k", "role=tpm&name=big.png");
    for (uri, content_type, bytes) in [
        (&page_uri, "text/html", page(max + 1)),
        (&file_uri, "image/png", file(max + 1)),
    ] {
        for streamed in [false, true] {
            let len = bytes.len();
            let mut builder = Request::post(uri.as_str())
                .header(header::HOST, HOST)
                .header(header::CONTENT_TYPE, content_type);
            let body = if streamed {
                chunked(bytes.clone(), 64)
            } else {
                builder = builder.header(header::CONTENT_LENGTH, len);
                Body::from(bytes.clone())
            };
            let (status, headers, body) = app.send(builder.body(body).unwrap()).await;
            assert_eq!(
                status,
                StatusCode::PAYLOAD_TOO_LARGE,
                "{uri} streamed={streamed}"
            );
            let message = refused(&headers, &body, "PayloadTooLarge");
            assert!(message.contains("16 MiB"), "{message}");
            assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
        }
    }
    assert_eq!(app.count("pages").await, 0);
    assert_eq!(app.count("files").await, 0);
    // ちょうど 16 MiB は通る（流しても）。
    let (status, _, json) = app.put("p", "k", "plan", "role=tpm", page(max)).await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    assert_eq!(json["size"], max);
    let request = Request::post(file_uri.as_str())
        .header(header::HOST, HOST)
        .header(header::CONTENT_TYPE, "image/png")
        .body(chunked(file(max), 64))
        .unwrap();
    let (status, _, body) = app.send(request).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(&body)
    );
    assert_eq!(app.count("pages").await, 1);
    assert_eq!(app.count("files").await, 1);
}
