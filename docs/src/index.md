# pagetray

ページ（HTML）の置き場のサーバー。置かれたページを DB（SQLite）に持ち、番号を付け、絞って探せる一覧の UI を出す。
この Mac の中（`127.0.0.1`）だけで使う物で、`roster page` の命令（[roster](https://github.com/uraitakahito/roster)）が、ここの API を `curl` で呼ぶ。

```mermaid
flowchart LR
  roster["roster page<br>（curl）"] -->|"POST・GET・DELETE /api/pages"| srv
  browser["browser"] -->|"一覧の UI・ページの本文"| srv
  subgraph mac["この Mac（127.0.0.1 だけ）"]
    srv["pagetray<br>（container）"] --> db[("SQLite<br>volume pagetray-data-v1")]
  end
```

## この本の章

| 章 | 中身 |
|---|---|
| [動かす](run.md) | 起こす・確かめる・DB の写しを取る・写しから戻す・止める・版を上げる |
| [CLI](cli.md) | 置き場を端末から操作する命令の入れ方と一覧 |
| [API の仕様](api.md) | 呼ぶ側が頼ってよい形と、API の版の決まり |
| [release を出す](release.md) | image を出す手順と、image に入ってよい物 |
| [開発](development.md) | source の作り・試験・この本の直し方 |
| [DB の作り](db.md) | 表・列・一意と索引と、表どうしの結び |
| [用語集](glossary.md) | 文書・画面・code で使う語と、使わない言い方 |

## API の早見

呼ぶ側が頼ってよい形と、API の版の決まりは [API の仕様](api.md)。ここは早見。

| エンドポイント | 中身 |
|---|---|
| `POST /api/pages?project=&key=&kind=&role=&workspace=` | 本文は HTML。番号を決めて `201` と JSON を返す。`n`・`created_at` は取り込みの時だけ付け、番号が使われていれば `409` |
| `GET /api/pages` | `project`・`key`・`kind`・`role`・`workspace`・`q`・`sort`・`limit`・`before`・`after` で絞る。`{"pages":[…]}` |
| `DELETE /api/pages?project=[&key=][&kind=][&n=]` | 合うページを消し、番号を表 `removed` に残して、消した path の一覧を返す。`n` には `key` と `kind` が要る。人は見分けない（だれが消してよいかは、呼ぶ側が決める） |
| `GET /<project>/<key>/<kind>-<n>.html` | 本文。無ければ `404`、消したページは `410` |
| `POST /api/files?project=&key=&name=&role=&workspace=` | 本文は file の中身（`Content-Type` が要る）。`name` は `<語>.<拡張子>` で、番号を決めて `201` と JSON を返す。受けない種類は `415`、16 MiB を超えれば `413` |
| `GET /api/files` | `project`・`key`・`role`・`workspace`・`q`・`sort`・`limit`・`before`・`after` で絞る。`{"files":[…]}` |
| `DELETE /api/files?project=[&key=][&name=][&n=]` | 合う file を消し、番号を表 `files_removed` に残して、消した path の一覧を返す。人は見分けない |
| `GET /<project>/<key>/files/<語>-<n>.<拡張子>` | file の中身。無ければ `404`、消した file は `410` |
| `GET /`・`/<project>/`・`/<project>/<key>/` | 一覧の UI（絞った一覧） |
| `GET /api/events` | 置かれるたびに 1 件を push（SSE。file は `event: file`） |
| `GET /api/backup` | DB の写し |
| `GET /api/version` | サーバーの版と API の版（`{"version":"0.2.0","api":1}`）。どの答えにも header `Pagetray-Api` が付く |
| `GET /healthz` | 生きているか |

- **この Mac の中からだけ**: `Host` が `127.0.0.1`・`localhost` でない要求と、`Origin` の付いた書く要求は断る。例外は、一覧の UI が同じ `Origin` から file を上げる `POST /api/files` だけ（役割は `product-manager` で記録する。[API の仕様](api.md) の「共通」）。
- **種類と役割の名**: 一覧は持たない。字の形（英小文字で始まり、英小文字・数字・`-` で 32 字まで）だけを見る。
- **一覧の UI**: サーバーが HTML を組んで返す。絞り込み・並べ替え・検索は link と form で、条件は URL に出る（script が無くても動く）。script がするのは、ページが置かれた時に一覧の列と絞り込みの列をサーバーが組んだ HTML に差し替えること（本文の枠は読み込み直さない）と、新着の印・未読の印（browser が覚える）・時刻をこの Mac の時刻で出すことと、file を上げることだけ。画面は 4 つの区画（操作の帯・絞り込みの列・一覧の列・本文の枠）でできていて、選んだページは、本文の枠（`sandbox` を付けた iframe）に 1 枚だけ出す。Key の画面（project と Key を 1 つずつ選んだ時）には、file の枠と上げる枠が足される（[動かす](run.md) の「一覧の UI から file を上げる」）。語の意味は [用語集](glossary.md)。
- **番号**: 書く接続は 1 本で、1 つの SQL が「今の最大 ＋ 1」を取って足す。最大は、置いてあるページ（`pages`）と消したページ（`removed`）の両方から取るので、消した番号は 2 度使わない（古い URL が別のページを指さない）。取り込みで消した番号を名指すと `409`。置いたページを書き換えるエンドポイントは無い。
- **消す**: 1 つの取引で、`removed` に番号を写し、`pages_fts` と `pages` の行を消す。本文は残さない（戻せない）。
