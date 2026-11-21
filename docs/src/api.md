# API の仕様（API の版 1）

pagetray を呼ぶ側（skills の `tpm page` など）が頼ってよい形を書く。ここに書いた形を、前からある呼ぶ側が困るように変える時は、API の版を上げる。

## 版

- `GET /api/version` が `{"version":"0.2.0","api":1}` を返す。`version` はサーバーの版（`Cargo.toml`）、`api` は API の版。
- どの答えにも（誤りにも、断った答えにも）header `Pagetray-Api: 1` が付く。呼ぶ側は、頼んだその答えで、API の版が合っているかを見られる。
- この header が無い答えは、API の版ができる前のサーバー（pagestore 0.1.0）の物。形は API の版 1 と同じ。

### API の版を上げる時・上げない時

| 変更 | API の版 |
|---|---|
| エンドポイント・引数・答えの欄を**足す**（前からある呼ぶ側は、知らない欄を読み飛ばす） | 上げない |
| エンドポイント・引数・答えの欄を消す、名や型を変える | 上げる |
| code の意味を変える（`409` が別の訳で返る、など） | 上げる |
| URL の形（`/<project>/<key>/<kind>-<n>.html`）を変える | 上げる（置いたページの URL が切れるので、まずしない） |
| file の URL の形（`/<project>/<key>/files/<語>-<n>.<拡張子>`）を変える | 上げる（ページの中の参照が切れるので、まずしない） |
| 受ける file の種類を減らす | 上げる（前は通った file が通らなくなる） |
| 引数の字の決まりを厳しくする（前は通った字が通らなくなる） | 上げる |
| 一覧の UI の見た目・文・HTML の作り | 上げない（UI は仕様に入らない） |
| 誤りの文（`error` の中身） | 上げない（人が読む文。呼ぶ側は code で見分ける） |

呼ぶ側は、知らない欄を読み飛ばし、`error` の文で分けないこと。

## 共通

- 受けるのは、この Mac の中からの要求だけ。`Host` が `127.0.0.1`・`localhost`（port は何でもよい）でなければ `403`。`GET`・`HEAD` のほかの要求に `Origin` が付いていれば `403`（browser の中のページからは書けない）。
- その例外は 1 つだけ: 一覧の UI の Key の画面から file を上げる要求。`POST /api/files` で、`Origin` が `http://<要求の Host（port も）>` とそのまま同じで、header `Sec-Fetch-Site` が無いか `same-origin` の時だけ通す。`Origin: null`（本文の枠の中のページ）・ほかの port・ほかの名・`https://`・`Sec-Fetch-Site: cross-site` は `403`。同じ `Origin` でも、ほかの道（`POST /api/pages`・どの `DELETE` も）は `403`（一覧の UI からは、ページを置けず、何も消せない）。
- 誤りの答えは `{"error":"<人が読む文>"}`。
- 字の決まり:

| 引数 | 決まり |
|---|---|
| `project`・`key` | 英数字と `.` `_` `-` で 1〜255 字。`.` と `..` は駄目。`project` に `api` は使えない |
| `kind`・`role` | 英小文字で始まり、英小文字・数字・`-` で 1〜32 字。名の一覧はサーバーが持たない（呼ぶ側が決める） |
| `workspace` | 英数字と `:` `.` `_` `-` で 0〜64 字 |
| `name`（file） | `<語>.<拡張子>`。語は英小文字・数字・`-` `_` で 1〜64 字（英小文字か数字で始まる）。拡張子は下の「受ける file の種類」の物だけ |
| `n` | 1 以上の整数 |
| `created_at` | `2026-01-02T03:04:05Z` か、秒の小数（3 桁まで）が付いた形。UTC だけ |

## ページの記録

置く・探すの答えに出る、ページ 1 枚の記録（本文は入らない）。

```json
{
  "project": "-Users-me-projects-x",
  "key": "K-1",
  "kind": "result",
  "n": 1,
  "title": "<title> から取った題",
  "role": "software-engineer",
  "workspace": "workspace:1",
  "created_at": "2026-11-21T04:45:22.472Z",
  "size": 1234,
  "path": "/-Users-me-projects-x/K-1/result-1.html"
}
```

`path` は、このサーバーの中の path。`<サーバーの URL><path>` で本文が開く。

## file の記録

置く・探すの答えに出る、file 1 つの記録（中身は入らない）。file は、ページから参照する画像・PDF・字の file。

```json
{
  "project": "-Users-me-projects-x",
  "key": "K-1",
  "name": "screen",
  "ext": "png",
  "n": 1,
  "type": "image/png",
  "size": 48213,
  "sha256": "4ff6ab670a58c14270e034e2090d9a432caa263a14e0a25785386b0c12f880b5",
  "role": "software-engineer",
  "workspace": "workspace:1",
  "created_at": "2026-11-21T04:45:22.472Z",
  "path": "/-Users-me-projects-x/K-1/files/screen-1.png"
}
```

- `name` は名の語、`ext` は拡張子、`n` は名と拡張子ごとの番号。`type` は返す時の `Content-Type`（拡張子から決まる）。`sha256` は中身の SHA-256（16 進の小文字。サーバーが数える）。
- `path` は、このサーバーの中の path。ページの URL（3 段）と違い、4 段目に `files/` を持つので、ぶつからない。
- ページからは、相対の path で参照する（同じ Key なら `files/screen-1.png`、別の Key なら `../K-2/files/screen-1.png`）。

### 受ける file の種類

拡張子がこの表に無い file は受けない（動画・HTML・SVG など）。1 つ 16 MiB まで。

| 拡張子 | `Content-Type` | 中身の見方 |
|---|---|---|
| `png` | `image/png` | 頭の 8 byte が PNG の印 |
| `jpg`・`jpeg` | `image/jpeg` | 頭が `FF D8 FF` |
| `gif` | `image/gif` | 頭が `GIF87a` か `GIF89a` |
| `webp` | `image/webp` | 頭が `RIFF`、9〜12 byte 目が `WEBP` |
| `pdf` | `application/pdf` | 頭が `%PDF-` |
| `txt`・`log` | `text/plain; charset=utf-8` | UTF-8 として読めて、NUL を含まない |
| `md` | `text/markdown; charset=utf-8` | 同じ |
| `csv` | `text/csv; charset=utf-8` | 同じ |
| `json` | `application/json; charset=utf-8` | 同じ |

## エンドポイント

### `POST /api/pages` — 置く

- 引数（query）: `project`・`key`・`kind`・`role`（要る）、`workspace`（無ければ空）、`n`・`created_at`（取り込みの時だけ）。
- 本文: HTML（1 枚 16 MiB まで。空は駄目）。
- `n` が無ければ、その `project`・`key`・`kind` の「今の最大 ＋ 1」を付ける。最大は、消したページの番号も数える（同じ番号を 2 度使わない）。
- `201`: ページの記録に `url`（要求の `Host` で組んだ URL）を足した JSON。header `Location` も同じ URL。
- `409`: `n` を名指して、その番号が使われている（置いてある・消した）。
- `400`: 引数が決まりに合わない・本文が空。`413`: 本文が大きすぎる。

### `GET /api/pages` — 探す

- 引数（みな任意）: `project`・`key`・`kind`・`role`・`workspace`（同じ物だけ）、`q`（題と本文の字を探す）、`sort`（`-created_at` 新しい順＝既定・`created_at` 古い順）、`limit`（既定 100・1〜1000 に丸める）、`before`・`after`（`created_at` がそれより前・後）。
- `200`: `{"pages":[<ページの記録>, …]}`。合う物が無ければ空の配列。
- `400`: `sort` がどちらでもない。

### `DELETE /api/pages` — 消す

- 引数: `project`（要る）、`key`・`kind`・`n`（任意。`n` を書く時は `key` と `kind` も要る）。合うページをみな消す。
- 本文は残さない（戻せない）。番号は残り、同じ番号は 2 度使わない。
- `200`: `{"removed":["<path>", …]}`。合う物が無ければ空の配列。
- `400`: 引数が決まりに合わない。
- だれが消してよいかは見ない（呼ぶ側が決める）。

### `GET /<project>/<key>/<kind>-<n>.html` — 本文

- `200`: 置いた HTML をそのまま（`Content-Type: text/html; charset=utf-8`、`Content-Security-Policy: sandbox allow-scripts`）。
- `404`: 無い。`410`: 消した。

### `POST /api/files` — file を置く

- 引数（query）: `project`・`key`・`name`（`<語>.<拡張子>`）・`role`（要る）、`workspace`（無ければ空）、`n`・`created_at`（取り込みの時だけ）。
- 本文: file の中身そのもの（1 つ 16 MiB まで。空は駄目）。header `Content-Type` が要る。
- `n` が無ければ、その `project`・`key`・語・拡張子の「今の最大 ＋ 1」を付ける（`screen.png` を置くと `screen-1.png`、もう 1 度置くと `screen-2.png`。前の物は残る）。最大は、消した file の番号も数える。
- `201`: file の記録に `url`（要求の `Host` で組んだ URL）を足した JSON。header `Location` も同じ URL。
- `409`: `n` を名指して、その番号が使われている（置いてある file・消した file）。
- `400`: 引数が決まりに合わない・本文が空・`Content-Type` が無い。`413`: 本文が大きすぎる。
- `415`: 拡張子を受けない、`Content-Type` の種類（`;` の前。大文字・小文字は問わない）が拡張子の物と違う、中身が拡張子の形でない。
- 一覧の UI から上げた物（上の「共通」の例外）は、引数に何が書いてあっても、`role` を `product-manager`、`workspace` を空で記録する（browser から担当を名乗らせない）。`n`・`created_at` も読まない（番号と時刻は置き場が決める）。`role` は書かなくてよい。

### `GET /api/files` — file を探す

- 引数（みな任意）: `project`・`key`・`role`・`workspace`（同じ物だけ）、`q`（名 `<語>.<拡張子>` の字を探す）、`sort`・`limit`・`before`・`after`（`GET /api/pages` と同じ）。
- `200`: `{"files":[<file の記録>, …]}`。合う物が無ければ空の配列。
- `400`: `sort` がどちらでもない。

### `DELETE /api/files` — file を消す

- 引数: `project`（要る）、`key`・`name`（`<語>.<拡張子>`）・`n`（任意。`n` を書く時は `key` と `name` も要る）。合う file をみな消す。
- 中身は残さない（戻せない）。番号は残り、同じ番号は 2 度使わない。ページは消さない（ページを消しても file は消えない）。
- `200`: `{"removed":["<path>", …]}`。合う物が無ければ空の配列。
- `400`: 引数が決まりに合わない。
- だれが消してよいかは見ない（呼ぶ側が決める）。

### `GET /<project>/<key>/files/<語>-<n>.<拡張子>` — file の中身

- `200`: 置いた中身をそのまま。`Content-Type` は上の表の物、`Content-Security-Policy: sandbox`（直に開いても script は動かない）、`X-Content-Type-Options: nosniff`、`Cache-Control: max-age=31536000, immutable`（同じ URL の中身は変わらない）。
- `404`: 無い。`410`: 消した。

### そのほか

| エンドポイント | 中身 | 仕様 |
|---|---|---|
| `GET /healthz` | `200` と `ok` | 入る |
| `GET /api/version` | サーバーの版と API の版 | 入る |
| `GET /api/backup` | DB の写し（SQLite の file 1 つ） | 入る |
| `GET /api/events` | 置かれるたびに 1 件を push（SSE）。ページは名の無い event でページの記録、file は `event: file` で file の記録 | 入る |
| `GET /`・`/<project>/`・`/<project>/<key>/` | 一覧の UI | URL が開くことだけ。HTML の作りは入らない |
