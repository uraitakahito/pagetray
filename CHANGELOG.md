# 変えたこと

版ごとに、口の版（[docs/src/api.md](docs/src/api.md)）が変わったかを書く。

## 次の版（まだ出していない）

口の版: 1（変わらない。呼ぶ側は変えなくてよい）

- サーバーの binary の名を `pagetray` から `pagetray-server` に替えた（`pagetray` の名は、人が打つ CLI が使う）。
  - image の中の file は `/pagetray-server` と空の `/data`。`ENTRYPOINT` も替えたので、`container run` の 1 行は変わらない。
  - 口・環境変数（`PAGETRAY_DB`・`PAGETRAY_LISTEN`）・起きた時の 1 行（`pagetray <版> listen=… db=…`）は変わらない。
- 移行の SQL は足していない。

## 0.2.0

口の版: 1（前の 0.1.0 と同じ形。呼ぶ側は変えなくてよい）

- 名前を pagestore から pagetray に替えた。
  - binary は `pagetray`、起きた時の 1 行は `pagetray <版> listen=… db=…`。
  - 環境変数は `PAGETRAY_DB`・`PAGETRAY_LISTEN`（前の `PAGESTORE_DB`・`PAGESTORE_LISTEN` は読まない。既定の値は同じ）。
  - 一覧の UI が browser に覚える未読の印は、前の名で覚えた物も読む。
- `GET /api/version` を足した。どの答えにも header `Pagetray-Api` が付く。
- 移行の SQL は足していない（0.1.0 の DB が、そのまま使える。0.1.0 に戻しても、同じ DB で起きる）。
- 当てた移行の SQL が変わっていないかを見る試験を足した。

## 0.1.0

skills の repo の `pagestore/` として作った物（口・一覧の UI・新着の push・消す口・DB の写し）。この repo には、その履歴ごと移した。
