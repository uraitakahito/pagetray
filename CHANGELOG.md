# 変えたこと

版ごとに、API の版（[docs/src/api.md](docs/src/api.md)）が変わったかを書く。

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
