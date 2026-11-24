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
| 誤り | 定義に書いた型だけ: 400 `ValidationException`・404 `NotFound`・409 `Conflict`・410 `Gone`・500 `InternalError`（server の中の失敗。訳は記録に書き、答えは決まった字。`@retryable` は付けない）・503 `Unavailable`（混んでいる・時間切れ・呼ぶ先の castlist が答えない。`@retryable`）。守りの層が断る 403 `Forbidden`・413 `PayloadTooLarge`・503 `Unavailable` は、service の `errors` に置き、どの操作の誤りにも入る。生成した層の外の道（本文・SSE・backup・logs）の誤りも、同じ形で返す。生成した層が自分で返す物は、知らない道の 404 `UnknownOperationException`（本文は `{}`）・読めない引数の 400 `SerializationException`・違う `Content-Type` の 415 `UnsupportedMediaTypeException` |
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
| `CreateFile` | `POST /api/projects/{project}/keys/{key}/files`（本文は file の中身。`Content-Type` が要る） | `POST /api/files` |
| `ListFiles` | `GET /api/files` | `GET /api/files` |
| `DeleteFiles` | `DELETE /api/files` | `DELETE /api/files` |

手の道（Smithy の外）: ページの本文 `GET /{project}/{key}/{kind}-{n}.html`・file の中身 `GET /{project}/{key}/files/{語}-{n}.{拡張子}`（どちらも今と同じ URL）・新着の知らせ `GET /api/events`（SSE）・`GET /api/backup`・`GET /api/logs`・画面（`/`・`/{project}/`・`/{project}/{key}/`）。

- 置かれたページの URL は切らない。だから本文の URL は今のまま、Smithy の外の手の道にする（`.html` の付いた 1 段は、Smithy の道の label にできない）。
- `/api/` の下の道は、みな生成した層か、名指した手の道に行く。本文の道（`/{project}/{key}/{file}`）は `/api/…` を受けない（`project` に `api` は置けない）。
- この章の操作のうち、作ってあるのは `GetHealth`・`CreatePage`（送り手も）・`ListPages`・`SetReplyState`・`AddReviewMarks`・`DeletePages` と本文の道。残りは、AI-36 の段 4 の次の PR で足す（下の「段 4 の分け方」）。

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
- 本文の上限（16 MiB で 413 `PayloadTooLarge`）は、守りの層の PR で足す。

## 送り手

付随する id（`actorIssuer`・`actorRef`）を書いて置けば、castlist に送り手を聞き、答えと一緒に残す（今の pagetray と同じ振る舞いを、新しい castlist の API で）。

- castlist の居場所は `PAGETRAY_CASTLIST_URL`（聞く先）。`http://<host>:<port>` の形だけ（後ろの `/` はよい。path・利用者の名・`https://`・port の無い物・port 0 は、起きる前に断る）。空か無ければ castlist は無い。画面の link の頭 `PAGETRAY_CASTLIST_LINK_URL` は別に決め（container の中から届く所と、host の browser から開く所は違う）、形は同じ、無いか空なら `http://127.0.0.1:8802`。形の違う link は、castlist の URL が無くても断る。link を使う画面は、画面の PR で足す。起きる前に断る所（`from_env` を起こす時に呼ぶ）は、起こし方の PR（binary を足す所）でつなぐ。今の土台は、castlist の無い設定で動く。
- 聞くのは `ResolveActors`（`POST /api/resolve`、本文 `{"issuer":…,"refs":[…]}`）を 1 度。castlist の定義から作った client（`crates/castlist-client/`）で呼ぶ。書く取引を始める前に聞く（castlist を待つ間、書く接続を握らない）。
- HTTP の client は、`aws-smithy-http-client` の hyper の作りの、TLS の無い HTTP/1.1（`Builder::new().build_http()`。features は `default-client` だけ。`default-client` は証明書を読む crate（`rustls-native-certs`・`rustls-pki-types`）を引くが、TLS の実装（rustls・aws-lc・ring・s2n）は入らない）。接続は使い回さない（`pool_max_idle_per_host(0)`。1 度ごとにつなぐので、castlist が閉じた古い接続で落ちることがない）。
- 待つのは 1 度に 1 秒（つなぐ・送る・答えを読み終えるまでの全部。client の `operation_timeout`）。送り直さない（`RetryConfig::disabled()`。castlist の `Unavailable` は `@retryable` だが、送り直すかは pagetray の呼び手が決める）。
- castlist は `Host` が 127.0.0.1・localhost でない要求と、`Origin` の付いた書く要求を断るので、つなぐ先がどこでも `Host: 127.0.0.1` で送り、`Origin` は付けない。
- 答えの見方: 聞いた `ref` の答えが無い・`asOf` が無い（生成した client は、答えに無い `@required` の欄を既定の値で埋めるので、1970 年の初めの時刻は無い物とする）・アクターの `id` が `ai.common#ActorId` の形でない（画面の link の path に入る）・`kind` が `ai`・`human` でない・`name` が 1〜128 字でないか制御字を持つ、のどれかなら読めない答えとする。今の pagetray の、答えの大きさの上限（64 KiB）は、1 秒の時間切れに任せて持たない。

| castlist の答え | pagetray の答え | 残す物 |
|---|---|---|
| 送り手を知っている | 201。`sender` に `issuer`・`ref`・`actor`（`id`・`name`・`kind`）・`resolvedAt`（答えの `asOf`） | ページと、送り手の行（答えた時刻は残す字の 24 字） |
| 送り手を知らない（`actor` が無い） | 400 `ValidationException`（`message` は「castlist に登録の無い送り手（…）」、`fieldList` の path は `/actorRef`） | 何も残さない |
| つながらない・1 秒で答えない・2xx でない・読めない | 503 `Unavailable`（`message` は「castlist に聞けない（訳）。…」。訳は記録にも書く） | 何も残さない |
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

## 新着の知らせ（SSE）

SSE（`GET /api/events`）は、この後の別の PR で足す。返事と印を書いた後（替わらなかった時も）に知らせを送る所は、`crates/server/src/events.rs` の `send` として置いてあり、今は記録に 1 行書くだけ。SSE の PR で、ここを broadcast の channel に送る形にする（今の server と同じく、消した時は送らない）。

## ページの本文

`GET /{project}/{key}/{kind}-{n}.html`（手の道。今と同じ）。

- 形の合わない URL（`kind` が名の形でない・`n` が頭に 0 のある数や 0・`.html` でない）は 404 `NotFound`。
- 置いたページは 200 で、置いたままの byte。`Content-Type: text/html; charset=utf-8`・`Content-Security-Policy: sandbox allow-scripts`（隔てた枠として出す。ページの中の script は動くが、置き場の名で API を呼べない）・`X-Content-Type-Options: nosniff`。
- 無いページは 404 `NotFound`、消したページは 410 `Gone`（`message` に消した時刻）。

## 守りの層

生成した道と手の道の前に、外から順に掛ける（`crates/server/src/guard.rs`。castlist と同じ作り）。断る答えは restJson1 の形。

| 順 | 守り | 振る舞い |
|---|---|---|
| 1 | 頭の印 | どの答えにも `X-Content-Type-Options: nosniff` と `Referrer-Policy: no-referrer`。今の `Pagetray-Api` は付けない。本文の CSP は本文の道が自分で付ける（画面の CSP は画面の PR で決める） |
| 2 | 問い合わせの記録 | 1 つにつき 1 行（tower-http の TraceLayer。今の server と同じ設定） |
| 3 | Host と Origin | `Host`（port を外して）が 127.0.0.1・localhost でなければ、`GET`・`HEAD` のほかの要求に `Origin` が付いていれば、403 `Forbidden`。今の server の例外（一覧の UI の form が file を上げる `POST /api/files`）は、file の PR で `CreateFile` の道に足す |
| 3 | HEAD | `GET` にして通し、本文を捨てる（生成した骨の道は GET だけなので、ここで揃える） |
| 4 | 時間切れ | 答えの頭まで 5 秒（`REQUEST_TIMEOUT`）を過ぎれば 503 `Unavailable`。接続を待つ長さ（書く `WRITE_WAIT` 2 秒・読む `READ_WAIT` 3 秒）は、これより短くする |
| 5 | 本文の上限 | 16 MiB（ページも file も）で 413 `PayloadTooLarge`。守りの層の PR で足す |

## データ

- DB は SQLite のまま。新しい migrations（`crates/server/migrations/`）は 1 つ（`0001_init.sql`）で、今の `migrations/0001`〜`0006` を当てた後の作りと同じ表・列（順も）・型・空を許すか・既定・主キー・一意・索引（部分の索引の条件も）・外のキー・check・trigger・全文検索の表を作る（`crates/server/tests/db.rs` が、今の migrations を当てた DB と突き合わせる）。
- 接続は castlist と同じ: 書く接続は 1 本（`WRITE_WAIT` 2 秒まで待つ）、読む接続は読むだけの 4 本（`READ_WAIT` 3 秒）。どの接続も WAL・`foreign_keys`・`recursive_triggers`・`busy_timeout` 5 秒・一時の物は memory。開く時に移行を当て、`foreign_key_check` に崩れがあれば開かない。
- 今の server の DB（`_sqlx_migrations` に、新しい migrations に無い版か、中身の checksum の違う版がある物）は、読むだけで見て、何も書かずに断る（castlist の `refuse_current` と同じ）。今の DB からの移しは、切り替えの前の PR で作る。

## 直す今の振る舞い

- 置いた時刻の字: 今の server は `createdAt` を書いた字のまま残す（`…:05Z` と `…:05.5Z` が混ざる）。`.` は `Z` より前の字なので、字で比べると同じ秒の中の順が狂う（`…:05.5Z` が `…:05Z` より前になる）。新しい server は、いつも 24 字（`…:05.000Z`）で残す。
- 一覧の頁: 今の server は `before`・`after` を `created_at` の字で比べ、同じ時刻の行が頁の境に並ぶと抜けるか重なる。新しい `ListPages`・`ListFiles` は、`createdAt` と行の id の組で続きを決める（keyset。`nextToken` に入れる）。`ListPages` は作ってあり、`crates/server/tests/list.rs` が、同じ時刻の行を頁の境に並べて、抜けず重ならないことを見る。
- 引数の形: 今の server は一覧の `project`・`kind` などと `actor` の形を見ず、`limit` は 1〜1000 に丸めていた。新しい `ListPages` は定義の形で 400 にする（`maxResults` も）。

## 定義の取り方

- pagetray の定義は `model/model/pagetray.smithy`（名前空間 `ai.pagetray`）。共通の形 `ai.common` は castlist の `model/model/common.smithy` を使う。
- `ci/gen.sh` が、castlist の release の tag（今は `v1.0.0-rc.1`）の `common.smithy` と `castlist.smithy` を `model/castlist/` に取り、`ci/gen.sh` に固めた SHA-256 と合うかを見てから置く。tag と 2 つの digest は `ci/gen.sh` の 1 か所に書く。取った定義は commit するので、`ci/gen.sh --check` は castlist に取りに行かず、入れてある物が digest と合うかを見る。
- 1 つの Smithy の build から、2 つの crate を作る: pagetray の server の骨（`crates/pagetray-sdk/`。smithy-rs の codegen-server 0.1.40・`http-1x`・`publicConstrainedTypes: false`）と、castlist を呼ぶ crate（`crates/castlist-client/`。codegen-client 0.1.40 の `rust-client-codegen`。http 1 の形）。呼ぶ側の crate は、送り手を castlist に聞く処理（段 4 の次の PR）で使う。
- 呼ぶ側の crate の既定の features から、https の client（rustls・aws-lc）を外す（`ci/gen.sh` が `Cargo.toml` の 1 行を替える）。pagetray が castlist を呼ぶのは同じ機械の中の `http://127.0.0.1` だけなので、TLS の crate を build しない。HTTP の client は、使う側が設定で渡す（TLS の無い HTTP/1.1。上の「送り手」）。castlist を 1 度呼べること・1 秒の時間切れ・送り直さないことは、`crates/server/tests/senders.rs` が偽の castlist で止める。
- 例（`@httpRequestTests`・`@httpResponseTests`）の id は、castlist の定義と 1 つの model に入るので重なれない。pagetray の物は頭に `Pagetray` を付ける。

## 段 4 の分け方（予定）

| PR | 中身 |
|---|---|
| 1（この章と同じ PR） | この章・定義（`GetHealth`・`CreatePage`）・`ci/gen.sh`・生成した 2 つの crate・新しい server の土台（生成した層の包み・守りの層の Host と Origin と頭の印と時間切れ・誤り・DB）・`CreatePage`・ページの本文の道 |
| 2 | 送り手（`actorIssuer`・`actorRef` を受け、castlist を生成した client で引く） |
| 3 | 探す・返事・レビューの印・消す（`ListPages`・`SetReplyState`・`AddReviewMarks`・`DeletePages`）。SSE は 3 の後の別の PR に分けた（知らせを送る所は 3 で置いた。上の「新着の知らせ」） |
| 4 | file（`CreateFile`・`ListFiles`・`DeleteFiles`・file の中身の道・一覧の UI からの例外） |
| 5 | 守りの層の本文の上限・backup・logs・画面・起こす binary |
