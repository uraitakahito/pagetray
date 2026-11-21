# 変えたこと

版ごとに、API の版（[docs/src/api.md](docs/src/api.md)）が変わったかを書く。

## 0.4.0（まだ出していない）

API の版: 1（変わらない。エンドポイントを足しただけ。呼ぶ側は変えなくてよい）

- file（画像・PDF・字の file）を置く・探す・消すエンドポイントを足した（足しただけ。前からある呼ぶ側は変えなくてよい）。
  - `POST /api/files`・`GET /api/files`・`DELETE /api/files` と、中身を返す `GET /<project>/<key>/files/<語>-<n>.<拡張子>`。受ける種類は png・jpg・jpeg・gif・webp・pdf・txt・log・md・csv・json で、1 つ 16 MiB まで。形は docs の「API の仕様」。
  - `GET /api/events` は、file が置かれると `event: file` で file の記録を送る。ページの知らせ（名の無い event）は前のまま。
- 移行 `0003` を足した（表 `files`・`files_removed`）。当てた後は、写しから戻さないと前の版に戻せない（前の版は、0003 を当てた DB では起きない）。
  - DB の写し（`/api/backup`）にも file が入り、その分大きくなる。
- CLI に `file list`・`file view`・`file put`・`file delete` を足した。形は docs の「CLI」。
  - `file put` は、画像と PDF を `--reviewed` が無いと置かない。字の file は秘密の形（token・鍵の頭）を探し、当たれば置かずに行の番号を出す（`--allow-secret` で置ける）。JPEG に EXIF があれば知らせる。どれも置き場に送る前に止まる。
  - `file delete` は、`page delete` と同じく `--yes` を付けた時だけ消し、`PAGETRAY_NO_DELETE` で止まる。
  - `file put` は、0.4.0 より前の置き場（`/api/files` が無い）には置けない（置き場が断り、code 1 で止まる）。
- 文書の本を、本物の browser で見る試験（`e2e/docs-tests/`）を足した。検索・言葉の切り替え・図が動くかを、CI の job `docs` が見る。

## 0.3.2

API の版: 1（変わらない。呼ぶ側は変えなくてよい）

- 一覧の UI の 4 つの区画に、名を決めた: 操作の帯（`#toolbar`）・絞り込みの列（`#filters`）・一覧の列（`#list`）・本文の枠（`#viewer`）。
  - HTML に id と読み上げ用の名（`aria-label`）を付けた。見た目は変わらない。`<nav>` の `aria-label` は「絞り込み」から「絞り込みの列」に、`<body>` の `data-open` は `data-viewer` に替わった。
  - URL の引数（`open=` など）は変えていない。
- 文書の本に「用語集」の章を足した（置き場の物・API・一覧の UI の語。語・code の名・意味・使わない言い方）。
- 一覧の UI に icon を付けた。browser が `/favicon.ico` を取りに行かなくなり、console に 404 が出なくなる。
- 一覧の UI を、本物の browser で見る試験（`e2e/`。Playwright）を足した。image にも CLI にも入らない。
  - 動いている置き場に向けて流せる、読むだけの試験も足した。版を上げた後の「確かめる」で、browser で一覧を見る代わりに使う（[動かす](https://uraitakahito.github.io/pagetray/run.html)）。
- サーバーの API の動きは 0.3.1 と同じ。移行の SQL は足していない（0.3.1 に戻しても、同じ DB で起きる）。

## 0.3.1

API の版: 1（変わらない。呼ぶ側は変えなくてよい）

- 文書と出力の語を替えた（動きは変わらない）: 「口の約束」を「API の仕様」に、「口の版」を「API の版」に、1 つ 1 つの「口」を「エンドポイント」に。
  - CLI の出力も替わる: `pagetray status` の「口の版: 1」が「API の版: 1」に、版が合わない時の文が「置き場（…）の API の版は …」に。出力の字を読んでいる script があれば直す（終わりの code と `--json` の欄は変わらない）。
  - HTTP の形（path・header `Pagetray-Api`・JSON の欄）は変えていない。
- 文書の本の冒頭から、もとの repo から分けた経緯の 1 文を消した。
- サーバーの動きは 0.3.0 と同じ。移行の SQL は足していない（0.3.0 に戻しても、同じ DB で起きる）。

## 0.3.0

API の版: 1（変わらない。呼ぶ側は変えなくてよい）

- CLI（`pagetray`）を足した。置き場を端末から操作する。命令は `status`・`page list`・`page view`・`page put`・`page delete`・`page watch`・`browse`・`backup`・`api`・`completion`。
  - `page delete` は、まず消す物を一覧で出し、`--yes` を付けた時だけ消す。環境変数 `PAGETRAY_NO_DELETE` が付いていれば、消す命令（`api` の `DELETE` も）は何もせずに止まる。
  - release に、macOS（arm64）用の binary（`pagetray-v0.3.0-aarch64-apple-darwin.tar.gz`）と checksum が付く。image と同じ走査を通った物だけが付く。使い方は docs の「CLI」。
- サーバーの binary の名を `pagetray` から `pagetray-server` に替えた（`pagetray` の名は、人が打つ CLI が使う）。
  - image の中の file は `/pagetray-server` と空の `/data`。`ENTRYPOINT` も替えたので、`container run` の 1 行は変わらない。
  - API・環境変数（`PAGETRAY_DB`・`PAGETRAY_LISTEN`）・起きた時の 1 行（`pagetray <版> listen=… db=…`）は変わらない。
- 移行の SQL は足していない。

## 0.2.0

API の版: 1（前の 0.1.0 と同じ形。呼ぶ側は変えなくてよい）

- 名前を pagestore から pagetray に替えた。
  - binary は `pagetray`、起きた時の 1 行は `pagetray <版> listen=… db=…`。
  - 環境変数は `PAGETRAY_DB`・`PAGETRAY_LISTEN`（前の `PAGESTORE_DB`・`PAGESTORE_LISTEN` は読まない。既定の値は同じ）。
  - 一覧の UI が browser に覚える未読の印は、前の名で覚えた物も読む。
- `GET /api/version` を足した。どの答えにも header `Pagetray-Api` が付く。
- 移行の SQL は足していない（0.1.0 の DB が、そのまま使える。0.1.0 に戻しても、同じ DB で起きる）。
- 当てた移行の SQL が変わっていないかを見る試験を足した。

## 0.1.0

skills の repo の `pagestore/` として作った物（API・一覧の UI・新着の push・消すエンドポイント・DB の写し）。この repo には、その履歴ごと移した。
