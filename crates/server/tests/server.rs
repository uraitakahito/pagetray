//! 受け入れの試験: 本物の binary（pagetray-server）を空いた port で起こし、HTTP で叩く（castlist の server の
//! tests/server.rs と同じ形）。守りの層の誤りの形・HEAD・本文の上限・backup・止め方・起きた行は、hyper を通した本物の答えでも見る
//! （0.8.x までの server の DB を断る所は tests/refuse_old.rs）。

mod common;

use std::io::{Read, Write};
use std::time::Duration;

use common::server::Server;
use common::{remove_db, temp_db};

#[test]
fn healthz_は版を返す() {
    let server = Server::start();
    let answer = server.get("/healthz");
    assert_eq!(answer.status, 200, "{answer:?}");
    let body = answer.json();
    assert_eq!(body["status"], "ok");
    assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
}

#[test]
fn head_の_healthz_は_本文なしの_200() {
    let server = Server::start();
    let answer =
        server.send_text("HEAD /healthz HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    assert_eq!(answer.status, 200, "{answer:?}");
    assert!(answer.body.is_empty(), "{answer:?}");
    assert_eq!(
        answer.header("content-type").as_deref(),
        Some("application/json")
    );
}

#[test]
fn host_が_127_0_0_1_と_localhost_でなければ_forbidden_で断る() {
    let server = Server::start();
    let answer = server
        .send_text("GET /healthz HTTP/1.1\r\nHost: evil.example\r\nConnection: close\r\n\r\n");
    assert_eq!(answer.status, 403, "{answer:?}");
    assert_eq!(
        answer.header("x-amzn-errortype").as_deref(),
        Some("Forbidden")
    );
    assert!(answer.json()["message"].is_string(), "{answer:?}");
    let answer =
        server.send_text("GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    assert_eq!(answer.status, 200, "{answer:?}");
}

#[test]
fn origin_の付いた書く要求を_forbidden_で断る() {
    let server = Server::start();
    let answer = server.send_text(
        "POST /api/projects/p/keys/k/kinds/plan/pages?role=tpm HTTP/1.1\r\nHost: 127.0.0.1\r\nOrigin: https://evil.example\r\nContent-Type: text/html\r\nContent-Length: 2\r\nConnection: close\r\n\r\nhi",
    );
    assert_eq!(answer.status, 403, "{answer:?}");
    assert_eq!(
        answer.header("x-amzn-errortype").as_deref(),
        Some("Forbidden")
    );
}

#[test]
fn 上限を_1_byte_超える本文は_payload_too_large_で断る() {
    let server = Server::start();
    // 頭だけを送る（Content-Length で超えると言う要求は、本文を読まずに断る。16 MiB を送り切る前に答えて閉じるので、
    // 本文まで送ると書く側が切られる）。
    let answer = server.send_text(&format!(
        "POST /api/projects/p/keys/k/kinds/plan/pages?role=tpm HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        pagetray_server::MAX_BODY + 1
    ));
    assert_eq!(answer.status, 413, "{}", answer.head);
    assert_eq!(
        answer.header("x-amzn-errortype").as_deref(),
        Some("PayloadTooLarge")
    );
    assert!(answer.json()["message"].is_string(), "{answer:?}");
}

#[test]
fn backup_は_置いたページの入った_sqlite_の_file_を返す() {
    let server = Server::start();
    let placed = server.post(
        "/api/projects/p/keys/k/kinds/plan/pages?role=tpm",
        "text/html",
        b"<title>a</title>",
    );
    assert_eq!(placed.status, 201, "{placed:?}");
    let answer = server.get("/api/backup");
    assert_eq!(answer.status, 200, "{}", answer.head);
    assert_eq!(
        answer.header("content-type").as_deref(),
        Some("application/vnd.sqlite3")
    );
    assert_eq!(
        answer.header("content-disposition").as_deref(),
        Some("attachment; filename=\"pages.db\"")
    );
    assert!(
        answer.body.starts_with(b"SQLite format 3\0"),
        "{}",
        answer.head
    );
    // 中身は DB 全体（ページの本文が入る）。
    assert!(
        answer
            .body
            .windows(b"<title>a</title>".len())
            .any(|w| w == b"<title>a</title>"),
        "ページの本文が無い"
    );
}

#[test]
fn 止める合図で自分から終わる() {
    let mut server = Server::start();
    server.terminate();
    let status = server
        .wait_for(Duration::from_secs(5))
        .expect("SIGTERM の後 5 秒で終わらない");
    assert_eq!(status.code(), Some(0), "{status:?}");
}

/// 合図の前に受け始めた問い合わせは、合図の後に答え終わってから閉じる（0.8.x の server は待たずに捨てていた）。
#[test]
fn 合図の前に受け始めた置く要求は_合図の後に答え終わってから閉じる() {
    let mut server = Server::start();
    let body = b"<title>a</title>";
    let mut stream = server.connect();
    write!(
        stream,
        "POST /api/projects/p/keys/k/kinds/plan/pages?role=tpm HTTP/1.1\r\nHost: 127.0.0.1\r\n\
         Content-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .unwrap();
    // 頭を受けさせてから合図を送り、その後に本文を送る。
    std::thread::sleep(Duration::from_millis(300));
    server.terminate();
    std::thread::sleep(Duration::from_millis(300));
    stream.write_all(body).unwrap();
    let mut answer = String::new();
    stream.read_to_string(&mut answer).unwrap();
    assert!(answer.starts_with("HTTP/1.1 201"), "{answer}");
    let status = server
        .wait_for(Duration::from_secs(5))
        .expect("答えた後 5 秒で終わらない");
    assert_eq!(status.code(), Some(0), "{status:?}");
}

/// 合図の後、終わらない問い合わせを待つのは SHUTDOWN_WAIT（3 秒）まで。過ぎたら記録に 1 行書いて閉じ、0 で終わる
/// （問い合わせの時間切れ 5 秒より前に）。
#[test]
fn 終わらない問い合わせがあっても_合図の後_上限で閉じて終わる() {
    let db = temp_db("stop-wait");
    let mut server = Server::start_at(&db);
    let mut stream = server.connect();
    // 本文を送らずに待たせる。
    stream
        .write_all(
            b"POST /api/projects/p/keys/k/kinds/plan/pages?role=tpm HTTP/1.1\r\nHost: 127.0.0.1\r\n\
              Content-Type: text/html\r\nContent-Length: 10\r\nConnection: close\r\n\r\n",
        )
        .unwrap();
    std::thread::sleep(Duration::from_millis(200));
    server.terminate();
    let status = server
        .wait_for(Duration::from_millis(4500))
        .expect("合図の後 4.5 秒で終わらない（待ちに上限が無い）");
    assert_eq!(status.code(), Some(0), "{status:?}");
    let log = std::fs::read_to_string(db.with_file_name("logs").join("server.log")).unwrap();
    assert!(log.contains("stop wait exceeded"), "{log}");
    drop(stream);
    remove_db(&db);
}

#[test]
fn 新着の知らせを聞いている者がいても_止める合図で閉じて終わる() {
    let mut server = Server::start();
    let mut stream = server.connect();
    stream
        .write_all(b"GET /api/events HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .unwrap();
    // 頭が届くまで待つ（聞き始めてから合図を送る）。
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        assert_eq!(stream.read(&mut byte).unwrap(), 1, "頭の途中で切れた");
        head.push(byte[0]);
    }
    let head = String::from_utf8_lossy(&head).into_owned();
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    assert!(head.contains("text/event-stream"), "{head}");
    server.terminate();
    let status = server
        .wait_for(Duration::from_secs(5))
        .expect("聞いている者がいると、SIGTERM の後 5 秒で終わらない");
    assert_eq!(status.code(), Some(0), "{status:?}");
    // 知らせの流れも閉じている（読み切れる）。
    let mut rest = Vec::new();
    stream
        .read_to_end(&mut rest)
        .expect("知らせの流れが閉じない");
}

#[test]
fn 起きた行は版と待ち受ける所と_db_を出す() {
    let db = temp_db("start-line");
    let mut server = Server::start_at(&db);
    let out = server.stdout();
    let line = out
        .lines()
        .find(|l| l.contains(" listen="))
        .unwrap_or_else(|| panic!("{out}"));
    assert!(
        line.contains(&format!(
            "pagetray {} listen=127.0.0.1:{} db={}",
            env!("CARGO_PKG_VERSION"),
            server.port,
            db.display()
        )),
        "{line}"
    );
    // castlist を設定しなければ、castlist の欄は出ない。
    assert!(!line.contains("castlist="), "{line}");
    assert_eq!(server.stop(), Some(0));
    remove_db(&db);
}

#[test]
fn castlist_を設定すると_起きた行に聞く先と_link_の頭が出る() {
    let db = temp_db("start-castlist");
    let mut server = Server::start_with(
        &db,
        &[
            ("PAGETRAY_CASTLIST_URL", "http://127.0.0.1:8802"),
            ("PAGETRAY_CASTLIST_LINK_URL", "http://localhost:8802"),
        ],
    );
    let out = server.stdout();
    let line = out
        .lines()
        .find(|l| l.contains(" listen="))
        .unwrap_or_else(|| panic!("{out}"));
    assert!(
        line.ends_with(&format!(
            "db={} castlist=http://127.0.0.1:8802 castlist_link=http://localhost:8802",
            db.display()
        )),
        "{line}"
    );
    assert_eq!(server.stop(), Some(0));
    remove_db(&db);
}
