# API の仕様（API の版 1）

pagetray を呼ぶ側（`roster page` など）が頼ってよい形を書く。ここに書いた形を、前からある呼ぶ側が困るように変える時は、API の版を上げる。

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
- 誤りの答えは `{"error":"<人が読む文>"}`。ただし、axum が自分で返す誤りはこの形でない: 引数を読めない（`limit=abc` など）`400` は平文（`text/plain`）、道の無い path の `404` と、道にない method の `405`（`Allow` に受ける method）は本文が空。
- 字の決まり:

| 引数 | 決まり |
|---|---|
| `project`・`key` | 英数字と `.` `_` `-` で 1〜255 字。`.` と `..` は駄目。`project` に `api` は使えない |
| `kind`・`role` | 英小文字で始まり、英小文字・数字・`-` で 1〜32 字。名の一覧はサーバーが持たない（呼ぶ側が決める） |
| `workspace` | 英数字と `:` `.` `_` `-` で 0〜64 字 |
| `name`（file） | `<語>.<拡張子>`。語は英小文字・数字・`-` `_` で 1〜64 字（英小文字か数字で始まる）。拡張子は下の「受ける file の種類」の物だけ |
| `n` | 1 以上の整数 |
| `created_at` | `2026-01-02T03:04:05Z` か、秒の小数（3 桁まで）が付いた形。UTC だけ |
| `actor_issuer` | 英小文字・数字・`-` `.` で 1〜64 字（castlist の名の形） |
| `actor_ref` | `<種類>:<値>`。種類は `actor_issuer` と同じ形、値は英数字と `:` `.` `_` `-` で 1〜256 字（初めの `:` で分けるので、値に `:` があってよい） |

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

返事の状態を書いたページ（下の `POST /api/pages/reply`）の記録には、次の欄も出る（0.5.0 から。書いていないページには出ない）。

| 欄 | 中身 |
|---|---|
| `asked_at` | 返事待ちにした時刻 |
| `answered_at` | 答えた時刻。返事待ちの間は出ない |
| `waits_on` | 誰の返事を待つか（`review`・`tpm`）。Product Manager を待つページには出ない（0.8.0 から） |

レビューの印を書いたページ（下の `POST /api/pages/review`）の記録には、次の欄も出る（0.8.0 から。書いていないページには出ない）。

| 欄 | 中身 |
|---|---|
| `verdict` | レビューの判定（`approve`・`concerns`・`reject`・`not-assessed`）。`kind` が `review` のページだけ |
| `targets` | 見た物・見てほしい物の配列（書いた順）。置き場のページの path か、`https://` の URL |

送り手を記録したページ（下の「送り手」）の記録には、`sender` の欄も出る（0.6.0 から。記録していないページには出ない）。

## 送り手

ページと file を置く時に、呼ぶ側は、置く人を指す付随する id（`actor_issuer`・`actor_ref`）を書ける（0.6.0 から。書かなくてよい）。置き場は、受けたその時に [castlist](glossary.md)（アクターの本当の id と名を持つ、pagetray を動かす機械の別の container のサーバー。聞く先は `PAGETRAY_CASTLIST_URL`）に送り手を聞き、答えを残す。後から castlist の答えが変わっても、残した物は変えない。

```json
"sender": {
  "issuer": "tpm",
  "ref": "claude.session:5f0c2a8e-…",
  "actor": { "id": "019a…", "name": "t-23zs.swe.1", "kind": "ai" },
  "resolved_at": "2026-11-22T04:00:02.000Z"
}
```

| 欄 | 中身 |
|---|---|
| `issuer`・`ref` | 置く時に書いた `actor_issuer`・`actor_ref` |
| `actor` | castlist が答えたアクター（id・名・種類）。置き場に castlist の URL が無かった時は `null`（未確認） |
| `resolved_at` | castlist が答えた時刻（castlist の答えの `as_of`）。聞かなかった時は出ない |

置き場が castlist に聞くかどうかと、答えで、置く要求の答えが決まる。

| 場合 | 答え |
|---|---|
| `actor_issuer`・`actor_ref` が無い | 前と同じに置く（`sender` は出ない） |
| 置き場に castlist の URL（`PAGETRAY_CASTLIST_URL`）が無い | 置く。`sender.actor` は `null` |
| castlist がアクターを答えた | 置く。`sender.actor` にその答え |
| castlist が `actor: null`（登録の無い送り手）を答えた | `422`。何も置かない |
| castlist につながらない・1 秒で答えない・`200` でない・答えが読めない | `503`。何も置かない。少し待ってから送り直す |

- 片方だけ書いた・形が違う時は `400`（castlist には聞かない）。
- castlist には `POST /v1/resolve` で、`{"issuer":"<actor_issuer>","refs":["<actor_ref>"]}` を送る。`Host: 127.0.0.1` で送り、`Origin` は付けない（castlist がほかの物を断る）。
- castlist を待つ間は、書く取引を始めない（ほかの置く要求を待たせない）。

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

- 引数（query）: `project`・`key`・`kind`・`role`（要る）、`workspace`（無ければ空）、`n`・`created_at`（取り込みの時だけ）、`actor_issuer`・`actor_ref`（送り手。両方書くか、両方書かない。0.6.0 から）。
- 本文: HTML（1 枚 16 MiB まで。空は駄目）。
- `n` が無ければ、その `project`・`key`・`kind` の「今の最大 ＋ 1」を付ける。最大は、消したページの番号も数える（同じ番号を 2 度使わない）。
- `201`: ページの記録に `url`（要求の `Host` で組んだ URL）を足した JSON。header `Location` も同じ URL。
- `409`: `n` を名指して、その番号が使われている（置いてある・消した）。
- `400`: 引数が決まりに合わない・本文が空・`actor_issuer` と `actor_ref` の片方だけ。`413`: 本文が大きすぎる。
- `422`: castlist に登録の無い送り手。`503`: castlist に聞けない（上の「送り手」）。どちらも何も置かない。

### `GET /api/pages` — 探す

- 引数（みな任意）: `project`・`key`・`kind`・`role`・`workspace`（同じ物だけ）、`q`（題と本文の字を探す）、`sort`（`-created_at` 新しい順＝既定・`created_at` 古い順）、`limit`（既定 100・1〜1000 に丸める）、`before`・`after`（`created_at` がそれより前・後）、`reply`（返事の状態。`waiting` は返事待ち、`answered` は答えた物だけ。0.5.0 から）、`on`（`reply` で絞る時の待ちの相手。`pm`＝既定・`review`・`tpm`。0.8.0 から）、`verdict`（レビューの判定。0.8.0 から）、`actor`（castlist のアクターの id。その送り手が置いた物だけ。未確認の送り手は当たらない。0.6.0 から）。
- `200`: `{"pages":[<ページの記録>, …]}`。合う物が無ければ空の配列。
- `400`: `sort` がどちらでもない・`reply` がどちらでもない・`on` か `verdict` が決まった名でない・`reply` の無い `on`。
- `on` を書かない `reply` は、今までどおり Product Manager の返事だけを見る（レビューの待ちと上げた件は入らない）。

### `POST /api/pages/reply` — 返事の状態を書く

Product Manager の返事が要るページに、返事待ち・答えたの状態を書く（0.5.0 から）。`roster ask pm` が返事待ちにし、`roster answer` が答えたにする。一覧の UI は、この状態で印と件数を出す。

- 引数（query）: `project`・`key`・`kind`・`n`（ページ 1 枚を名指す）、`state`（`waiting` 返事待ち・`answered` 答えた）は要る。`on`（待ちの相手。`pm` Product Manager＝既定・`review` レビューの席・`tpm` TPM 担当。0.8.0 から）は任意。本文は読まない。
- `waiting`: `on` の相手の返事待ちにする。もう同じ相手の返事待ちなら何も変えない。今ほかの相手の返事待ちなら、何も変えずに `409`（待ちを黙って移さない）。答えた後にもう 1 度書くと、`asked_at` が今になり、`answered_at` が消える（相手も替えてよい）。
- `answered`: 答えたにする。もう答えていれば何も変えない。`on` の相手と、そのページの待ちの相手が違えば、何も変えずに `409`。
  - `on` が `pm`（書かない時）: 返事待ちにしていないページも答えたにでき、`asked_at` にも同じ時刻が入る（0.5.0 からの形）。
  - `on` が `review`・`tpm`: その相手の返事待ちにしたページだけ。返事待ちにしていないページは `409`（Product Manager の「答えた」に入れないため）。
- `200`: そのページの記録（`asked_at`・`answered_at` と、Product Manager のほかを待つ物は `waits_on` が付く）。同じ状態を 2 度書いても同じ答え。
- `404`: そのページは無い（消した物も）。`409`: 待ちの相手が違う（何も変えていない）。`400`: 引数が決まりに合わない。
- だれが書いてよいかは見ない（呼ぶ側が決める）。browser の中のページからは書けない（「共通」の `Origin` の決まり）。
- 0.5.0 より前のサーバーには、このエンドポイントが無い（`404` か `405` が返る）。呼ぶ側は、それを「状態を書けない置き場」として扱う。
- 0.8.0 より前のサーバーは、知らない `on` を読まずに捨て、Product Manager の返事待ちにしてしまう。`on` を書く呼ぶ側は、先に `GET /api/version` で 0.8.0 以上かを見る。

### `POST /api/pages/review` — レビューの印を書く

レビューのページに判定と見た物を、レビューの依頼書に見てほしい物を書く（0.8.0 から）。`roster ask review`・`roster report review` が書く。一覧の UI は、判定の印・見た物の link・見られたページの「レビュー:」を出す。

- 引数（query。みな要る）: `project`・`key`・`kind`・`n`（ページ 1 枚を名指す）。
- 本文（JSON。無くてもよい）: `{"verdict": "<判定>", "targets": ["<見た物>", …]}`。どちらの欄も任意で、ほかの欄は受けない。
  - `verdict`: `approve`・`concerns`・`reject`・`not-assessed` のどれか。`kind` が `review` のページだけに書ける。書けば替わり、書かなければ前の物のまま。
  - `targets`: 置き場のページの path（`/<project>/<key>/<kind>-<n>.html`。`http://…` の URL のままは受けない）か、`https://` で始まる URL（2048 字まで。空白と制御の字は駄目）。足すだけで、同じ物は 1 つ。ページ 1 枚に 100 個まで（足すと超える要求は、判定も含めて何も変えずに `400`）。外す道は無い。ページを消すと一緒に消える。
- `200`: そのページの記録（`verdict`・`targets` が付く）。
- `404`: そのページは無い。`400`: 引数・本文が決まりに合わない。
- だれが書いてよいかは見ない（呼ぶ側が決める）。browser の中のページからは書けない（「共通」の `Origin` の決まり）。
- 0.8.0 より前のサーバーには、このエンドポイントが無い（`405` が返る）。

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

- 引数（query）: `project`・`key`・`name`（`<語>.<拡張子>`）・`role`（要る）、`workspace`（無ければ空）、`n`・`created_at`（取り込みの時だけ）、`actor_issuer`・`actor_ref`（送り手。`POST /api/pages` と同じ。0.6.0 から）。
- 本文: file の中身そのもの（1 つ 16 MiB まで。空は駄目）。header `Content-Type` が要る。
- `n` が無ければ、その `project`・`key`・語・拡張子の「今の最大 ＋ 1」を付ける（`screen.png` を置くと `screen-1.png`、もう 1 度置くと `screen-2.png`。前の物は残る）。最大は、消した file の番号も数える。
- `201`: file の記録に `url`（要求の `Host` で組んだ URL）を足した JSON。header `Location` も同じ URL。
- `409`: `n` を名指して、その番号が使われている（置いてある file・消した file）。
- `400`: 引数が決まりに合わない・本文が空・`Content-Type` が無い・`actor_issuer` と `actor_ref` の片方だけ。`413`: 本文が大きすぎる。
- `422`・`503`: 送り手を castlist に聞いた答え（上の「送り手」）。castlist には、ほかの決まり（`400`・`415`）を見終えてから聞く。
- `415`: 拡張子を受けない、`Content-Type` の種類（`;` の前。大文字・小文字は問わない）が拡張子の物と違う、中身が拡張子の形でない。
- 一覧の UI から上げた物（上の「共通」の例外）は、引数に何が書いてあっても、`role` を `product-manager`、`workspace` を空で記録する（browser から担当を名乗らせない）。`n`・`created_at`・`actor_issuer`・`actor_ref` も読まない（番号と時刻は置き場が決め、送り手は記録しない）。`role` は書かなくてよい。

### `GET /api/files` — file を探す

- 引数（みな任意）: `project`・`key`・`role`・`workspace`（同じ物だけ）、`q`（名 `<語>.<拡張子>` の字を探す）、`actor`・`sort`・`limit`・`before`・`after`（`GET /api/pages` と同じ）。
- file の記録にも、送り手を記録した物には `sender` が出る（0.6.0 から）。
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
| `GET /api/logs` | 残した記録（終わり方の印と、時刻つきの行）を、古い順の字で返す（0.7.0 から。`text/plain`。多くて 4 MiB と 64 KiB）。中身の形は [動かす](run.md) の「落ちた後に調べる」 | URL が開くことと、字で返ることだけ。行の形は入らない |
| `GET /api/events` | 置かれるたび・返事の状態が書かれるたびに 1 件を push（SSE）。ページは名の無い event でページの記録、file は `event: file` で file の記録、返事の状態は `event: reply` でページの記録（0.5.0 から）、レビューの印は `event: review` でページの記録（0.8.0 から） | 入る |
| `GET /`・`/<project>/`・`/<project>/<key>/` | 一覧の UI | URL が開くことだけ。HTML の作りは入らない |
