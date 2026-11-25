# 変えたこと

版ごとに、API の版（[docs/src/api.md](docs/src/api.md)）が変わったかを書く。

## 1.0.0-rc.1

API の版: 無くなった（今の API の版 1 との互換は無い。新しい API に版の数は無く、形の替わりは定義の release の tag との `smithy diff`（`ci/gen.sh --diff`）が守る）

AI-36 の作り直しの初めの候補（pre-release）。この版の image は、新しい server（`crates/server`。Smithy の定義 `model/` から作った骨の上に処理を入れた物）を `Containerfile.next` で作った物で、今の server（0.8.0）とは別の物。動いている置き場（`pagetray`）は替えない。roster の `.pagetray-version` は上げない（上げると、今の API を話す roster の試験が落ち、運用の image を作り直すと新しい server が今の volume で起きずに止まる。上げるのは切り替えの時、新しい volume と一緒に）。切り替え（AI-36 の段 8）までは、試しと予行にだけ使う。release に付ける CLI は今の CLI（`pagetray`。今の API に向けた物）で、新しい API に向けた CLI（`crates/cli-next`）は付けない。

- **API がみな替わった**（今の server との互換は無い。Product Manager の決め: AI-36）: protocol は restJson1、欄の名は camelCase、ページの番号 `n` は字、誤りは定義に書いた型（`X-Amzn-Errortype` と `{"message":…}`）、ページ送りは `nextToken`・`maxResults`、時刻は UTC だけ（24 字で残す）。header `Pagetray-Api` と `GET /api/version` は無く、版は `GET /healthz` の `version`。知らない送り手は 422 でなく、`fieldList` に `/actorRef` のある 400。送り手は castlist の新しい API（`v1.0.0-rc.1` の定義）で引く。形は [新しい API の形](https://uraitakahito.github.io/pagetray/next.html)（`docs/src/next.md`）。
- **ページと file の URL は今のまま**（本文 `/{project}/{key}/{kind}-{n}.html`・file `/{project}/{key}/files/{語}-{n}.{拡張子}`）。消したページと file は 410 のまま。
- **DB の移行の SQL が替わった**: 新しい server は、新しい migrations（`crates/server/migrations/`）で新しい DB を作る。**今の volume（`pagetray-data`）では起こさない**（新しい server は今の server の DB を開かずに止める）。今のデータは、今の server の `GET /api/backup` の写しから `migrate --from … --to …` で新しい volume に移す（id・番号・消した番号・送り手・印・本文を守り、時刻を 24 字に揃え、行の数と digest・本文の SHA-256・時点・外のキーを照らす）。手順は [release を出す](https://uraitakahito.github.io/pagetray/release.html) の「rc を動かす時と切り替え」。
- **呼ぶ側に効く振る舞いの替わり**（くわしくは [新しい API の形](https://uraitakahito.github.io/pagetray/next.html)）:
  - 守りの層の時間切れ: 答えの頭まで 5 秒を過ぎれば 503 `Unavailable`（今の server には無かった）。backup も、写しを作るのに 5 秒を過ぎれば 503（「守りの層」「操作」）。
  - 一覧の引数は定義の形で見て 400（今の `limit` の丸めは無く、`maxResults` が 1〜1000 の外は 400。「探す」「直す今の振る舞い」）。
  - 一覧の頁は `createdAt` と id の組で続け、同じ時刻の行が頁の境で抜けず重ならない。画面の「続きを出す」も同じ（直した不具合。「直す今の振る舞い」）。
  - castlist の設定の形が違う時は、記録の置き場を作らずに 1 で終わる。castlist に聞けない時の記録の訳は、答えと同じ粗い字（「起こし方」「送り手」）。
  - 引数を付けない起こし方のほかは、`migrate --from <写し> --to <新しい DB>` だけを受け、違う引数は使い方を出して 2 で終わる（「データ」）。
- 画面（一覧の UI）と新着の知らせ（SSE）は今と同じ働き。画面に CSP を付けた。知らせの記録は camelCase。
- 止める合図では、走っている問い合わせを 3 秒まで待ってから閉じる（今の server は待たずに閉じていた）。
- image の中身・環境変数・port は今と同じ（binary は `/pagetray-server`）。起こし方は、volume のほかは今と同じ（volume は新しい物にする）。

## 0.8.0

API の版: 1（変わらない。引数・エンドポイント・答えの欄・code を足しただけなので、呼ぶ側は変えなくてよい）

**移行の SQL を足した（`0006_review.sql`）。この版が起きた後の DB では、0.7.0 までの版は起きない。** 上げる前に、DB の写しを取る（[動かす](https://uraitakahito.github.io/pagetray/run.html) の「版を上げる」）。戻す時は、その写しから戻す。

- レビューのやりとり（roster の `tpm ask review`・`tpm report review`・`tpm ask tpm --review`）を、一覧で追えるようにした。
  - DB: `pages` に `waits_on`（誰の返事を待つか。`review`・`tpm`。空は Product Manager）と `verdict`（レビューの判定）を、表 `page_targets`（ページが見た物。1 枚にいくつでも。ページを消すと一緒に消える）を足した。前からあるページは、どちらも空で、見た物も無い。
  - API: `POST /api/pages/reply` に `on=pm|review|tpm`（待ちの相手。無ければ `pm`）。待ちの相手が違えば何も替えずに `409`（答えたにする時も、今ほかの相手を待っているページを返事待ちにする時も。`review`・`tpm` は、返事待ちにしたページだけを答えたにできる）。`on` を `reply` 無しで書くと `400`。見た物はページ 1 枚に 100 個まで。`GET /api/pages` に `on`（`reply` と組で）と `verdict` の絞り。**`on` の無い `reply` は、Product Manager の分だけを返す**（レビュー待ち・上げた件は入らない。前からの呼ぶ側の数は変わらない）。新しく `POST /api/pages/review`（判定と見た物を JSON で書く）。ページの記録に `waits_on`・`verdict`・`targets`、知らせ（SSE）に `event: review` を足した。
  - 一覧の UI: レビューのページの行に判定の印（よい・気になる所あり・直すべき・見られなかった）と「見た物:」の link、依頼書の行に「見る物:」の link、見られたページの行に「レビュー: review-N 判定」。待ちの印は相手ごとに「レビュー待ち」「レビュー済み」・「上げた」「決まった」。操作の帯に「レビュー待ち N 件」「上げた N 件」（押すとそれだけに絞る。0 件なら出ない）。絞り込みの列の「返事」に「レビュー待ち」「上げた」を、新しく「判定」を足した。tab の題と「返事待ち N 件」は、今までどおり Product Manager の分だけ。
  - CLI: `pagetray page reply --on review|tpm`、`pagetray page list --on … --verdict …`。`page list` の表の返事の状態の欄は、Product Manager のほかを待つ物に `waiting:review` のように相手を添える。`--json` の欄に `waits_on`・`verdict`・`targets` を足した。
- **0.8.0 より前の置き場は、知らない `on` を黙って捨てる**（Product Manager の返事待ちにしてしまう）。`on` を書く呼ぶ側は、先に `GET /api/version` で版を見る。

## 0.7.0

API の版: 1（変わらない。エンドポイントを 1 つ足しただけなので、呼ぶ側は変えなくてよい）

移行の SQL は足していない（0.6.0 と同じ volume で起き、0.6.0 へもそのまま戻せる）。起こし方・環境変数・image の中身も変わらない。

- 不正終了の原因を、後から調べられるようにした。自分の終わり方と、時刻つきの記録を、volume の `/data/logs/` に残す（[動かす](https://uraitakahito.github.io/pagetray/run.html) の「落ちた後に調べる」）。`container logs` は container を起こし直すと消えるが、こちらは残る。
  - 起きるたびに、前回の終わり方を 1 行で出す（`previous: …`）: きれいに止まった・止める合図は受けたが終わり切らなかった・panic（場所と文）・誤りで終わった（訳）・停止の印なし（最後に生きていた時刻）。
  - 記録の file（`server.log`）には、今までの行に加えて、問い合わせごとの 1 行（method・path・status・かかった時間）を書く。
  - **ディスクの上限**: 記録は 1 MiB × 4 file で回し、終わり方の印は 64 KiB まで。合わせて 4 MiB と 64 KiB を超えない。書けない時も pagetray は動く（行を捨てる）。
  - `GET /api/logs` が、印と記録を古い順の字で返す（止めずに読める）。
- **log の行の形が変わった**: `container logs` に出るどの行も、頭に時刻（UTC）と重さ（`INFO` など）が付く（`2026-01-02T03:04:05.000000Z  INFO pagetray_server: pagetray 0.7.0 listen=… db=…`）。行の中の文（`pagetray <版> listen=… db=…`・`put …`・`remove …` など）は変わらない。初めの行は、前回の終わり方になった。問い合わせごとの行は `container logs` には出ない（量を増やさないため）。誤りの行（`error: …`・`castlist: …`）は、stderr でなく stdout に出る。
- 止める合図で終わる時、止まる前に `signal SIGTERM`（か `SIGINT`）の行を出す。起きられずに終わる時の code は、前と同じ 1。
- CLI は変えていない。

## 0.6.0

API の版: 1（変わらない。引数と答えの欄と code を足しただけなので、呼ぶ側は変えなくてよい）

**移行の SQL を足した（`0005_senders.sql`）。この版が起きた後の DB では、0.5.0 までの版は起きない。** 上げる前に、DB の写しを取る（[動かす](https://uraitakahito.github.io/pagetray/run.html) の「版を上げる」）。戻す時は、その写しから戻す。

- ページと file の送り手を記録する。置く時に付随する id（`actor_issuer`・`actor_ref`）を書くと、受けたその時に castlist に送り手を聞き、答え（アクターの id・名・種類と、答えた時刻）を残す。
  - 設定: 環境変数 `PAGETRAY_CASTLIST_URL`（聞く先。`http://<host>:<port>`）。無ければ castlist に聞かず、付随する id だけを残す（未確認）。本番は `http://castlist.box:8080`（container の中から、castlist の container に名でつなぐ）。
  - 設定: 環境変数 `PAGETRAY_CASTLIST_LINK_URL`（一覧の UI の、castlist のアクターの画面への link の頭。`http://<host>:<port>`。既定 `http://127.0.0.1:8802`）。Product Manager が host の browser で開く所なので、聞く先と別に決める。形が違えば起きない。起きた時の log の頭の行に ` castlist=<聞く先> castlist_link=<link の頭>`。
  - API: `POST /api/pages`・`POST /api/files` に `actor_issuer`・`actor_ref`（両方書くか、両方書かない。片方だけは `400`）。castlist に聞けない（つながらない・1 秒で答えない・`200` でない）時は `503`、castlist に登録の無い送り手は `422` で、どちらも何も置かない。ページと file の記録に `sender` の欄を、`GET /api/pages`・`GET /api/files` に `actor=<アクターの id>` の絞りを足した。一覧の UI から上げた file は、付随する id を読まない。
  - castlist を待つ間は書く取引を始めないので、ほかの置く要求と一覧は待たされない。
  - DB: 表 `senders`（足すだけ。書き換えと消しは trigger が断る）と、`pages.sender_id`・`files.sender_id`（`senders.id` への外部キー）を足した。前からある行は空（送り手なし）。読む接続と書く接続の両方で `foreign_keys` と `recursive_triggers` を付け、起きる時に `PRAGMA foreign_key_check` で合わない行があれば起きない。
  - 一覧の UI: 送り手を記録したページと file の行に、castlist の名（castlist の URL があれば、`PAGETRAY_CASTLIST_LINK_URL` を頭にした、castlist のアクターの画面への link）と、その下に小さく付随する id。castlist に聞かなかった送り手は「未確認」。絞り込みの列に「送り手」。
- CLI は変えていない（付随する id を書く道は、まだ無い）。

## 0.5.0

API の版: 1（変わらない。足しただけなので、呼ぶ側は変えなくてよい）

**移行の SQL を足した（`0004_reply.sql`）。この版が起きた後の DB では、0.4.2 までの版は起きない。** 上げる前に、DB の写しを取る（[動かす](https://uraitakahito.github.io/pagetray/run.html) の「版を上げる」）。戻す時は、その写しから戻す。

- Product Manager の返事が要るページを、見分けられるようにした。ページに、返事の状態（返事待ち・答えた）を持たせる。
  - DB: `pages` に `asked_at`（返事待ちにした時刻）・`answered_at`（答えた時刻）を足した。前からあるページは、どちらも空（返事を求めていない）で、印は付かない。
  - API: `POST /api/pages/reply?project=…&key=…&kind=…&n=…&state=waiting|answered` で、ページ 1 枚の状態を書く。`GET /api/pages` に `reply=waiting|answered` の絞りを、ページの記録に `asked_at`・`answered_at` を、知らせ（SSE）に `event: reply` を足した。
  - 一覧の UI: 返事待ちのページの行に「返事待ち」、答えた行に「答えた」の印。操作の帯に「返事待ち N 件」（押すと返事待ちだけに絞る。0 件なら出ない）、tab の題にも件数。絞り込みの列に「返事」。状態が書かれると、押さずに替わる（本文の枠は読み込み直さない）。
  - CLI: `pagetray page reply <page> --waiting|--answered` と、`pagetray page list --reply waiting|answered`。`page list` の表に、返事の状態の欄（役割の後ろ）を足した。**tab 区切りの出力を欄の位置で読んでいる script は、URL と題が 1 つ後ろへずれる**（`--json` は変わらない）。
- 状態を書くのは呼ぶ側（roster の `tpm ask pm`・`tpm answer`）。置き場は、だれが書いてよいかを見ない。browser の中のページからは書けない。

## 0.4.2

API の版: 1（変わらない。呼ぶ側は変えなくてよい）

- 一覧の UI: ページが置かれると、一覧の列と絞り込みの列が、押さずに新しくなる。本文の枠（読んでいるページ）は読み込み直さない。
  - 新着の数（tab の題の「(N)」と新着の印）は、今の絞り込みで一覧の列に入ったページだけを数える（前は、ほかの project のページも数えていた）。
  - 一覧の列を下へ送っている時は、見ていた行の位置を保ち、新着の印「● 新着 N 件（上へ）」を出す（押すと一覧の列の頭へ送る）。古い方の続きを見ている時は、前と同じく「（読み直す）」。
  - 置き場を起こし直して知らせがつなぎ直った時にも、一覧の列を新しくする。
  - 変えたのは一覧の UI の template の script だけ。前の版で開いていた tab は、1 度読み直すと新しい動きになる。
- 文書の本: DB の作りの章などから「表の形は API の仕様に入らない」を消した。呼ぶ側の repo の名を roster に替えた。
- `ci/docs-build.sh`: pagefind が無い時に、正しい誤りの文を出す。
- サーバーの API の動きは 0.4.1 と同じ。移行の SQL は足していない（0.4.1 に戻しても、同じ DB で起きる）。

## 0.4.1

API の版: 1（変わらない。呼ぶ側は変えなくてよい）

- 文書の本に「DB の作り」の章を足した（表ごとの列・型・空を許すか・主キーと一意と索引と、表どうしの結びの図）。表の形は、前と同じく API の仕様に入らない。
  - 章と本物の DB（移行を当てた後の作り）を突き合わせる試験（`tests/schema_doc.rs`）を足した。表や列を変えて章を直さないと、CI の `test` が落ちる。
- 用語集に、file（`files`）・消した番号（`removed`・`files_removed`）・全文の索引（`pages_fts`）・移行の行を足した。本の概要の章の表に、CLI と用語集の章を足した。
- サーバーの API の動きは 0.4.0 と同じ。移行の SQL は足していない（0.4.0 に戻しても、同じ DB で起きる）。

## 0.4.0

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
- 一覧の UI の Key の画面（`/<project>/<key>/` など）に、file の枠（`#files`。その Key の file の図の見本・名・大きさ・上げた人・時刻・参照の path）と、上げる枠（`#upload`）を足した。
  - 上げる枠は、file を選ぶか落とすと見本（図・字の頭の 20 行・PDF は名と大きさ）を出し、「秘密が写っていないか見た」に印を付けるまで送れない。拡張子と 16 MiB は送る前に見る。
  - 守りを 1 か所だけ緩めた: `POST /api/files` は、`Origin` が `http://<要求の Host>` と同じで `Sec-Fetch-Site` が無いか `same-origin` なら通す。その時は、引数に関わらず役割 `product-manager`・workspace 空で記録し、`n`・`created_at` は読まない。`Origin: null`・ほかの port や名・ほかの道（`POST /api/pages`・`DELETE`）は前のまま `403`。
  - `POST /api/files` の `role` が無い時の答えが、axum の字から JSON の `400`（`{"error":…}`）に替わった（code は前と同じ `400`）。
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
