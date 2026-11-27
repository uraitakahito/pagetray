# API の仕様

pagetray を呼ぶ側（`roster page`・[CLI](cli.md) の `pagetray` など）が頼ってよい形を書く。API は Smithy の定義（`model/model/pagetray.smithy`。名前空間 `ai.pagetray`）から作る。protocol は `aws.protocols#restJson1`。

守るのは、データ（ページと file の行・番号・消した番号・メッセージの記録）と、そこから決まる振る舞い（番号は 2 度使わない・消したページは 410・本文は置いたままの byte で返す など）と、ページと file の URL の形。

## 共通の決まり

castlist の API の「[共通の決まり](https://uraitakahito.github.io/castlist/api.html)」と同じ決まりに従う。共通の形（誤り・時刻・ページ送り）は、castlist の定義の `ai.common` を、castlist の release の tag で固めて取る（下の「定義の取り方」）。

| 決まり | 形 |
|---|---|
| 道の頭 | API は `/api/` の下。`/healthz` と、ページと file の本文の URL だけは外 |
| protocol | `aws.protocols#restJson1`。誤りの種類は header `X-Amzn-Errortype` と本文の `message` |
| 欄と引数の名 | camelCase（`createdAt`・`askedAt`・`answeredAt`・`waitsOn`・`actorIssuer`・`actorRef`・`sentAt`・`deliveredAt`・`replyTo`・`maxResults`・`nextToken`） |
| 時刻 | RFC 3339 の UTC（末尾 `Z`。`ai.common#DateTime`）。時差の付いた時刻（`+09:00`）は生成した層が 400 で断る。残す字は UTC・ミリ秒まで・24 字（`2026-11-24T00:00:00.000Z`）にそろえ、字の順が時刻の順になる |
| 誤り | 定義に書いた型だけ: 400 `ValidationException`・404 `NotFound`・409 `Conflict`・410 `Gone`・415 `UnsupportedMediaType`（`ai.pagetray` の型。file の種類を受けない。下の「file を置く」）・500 `InternalError`（server の中の失敗。訳は記録に書き、答えは決まった字。`@retryable` は付けない）・503 `Unavailable`（混んでいる・時間切れ・呼ぶ先の castlist が答えない。`@retryable`）。守りの層が断る 403 `Forbidden`・413 `PayloadTooLarge`・503 `Unavailable` は、service の `errors` に置き、どの操作の誤りにも入る。生成した層の外の道（本文・SSE・backup・logs）の誤りも、同じ形で返す。生成した層が自分で返す物は、知らない道の 404 `UnknownOperationException`（本文は `{}`）・読めない引数の 400 `SerializationException`・違う `Content-Type` の 415 `UnsupportedMediaTypeException` |
| id | 道に入る id は字にする。ページの番号 `n` も、頭に 0 の無い数の字（`"7"`。`^[1-9][0-9]{0,17}$`）で、答えの `n` も字 |
| ページ送り | `@paginated`。`maxResults`（1〜1000。既定 100）と、前の答えの `nextToken` をそのまま渡す |
| 状態の数 | 決まっている物は `@http` の `code`（置けば 201）。答えで替わる物だけ `@httpResponseCode`（`PutMessage` の 201・200） |
| 決まった字の欄 | `waitsOn`・`verdict`・`reply`・メッセージの記録の `state` などは、enum でなく `@pattern` の字で書く（castlist と同じ訳: codegen-server 0.1.40 の作る protocol の試験が、要る enum を空の字から作って落ちる） |
| 知らない欄と引数 | restJson1 は読み飛ばす。重ねられない引数を 2 度付ければ初めの物を使う |
| 版 | 版の header と版だけを返す道は無い。形の版は、service の版（`GET /healthz` の `version`）と、定義の release の tag との `smithy diff`（`ci/gen.sh --diff`。壊す変わりで落ちる）で守る |

呼ぶ側は、知らない欄を読み飛ばし、`message` の文で分けないこと（誤りは状態の数と `X-Amzn-Errortype` で見分ける）。

## 操作

| 操作 | 道 |
|---|---|
| `GetHealth` | `GET /healthz`（`{"status":"ok","version":…}`） |
| `CreatePage` | `POST /api/projects/{project}/keys/{key}/kinds/{kind}/pages`（本文は HTML。引数 `role`・`workspace`・`n`・`createdAt`・`actorIssuer`・`actorRef`） |
| `ListPages` | `GET /api/pages`（引数 `project`・`key`・`kind`・`role`・`workspace`・`q`・`reply`・`on`・`verdict`・`actor`・`sort`・`maxResults`・`nextToken`） |
| `SetReplyState` | `PUT /api/projects/{project}/keys/{key}/kinds/{kind}/pages/{n}/reply`（本文 `{"state":…,"on":…}`） |
| `AddReviewMarks` | `POST /api/projects/{project}/keys/{key}/kinds/{kind}/pages/{n}/review`（本文 `{"verdict":…,"targets":[…]}`） |
| `DeletePages` | `DELETE /api/pages?project&key&kind&n` |
| `CreateFile` | `POST /api/projects/{project}/keys/{key}/files`（本文は file の中身。`Content-Type` が要る。引数 `name`・`role`・`workspace`・`n`・`createdAt`・`actorIssuer`・`actorRef`） |
| `ListFiles` | `GET /api/files`（引数 `project`・`key`・`role`・`workspace`・`q`・`actor`・`sort`・`maxResults`・`nextToken`） |
| `DeleteFiles` | `DELETE /api/files?project&key&name&n` |
| `PutMessage` | `PUT /api/projects/{project}/messages/{id}`（本文は JSON。引数 `actorIssuer`・`actorRef`） |
| `ListMessages` | `GET /api/messages`（引数 `project`・`key`・`to`・`from`・`state`・`heading`・`q`・`sort`・`maxResults`・`nextToken`） |
| `GetMessageFacets` | `GET /api/messages/facets`（引数 `to`・`from`・`key`。1.2.0 から） |
| `GetPageFacets` | `GET /api/pages/facets`（引数なし。1.3.0 から） |
| `ListReviews` | `GET /api/reviews`（引数 `target`。1.3.0 から） |
| `GetFileRules` | `GET /api/files/rules`（引数なし。1.3.0 から） |

手の道（Smithy の外）: ページの本文 `GET /{project}/{key}/{kind}-{n}.html`・file の中身 `GET /{project}/{key}/files/{語}-{n}.{拡張子}`・新着の知らせ `GET /api/events`（SSE）・`GET /api/backup`・`GET /api/logs`。画面の道は無い（2.0.0 で外した。下の「画面」）。

- 置かれたページの URL は切らない。だから本文の URL は Smithy の外の手の道にする（`.html` の付いた 1 段は、Smithy の道の label にできない）。
- `/api/` の下の道は、みな生成した層か、名指した手の道に行く。本文の道（`/{project}/{key}/{file}`）は `/api/…` を受けない（`project` に `api` は置けない）。
- `GET /api/backup`（`crates/server/src/hand.rs`）: DB の隣の一時の file（`<DB>.backup-<pid>-<番号>`）に `VACUUM INTO` で揃ったバックアップを作り、開いてから名を消し（作る・開く・名を消すは別の task でするので、写しを作る間に時間切れで 503 を返しても、client が切っても、一時の file は残らない。止める合図で、写しを作る途中に process が終わった時だけは残り得る）、`application/vnd.sqlite3`（`Content-Disposition: attachment; filename="pages.db"`）で読みながら送る。読む接続で作る（写しを作る間、書く物を待たせない）。誤りは restJson1 の形の 503 `Unavailable`・500 `InternalError`。時間切れは答えの頭まで（写しを作る間）を数え、送る間は数えない（下の「守りの層」）。写しを作るのに 5 秒を過ぎれば 503 `Unavailable`。
- `GET /api/logs`: `text/plain; charset=utf-8` で、終わり方の印と、古い順の記録（中身の形は [動かす](run.md) の「落ちた後に調べる」。行の形は仕様に入らない）。記録の置き場が無い時（Router を直に呼ぶ試験）は 404（形は restJson1 の `NotFound`）。

## ページを置く（CreatePage）

| 欄・引数 | 決まり |
|---|---|
| `project`・`key`（道） | 英数字と `.` `_` `-` で 1〜255 字。`.` と `..` は 400（server）。`project` に `api` は 400（server） |
| `kind`（道）・`role`（引数。要る） | 英小文字で始まり、英小文字・数字・`-` で 1〜32 字。名の一覧は server が持たない（呼ぶ側が決める） |
| `workspace` | 英数字と `:` `.` `_` `-` で 0〜64 字。無ければ空 |
| `n` | 取り込みの時だけ。数の字（`0`・`01`・`abc` は 400） |
| `createdAt` | 取り込みの時だけ。UTC の RFC 3339。ミリ秒より細かい時刻は 400（切って残すと、取り込んだ時刻が替わる）。0001 年から 9999 年まで（0000 年は答えの時刻に書けないので、生成した層が 400） |
| `actorIssuer`・`actorRef` | 送り手の付随する id。両方書くか、両方書かない（片方だけは 400）。`actorIssuer` は英小文字・数字・`-`・`.` で 1〜64 字、`actorRef` の形は `ai.common#ActorRef`（castlist と同じ。`種類:値` で、値は制御字の無い 1〜256 字。空白や `/` も通る）。castlist に聞く所は下の「送り手」 |
| 本文 | `Content-Type: text/html`（`; charset=…` は付いてよい）。ほかの種類と、`Content-Type` の無い要求は、生成した層が 415 `UnsupportedMediaTypeException`。空の本文は、生成した層が 400 `ValidationException` で断る（欄の path は `/body`）。UTF-8 でなくても受ける（題と検索の字は、読めない byte を置き換えて取る） |

- 番号: 書かなければ、同じ `project`・`key`・`kind` の置いた番号と消した番号の最大 + 1（初めは 1）。書く接続は 1 本なので、並べて置いても重ならず抜けない。書けばその番号で置き、置いた番号・消した番号なら 409 `Conflict`（何も替えない）。次に任せた番号は、取り込んだ番号の次になる。
- 答えは 201 で、ページの記録（本文は入らない）と、要求の `Host` で組んだ `url`（header `Location` にも）。置いた時は、返事・判定・見た物の欄は無い。送り手の欄（`sender`）は、付随する id を書いた時だけ。
- 検査の順: 引数の形（400）→ 付随する id の組（400）→ castlist（400・503）→ DB（409・503）。前で断れば、後は見ない（castlist に聞かない・何も残さない）。
- 題は大小を問わない初めの `<title>` の中身（文字参照を戻し、空白を 1 つにまとめる。無ければ空）。検索の索引には、題と、tag・script・style・comment を除いて文字参照を 1 度戻した字を入れる（`crates/server/src/text.rs`）。
- 混んでいて書く接続を 2 秒（`WRITE_WAIT`）待っても取れなければ 503 `Unavailable`（問い合わせの時間切れ 5 秒より前に答える）。
- 本文は 16 MiB まで（超えれば、守りの層が 413 `PayloadTooLarge`。下の「守りの層」）。

## 送り手

付随する id（`actorIssuer`・`actorRef`）を書いて置けば、[castlist](glossary.md)（アクターの本当の id と名を持つ、pagetray を動かす機械の別の container のサーバー）に送り手を聞き、答えと一緒に残す。後から castlist の答えが変わっても、残した物は変えない。

- castlist の居場所は `PAGETRAY_CASTLIST_URL`（聞く先）。`http://<host>:<port>` の形だけ（後ろの `/` はよい。path・利用者の名・`https://`・port の無い物・port 0 は、起きる前に断る）。空か無ければ castlist は無い。起こす binary（`pagetray-server`。下の「起こし方」）が、DB を開く前に `Castlist::from_env` で見て、形が違えば止まる。画面の link の頭 `PAGETRAY_CASTLIST_LINK_URL` は、2.0.0 で画面と一緒に無くした（書いてあっても読まない。形の違う値でも起きる）。
- 聞くのは `ResolveActors`（`POST /api/resolve`、本文 `{"issuer":…,"refs":[…]}`）を 1 度。castlist の定義から作った client（`crates/castlist-client/`）で呼ぶ。書く取引を始める前に聞く（castlist を待つ間、書く接続を握らない）。
- HTTP の client は、`aws-smithy-http-client` の hyper の作りの、TLS の無い HTTP/1.1（`Builder::new().build_http()`。features は `default-client` だけ。`default-client` は証明書を読む crate（`rustls-native-certs`・`rustls-pki-types`）を引くが、TLS の実装（rustls・aws-lc・ring・s2n）は入らない）。接続は使い回さない（`pool_max_idle_per_host(0)`。1 度ごとにつなぐので、castlist が閉じた古い接続で落ちることがない）。
- 待つのは 1 度に 1 秒（つなぐ・送る・答えを読み終えるまでの全部。client の `operation_timeout`）。送り直さない（`RetryConfig::disabled()`。castlist の `Unavailable` は `@retryable` だが、送り直すかは pagetray の呼び手が決める）。
- castlist は `Host` が 127.0.0.1・localhost でない要求と、`Origin` の付いた書く要求を断るので、つなぐ先がどこでも `Host: 127.0.0.1` で送り、`Origin` は付けない。
- 答えの見方: 聞いた `ref` の答えが無い・`asOf` が無い（生成した client は、答えに無い `@required` の欄を既定の値で埋めるので、1970 年の初めの時刻は無い物とする）・アクターの `id` が `ai.common#ActorId` の形でない（答えに入れて返す）・`kind` が `ai`・`human` でない・`name` が 1〜128 字でないか制御字を持つ、のどれかなら読めない答えとする。答えの大きさの上限は持たず、1 秒の時間切れに任せる。

| castlist の答え | pagetray の答え | 残す物 |
|---|---|---|
| 送り手を知っている | 201。`sender` に `issuer`・`ref`・`actor`（`id`・`name`・`kind`）・`resolvedAt`（答えの `asOf`） | ページと、送り手の行（答えた時刻は残す字の 24 字） |
| 送り手を知らない（`actor` が無い） | 400 `ValidationException`（`message` は「castlist に登録の無い送り手（…）」、`fieldList` の path は `/actorRef`） | 何も残さない |
| つながらない・1 秒で答えない・2xx でない・読めない | 503 `Unavailable`（`message` は「castlist に聞けない（訳）。…」。訳は記録にも書く。記録の訳も答えと同じ粗い字（`castlist: つながらない ref=…`）で、つなぎの誤りの細かい字（hyper の誤り）は書かない） | 何も残さない |
| castlist が無い（`PAGETRAY_CASTLIST_URL` が空） | 201。`sender` は `issuer`・`ref` だけ | ページと、答えの欄の空の送り手の行 |

呼び手は、`fieldList` に `/actorRef` のある 400 を「castlist に登録が無い送り手（session が castlist に登録されてから打ち直す）」と見分ける。503 は、少し待ってから送り直す。

- 知らない送り手を 400 にするのは、castlist の `PutEvent` が、引けない `by` を 400 `ValidationException` にするのと同じにするため（誤りの型を増やさない）。
- 送り手の行は足すだけ（trigger が書き換えと消すのを断る）で、置くたびに 1 行（同じ送り手でもまとめない）。ページと同じ取引で足すので、番号が使われていて 409 なら、送り手の行も残らない。
- `sender` は `ai.pagetray#Sender`（`actor` は castlist と同じ `ai.common#ActorSummary`）。server の骨は、答えにだけ使う形の決まり（`ActorSummary` の `id`・`kind` の `@pattern`）にも使わない検査の関数を作るので、`ci/gen.sh` が骨の `lib.rs` の頭に `#![allow(dead_code)]` を足す。

## ページの記録

`ListPages` の行と、`SetReplyState`・`AddReviewMarks` の答えは、同じページの記録（`ai.pagetray#Page`。本文と `url` は入らない）。`CreatePage` の答えの欄（`url`・`Location` のほか）に、返事と印の欄（`PageState`）を足した物。

- `askedAt`（返事待ちにした時刻）・`answeredAt`（答えた時刻）・`waitsOn`（`review`・`tpm`）・`verdict`・`targets`（書いた順）・`sender` は、値のある時だけ。Product Manager を待つページに `waitsOn` は無い（DB でも空）。
- 送り手は、castlist の答えがあれば `actor` と `resolvedAt` も。castlist の無い pagetray が置いたページは `issuer`・`ref` だけ。
- `path` は、この server の中の path（`/<project>/<key>/<kind>-<n>.html`）。`<server の URL><path>` で本文が開く。

## 探す（ListPages）

| 引数 | 決まり |
|---|---|
| `project`・`key`・`kind`・`role`・`workspace` | 同じ値の物。形は置く時と同じ（違えば 400）。`workspace=`（空）も条件で、workspace を書かずに置いたページに当たる |
| `q` | 前後の空白を落とし、空なら絞らない。どの語も 3 字以上なら、語ごとに引用符で囲んだ trigram の全文検索（どの語も含む物。大小を問わない。`"`・`*`・`AND` などはただの字）。3 字に満たない語があれば、`q` 全体を題と本文の字で探す（`instr`。大小を分ける） |
| `reply`・`on` | `reply` は `waiting`（求めて、まだ答えていない）か `answered`（答えた）。`on` はその待ちの相手（`pm`・`review`・`tpm`。既定 `pm`）。`reply` の無い `on` は 400 |
| `verdict` | `approve`・`concerns`・`reject`・`not-assessed` |
| `actor` | 送り手の castlist のアクターの id（`ai.common#ActorId` の形）。castlist の答えの無い送り手のページには当たらない |
| `sort` | `-createdAt`（新しい順。既定）か `createdAt`（古い順）。同じ時刻は置いた順（新しい順なら、後に置いた物が先） |
| `maxResults`・`nextToken` | 1 頁は既定 100。1〜1000 の外は、生成した層が 400 |

- 頁は keyset で続ける: `nextToken` は、検索の種類（`pages`）・`sort`・前の頁の最後の行の `createdAt`（残す字）と id を繋いで 16 進にした物（castlist の頁の印と同じ形）。同じ時刻の行が頁の境に並んでも、抜けず重ならない。辿る間に置いたページ（前の頁より新しい）は、新しい順の続きに出ない。
- `nextToken` は同じ `sort` の答えの物を渡す。崩れた物・ほかの検索の物・`sort` の違う物は 400 `ValidationException`（`message` に `nextToken`）。ほかの条件は印に結ばない（castlist の `ListActors` と同じ）。
- 続きは `nextToken` だけ（時刻の字で切る引数は無い）。
- 読む接続の 1 つの取引で、1 頁より 1 つ多く読み、次の頁があるかを決める。読む接続を 3 秒（`READ_WAIT`）待っても取れなければ 503 `Unavailable`、DB の誤りは 500 `InternalError`。
- SQL は、書いた条件の欠片だけを繋ぐ（`crates/server/src/search.rs`。値は bind する）。`explain query plan` の試験（`crates/server/tests/list.rs`）が、pages を索引なしに端から読まないこと、project・key・送り手・返事待ち・`nextToken` の条件は索引で引くこと、条件の無い一覧と `nextToken` は時刻の索引の順に読む（並べ替えの一時の表を作らない）ことを見る。返事待ちは、返事待ちだけの部分の索引（`pages_waiting`）から引く（条件をじかに書くと、planner は時刻の索引の順に端から読む方を選ぶ）。短い語の検索は、索引の表を端から見る。

## 絞り込みの候補と待ちの数（GetPageFacets）

callboard のページの一覧の画面が、絞り込みの列と操作の帯の待ちの数に使う数（1.3.0 から）。`GET /api/pages/facets` に引数は無く、どの数も**置き場の全部を数える**（絞らない）。件数が 0 の値は出さない。

| 欄 | 中身 |
|---|---|
| `waiting`・`answered` | Product Manager の返事待ち・答えたページの数 |
| `reviewWaiting` | レビュー待ちのページ（レビューの依頼書）の数 |
| `escalated` | TPM 担当に上げて、まだ決まっていないページ（レビューのページ）の数 |
| `senders` | 送り手ごとの数。castlist の答えのある送り手だけ。`value` はアクターの id、`note` はその名。多い順に 100 まで |
| `roles`・`kinds`・`projects`・`verdicts` | 役割・種類・project・レビューの判定ごとの数。件数の多い順（同じなら名の順） |
| `workspaces` | workspace の ID ごとの数（書かずに置いた物は空の値）。新しく置かれた順に 100 まで。`note` は、その ID で置かれた Key（`, ` で繋ぐ） |
| `keys` | Key ごとの数。新しく置かれた順に 100 まで |

- 並びの 1 つは `{"value":…,"count":…}` で、添える字のある並び（`senders`・`workspaces`）だけ `note` が付く（無ければ欄ごと無い）。`GetMessageFacets` の並びと同じ型。
- どの数も、読む接続の 1 つの取引で読む（同じ時点の数）。`ListPages` の答えとは別の時点。
- 誤りは `ListPages` と同じ（引数の誤りは無い）。

## ページを見たレビュー（ListReviews）

名指したページを見たレビューのページ（種類が `review` で、見た物にその path を書いた物）を返す（1.3.0 から）。callboard のページの一覧の画面が、行の下に「レビュー」の link と判定を出すのに使う。

- 引数 `target` に、見られたページの path（`/<project>/<key>/<kind>-<n>.html`）を並べる（`?target=…&target=…`）。1 つから 200 まで。無い・超える物は 400 `ValidationException`。path の形は見ない（置き場に無い path には、何も返さない）。
- 答えの `reviews` の 1 つは `{"target":…,"path":…,"verdict":…}`: 見られたページの path・レビューのページの path・判定（書いていなければ欄ごと無い）。レビューのページの古い順。1 つのレビューが見た物どうしの順は決めない。
- 読む接続・誤りは `ListPages` と同じ。

## 返事（SetReplyState）

ページ 1 枚の返事の状態を書く。`roster ask pm` が返事待ちにし、`roster answer` が答えたにする。callboard のページの一覧の画面は、この状態で印と件数を出す。本文は JSON の `{"state":"waiting"|"answered","on":"pm"|"review"|"tpm"}`（`on` は無ければ `pm`）。答えは 200 で、書いた後のページの記録。

| 今のページ | `waiting`（`on` の相手） | `answered`（`on` の相手） |
|---|---|---|
| 返事を求めていない | 求めた時刻を今にし、待ちの相手を `on` に | `pm` なら、求めた時刻と答えた時刻に同じ今を入れる。ほかの相手は 409 |
| `on` の相手を待っている | 何も替えない（時刻は初めの物） | 答えた時刻を今に（待ちの相手は替えない） |
| ほかの相手を待っている | 409（待ちを黙って移さない） | 409 |
| 答えた（相手が `on`） | 求めた時刻を今にし、答えた時刻を消す | 何も替えない（答えた時刻を保つ） |
| 答えた（相手が `on` でない） | 求めた時刻を今にし、答えた時刻を消し、待ちの相手を `on` に（求め直しは相手を替えてよい） | 409 |

- 409 `Conflict` は何も替えない（`message` は「そのページは、on の相手の返事を待っていない（何も替えていない）」）。
- 無いページも、消したページも 404 `NotFound`（410 でない）。`project`・`key` の `.`・`..` は 400。`state` の無い本文・JSON でない本文は 400。
- 見る・書く・答えの記録を読むは、書く接続の 1 つの取引。書く接続を 2 秒待っても取れなければ 503、DB の誤りは 500（どちらも何も替えない）。時刻は残す字（24 字）。
- だれが書いてよいかは、呼ぶ側（roster）が持つ。browser の中のページからは書けない（守りの層が `Origin` の付いた要求を 403）。

## レビューの印（AddReviewMarks）

ページ 1 枚に、判定と見た物を書く（`roster ask review`・`roster report review` が書く）。本文は JSON の `{"verdict":…,"targets":[…]}`（どちらも書かなくてよく、本文が無くてもよい。何も書かなければ、何も替えずに記録を返す）。答えは 200 で、書いた後のページの記録。

- `verdict` は、書けば替わり、書かなければ前のまま（消す道は無い）。種類が `review` のページだけ（ほかは 400）。
- `targets` は、置き場のページの path（`/<project>/<key>/<kind>-<n>.html`。本文の道と同じ形）か、空白と制御字の無い `https://…`（2048 字まで。byte でなく字で数える）。1 度に 100 個まで。足すだけで、同じ物は 1 つ（1 度の要求の中で重ねても）、書いた順。足すとページ 1 枚で 100 個を超えるなら 400（判定も替えない）。
- 無いページも、消したページも 404。ページを消すと、見た物も消える（外のキーの `on delete cascade`）。
- 本文の知らない欄は読み飛ばす（上の「共通の決まり」の restJson1）。形の違う本文（`targets` が並びでない など）は、生成した層が 400 `SerializationException`。
- 書く接続・誤り・`Origin` は返事と同じ。

## 消す（DeletePages）

`DELETE /api/pages?project=…[&key=…][&kind=…[&n=…]]`。合うページをみな消す（戻せない）。答えは 200 の `{"removed":[path…]}`（置いた順。合う物が無ければ空）。

- `project` は要る（`.`・`..` は 400）。`key`・`kind`・`n` は、書けば絞る。`kind` だけでもよい（project の中の、その種類をみな）。`n` には `key` と `kind` が要る（無ければ 400「n を書くときは key と kind も書く」）。
- 消したページの番号は、消した時刻（残す字）と `removed` に残し、2 度使わない（本文の URL は 410）。全文の索引の行も消える。見た物（`page_targets`）は外のキーで一緒に消える。送り手の行は残る（足すだけの表）。
- 1 つの取引で、番号を残す・索引の行を消す・ページを消す。書く接続を 2 秒待っても取れなければ 503、DB の誤りは 500（どちらも何も消さない）。
- だれが消してよいかは見ない（呼ぶ側が決める）。browser の中のページからは消せない（守りの層が `Origin` の付いた要求を 403）。

## file を置く（CreateFile）

ページから参照する file（画面の絵・PDF・字の file）を 1 つ置く。本文は中身をそのまま。答えは 201 で、file の記録（中身は入らない）と、要求の `Host` で組んだ `url`（header `Location` にも）。

| 欄・引数 | 決まり |
|---|---|
| `project`・`key`（道） | ページと同じ（`.`・`..`・`project` の `api` は 400） |
| `name`（引数。要る） | `<語>.<拡張子>`。語は英小文字か数字で始まり、英小文字・数字・`-`・`_` で 1〜64 字。拡張子は英数字で 1〜16 字。`.` は 1 つ（形の違いは、生成した層が 400） |
| `role` | ページと同じ形。要る（無ければ server が 400「role が要る」。定義では `@required` にしていない） |
| `workspace`・`n`・`createdAt`・`actorIssuer`・`actorRef` | ページと同じ（番号は語と拡張子ごと） |
| `Content-Type`（header。要る） | 拡張子の種類と同じ物（`;` の前を、大小を問わずに比べる。`; charset=…` などは問わない）。無ければ、生成した層が 400 `ValidationException`（欄の path は `/contentType`） |
| 本文 | file の中身（空は、生成した層が 400。欄の path は `/body`）。16 MiB まで |

### 受ける file の種類

`crates/server/src/files.rs` の `FILE_TYPES`。表に無い種類（動画・HTML・SVG など）は受けない。

| 拡張子 | 返す時の `Content-Type` | 中身の見方 |
|---|---|---|
| `png` | `image/png` | 頭が PNG の印 |
| `jpg`・`jpeg` | `image/jpeg` | 頭が `FF D8 FF` |
| `gif` | `image/gif` | 頭が `GIF87a` か `GIF89a` |
| `webp` | `image/webp` | 頭が `RIFF` で、8〜11 byte 目が `WEBP` |
| `pdf` | `application/pdf` | 頭が `%PDF-` |
| `txt`・`log` | `text/plain; charset=utf-8` | UTF-8 で読めて、NUL が無い |
| `md` | `text/markdown; charset=utf-8` | 同上 |
| `csv` | `text/csv; charset=utf-8` | 同上 |
| `json` | `application/json; charset=utf-8` | 同上 |

- 表に無い拡張子（大文字の `PNG` も）・拡張子と合わない `Content-Type`・頭が拡張子の形でない中身は、415 `UnsupportedMediaType`（`message` に訳。受ける拡張子の一覧・`.png の Content-Type は image/png`・`中身が .png の形でない`）。返す時の `Content-Type` は、送った header でなく表の値。
- 番号: 書かなければ、同じ `project`・`key`・語・拡張子の置いた番号と消した番号の最大 + 1（初めは 1。`screen.png` を置くと `screen-1.png`、もう 1 度置くと `screen-2.png`。前の物は残る）。書けばその番号で置き、置いた番号・消した番号なら 409 `Conflict`（何も替えない）。ページの番号とは別に数える。
- `sha256` は中身の SHA-256（16 進の小文字）、`size` は byte の数、`type` は表の `Content-Type`。
- 検査の順: 引数の形（400）→ 引数の決まりと付随する id の組（400）→ 種類・`Content-Type`・中身（415）→ castlist（400・503）→ DB（409・503）。castlist には、ほかの検査を見終えてから聞く。送り手の残し方は、ページと同じ（上の「送り手」）。
- browser の中のページからは置けない（守りの層が `Origin` の付いた要求を 403。2.0.0 で、画面の form から上げる例外を無くした）。画面（callboard）は、Product Manager から受けた file を、`Origin` を付けずに `role=product-manager` で置く。
- 置いた後に、新着の知らせを送る（下の「新着の知らせ」）。

決め:

- 本文の種類: 中身は `@httpPayload` の `blob`（`@mediaType` を付けない）にし、要求の `Content-Type` を `@httpHeader("Content-Type")` の欄 `contentType` に結ぶ。`Content-Type` を欄に結ぶと、smithy-rs の server は `Content-Type` を決まった値（`@mediaType` の無い blob なら `application/octet-stream`）と比べる検査を作らないので、どの種類でも handler まで届き、種類の決まりは handler が見る（S3 の `PutObject` と同じ形）。
- 415 の型: 生成した層の `UnsupportedMediaTypeException`（操作の決めた `Content-Type` でない要求。ページの本文が `text/html` でない時など）とは別に、`ai.pagetray#UnsupportedMediaType`（`@httpError(415)`）を CreateFile の誤りに置く。呼び手は header `X-Amzn-Errortype` で見分ける（`ai.common` には足さない。file の種類は pagetray だけの決まり）。

## file を探す（ListFiles）

| 引数 | 決まり |
|---|---|
| `project`・`key`・`role`・`workspace` | 同じ値の物。形は置く時と同じ（違えば 400）。`workspace=`（空）も条件 |
| `q` | 前後の空白を落とし、空なら絞らない。名（`<語>.<拡張子>`）に、小文字にした `q` を含む物 |
| `actor` | 送り手の castlist のアクターの id。castlist の答えの無い送り手の file には当たらない |
| `sort`・`maxResults`・`nextToken` | `ListPages` と同じ（keyset。印の頭は `files` で、ページの検索の印は 400） |

- 行は file の記録（`ai.pagetray#File`。中身と `url` は入らない）。送り手の欄は、付随する id を書いて置いた時だけ。置いた答えと一覧の行は、`url` のほかは同じ欄・同じ値。
- `path` は、この server の中の path（`/<project>/<key>/files/<語>-<n>.<拡張子>`）。ページの URL（3 段）と違い 4 段で、3 段目が `files` なので、ぶつからない。ページからは、相対の path で参照する（同じ Key なら `files/screen-1.png`、別の Key なら `../K-2/files/screen-1.png`）。
- 読む接続・誤りは `ListPages` と同じ。`explain query plan` の試験（`crates/server/tests/files.rs`）が、files を索引なしに端から読まないこと、project・key・送り手・`nextToken` の条件は索引で引くこと、条件の無い一覧と `nextToken` は時刻の索引（`files_created`）の順に読むことを見る。

## 受ける file の種類（GetFileRules）

`GET /api/files/rules` は、受ける file の種類と、1 つの大きさの上限を返す（1.3.0 から）。callboard のページの一覧の画面の、file を上げる枠が、送る前に見る。

- `types`: `{"ext":…,"type":…}` の並び。拡張子（`.` は付けない）と、その file を返す時の `Content-Type`。上の「受ける file の種類」の表と同じ物で、並びは、置き場が種類を断る時の文と同じ。
- `maxBytes`: file 1 つの大きさの上限（byte。16 MiB）。
- DB は読まない。誤りは、守りの層の物だけ。

## file を消す（DeleteFiles）

`DELETE /api/files?project=…[&key=…][&name=<語>.<拡張子>[&n=…]]`。合う file をみな消す（戻せない）。答えは 200 の `{"removed":[path…]}`（置いた順。合う物が無ければ空）。

- `project` は要る（`.`・`..` は 400）。`key`・`name`・`n` は、書けば絞る。`name` だけでもよい（project の中の、その名をみな）。`n` には `key` と `name` が要る（無ければ 400「n を書くときは key と name も書く」）。`name` の形は file を置く時と同じ（違えば生成した層が 400）。
- 消した file の番号は、消した時刻（残す字）と `files_removed` に残し、2 度使わない（中身の URL は 410）。ページは消えない（ページを消しても file は消えない）。送り手の行は残る。
- 書く接続・誤り・`Origin` はページを消す時と同じ。消した時は、新着の知らせを送らない。

## メッセージの記録（PutMessage・ListMessages）

ほかの道具（roster。AI の担当どうしのメッセージを配る）が、「誰が誰に何を送り、いつ届いたか」の記録を置き場に写す（1.1.0 から）。置き場は**見せるだけ**: メッセージを配らず、時刻を比べて状態を決めず、期限を持たない。状態は、呼ぶ側が書いた字を、そのまま残して返す。消す操作は無い。

### 置く・書き足す（PutMessage）

`PUT /api/projects/{project}/messages/{id}`。同じ project と id に、何度打ってもよい。初めての id なら作って 201、もうある id なら書き足して 200。答えは、書いた後の記録（下の「記録」）。

| 欄・引数 | 決まり |
|---|---|
| `project`（道） | ページと同じ（`.`・`..`・`api` は 400） |
| `id`（道） | 英数字と `.` `_` `-` で 1〜128 字。呼ぶ側が付ける（project の中で 1 つ）。`.`・`..` は 400（server） |
| `actorIssuer`・`actorRef`（引数） | 形はページを置く時と同じ（上の「送り手」）。両方書くか、両方書かない（片方だけは 400）。castlist に聞いて残すのは、「送った側」の書き込みの時だけ（下の「書き足しの決まり」） |
| `key` | 任意。ページの Key と同じ形（`.`・`..` は 400） |
| `heading` | 要る。メッセージの見出しの名。英小文字で始まり、英小文字・数字・`-` で 1〜32 字（名の一覧は server が持たない） |
| `from`・`to` | 要る。送り手・受け手の名。1〜200 字 |
| `text` | 要る。本文。1〜8000 字。字のまま持つ（HTML として解釈しない） |
| `sentAt` | 要る。送った時刻。UTC の RFC 3339 で、ミリ秒まで（細かい時刻は 400） |
| `state` | 要る。下の「状態」のどれか |
| `deliveredAt` | 任意。届いた時刻（`sentAt` と同じ形） |
| `via` | 任意。どの時に渡したかの名（`heading` と同じ形） |
| `deliveries` | 任意。受け手に本文を出した回数（呼ぶ側が数えて書く）。1 以上 |
| `replyTo` | 任意。返事の先のメッセージの id（`id` と同じ形。その記録が在るかは見ない） |

- 字の長さは、byte でなく字（Unicode の符号位置）で数える。長さと形の違いは、生成した層が 400 `ValidationException`（`fieldList` に欄の path）。読めない時刻（時差の付いた物・数）と JSON でない本文は 400 `SerializationException`。
- 検査の順: 引数と本文の形（400）→ `.`・`..`・`api`・時刻の細かさ・付随する id の組（400）→ castlist（400・503。「送った側」の書き込みの時だけ）→ DB（503・500）。前で断れば、後は見ない（castlist に聞かない・何も替えない）。
- 混んでいて書く接続を 2 秒待っても取れなければ 503 `Unavailable`。409 は無い（同じ id は書き足す）。
- だれが置いてよいかは、呼ぶ側が持つ。browser の中のページからは書けない（守りの層が `Origin` の付いた要求を 403）。

### 状態

| `state` | 意味 |
|---|---|
| `waiting` | まだ届いていない |
| `delivered` | 届いた |
| `undeliverable` | 渡せない |
| `expired` | 期限切れ |
| `carried` | ほかの道具で運んだ |

置き場は、時刻を比べて状態を決めない。ただし、書き足しでは、状態は前へだけ進む（下の「書き足しの決まり」）。

### 書き足しの決まり

「送った」の記録は送り手の process が、「渡した」の記録は受け手の process が、別々に・後からまとめて写すことがある。だから「渡した」が「送った」より先に届くことが、ふつうにある。書き足しは、届く順に依らず、同じ記録になるようにする。

| 欄 | もうある id に打った時 |
|---|---|
| `state` | 前へだけ進む。強さは `waiting`＝`carried`（0）＜ `expired`＝`undeliverable`（1）＜ `delivered`（2）で、今より強い時だけ替える（同じ強さ・弱い時は今のまま）。初めて置く時は、来た `state` のまま |
| `deliveries` | 大きい方を保つ。書かなければ前のまま |
| `via` | `state` を替えた時か、今が空の時だけ入れる。書かなければ前のまま（消す道は無い） |
| `deliveredAt` | まだ無い時だけ入れる。もうあれば、違う時刻が来ても初めの時刻を保つ（返事の状態の `askedAt`・`answeredAt` と同じ考え） |
| `key`・`heading`・`from`・`to`・`text`・`sentAt`・`replyTo` | 初めに置いた値を保つ。違う値が来ても変えず、誤りにもしない（形は、書き足す時も見る）。初めに書かなかった `key`・`replyTo` を、後から入れることもしない |
| `sender` | 「送った側」の書き込み（`state` が `waiting` か `carried` の書き込み）の送り手だけを残す。まだ空なら入れ、もう入っていれば替えない |
| `createdAt`・`updatedAt` | `createdAt` は初めて受けた時刻のまま。`updatedAt` は、書き足すたびに今にする（値が替わらなくても） |

- **送り手と castlist**: castlist に聞くのは、「送った側」の書き込み（`waiting`・`carried`）に付随する id を書いた時だけ（置く時も書き足す時も。登録の無い送り手は 400、聞けなければ 503 で、何も替えない）。ほかの `state` の書き込みは、付随する id が付いていても castlist に聞かず、送り手として残さない（castlist が止まっていても、渡した時刻の書き足しは通る）。初めて行を作った書き込みが「渡した側」なら、送り手は空のままで、後から来た「送った側」の書き込みの送り手で埋まる。送り手の行（`senders`）を足すのは、記録の送り手を入れる時だけ。
- 見る・書く・答えの記録を読むは、書く接続の 1 つの取引。

### 記録

`PutMessage` の答えと、`ListMessages` の行と、新着の知らせ（`message-record`）の data は、同じメッセージの記録（`ai.pagetray#Message`）。

- いつもある欄: `project`・`id`・`heading`・`from`・`to`・`text`・`sentAt`・`state`・`createdAt`（置き場が初めて受けた時刻）・`updatedAt`（最後に書き足された時刻）。
- 値のある時だけ: `key`・`deliveredAt`・`via`・`deliveries`・`replyTo`・`sender`（`ai.pagetray#Sender`。ページの送り手と同じ形）。
- 本文（`text`）は、置いたままの字。`url`・`path` は無い（メッセージの記録に、開く URL は無い）。

### 探す（ListMessages）

| 引数 | 決まり |
|---|---|
| `project`・`key`・`to`・`from`・`state`・`heading` | 同じ値の物。形は置く時と同じ（違えば 400） |
| `q` | 前後の空白を落とし、空なら絞らない。本文か `from` か `to` に `q` を含む物（`instr`。大小を分ける。全文の索引は使わない） |
| `sort` | `-sentAt`（送った時刻の新しい順。既定）か `sentAt`（古い順）。同じ時刻は置いた順（新しい順なら、後に置いた物が先） |
| `maxResults`・`nextToken` | `ListPages` と同じ（keyset。印の頭は `messages` で、前の頁の最後の行の `sentAt` と行の id。ほかの検索の印・`sort` の違う印は 400） |

- 並びも頁の続きも、置き場が受けた時刻でなく、呼ぶ側が書いた `sentAt` で決める。辿る間に、前の頁より新しい `sentAt` の記録が置かれても、新しい順の続きには出ない。
- 読む接続・誤りは `ListPages` と同じ。`explain query plan` の試験（`crates/server/tests/messages.rs`）が、messages を索引なしに端から読まないこと、条件の無い一覧と `nextToken` は送った時刻の索引（`messages_sent`）の順に読むことを見る。

### 絞り込みの候補と件数（GetMessageFacets）

callboard のメッセージの一覧の画面が、絞り込みの列と「まだ届いていない n 件」に使う数（1.2.0 から）。`GET /api/messages/facets` の答えは、5 つの並び。どの並びも `{"value":…,"count":…}` の並びで、**置き場の全部を数える**（引数では絞らない）。件数が 0 の値は出さない。

| 欄 | 中身 |
|---|---|
| `states` | 状態ごとの数。`waiting`・`delivered`・`undeliverable`・`expired`・`carried` の順 |
| `to`・`from` | 受け手・送り手の名ごとの数。件数の多い順（同じなら名の順）に 100 まで |
| `keys` | Key ごとの数。新しく送られた順に 100 まで（Key を書かずに置いた記録は数えない） |
| `projects` | project ごとの数。件数の多い順（同じなら名の順） |

- 引数 `to`・`from`・`key` は、画面が今絞っている値。絞らない。書いた値が、その並びの 100 件に入っていなければ、その値と件数（置き場に無ければ 0）を並びの後ろに足す（画面が、その値を候補に出して、押して外せるように）。形は `ListMessages` の同じ名の引数と同じ（違えば 400）。
- 5 つの並びは、読む接続の 1 つの取引で読む（同じ時点の数）。`ListMessages` の答えとは別の時点なので、間に記録が置かれると、数と行が 1 件ずれることがある。
- 誤りは `ListMessages` と同じ。

## 新着の知らせ（SSE）

`GET /api/events`（手の道。`text/event-stream`）。聞き始めた後に、置いた・書いた物の記録を 1 件ずつ流す（`crates/server/src/events.rs`）。

| 起きた事 | event の名 | data |
|---|---|---|
| ページを置いた（`CreatePage`） | 無し（前からの受け手は、これだけを読む） | ページの記録（`ListPages` の行と同じ。`url`・`Location` は入らない） |
| file を置いた（`CreateFile`） | `file` | file の記録（`ListFiles` の行と同じ） |
| 返事の状態を書いた（`SetReplyState`。替わらなかった時も） | `reply` | 書いた後のページの記録（答えと同じ） |
| レビューの印を書いた（`AddReviewMarks`。替わらなかった時も） | `review` | 書いた後のページの記録（答えと同じ） |
| メッセージの記録を置いた・書き足した（`PutMessage`。替わらなかった時も） | `message-record` | 書いた後のメッセージの記録（答えと同じ。`ListMessages` の行と同じ） |

- 断った書き込み（400・404・409 など）と、消した時（`DeletePages`・`DeleteFiles`）は送らない。答えの記録に直せてから送る（直せずに 500 を返す書き込みは知らせない）。
- 記録の形は API の答えと同じ camelCase（`createdAt`・`askedAt`・`waitsOn`・`sender.resolvedAt` など。時刻は答えと同じ字）。
- メッセージの記録の event の名は `message-record`（`message` は、`EventSource` の既定の event の型と同じ名なので使わない）。
- メッセージの記録の知らせは、ページの知らせと同じ流れ（聞き手ごとに 64 件）を使う（channel は分けない）。記録をまとめて写し直した時は、遅い聞き手が、ページの新着の知らせを落としうる（画面は、一覧を読み直せば出る）。
- broadcast の channel（容量 64）。聞き手が無くても失敗にしない。遅れて取りこぼした聞き手は、捨てた分を飛ばして続ける（画面は一覧を読み直せば出る）。keep-alive は axum の既定（15 秒ごとの注釈の行）。
- 守りの層は、ほかの道と同じ（`Host` を見る。時間切れは答えの頭までなので、流し続ける本文には掛からない）。

## 画面

pagetray は画面を持たない（2.0.0 で外した）。ページの一覧とメッセージの一覧は、callboard（castlist と pagetray の画面を出すサーバー。0.4.0 から）が、この API を通して出す（`http://127.0.0.1:8803/pages/`・`/messages`）。

- 前の画面の道（`/`・`/{project}/`・`/{project}/{key}/`・`/messages`。引数が付いていても）は 404 で、画面（HTML）を返さない。callboard へ送る redirect も無い（古い URL は閉じる）。
- 置いたページの本文と file の中身の URL は、前のまま開く（下の「ページの本文」「file の中身」）。

## 起こし方

起こす binary は `pagetray-server`（`crates/server/src/main.rs`）。image（`Containerfile`）に `/pagetray-server` の名で入る（[release を出す](release.md)）。

- 引数は付けない。引数を付けると、使い方を出して 2 で終わる。
- 環境変数: `PAGETRAY_DB`（既定 `/data/pages.db`）・`PAGETRAY_LISTEN`（既定 `0.0.0.0:8080`）・`PAGETRAY_CASTLIST_URL`（上の「送り手」）。
- 起きずに 1 で終わる物: castlist の設定の形が違う時（stderr に訳を書き、DB も記録の置き場も作らない）と、DB を開けない時（後の版の DB を含む。下の「データ」。記録の置き場に訳を残す）。
- 記録の置き場は DB の隣の `logs/`（大きさで回す記録の file・終わり方の印・生きている印。[動かす](run.md) の「落ちた後に調べる」）。起きて初めに前の回の終わり方の行（`previous: …`）、待ち受けたら起きた行（`pagetray <版> listen=… db=…`。castlist を設定すれば後ろに ` castlist=…`）。
- 問い合わせごとの 1 行（method・uri・status・latency）は記録の file にだけ書き、stdout には出さない（下の「守りの層」）。
- SIGTERM と SIGINT で新しい問い合わせを断り、新着の知らせの流れを閉じ、走っている物を終えてから閉じる（印は `signal` と `stopped`）。知らせの流れは聞き手が切るまで続くので、閉じないと止まらない。走っている物を待つのは 3 秒（`SHUTDOWN_WAIT`。問い合わせの時間切れ 5 秒と、container を止める命令が SIGKILL を送るまでの待ちより短い）まで。過ぎたら記録に `stop wait exceeded wait=3s` を書き、DB を閉じて 0 で終わる（印は `stopped`。固まって切られた時と見分けられる）。ただし backup の写しを作っている間に合図が来ると、DB を閉じるのは写しを作り終えるまで待つ（その分は上限に入らない）。
- 記録の書き手は `crates/server/src/runlog.rs`。
- 試験: 本物の binary を空いた port で起こす物は `crates/server/tests/server.rs`（守りの層・backup・止め方・起きた行）と `crates/server/tests/runlog.rs`（前の回の終わり方・問い合わせの行・logs・上限で回る）。道具は `crates/server/tests/common/server.rs`。backup と logs の中身は `crates/server/tests/backup.rs`（Router を直に呼ぶ）。

## ページの本文

`GET /{project}/{key}/{kind}-{n}.html`（手の道）。

- 形の合わない URL（`kind` が名の形でない・`n` が頭に 0 のある数や 0・`.html` でない）は 404 `NotFound`。
- 置いたページは 200 で、置いたままの byte。`Content-Type: text/html; charset=utf-8`・`Content-Security-Policy: sandbox allow-scripts`（隔てた枠として出す。ページの中の script は動くが、置き場の名で API を呼べない）・`X-Content-Type-Options: nosniff`。
- 無いページは 404 `NotFound`、消したページは 410 `Gone`（`message` に消した時刻）。

## file の中身

`GET /{project}/{key}/files/{語}-{n}.{拡張子}`（手の道）。

- 形の合わない URL（語が名の形でない・受けない拡張子（大文字も）・`n` が頭に 0 のある数や 0）は 404 `NotFound`。
- 置いた file は 200 で、置いたままの byte。`Content-Type` は置いた時に表で決めた物・`Content-Security-Policy: sandbox`（直に開いても中の script を動かさない）・`X-Content-Type-Options: nosniff`・`Cache-Control: max-age=31536000, immutable`（同じ URL の中身は替わらない。上げ直すと次の番号）。
- 無い file は 404 `NotFound`、消した file は 410 `Gone`（`message` に消した時刻）。読む接続を待ちきれなければ 503、DB の誤りは 500。
- ページの本文の道と段の数が違うので、種類 `files` のページ（`/{project}/{key}/files-1.html`）とぶつからない。

## 守りの層

生成した道と手の道の前に、外から順に掛ける（`crates/server/src/guard.rs`。castlist と同じ作り）。断る答えは restJson1 の形。

| 順 | 守り | 振る舞い |
|---|---|---|
| 1 | 頭の印 | どの答えにも `X-Content-Type-Options: nosniff` と `Referrer-Policy: no-referrer`。CSP は、本文の道が自分で付ける（上の「ページの本文」「file の中身」） |
| 2 | 問い合わせの記録 | 1 つにつき 1 行（method・uri・status・latency。tower-http の TraceLayer）。記録の file にだけ書き、stdout には出さない。断った物も残る |
| 3 | Host と Origin | `Host`（port を外して）が 127.0.0.1・localhost でなければ、`GET`・`HEAD` のほかの要求に `Origin` が付いていれば、403 `Forbidden`。例外は無い（`Origin` が置き場自身の `http://<Host>` でも、`Origin: null` でも 403。画面の form が file を上げる要求だけを通す例外は、2.0.0 で画面と一緒に無くした） |
| 3 | HEAD | `GET` にして通し、本文を捨てる（生成した骨の道は GET だけなので、ここで揃える） |
| 4 | 時間切れ | 答えの頭まで 5 秒（`REQUEST_TIMEOUT`）を過ぎれば 503 `Unavailable`（待っている物は捨てる）。本文を送る間は数えないので、大きな backup の file や新着の知らせの流れを途中で切らない。試験は `Config` で短くする。接続を待つ長さ（書く `WRITE_WAIT` 2 秒・読む `READ_WAIT` 3 秒）は、これより短くする（混んでいる時に「混んでいる」の 503 が先に答える。`crates/server/tests/guard.rs` で止めている） |
| 5 | 本文の上限 | 16 MiB（`MAX_BODY`。ページも file も）。`Content-Length` で超えると言う要求は読まずに、大きさを言わずに流す要求は 16 MiB まで読んで、413 `PayloadTooLarge`（何も置かない）。ちょうど 16 MiB は通る。生成した層は本文をみな読んでから引数にするので、ここで読んでおく（smithy-rs の生成した層にも本文の上限の設定があるが、既定は上限無しで、超えた時の答えが 413 にならない）。本文を読む間も、外の層の 5 秒の時間切れに数える（送り手は同じ機械の中の roster・CLI・callboard なので、16 MiB でも 5 秒よりずっと短く届く。遅い送り手に書く接続を握らせない利点もある） |

## データ

- DB は SQLite で、表は `crates/server/migrations/` の SQL で作る（[DB の作り](db.md)）。
- 接続は castlist と同じ: 書く接続は 1 本（`WRITE_WAIT` 2 秒まで待つ）、読む接続は読むだけの 4 本（`READ_WAIT` 3 秒）。どの接続も WAL・`foreign_keys`・`recursive_triggers`・`busy_timeout` 5 秒・一時の物は memory。開く時に移行を当て、`foreign_key_check` に崩れがあれば開かない。
- 0.8.x の DB は、v1.0.0 の image の `migrate` で新しい volume へ移した（AI-36 の切り替え。手順と照らしは [release を出す](release.md) の「切り替え（済み）」）。
- `_sqlx_migrations` に、`crates/server/migrations/` に無い版か、中身の写しの違う版がある DB は、開かない（sqlx の移行が、当てる前に突き合わせて断る）。移行は 1 つも当てないが、記録の置き場（`logs/`）と WAL の file（`-wal`・`-shm`）は書く。無い版がこの server の最も新しい版より後なら、後の版（移行を足した版）が書いた DB として、「この server より新しい版の DB。…書いた版の image で起こす」の訳で断る。ほかは「DB の移行に落ちた: …」。binary は 1 で終わる。試験は `crates/server/tests/migrations.rs`（出した migrations の中身が 1 byte も替わっていないことも見る）。

## 定義の取り方

- pagetray の定義は `model/model/pagetray.smithy`（名前空間 `ai.pagetray`）。共通の形 `ai.common` は castlist の `model/model/common.smithy` を使う。
- `ci/gen.sh` が、castlist の release の tag（今は `v1.0.0`）の `common.smithy` と `castlist.smithy` を `model/castlist/` に取り、`ci/gen.sh` に固めた SHA-256 と合うかを見てから置く。tag と 2 つの digest は `ci/gen.sh` の 1 か所に書く。取った定義は commit するので、`ci/gen.sh --check` は castlist に取りに行かず、入れてある物が digest と合うかを見る。
- 1 つの Smithy の build から、3 つの crate を作る: pagetray の server の骨（`crates/pagetray-sdk/`。smithy-rs の codegen-server 0.1.40・`http-1x`・`publicConstrainedTypes: false`）と、castlist を呼ぶ crate（`crates/castlist-client/`。codegen-client 0.1.40 の `rust-client-codegen`。http 1 の形）と、pagetray を呼ぶ crate（`crates/pagetray-client/`。同じ `rust-client-codegen` を、`model/smithy-build.json` の `rust-client-codegen::pagetray-client`（`<plugin>::<名>`）の名でもう 1 度使う）。castlist を呼ぶ crate は送り手を castlist に聞く処理（上の「送り手」）で、pagetray を呼ぶ crate は人が打つ CLI（[CLI](cli.md)）で使う。
- 呼ぶ側の 2 つの crate の既定の features から、https の client（rustls・aws-lc）を外す（`ci/gen.sh` が `Cargo.toml` の 1 行を替える）。pagetray が castlist を呼ぶのも、CLI が pagetray を呼ぶのも同じ機械の中の `http://` だけなので、TLS の crate を build しない。HTTP の client は、使う側が設定で渡す（TLS の無い HTTP/1.1。上の「送り手」）。castlist を 1 度呼べること・1 秒の時間切れ・送り直さないことは、`crates/server/tests/senders.rs` が偽の castlist で止める。
- 例（`@httpRequestTests`・`@httpResponseTests`）の id は、castlist の定義と 1 つの model に入るので重なれない。pagetray の物は頭に `Pagetray` を付ける。
