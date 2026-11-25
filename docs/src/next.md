# 新しい API の形（作り直しの間の設計）

AI-36 の作り直しで、pagetray の API を Smithy の定義（`model/`）から作る。この章は、新しい server（`crates/server`）が従う形を先に決めておく物。切り替えの後は、[API の仕様](api.md) の章をこの形に書き直し、この章は消す。

今の API との互換は守らない（Product Manager の決め: AI-36）。守るのは、データ（ページと file の行・番号・消した番号）と、そこから決まる振る舞い（番号は 2 度使わない・消したページは 410・本文は置いたままの byte で返す など）と、ページと file の URL の形。

## 共通の決まり（castlist と同じ）

castlist の [新しい API の形](https://github.com/uraitakahito/castlist/blob/main/docs/src/next.md) の「共通の決まり」と同じ決まりに従う。共通の形（誤り・時刻・ページ送り）は、castlist の定義の `ai.common` を、castlist の release の tag で固めて取る（下の「定義の取り方」）。

| 決まり | 形 |
|---|---|
| 道の頭 | API は `/api/` の下。`/healthz` と、ページと file の本文の URL と、画面だけは外 |
| protocol | `aws.protocols#restJson1`。誤りの種類は header `X-Amzn-Errortype` と本文の `message`（今の server の `{"error":…}` はやめる） |
| 欄と引数の名 | camelCase（`createdAt`・`askedAt`・`answeredAt`・`waitsOn`・`actorIssuer`・`actorRef`・`maxResults`・`nextToken`） |
| 時刻 | RFC 3339 の UTC（末尾 `Z`。`ai.common#DateTime`）。時差の付いた時刻（`+09:00`）は生成した層が 400 で断る。残す字は UTC・ミリ秒まで・24 字（`2026-11-24T00:00:00.000Z`）にそろえ、字の順が時刻の順になる |
| 誤り | 定義に書いた型だけ: 400 `ValidationException`・404 `NotFound`・409 `Conflict`・410 `Gone`・415 `UnsupportedMediaType`（`ai.pagetray` の型。file の種類を受けない。下の「file を置く」）・500 `InternalError`（server の中の失敗。訳は記録に書き、答えは決まった字。`@retryable` は付けない）・503 `Unavailable`（混んでいる・時間切れ・呼ぶ先の castlist が答えない。`@retryable`）。守りの層が断る 403 `Forbidden`・413 `PayloadTooLarge`・503 `Unavailable` は、service の `errors` に置き、どの操作の誤りにも入る。生成した層の外の道（本文・SSE・backup・logs）の誤りも、同じ形で返す。生成した層が自分で返す物は、知らない道の 404 `UnknownOperationException`（本文は `{}`）・読めない引数の 400 `SerializationException`・違う `Content-Type` の 415 `UnsupportedMediaTypeException` |
| id | 道に入る id は字にする。ページの番号 `n` も、頭に 0 の無い数の字（`"7"`。`^[1-9][0-9]{0,17}$`）で、答えの `n` も字 |
| ページ送り | `@paginated`。`maxResults`（1〜1000。既定 100）と、前の答えの `nextToken` をそのまま渡す |
| 状態の数 | 決まっている物は `@http` の `code`（置けば 201）。答えで替わる物だけ `@httpResponseCode` |
| 決まった字の欄 | `waitsOn`・`verdict`・`reply` などは、enum でなく `@pattern` の字で書く（castlist と同じ訳: codegen-server 0.1.40 の作る protocol の試験が、要る enum を空の字から作って落ちる） |
| 知らない欄と引数 | restJson1 は読み飛ばす。重ねられない引数を 2 度付ければ初めの物を使う |
| 版 | 今の header `Pagetray-Api` と `GET /api/version` は無くす。形の版は、service の版（`GET /healthz` の `version`）と、定義の release の tag との `smithy diff`（`ci/gen.sh --diff`。壊す変わりで落ちる）で守る |

## 操作

| 操作 | 道 | 今の道 |
|---|---|---|
| `GetHealth` | `GET /healthz`（`{"status":"ok","version":…}`） | `GET /healthz`（字の `ok`）・`GET /api/version` |
| `CreatePage` | `POST /api/projects/{project}/keys/{key}/kinds/{kind}/pages`（本文は HTML。引数 `role`・`workspace`・`n`・`createdAt`・`actorIssuer`・`actorRef`） | `POST /api/pages?project&key&kind&role…` |
| `ListPages` | `GET /api/pages`（引数 `project`・`key`・`kind`・`role`・`workspace`・`q`・`reply`・`on`・`verdict`・`actor`・`sort`・`maxResults`・`nextToken`） | `GET /api/pages` |
| `SetReplyState` | `PUT /api/projects/{project}/keys/{key}/kinds/{kind}/pages/{n}/reply`（本文 `{"state":…,"on":…}`） | `POST /api/pages/reply` |
| `AddReviewMarks` | `POST /api/projects/{project}/keys/{key}/kinds/{kind}/pages/{n}/review`（本文 `{"verdict":…,"targets":[…]}`） | `POST /api/pages/review` |
| `DeletePages` | `DELETE /api/pages?project&key&kind&n` | `DELETE /api/pages` |
| `CreateFile` | `POST /api/projects/{project}/keys/{key}/files`（本文は file の中身。`Content-Type` が要る。引数 `name`・`role`・`workspace`・`n`・`createdAt`・`actorIssuer`・`actorRef`） | `POST /api/files?project&key&name…` |
| `ListFiles` | `GET /api/files`（引数 `project`・`key`・`role`・`workspace`・`q`・`actor`・`sort`・`maxResults`・`nextToken`） | `GET /api/files` |
| `DeleteFiles` | `DELETE /api/files?project&key&name&n` | `DELETE /api/files` |

手の道（Smithy の外）: ページの本文 `GET /{project}/{key}/{kind}-{n}.html`・file の中身 `GET /{project}/{key}/files/{語}-{n}.{拡張子}`（どちらも今と同じ URL）・新着の知らせ `GET /api/events`（SSE）・`GET /api/backup`・`GET /api/logs`・画面（`/`・`/{project}/`・`/{project}/{key}/`）。

- 置かれたページの URL は切らない。だから本文の URL は今のまま、Smithy の外の手の道にする（`.html` の付いた 1 段は、Smithy の道の label にできない）。
- `/api/` の下の道は、みな生成した層か、名指した手の道に行く。本文の道（`/{project}/{key}/{file}`）は `/api/…` を受けない（`project` に `api` は置けない）。
- この章の操作と手の道は、みな作ってある。
- `GET /api/backup`（`crates/server/src/hand.rs`）: 今の server と同じ。DB の隣の一時の file（`<DB>.backup-<pid>-<番号>`）に `VACUUM INTO` で揃ったバックアップを作り、開いてから名を消し（作る・開く・名を消すは別の task でするので、写しを作る間に時間切れで 503 を返しても、client が切っても、一時の file は残らない。止める合図で、写しを作る途中に process が終わった時だけは残り得る）、`application/vnd.sqlite3`（`Content-Disposition: attachment; filename="pages.db"`）で読みながら送る。読む接続で作る（今の server は書く接続。写しを作る間、書く物を待たせない）。誤りは restJson1 の形の 503 `Unavailable`・500 `InternalError`。時間切れは答えの頭まで（写しを作る間）を数え、送る間は数えない（下の「守りの層」）。写しを作るのに 5 秒を過ぎれば 503 `Unavailable`。
- `GET /api/logs`: 今の server と同じ字（`text/plain; charset=utf-8`。印と、古い順の記録）。記録の置き場が無い時（Router を直に呼ぶ試験）は、今の server と同じく 404（形は restJson1 の `NotFound`）。

## ページを置く（CreatePage）

| 欄・引数 | 決まり |
|---|---|
| `project`・`key`（道） | 英数字と `.` `_` `-` で 1〜255 字。`.` と `..` は 400（server）。`project` に `api` は 400（server） |
| `kind`（道）・`role`（引数。要る） | 英小文字で始まり、英小文字・数字・`-` で 1〜32 字 |
| `workspace` | 英数字と `:` `.` `_` `-` で 0〜64 字。無ければ空 |
| `n` | 取り込みの時だけ。数の字（`0`・`01`・`abc` は 400） |
| `createdAt` | 取り込みの時だけ。UTC の RFC 3339。ミリ秒より細かい時刻は 400（切って残すと、取り込んだ時刻が替わる）。0001 年から 9999 年まで（0000 年は答えの時刻に書けないので、生成した層が 400） |
| `actorIssuer`・`actorRef` | 送り手の付随する id。両方書くか、両方書かない（片方だけは 400）。`actorIssuer` は英小文字・数字・`-`・`.` で 1〜64 字、`actorRef` の形は `ai.common#ActorRef`（castlist と同じ。`種類:値` で、値は制御字の無い 1〜256 字。今の pagetray の形より広く、空白や `/` も通る）。castlist に聞く所は下の「送り手」 |
| 本文 | `Content-Type: text/html`（`; charset=…` は付いてよい）。ほかの種類と、`Content-Type` の無い要求は、生成した層が 415 `UnsupportedMediaTypeException`（今の server は見ていなかった）。空の本文は、生成した層が 400 `ValidationException` で断る（欄の path は `/body`）。UTF-8 でなくても受ける（題と検索の字は、読めない byte を置き換えて取る） |

- 番号: 書かなければ、同じ `project`・`key`・`kind` の置いた番号と消した番号の最大 + 1（初めは 1）。書く接続は 1 本なので、並べて置いても重ならず抜けない。書けばその番号で置き、置いた番号・消した番号なら 409 `Conflict`（何も替えない）。次に任せた番号は、取り込んだ番号の次になる。
- 答えは 201 で、ページの記録（本文は入らない）と、要求の `Host` で組んだ `url`（header `Location` にも）。置いた時は、返事・判定・見た物の欄は無い。送り手の欄（`sender`）は、付随する id を書いた時だけ。
- 検査の順: 引数の形（400）→ 付随する id の組（400）→ castlist（400・503）→ DB（409・503）。前で断れば、後は見ない（castlist に聞かない・何も残さない）。
- 題は大小を問わない初めの `<title>` の中身（文字参照を戻し、空白を 1 つにまとめる。無ければ空）。検索の索引には、題と、tag・script・style・comment を除いて文字参照を 1 度戻した字を入れる（今の `src/text.rs` と同じ中身の `crates/server/src/text.rs`）。
- 混んでいて書く接続を 2 秒（`WRITE_WAIT`）待っても取れなければ 503 `Unavailable`（問い合わせの時間切れ 5 秒より前に答える）。
- 本文は 16 MiB まで（超えれば、守りの層が 413 `PayloadTooLarge`。下の「守りの層」）。

## 送り手

付随する id（`actorIssuer`・`actorRef`）を書いて置けば、castlist に送り手を聞き、答えと一緒に残す（今の pagetray と同じ振る舞いを、新しい castlist の API で）。

- castlist の居場所は `PAGETRAY_CASTLIST_URL`（聞く先）。`http://<host>:<port>` の形だけ（後ろの `/` はよい。path・利用者の名・`https://`・port の無い物・port 0 は、起きる前に断る）。空か無ければ castlist は無い。画面の link の頭 `PAGETRAY_CASTLIST_LINK_URL` は別に決め（container の中から届く所と、host の browser から開く所は違う）、形は同じ、無いか空なら `http://127.0.0.1:8802`。形の違う link は、castlist の URL が無くても断る。起こす binary（`pagetray-next`。下の「起こし方」）が、DB を開く前に `Castlist::from_env` で見て、形が違えば止まる。link を使うのは画面の送り手の名（下の「画面」）。
- 聞くのは `ResolveActors`（`POST /api/resolve`、本文 `{"issuer":…,"refs":[…]}`）を 1 度。castlist の定義から作った client（`crates/castlist-client/`）で呼ぶ。書く取引を始める前に聞く（castlist を待つ間、書く接続を握らない）。
- HTTP の client は、`aws-smithy-http-client` の hyper の作りの、TLS の無い HTTP/1.1（`Builder::new().build_http()`。features は `default-client` だけ。`default-client` は証明書を読む crate（`rustls-native-certs`・`rustls-pki-types`）を引くが、TLS の実装（rustls・aws-lc・ring・s2n）は入らない）。接続は使い回さない（`pool_max_idle_per_host(0)`。1 度ごとにつなぐので、castlist が閉じた古い接続で落ちることがない）。
- 待つのは 1 度に 1 秒（つなぐ・送る・答えを読み終えるまでの全部。client の `operation_timeout`）。送り直さない（`RetryConfig::disabled()`。castlist の `Unavailable` は `@retryable` だが、送り直すかは pagetray の呼び手が決める）。
- castlist は `Host` が 127.0.0.1・localhost でない要求と、`Origin` の付いた書く要求を断るので、つなぐ先がどこでも `Host: 127.0.0.1` で送り、`Origin` は付けない。
- 答えの見方: 聞いた `ref` の答えが無い・`asOf` が無い（生成した client は、答えに無い `@required` の欄を既定の値で埋めるので、1970 年の初めの時刻は無い物とする）・アクターの `id` が `ai.common#ActorId` の形でない（画面の link の path に入る）・`kind` が `ai`・`human` でない・`name` が 1〜128 字でないか制御字を持つ、のどれかなら読めない答えとする。今の pagetray の、答えの大きさの上限（64 KiB）は、1 秒の時間切れに任せて持たない。

| castlist の答え | pagetray の答え | 残す物 |
|---|---|---|
| 送り手を知っている | 201。`sender` に `issuer`・`ref`・`actor`（`id`・`name`・`kind`）・`resolvedAt`（答えの `asOf`） | ページと、送り手の行（答えた時刻は残す字の 24 字） |
| 送り手を知らない（`actor` が無い） | 400 `ValidationException`（`message` は「castlist に登録の無い送り手（…）」、`fieldList` の path は `/actorRef`） | 何も残さない |
| つながらない・1 秒で答えない・2xx でない・読めない | 503 `Unavailable`（`message` は「castlist に聞けない（訳）。…」。訳は記録にも書く。記録の訳も答えと同じ粗い字（`castlist: つながらない ref=…`）で、今の server と違い、つなぎの誤りの細かい字（hyper の誤り）は書かない） | 何も残さない |
| castlist が無い（`PAGETRAY_CASTLIST_URL` が空） | 201。`sender` は `issuer`・`ref` だけ | ページと、答えの欄の空の送り手の行 |

呼び手は、`fieldList` に `/actorRef` のある 400 を「castlist に登録が無い送り手（session が castlist に登録されてから打ち直す）」と見分ける（今の 422 の替わり。roster は段 6c で、これを試験にする）。

- 知らない送り手を 400 にするのは、castlist の `PutEvent` が、引けない `by` を 400 `ValidationException` にするのと同じにするため（今の pagetray の 422 はやめる。誤りの型を増やさない）。
- 送り手の行は足すだけ（trigger が書き換えと消すのを断る）で、置くたびに 1 行（同じ送り手でもまとめない）。ページと同じ取引で足すので、番号が使われていて 409 なら、送り手の行も残らない。
- `sender` は `ai.pagetray#Sender`（`actor` は castlist と同じ `ai.common#ActorSummary`）。server の骨は、答えにだけ使う形の決まり（`ActorSummary` の `id`・`kind` の `@pattern`）にも使わない検査の関数を作るので、`ci/gen.sh` が骨の `lib.rs` の頭に `#![allow(dead_code)]` を足す。

## ページの記録

`ListPages` の行と、`SetReplyState`・`AddReviewMarks` の答えは、同じページの記録（`ai.pagetray#Page`。本文と `url` は入らない）。`CreatePage` の答えの欄（`url`・`Location` のほか）に、返事と印の欄（`PageState`）を足した物。

- `askedAt`（返事待ちにした時刻）・`answeredAt`（答えた時刻）・`waitsOn`（`review`・`tpm`）・`verdict`・`targets`（書いた順）・`sender` は、値のある時だけ。Product Manager を待つページに `waitsOn` は無い（DB でも空）。
- 送り手は、castlist の答えがあれば `actor` と `resolvedAt` も。castlist の無い pagetray が置いたページは `issuer`・`ref` だけ。

## 探す（ListPages）

| 引数 | 決まり |
|---|---|
| `project`・`key`・`kind`・`role`・`workspace` | 同じ値の物。形は置く時と同じ（違えば 400）。`workspace=`（空）も条件で、workspace を書かずに置いたページに当たる |
| `q` | 前後の空白を落とし、空なら絞らない。どの語も 3 字以上なら、語ごとに引用符で囲んだ trigram の全文検索（どの語も含む物。大小を問わない。`"`・`*`・`AND` などはただの字）。3 字に満たない語があれば、`q` 全体を題と本文の字で探す（`instr`。大小を分ける） |
| `reply`・`on` | `reply` は `waiting`（求めて、まだ答えていない）か `answered`（答えた）。`on` はその待ちの相手（`pm`・`review`・`tpm`。既定 `pm`）。`reply` の無い `on` は 400 |
| `verdict` | `approve`・`concerns`・`reject`・`not-assessed` |
| `actor` | 送り手の castlist のアクターの id（`ai.common#ActorId` の形）。castlist の答えの無い送り手のページには当たらない |
| `sort` | `-createdAt`（新しい順。既定）か `createdAt`（古い順）。同じ時刻は置いた順（新しい順なら、後に置いた物が先） |
| `maxResults`・`nextToken` | 1 頁は既定 100。1〜1000 の外は、生成した層が 400（今の server は丸めていた） |

- 頁は keyset で続ける: `nextToken` は、検索の種類（`pages`）・`sort`・前の頁の最後の行の `createdAt`（残す字）と id を繋いで 16 進にした物（castlist の頁の印と同じ形）。同じ時刻の行が頁の境に並んでも、抜けず重ならない。辿る間に置いたページ（前の頁より新しい）は、新しい順の続きに出ない。
- `nextToken` は同じ `sort` の答えの物を渡す。崩れた物・ほかの検索の物・`sort` の違う物は 400 `ValidationException`（`message` に `nextToken`）。ほかの条件は印に結ばない（castlist の `ListActors` と同じ）。
- 今の `before`・`after`（時刻の字で切る）は無くす。続きは `nextToken` だけ。
- 読む接続の 1 つの取引で、1 頁より 1 つ多く読み、次の頁があるかを決める。読む接続を 3 秒（`READ_WAIT`）待っても取れなければ 503 `Unavailable`、DB の誤りは 500 `InternalError`。
- SQL は、書いた条件の欠片だけを繋ぐ（`crates/server/src/search.rs`。値は bind する）。`explain query plan` の試験（`crates/server/tests/list.rs`）が、pages を索引なしに端から読まないこと、project・key・送り手・返事待ち・`nextToken` の条件は索引で引くこと、条件の無い一覧と `nextToken` は時刻の索引の順に読む（並べ替えの一時の表を作らない）ことを見る。返事待ちは、返事待ちだけの部分の索引（`pages_waiting`）から引く（条件をじかに書くと、planner は時刻の索引の順に端から読む方を選ぶ）。短い語の検索は、今の server と同じく索引の表を端から見る。

## 返事（SetReplyState）

ページ 1 枚の返事の状態を書く。本文は JSON の `{"state":"waiting"|"answered","on":"pm"|"review"|"tpm"}`（`on` は無ければ `pm`）。答えは 200 で、書いた後のページの記録。状態の移りは今の server と同じ。

| 今のページ | `waiting`（`on` の相手） | `answered`（`on` の相手） |
|---|---|---|
| 返事を求めていない | 求めた時刻を今にし、待ちの相手を `on` に | `pm` なら、求めた時刻と答えた時刻に同じ今を入れる。ほかの相手は 409 |
| `on` の相手を待っている | 何も替えない（時刻は初めの物） | 答えた時刻を今に（待ちの相手は替えない） |
| ほかの相手を待っている | 409（待ちを黙って移さない） | 409 |
| 答えた（相手が `on`） | 求めた時刻を今にし、答えた時刻を消す | 何も替えない（答えた時刻を保つ） |
| 答えた（相手が `on` でない） | 求めた時刻を今にし、答えた時刻を消し、待ちの相手を `on` に（求め直しは相手を替えてよい） | 409 |

- 409 `Conflict` は何も替えない（`message` は「そのページは、on の相手の返事を待っていない（何も替えていない）」）。
- 無いページも、消したページも 404 `NotFound`（410 でない。今の server と同じ）。`project`・`key` の `.`・`..` は 400。`state` の無い本文・JSON でない本文は 400。
- 見る・書く・答えの記録を読むは、書く接続の 1 つの取引。書く接続を 2 秒待っても取れなければ 503、DB の誤りは 500（どちらも何も替えない）。時刻は残す字（24 字）。
- だれが書いてよいかは、呼ぶ側（roster）が持つ。browser の中のページからは書けない（守りの層が `Origin` の付いた要求を 403）。

## レビューの印（AddReviewMarks）

ページ 1 枚に、判定と見た物を書く。本文は JSON の `{"verdict":…,"targets":[…]}`（どちらも書かなくてよく、本文が無くてもよい。何も書かなければ、何も替えずに記録を返す）。答えは 200 で、書いた後のページの記録。

- `verdict` は、書けば替わり、書かなければ前のまま（消す道は無い）。種類が `review` のページだけ（ほかは 400）。
- `targets` は、置き場のページの path（`/<project>/<key>/<kind>-<n>.html`。本文の道と同じ形）か、空白と制御字の無い `https://…`（2048 字まで。byte でなく字で数える）。1 度に 100 個まで。足すだけで、同じ物は 1 つ（1 度の要求の中で重ねても）、書いた順。足すとページ 1 枚で 100 個を超えるなら 400（判定も替えない）。
- 無いページも、消したページも 404。ページを消すと、見た物も消える（外のキーの `on delete cascade`）。
- 今の server と違い、本文の知らない欄は 400 にせず読み飛ばす（上の「共通の決まり」の restJson1）。形の違う本文（`targets` が並びでない など）は、生成した層が 400 `SerializationException`。
- 書く接続・誤り・`Origin` は返事と同じ。

## 消す（DeletePages）

`DELETE /api/pages?project=…[&key=…][&kind=…[&n=…]]`。合うページをみな消す（戻せない）。答えは 200 の `{"removed":[path…]}`（置いた順。合う物が無ければ空）。

- `project` は要る（`.`・`..` は 400）。`key`・`kind`・`n` は、書けば絞る。`kind` だけでもよい（project の中の、その種類をみな）。`n` には `key` と `kind` が要る（無ければ 400「n を書くときは key と kind も書く」）。
- 消したページの番号は、消した時刻（残す字）と `removed` に残し、2 度使わない（本文の URL は 410）。全文の索引の行も消える。見た物（`page_targets`）は外のキーで一緒に消える。送り手の行は残る（足すだけの表）。
- 1 つの取引で、番号を残す・索引の行を消す・ページを消す。書く接続を 2 秒待っても取れなければ 503、DB の誤りは 500（どちらも何も消さない）。
- browser の中のページからは消せない（UI と同じ `Origin` でも 403）。

## file を置く（CreateFile）

ページから参照する file（画面の絵・PDF・字の file）を 1 つ置く。本文は中身をそのまま。答えは 201 で、file の記録（中身は入らない）と、要求の `Host` で組んだ `url`（header `Location` にも）。

| 欄・引数 | 決まり |
|---|---|
| `project`・`key`（道） | ページと同じ（`.`・`..`・`project` の `api` は 400） |
| `name`（引数。要る） | `<語>.<拡張子>`。語は英小文字か数字で始まり、英小文字・数字・`-`・`_` で 1〜64 字。拡張子は英数字で 1〜16 字。`.` は 1 つ（形の違いは、生成した層が 400） |
| `role` | ページと同じ形。一覧の UI から上げる時のほかは要る（無ければ 400「role が要る」）。定義では `@required` にしない（一覧の UI は書かない） |
| `workspace`・`n`・`createdAt`・`actorIssuer`・`actorRef` | ページと同じ（番号は語と拡張子ごと） |
| `Content-Type`（header。要る） | 拡張子の種類と同じ物（`;` の前を、大小を問わずに比べる。`; charset=…` などは問わない）。無ければ、生成した層が 400 `ValidationException`（欄の path は `/contentType`） |
| 本文 | file の中身（空は、生成した層が 400。欄の path は `/body`）。16 MiB まで |

受ける種類（今の server と同じ。`crates/server/src/files.rs` の `FILE_TYPES`）:

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
- 番号: 書かなければ、同じ `project`・`key`・語・拡張子の置いた番号と消した番号の最大 + 1（初めは 1）。書けばその番号で置き、置いた番号・消した番号なら 409 `Conflict`（何も替えない）。ページの番号とは別に数える。
- `sha256` は中身の SHA-256（16 進の小文字）、`size` は byte の数、`type` は表の `Content-Type`。
- 検査の順: 引数の形（400）→ 引数の決まりと付随する id の組（400）→ 種類・`Content-Type`・中身（415）→ castlist（400・503）→ DB（409・503）。castlist には、ほかの検査を見終えてから聞く。送り手の残し方は、ページと同じ（上の「送り手」）。
- 一覧の UI の form から上げた物（守りの層が `Origin` の例外として通し、印を付けた物。下の「守りの層」）は、引数に何が書いてあっても、役割 `product-manager`・workspace 空で残し、`n`・`createdAt`・`actorIssuer`・`actorRef` を読まない（browser から担当を名乗らせない・取り込ませない。送り手の行も作らない）。引数の形の違いは、例外でも生成した層が 400 で断る（今の server は、例外の時に読まない引数の形を見なかった）。
- 置いた後に、新着の知らせを送る（下の「新着の知らせ」）。

決め:

- 本文の種類: 中身は `@httpPayload` の `blob`（`@mediaType` を付けない）にし、要求の `Content-Type` を `@httpHeader("Content-Type")` の欄 `contentType` に結ぶ。`Content-Type` を欄に結ぶと、smithy-rs の server は `Content-Type` を決まった値（`@mediaType` の無い blob なら `application/octet-stream`）と比べる検査を作らないので、どの種類でも handler まで届き、種類の決まりは handler が見る（S3 の `PutObject` と同じ形）。
- 415 の型: 生成した層の `UnsupportedMediaTypeException`（操作の決めた `Content-Type` でない要求。ページの本文が `text/html` でない時など）とは別に、`ai.pagetray#UnsupportedMediaType`（`@httpError(415)`）を CreateFile の誤りに置く。呼び手は header `X-Amzn-Errortype` で見分ける（`ai.common` には足さない。file の種類は pagetray だけの決まり）。
- 一覧の UI の印: 守りの層が要求の extensions に `FromUi` を入れ、handler は smithy-rs の `Extension` の取り出し（`Option<Extension<FromUi>>`）で読む（`Host` を読む `RequestHost` と同じ道）。

## file を探す（ListFiles）

| 引数 | 決まり |
|---|---|
| `project`・`key`・`role`・`workspace` | 同じ値の物。形は置く時と同じ（違えば 400）。`workspace=`（空）も条件 |
| `q` | 前後の空白を落とし、空なら絞らない。名（`<語>.<拡張子>`）に、小文字にした `q` を含む物（今の server と同じ） |
| `actor` | 送り手の castlist のアクターの id。castlist の答えの無い送り手の file には当たらない |
| `sort`・`maxResults`・`nextToken` | `ListPages` と同じ（keyset。印の頭は `files` で、ページの検索の印は 400） |

- 行は file の記録（`ai.pagetray#File`。中身と `url` は入らない）。送り手の欄は、付随する id を書いて置いた時だけ。置いた答えと一覧の行は、`url` のほかは同じ欄・同じ値。
- 今の `limit`・`before`・`after` は無くす（`maxResults`・`nextToken`）。
- 読む接続・誤りは `ListPages` と同じ。`explain query plan` の試験（`crates/server/tests/files.rs`）が、files を索引なしに端から読まないこと、project・key・送り手・`nextToken` の条件は索引で引くこと、条件の無い一覧と `nextToken` は時刻の索引（`files_created`）の順に読むことを見る。

## file を消す（DeleteFiles）

`DELETE /api/files?project=…[&key=…][&name=<語>.<拡張子>[&n=…]]`。合う file をみな消す（戻せない）。答えは 200 の `{"removed":[path…]}`（置いた順。合う物が無ければ空）。

- `project` は要る（`.`・`..` は 400）。`key`・`name`・`n` は、書けば絞る。`name` だけでもよい（project の中の、その名をみな）。`n` には `key` と `name` が要る（無ければ 400「n を書くときは key と name も書く」）。`name` の形は file を置く時と同じ（違えば生成した層が 400）。
- 消した file の番号は、消した時刻（残す字）と `files_removed` に残し、2 度使わない（中身の URL は 410）。ページは消えない（ページを消しても file は消えない）。送り手の行は残る。
- 書く接続・誤り・`Origin`（一覧の UI と同じ `Origin` でも 403）はページを消す時と同じ。消した時は、新着の知らせを送らない。

## 新着の知らせ（SSE）

`GET /api/events`（手の道。`text/event-stream`）。聞き始めた後に、置いた・書いた物の記録を 1 件ずつ流す（`crates/server/src/events.rs`）。

| 起きた事 | event の名 | data |
|---|---|---|
| ページを置いた（`CreatePage`） | 無し（前からの受け手は、これだけを読む） | ページの記録（`ListPages` の行と同じ。`url`・`Location` は入らない） |
| file を置いた（`CreateFile`） | `file` | file の記録（`ListFiles` の行と同じ） |
| 返事の状態を書いた（`SetReplyState`。替わらなかった時も） | `reply` | 書いた後のページの記録（答えと同じ） |
| レビューの印を書いた（`AddReviewMarks`。替わらなかった時も） | `review` | 書いた後のページの記録（答えと同じ） |

- 断った書き込み（400・404・409 など）と、消した時（`DeletePages`・`DeleteFiles`）は送らない。答えの記録に直せてから送る（直せずに 500 を返す書き込みは知らせない）。
- 記録の形は API の答えと同じ camelCase（`createdAt`・`askedAt`・`waitsOn`・`sender.resolvedAt` など。時刻は答えと同じ字）。今の server は snake_case の記録を送っていた（替えた所。画面の script が読むのは `path`・`project`・`key` だけで、名は同じ）。
- broadcast の channel（容量 64。今の server と同じ）。聞き手が無くても失敗にしない。遅れて取りこぼした聞き手は、捨てた分を飛ばして続ける（画面は一覧を読み直せば出る）。keep-alive は axum の既定（15 秒ごとの注釈の行）。
- 守りの層は、ほかの道と同じ（`Host` を見る。時間切れは答えの頭までなので、流し続ける本文には掛からない）。

## 画面

一覧の UI（手の道。`/`・`/{project}/`・`/{project}/{key}/`。`crates/server/src/ui.rs`）。今の server の画面を、同じ URL・同じ引数・同じ字で、新しい DB の所で組む。

- template は、repo の `templates/index.html` を今の server と共に使う（askama。`crates/server/askama.toml` が `../../templates` を指す。castlist と同じ形）。違うのは上げる枠の送り先だけで、置き場が `data-upload` に渡す（今の server は `/api/files?project=…&key=…`、新しい server は `/api/projects/{project}/keys/{key}/files`。script が `name` の引数を足し、選んだ file の `Content-Type` で送る）。断った訳は、新しい server の `message` と今の server の `error` のどちらも読む。
- 引数（今の server と同じ）: `project`・`key`・`kind`・`role`・`workspace`・`q`・`reply`・`on`・`verdict`・`actor`・`sort`・`at`・`open`。空は書かないのと同じ（`workspace` だけは空も条件）。知らない `reply`・`verdict` は無視、`on` は `reply` と組の時だけ、`sort` は `created_at` だけ、`open` はページの path の形だけ。1 度に 200 件。同じ名の引数を 2 度書いた物など、読めない引数は 400 `ValidationException`。
- 続きの印 `at` は、前の頁の最後の行の時刻と id（`nextToken` と同じ形）。今の server は時刻だけで、同じ時刻の行が頁の境に並ぶと抜けた（下の「直す今の振る舞い」）。読めない印・ほかの並びの印は、書かないのと同じ（頭から出す）。
- 待ちの数（返事待ち・レビュー待ち・上げた）と絞り込みの候補は、絞りに関わらず置き場の全部。tab の題は Product Manager の返事待ちの数（`crates/server/src/facets.rs`。SQL は今の server と同じ）。
- 送り手は、castlist の名に castlist のアクターの画面への link（`<PAGETRAY_CASTLIST_LINK_URL>/actors/<id>`。castlist を設定していて、答えのある送り手だけ）と、その下に小さく付随する id。答えの無い送り手は「未確認」。絞り込みの「送り手」は、答えのある送り手を、ページの多い順に 100 まで。
- 字はみな HTML として逃がし、URL の引数に入れる値は、英数字と `-` `.` `_` `~` のほかを %XX にする（`actorRef` は castlist と同じ広い形で、空白・`?`・`&`・`#`・`/` も通る）。
- Key の画面（project と Key を 1 つずつ選んだ時）だけ、file の枠と上げる枠を出す（project が `api` なら出さない）。本文の枠の iframe は `sandbox="allow-scripts"`。
- 答えの頭: 守りの層の `nosniff`・`no-referrer` に足して、`Content-Security-Policy: default-src 'none'; script-src 'sha256-…'; style-src 'sha256-…'; img-src 'self' data: blob:; connect-src 'self'; frame-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'`。inline の style と script は template の物の hash だけを許す（hash は template から作る。今の server は CSP を付けていなかった）。図は置き場の file と、上げる前の見本（`blob:`）と favicon（`data:`）、本文の枠は置き場のページ、知らせと上げる枠の送り先は置き場だけ。

## 起こし方

起こす binary は `pagetray-next`（`crates/server/src/main.rs`）。今の `pagetray-server` と同じ振る舞いで、今の image（`Containerfile`）には入らない。rc の image（`Containerfile.next`）には、`/pagetray-server` の名で入る（[release を出す](release.md) の rc）。

- 引数は付けない。`pagetray-next migrate --from <写し> --to <新しい DB>` は起きずに今の DB を移す（下の「データ」）。ほかの引数は、使い方を出して 2 で終わる。
- 環境変数は今と同じ: `PAGETRAY_DB`（既定 `/data/pages.db`）・`PAGETRAY_LISTEN`（既定 `0.0.0.0:8080`）・`PAGETRAY_CASTLIST_URL`・`PAGETRAY_CASTLIST_LINK_URL`（上の「送り手」）。
- 起きずに 1 で終わる物: castlist の設定の形が違う時と、今の server の DB の時（下の「データ」）。どちらも stderr に訳を書き、DB も記録の置き場も作らない（今の volume に何も書かない）。今の server は、castlist の設定の誤りを記録の置き場に書いてから止まっていた。
- 記録の置き場は DB の隣の `logs/`（大きさで回す記録の file・終わり方の印・生きている印。[動かす](run.md) の「落ちた後に調べる」）。起きて初めに前の回の終わり方の行（`previous: …`）、待ち受けたら起きた行（`pagetray <版> listen=… db=…`。castlist を設定すれば後ろに ` castlist=… castlist_link=…`）。どれも今の server と同じ字。
- 問い合わせごとの 1 行（method・uri・status・latency）は記録の file にだけ書き、stdout には出さない（下の「守りの層」）。
- SIGTERM と SIGINT で新しい問い合わせを断り、新着の知らせの流れを閉じ、走っている物を終えてから閉じる（印は `signal` と `stopped`）。今の server は、合図で待ち受けをすぐに捨てていた（走っている物を待たない）。知らせの流れは聞き手が切るまで続くので、閉じないと止まらない。走っている物を待つのは 3 秒（`SHUTDOWN_WAIT`。問い合わせの時間切れ 5 秒と、container を止める命令が SIGKILL を送るまでの待ちより短い）まで。過ぎたら記録に `stop wait exceeded wait=3s` を書き、DB を閉じて 0 で終わる（印は `stopped`。固まって切られた時と見分けられる）。ただし backup の写しを作っている間に合図が来ると、DB を閉じるのは写しを作り終えるまで待つ（その分は上限に入らない）。
- 記録の書き手（`crates/server/src/runlog.rs`）は今の `src/runlog.rs` の写しで、段 9 まで 2 つを同じ中身に保つ（違うのは、hook の試験が見る file の場所の字だけ）。
- 試験: 本物の binary を空いた port で起こす物は `crates/server/tests/server.rs`（守りの層・backup・止め方・起きた行・今の server の DB を断る）と `crates/server/tests/runlog.rs`（前の回の終わり方・問い合わせの行・logs・上限で回る）。道具は `crates/server/tests/common/server.rs`。backup と logs の中身は `crates/server/tests/backup.rs`（Router を直に呼ぶ）。
- e2e の試験（`e2e/`）が、今の server と同じ試験をこの binary にも流す（`PAGETRAY_E2E_SERVER=../target/debug/pagetray-next`。`e2e/tests/wire.ts` が、置く・書く API の道と引数の名を合わせる）。

## ページの本文

`GET /{project}/{key}/{kind}-{n}.html`（手の道。今と同じ）。

- 形の合わない URL（`kind` が名の形でない・`n` が頭に 0 のある数や 0・`.html` でない）は 404 `NotFound`。
- 置いたページは 200 で、置いたままの byte。`Content-Type: text/html; charset=utf-8`・`Content-Security-Policy: sandbox allow-scripts`（隔てた枠として出す。ページの中の script は動くが、置き場の名で API を呼べない）・`X-Content-Type-Options: nosniff`。
- 無いページは 404 `NotFound`、消したページは 410 `Gone`（`message` に消した時刻）。

## file の中身

`GET /{project}/{key}/files/{語}-{n}.{拡張子}`（手の道。今と同じ）。

- 形の合わない URL（語が名の形でない・受けない拡張子（大文字も）・`n` が頭に 0 のある数や 0）は 404 `NotFound`。
- 置いた file は 200 で、置いたままの byte。`Content-Type` は置いた時に表で決めた物・`Content-Security-Policy: sandbox`（直に開いても中の script を動かさない）・`X-Content-Type-Options: nosniff`・`Cache-Control: max-age=31536000, immutable`（同じ URL の中身は替わらない。上げ直すと次の番号）。
- 無い file は 404 `NotFound`、消した file は 410 `Gone`（`message` に消した時刻）。読む接続を待ちきれなければ 503、DB の誤りは 500。
- ページの本文の道と段の数が違うので、種類 `files` のページ（`/{project}/{key}/files-1.html`）とぶつからない。

## 守りの層

生成した道と手の道の前に、外から順に掛ける（`crates/server/src/guard.rs`。castlist と同じ作り）。断る答えは restJson1 の形。

| 順 | 守り | 振る舞い |
|---|---|---|
| 1 | 頭の印 | どの答えにも `X-Content-Type-Options: nosniff` と `Referrer-Policy: no-referrer`。今の `Pagetray-Api` は付けない。CSP は、本文の道と画面が自分で付ける（上の「ページの本文」「file の中身」「画面」） |
| 2 | 問い合わせの記録 | 1 つにつき 1 行（method・uri・status・latency。tower-http の TraceLayer を今の server と同じ設定で）。記録の file にだけ書き、stdout には出さない。断った物も残る |
| 3 | Host と Origin | `Host`（port を外して）が 127.0.0.1・localhost でなければ、`GET`・`HEAD` のほかの要求に `Origin` が付いていれば、403 `Forbidden`。例外は 1 つだけ（今の server と同じ）: 一覧の UI の form が file を上げる `POST /api/projects/{project}/keys/{key}/files`（後ろに何も付かない道）で、`Origin` が `http://<Host（port も）>` とそのまま同じで、`Sec-Fetch-Site` が無いか `same-origin` の物。印（`FromUi`）を付けて通す（上の「file を置く」）。ほかの道・本文の枠の中のページ（`Origin: null`）・ほかの port や名や `https` は 403 |
| 3 | HEAD | `GET` にして通し、本文を捨てる（生成した骨の道は GET だけなので、ここで揃える） |
| 4 | 時間切れ | 答えの頭まで 5 秒（`REQUEST_TIMEOUT`）を過ぎれば 503 `Unavailable`（待っている物は捨てる）。本文を送る間は数えないので、大きな backup の file や新着の知らせの流れを途中で切らない。試験は `Config` で短くする。接続を待つ長さ（書く `WRITE_WAIT` 2 秒・読む `READ_WAIT` 3 秒）は、これより短くする（混んでいる時に「混んでいる」の 503 が先に答える。`crates/server/tests/guard.rs` で止めている） |
| 5 | 本文の上限 | 16 MiB（`MAX_BODY`。ページも file も。今の server と同じ）。`Content-Length` で超えると言う要求は読まずに、大きさを言わずに流す要求は 16 MiB まで読んで、413 `PayloadTooLarge`（何も置かない）。ちょうど 16 MiB は通る。生成した層は本文をみな読んでから引数にするので、ここで読んでおく（smithy-rs の生成した層にも本文の上限の設定があるが、既定は上限無しで、超えた時の答えが 413 にならない）。本文を読む間も、外の層の 5 秒の時間切れに数える（送り手は同じ機械の中の roster と一覧の UI なので、16 MiB でも 5 秒よりずっと短く届く。遅い送り手に書く接続を握らせない利点もある。今の server には時間切れが無かったので、新しい振る舞い） |

## データ

- DB は SQLite のまま。新しい migrations（`crates/server/migrations/`）は 1 つ（`0001_init.sql`）で、今の `migrations/0001`〜`0006` を当てた後の作りと同じ表・列（順も）・型・空を許すか・既定・主キー・一意・索引（部分の索引の条件も）・外のキー・check・trigger・全文検索の表を作る（`crates/server/tests/db.rs` が、今の migrations を当てた DB と突き合わせる）。
- 接続は castlist と同じ: 書く接続は 1 本（`WRITE_WAIT` 2 秒まで待つ）、読む接続は読むだけの 4 本（`READ_WAIT` 3 秒）。どの接続も WAL・`foreign_keys`・`recursive_triggers`・`busy_timeout` 5 秒・一時の物は memory。開く時に移行を当て、`foreign_key_check` に崩れがあれば開かない。
- 今の server の DB（`_sqlx_migrations` に、新しい migrations に無い版か、中身の checksum の違う版がある物）は、読むだけで見て、何も書かずに断る（castlist の `refuse_current` と同じ。訳は「今の server の DB。…新しい volume へ移してから起こす: `pagetray-next migrate --from <写し> --to <新しい DB>`（image では `<image> migrate --from <写し> --to /data/pages.db`）」）。
- 今の DB からの移し（`pagetray-next migrate --from <写し> --to <新しい DB>`。`crates/server/src/migrate.rs`）:
  - 移す元は、今の server の `GET /api/backup` で取った写し（`VACUUM INTO` の file）。読むだけ（SQLite の URI の `mode=ro&immutable=1`）で開き、書かない。隣に `-wal` か `-shm` があれば（空でも）、動いている DB かその残りなので断る（写しには、どちらも無い）。symlink は辿り、指す先の file の隣を見て、指す先を開く（動いている DB を指す link を通さない）。表 `pages` の無い file・SQLite でない file・無い file も断る。どれも、移す先を作る前に断る。
  - 今の migrations の途中の版（0001〜0005 まで当てた物）の写しも受ける。無い表（0002 の `removed`・0003 の `files` と `files_removed`・0005 の `senders`・0006 の `page_targets`）は空、後の版で足した列（0004 の `asked_at`・`answered_at`、0005 の `sender_id`、0006 の `waits_on`・`verdict`）は空として読む。在る表に、そのほかの列が無ければ、今の server の DB の形でないので断る。
  - 知らない物は、黙って落とさずに断る: `_sqlx_migrations` が無い・今の migrations（0001〜0006。repo の頭の `migrations/` を binary に埋め込む）に無い版か、中身の写し（checksum）の違う版か、失敗した版がある（後の版の今の server の写しを、それより前の rc で移す時など。その版を知る新しい server で移す）・移す表と SQLite・移行・探す索引（`pages_fts` と、その中の表）のほかの表がある・在る表に知らない列がある。
  - 移す先は、無ければ作り、新しい migrations を当ててから、行（送り手・ページ・消した番号・見た物・file・消した file の番号・索引）が 1 つも無いことを見る。あれば断る（2 度目の移しも、移し終えた先なので断る）。移す元と同じ file も断る。今の server の DB（今の volume）を移す先にした時は、何も書かずに「移す先に今の server の DB を渡した。…新しい volume を渡す」と分けて断る（打つ命令は合っていて、付ける volume が違う）。
  - 移しが落ちた時（どの断りでも・DB の誤りでも）、移す先がこの移しの前に無かったなら、移す先と隣の `-wal`・`-shm` を消す（移行を当てた空の DB を残すと、続けて起こした rc が空のまま起き、そこへ書かれた物が元のデータと分かれる）。前から在った移す先は消さない。
  - 1 つの transaction の中で、親の表から順（`senders`・`pages`・`removed`・`page_targets`・`files`・`files_removed`）に、insert だけで写す。足すだけの trigger は効いたまま。外のキーは、行ごとでなく transaction の終わりに見る（`defer_foreign_keys`。行き先の無い行（無い送り手を指すページ・無いページを指す見た物など）を、DB の誤りで落とさずに、下の `foreign_key_check` で表と行を出して断るため）。替えない物: ページ・file・送り手の id（ページと file から送り手への結びも）・番号・消した番号（落とすと番号が使い回され、古い URL が別のページを指す）・送り手の付随する id と castlist のアクターの id・名・種類・見た物（字は path なので、URL の形を残せばそのまま）とその順（`rowid` も写す。答えの `targets` は書いた順）・返事の状態とレビューの判定・題・本文の byte・file の種類と大きさと SHA-256。
  - 時刻（`created_at`・`asked_at`・`answered_at`・`removed_at`・送り手の `created_at`・`resolved_at`）は、新しい server の残す字（24 字の UTC。下の「直す今の振る舞い」）に揃える（`…:05Z` は `…:05.000Z`、`…:05.5Z` は `…:05.500Z`）。揃えるのは、今の server が受けた形（UTC の `Z` で終わり、秒の下は 0〜3 桁）で、在る日と時刻の物だけ。ほかの字（時差の付いた字・無い日・ミリ秒より細かい字・時刻でない字）は推して直さず（直すと置いた時刻が替わる）、表と列と行と字を出して断る。今の server は、この形のほかを受けないので、ふつうは出ない。
  - 探す索引（`pages_fts`）は写さず、写した本文から作り直す（置く時と同じ字。題は残した題、本文は tag などを除いた字）。途中の版の写しや、索引の欠けた写しでも、移した後は引ける。
  - 写した後、同じ transaction の中で突き合わせる: 表ごとの行の数と中身の digest（本文の外の列を 1 行 1 つの JSON の配列にして行の順に並べた字の SHA-256。移す元は、移す時と同じ式で読む）・本文の合計（ページと file ごとに、`<id> <本文の SHA-256>` の行を id の順に並べた字の SHA-256）・索引の行がページと同じ数でみなページを指すか・時刻の列ごとの同じ時点（移す元で値のある行の数と、移す先の同じ行（id か主キーで結ぶ）の時刻が同じ ms の行の数。digest は移す元も揃えた式で読むので、揃えた字が元と同じ時刻かは、ここで見る）・外のキー（`foreign_key_check`）・新しい API の形（下）・時刻の字（上）。1 つでも合わなければ、何も足さずに戻し、1 で終わる。合えば commit し、1 行ずつ出して 0 で終わる。引数の誤りは 2。
  - 新しい API の形に合わない行は、表と行と列を出して断る（新しい API の道と URL の形に入らず読めない・送り直しも形の誤りで断られるため。castlist の id の検査と同じ考え）。見るのは、URL の段（`project`・`key`。`.`・`..` と、`project` の `api` も）・種類と役割の名・workspace・番号（1〜18 桁）・file の語と拡張子（受ける種類だけ）・送り手の付随する id（`Issuer`・`ActorRef`）と castlist の答え（アクターの id は `ActorId` の小文字の UUID・名は 1〜128 字で制御字の無い物・種類は `ai` か `human`。3 つは揃って在るか、揃って無い）・見た物（ページの path か `https` の URL）。今の server の形の検査は新しい物と同じか狭いので、castlist のアクターの id（今の pagetray は英数字と `-` の 64 字までを受けた）のほかは、ふつうは出ない。
  - 出す行（予行と切り替えで、元と先の値を見比べる。digest と SHA-256 は頭の 16 字）:

    ```text
    senders 4 → 4 digest 0123456789abcdef → 0123456789abcdef ok
    pages 4 → 4 digest … → … ok
    removed 1 → 1 digest … → … ok
    page_targets 3 → 3 digest … → … ok
    files 2 → 2 digest … → … ok
    files_removed 1 → 1 digest … → … ok
    body pages 4 → 4 sha256 … → … ok
    body files 2 → 2 sha256 … → … ok
    index pages_fts 4 → 4 ok
    time senders.resolved_at 1 → 1 ok
    time senders.created_at 4 → 4 ok
    time pages.created_at 4 → 4 ok
    …
    time files_removed.removed_at 1 → 1 ok
    foreign_key_check ok
    rows ok
    times ok
    migrated: <from> → <to>
    ```

  - 起こす前に打つ物で、castlist の設定は読まず、記録の置き場（`logs/`）も作らない。commit の後に移す元を外せなかった時は、stderr に書くだけで、移した結果は替えない。試験は `crates/server/tests/migrate.rs`（移す元は、同じ workspace の今の server の Router に今の API で書き、`GET /api/backup` で取る。途中の版は、今の migrations をその版まで当てて SQL で行を置き、`VACUUM INTO` で写しにする。移した後は、新しい API で読み、今の API で読んだ物と比べる）。

## 直す今の振る舞い

- 置いた時刻の字: 今の server は `createdAt` を書いた字のまま残す（`…:05Z` と `…:05.5Z` が混ざる）。`.` は `Z` より前の字なので、字で比べると同じ秒の中の順が狂う（`…:05.5Z` が `…:05Z` より前になる）。新しい server は、いつも 24 字（`…:05.000Z`）で残す。今の DB の字は、移す時に揃える（上の「データ」）。
- 一覧の頁: 今の server は `before`・`after` を `created_at` の字で比べ、同じ時刻の行が頁の境に並ぶと抜けるか重なる。新しい `ListPages`・`ListFiles` は、`createdAt` と行の id の組で続きを決める（keyset。`nextToken` に入れる）。`crates/server/tests/list.rs` と `crates/server/tests/files.rs` が、同じ時刻の行を頁の境に並べて、抜けず重ならないことを見る。
- 引数の形: 今の server は一覧の `project`・`kind` などと `actor` の形を見ず、`limit` は 1〜1000 に丸めていた。新しい `ListPages`・`ListFiles` は定義の形で 400 にする（`maxResults` も）。
- 画面の続き: 今の server の画面の「続きを出す」は、境の時刻（`at`）だけで続け、同じ時刻の行が境に並ぶと抜けた。新しい画面の `at` は時刻と id の組（`crates/server/tests/ui.rs` が、同じ時刻の 205 枚を抜けずに出すことを見る）。
- 新着の知らせの記録: snake_case から、API の答えと同じ camelCase に（上の「新着の知らせ」）。

## 定義の取り方

- pagetray の定義は `model/model/pagetray.smithy`（名前空間 `ai.pagetray`）。共通の形 `ai.common` は castlist の `model/model/common.smithy` を使う。
- `ci/gen.sh` が、castlist の release の tag（今は `v1.0.0-rc.1`）の `common.smithy` と `castlist.smithy` を `model/castlist/` に取り、`ci/gen.sh` に固めた SHA-256 と合うかを見てから置く。tag と 2 つの digest は `ci/gen.sh` の 1 か所に書く。取った定義は commit するので、`ci/gen.sh --check` は castlist に取りに行かず、入れてある物が digest と合うかを見る。
- 1 つの Smithy の build から、2 つの crate を作る: pagetray の server の骨（`crates/pagetray-sdk/`。smithy-rs の codegen-server 0.1.40・`http-1x`・`publicConstrainedTypes: false`）と、castlist を呼ぶ crate（`crates/castlist-client/`。codegen-client 0.1.40 の `rust-client-codegen`。http 1 の形）。呼ぶ側の crate は、送り手を castlist に聞く処理（上の「送り手」）で使う。
- 呼ぶ側の crate の既定の features から、https の client（rustls・aws-lc）を外す（`ci/gen.sh` が `Cargo.toml` の 1 行を替える）。pagetray が castlist を呼ぶのは同じ機械の中の `http://127.0.0.1` だけなので、TLS の crate を build しない。HTTP の client は、使う側が設定で渡す（TLS の無い HTTP/1.1。上の「送り手」）。castlist を 1 度呼べること・1 秒の時間切れ・送り直さないことは、`crates/server/tests/senders.rs` が偽の castlist で止める。
- 例（`@httpRequestTests`・`@httpResponseTests`）の id は、castlist の定義と 1 つの model に入るので重なれない。pagetray の物は頭に `Pagetray` を付ける。

## 段 4 の分け方（予定）

| PR | 中身 |
|---|---|
| 1（この章と同じ PR） | この章・定義（`GetHealth`・`CreatePage`）・`ci/gen.sh`・生成した 2 つの crate・新しい server の土台（生成した層の包み・守りの層の Host と Origin と頭の印と時間切れ・誤り・DB）・`CreatePage`・ページの本文の道 |
| 2 | 送り手（`actorIssuer`・`actorRef` を受け、castlist を生成した client で引く） |
| 3 | 探す・返事・レビューの印・消す（`ListPages`・`SetReplyState`・`AddReviewMarks`・`DeletePages`）。SSE は後の PR に分けた（知らせを送る所は 3 で置いた） |
| 4 | file（`CreateFile`・`ListFiles`・`DeleteFiles`・file の中身の道・一覧の UI からの例外）と、守りの層の本文の上限（file の上限と一緒に見るため、5 から移した） |
| 5 | 新着の知らせ（SSE）・画面・e2e の試験を流せるだけの最小の起こす binary（CI の `ui` が、今の server と新しい server の両方に流す） |
| 6 | backup・logs・起こす binary の残り（記録・止める合図での閉じ方。上の「起こし方」） |
