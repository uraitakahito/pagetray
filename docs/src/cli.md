# CLI

`pagetray` は、置き場を端末から操作する命令。置き場の API（[API の仕様](api.md)）を HTTP で呼ぶ。相手は、この Mac の中（`127.0.0.1`）の置き場だけで、認証も設定の file も無い。

## 入れる

release に、macOS（arm64）用の binary が付いている。`gh` で取り、checksum を確かめてから置く。

```sh
cd "$(mktemp -d)"
gh release download v<version> -R uraitakahito/pagetray -p 'pagetray-v<version>-aarch64-apple-darwin.tar.gz*'
shasum -a 256 -c pagetray-v<version>-aarch64-apple-darwin.tar.gz.sha256
tar -xzf pagetray-v<version>-aarch64-apple-darwin.tar.gz -C ~/.local/bin pagetray
pagetray status
```

- browser で落とすと、macOS が開くのを止める（公証をしていない binary のため）。`gh` か `curl` で取る。
- Rust があれば、source からも入る: `cargo install --git https://github.com/uraitakahito/pagetray --tag v<version> --locked pagetray`（package の名。入る命令も `pagetray`）。
- 消す時は `rm ~/.local/bin/pagetray`。ほかに置く file は無い。

## 命令

| 命令 | すること |
|---|---|
| `pagetray status` | 置き場が生きているかと、サーバーの版と CLI の版を出す（`GetHealth`。`--json` は `url`・`server`・`cli`） |
| `pagetray page list` | ページを絞って探す（`ListPages`。新しい順） |
| `pagetray page view <page>` | ページの本文を出す（手の道）。`--web` で browser に、`-o <file>` で file に |
| `pagetray page put <file>` | ページを置く（`CreatePage`。`-` は stdin）。置いたページの URL を出す |
| `pagetray page delete <page>…` | ページを消す（`DeletePages`。戻せない。`--yes` の時だけ） |
| `pagetray page reply <page>` | ページの返事の状態を書く（`SetReplyState`）。`--waiting`・`--answered`・`--on` |
| `pagetray page watch` | ページが置かれるたびに 1 行出す（`GET /api/events`） |
| `pagetray file list` | file（画像・PDF・字の file）を絞って探す（`ListFiles`。新しい順） |
| `pagetray file view <file>` | file の中身を出す（手の道）。`-o <file>` で file に |
| `pagetray file put <file>` | file を置く（`CreateFile`）。置いた file の URL と、ページからの参照を出す |
| `pagetray file delete <file>…` | file を消す（`DeleteFiles`。戻せない。`--yes` の時だけ） |
| `pagetray backup -o <file>` | DB の写し（SQLite の file 1 つ）を取る（`GET /api/backup`。手の道） |
| `pagetray api <path>` | API を直に呼ぶ（手の道）。`-X <method>`・`-f <name>=<value>`・`--input <file>`・`--content-type <type>` |
| `pagetray completion <shell>` | shell の補完の script を出す（`bash`・`zsh`・`fish` など） |

旗の一覧は、どの命令も `--help` で出る。`<page>` は、ページの URL か、`<project>/<key>/<kind>-<n>` の形で書く。`file view`・`file delete` の `<file>` は、file の URL か、`<project>/<key>/files/<語>-<n>.<拡張子>` の形で書く。

```sh
pagetray page list --project . --key AI-22          # 今いる folder の project の、Key が AI-22 のページ
pagetray page list -q plan --json=key,title,url      # 題と本文から探し、名指した欄だけを JSON で
pagetray page view http://127.0.0.1:8801/p/AI-22/plan-1.html | less
pagetray page put report.html --project p --key AI-22 --kind result
pagetray file put shot.png --project p --key AI-24 --reviewed
pagetray backup -o ~/pagetray-$(date +%Y%m%d-%H%M).db
```

### 探す（`page list`）

- 絞る旗: `--project`・`--key`・`--kind`・`--role`・`--workspace`・`-q`（題と本文の字）・`--reply`（返事の状態。`waiting` は返事待ち、`answered` は答えた物だけ）・`--on`（`--reply` で絞る時の待ちの相手。`pm`＝既定・`review`・`tpm`）・`--verdict`（レビューの判定。`approve`・`concerns`・`reject`・`not-assessed`）。`--oldest` で古い順、`-L` で件数（既定 50。1〜1000 の外は、送る前に code 2）。
- 続き: 続きがあれば、続きの印を stderr に 1 行で出す（stdout は頼んだ物だけ）。同じ命令に `--next-token <印>` を付けて打つと、次の頁が出る。
- `--project` が無ければ、どの project のページも出す。`--project .` は、今いる folder の path から project の名を作る（英数字と `._-` のほかを `-` にする。`roster` と同じ作り方）。
- 端末には見出しの付いた表、pipe には tab 区切り（見出しは無い）で出す。欄は、置いた時刻・（project）・Key・種類と番号・役割・返事の状態（`waiting`・`answered`。返事を求めていないページは `-`。Product Manager のほかを待つ物は `waiting:review` のように相手を添える）・URL・題の順。`--json` は、API の「ページの記録」（camelCase。`n` は字。値の無い欄は出さない）に、頼んだ先（`--url`）で組んだ `url` を足した配列。`--json=key,url` のように欄を名指すと、その欄だけを出す（知らない名は code 2）。

### 置く（`page put`）

- `--project`（か環境変数 `PAGETRAY_PROJECT`）・`--key`・`--kind` が要る。project を今いる folder から勝手に決めることは無い（置き間違いを防ぐため）。
- 番号は置き場が決める。同じ project・Key・種類に置き直すと次の番号になり、前の版は残る。
- 置く人の役割は `--role`（か `PAGETRAY_ROLE`。既定は `human`）、居場所は `--workspace`。
- 出す URL は、一覧と同じく、頼んだ先（`--url`）で組む（`file put` も同じ）。`--json` は、ページの記録にその `url` を足した物。

### 返事の状態を書く（`page reply`）

- Product Manager の返事が要るページを返事待ちにし（`--waiting`）、答えたら答えたにする（`--answered`）。どちらか 1 つが要る。callboard のページの一覧の画面は、この状態で印と件数を出す（[API の仕様](api.md) の「返事」）。
- 出すのは、状態と URL の 1 行（tab 区切り）。`--json` で、ページの記録。
- `--on review|tpm` で、待ちの相手（レビューの席・TPM 担当）を名指す（書かなければ Product Manager）。答えたにする時は、その相手を待っているページだけが替わり、違えば何も替えずに code 1（置き場の訳のまま）。
- 無いページ・消したページは code 1。
- `roster` を使う project では、`roster ask pm`・`roster answer`・`roster ask review`・`roster report review`・`roster ask tpm --review` がこの状態を書く。手で直す時だけ使う。
- 判定と見た物（[API の仕様](api.md) の「レビューの印」）を書く命令は無い（書くのは roster の命令だけ）。

### 消す（`page delete`）

- 戻せない。まず消すページを一覧で出し（`nextToken` を辿って、合う物をみな見る）、**`--yes` を付けた時だけ**消す。`--yes` が無ければ、何も消さずに終わる（終わりの code は 0）。
- 消す先は、命令に書いた物だけ: ページの名指し（いくつでも）か、`--project <name> [--key <key> [--kind <kind>]]`。環境変数の project は、消す先にならない。
- いくつかを名指して消す時、途中で断られても、それまでに消した物（`消した（N 枚）:` と URL）を stderr に出してから、断られた訳で止まる（消した物は戻せないため）。
- `-` で始まる project の名（`roster` の project の名）は、URL で書くか、`--` の後ろに書く: `pagetray page delete --yes -- -Users-me-x/AI-22/plan-1`。
- 消したページの URL は `410` を返し、同じ番号は 2 度使われない。
- **`PAGETRAY_NO_DELETE`**: この環境変数が付いていると（空と `0` のほか）、`page delete`・`file delete` と `api` の `DELETE` は、何もせずに code 1 で止まる。置き場は、だれが消してよいかを見ない。消してよい人を呼ぶ側が決める時に、これを使う（`roster` は、担当を起こす時に付ける）。

### 新着を待つ（`page watch`）

- 置き場の新着の知らせ（`GET /api/events`）のうち、ページを置いた知らせ（event の名の無い物）だけを 1 行ずつ出す。file・返事・印の知らせ（`file`・`reply`・`review`）は出さない。
- 流れが途中で切れれば（置き場が落ちた・接続が切れた）code 3、置き場がきれいに閉じれば（止める合図で流れを閉じた）code 0。

### file（`file list`・`file view`・`file put`・`file delete`）

file は、ページから参照する画像・PDF・字の file。置ける種類と大きさは [API の仕様](api.md#受ける-file-の種類) の表のとおり（png・jpg・jpeg・gif・webp・pdf・txt・log・md・csv・json。1 つ 16 MiB まで）。

- **探す**（`file list`）: 絞る旗は `--project`・`--key`・`--role`・`--workspace`・`-q`（名 `<語>.<拡張子>` の字）。`--oldest`・`-L`・`--next-token`・`--json` は `page list` と同じ。`--json` は、API の「file の記録」に `url` を足した配列。
- **読む**（`file view`）: 中身をそのまま stdout に出す。`-o <file>` で file に書く。画像と PDF は、stdout が端末なら出さずに code 2 で止まる（画面が崩れるため）。`-o` を付けるか、pipe でつなぐ。
- **置く**（`file put <file>`）: `--project`（か `PAGETRAY_PROJECT`）と `--key` が要る。役割と居場所は `page put` と同じ。
  - 名は、file の名から作る: 英小文字にし、英小文字・数字・`-`・`_` のほかが続く所を `-` にし、頭と尻の `-` を除いて 64 字に切る（`Screenshot 2026-11-21 at 10.00.png` は `screenshot-2026-11-21-at-10-00.png`。字が残らなければ `file`）。拡張子も英小文字にする。`--name <語>.<拡張子>` で替えられる。
  - 番号は置き場が決める。同じ名で置き直すと次の番号になり、前の物は残る。
  - 置けると、2 行を出す: file の URL と、同じ Key のページから参照する相対の path（`files/<語>-<n>.<拡張子>`）。ページには `<img src="files/screen-1.png">` のように書く。
  - 置く前に、手元で見る（どれかに当たれば、何も送らずに止まる）: 拡張子が置ける種類か・空でないか・16 MiB までか・中身が拡張子の形か（ここまでは code 2）。
- **置く前に人が見る**（`file put`）: 置いた file は、置き場を開ける人ならだれでも読める。
  - 画像と PDF は、**`--reviewed` を付けないと置かない**（code 1）。file を開いて、token・鍵・個人の情報・ほかの人の画面・この Mac の path（home の folder の名など）が写っていないかを確かめてから付ける。
  - 字の file（txt・log・md・csv・json）は、秘密の形の字を探し、当たれば**置かずに、当たった行の番号を出す**（code 1。当たった字そのものは出さない）。探す形は、GitHub の token（`ghp_`・`gho_`・`ghu_`・`ghs_`・`ghr_`・`github_pat_`）、`sk-` で始まる API の鍵、AWS の access key（`AKIA`）、Slack の token（`xoxb-`・`xoxp-`）、Atlassian の API token（`ATATT`）、秘密の鍵の頭の行（`-----BEGIN … PRIVATE KEY`）。どれも、後ろに token の字が続く物だけ。わざと置くなら `--allow-secret` を付ける（置いた上で、当たった行の番号を stderr に出す）。
  - JPEG に EXIF があれば、止めずに「位置情報が入っているかもしれない」と stderr に出す。消すなら、`exiftool -all= <file>` などで消してから置く。
- **消す**（`file delete`）: `page delete` と同じく、まず消す file を一覧で出し、`--yes` を付けた時だけ消す。消す先は、file の名指し（いくつでも）か、`--project <name> [--key <key> [--name <語>.<拡張子>]]`（`--name` は、その名のどの番号も）。ページを消しても file は消えず、file を消してもページは消えない。`PAGETRAY_NO_DELETE` が付いていれば、何もせずに code 1 で止まる。

### DB の写し（`backup`）

- 書きかけの `<file>.part-<pid>` に読みながら書き、SQLite の頭を見てから名を付け替える。落ちれば書きかけを消す。止める合図（Ctrl-C の SIGINT・SIGTERM）で止めた時も、書きかけを消してから 130（SIGTERM は 143）で終わる（書きかけには、ページの本文がみな入る）。
- 待つのは下の「呼び方」の形。途中で止まった・読めない答えは code 3（置き場が応えない）。

### 直に呼ぶ（`api`）

- 本文の種類は `--content-type`（既定 `application/json`。返事と印は JSON の本文のため）。ページは `text/html`、file は file の種類を書く。header にできない値（改行など）は code 2。
- 断られれば、本文を出して code 1（`置き場が 400 ValidationException を返した` など）。知らない道（404 `UnknownOperationException`）も、打ち間違いが多いので code 4 にせず、ほかの断りと同じく code 1（code 4 にするのは、生成した操作と、CLI が自分で組む手の道だけ。下の「終わりの code」）。
- 送り手は、命令からは書かない。`api` で `actorIssuer`・`actorRef` を書くと、castlist に登録の無い送り手なら 400 `ValidationException`（`fieldList` に `/actorRef`）で code 1。

## 設定

旗が環境変数より先、環境変数が既定より先。

| 旗 | 環境変数 | 既定 | 中身 |
|---|---|---|---|
| `--url` | `PAGETRAY_URL` | `http://127.0.0.1:8801` | 置き場の URL（`http://` だけ。`https://` は code 2） |
| `--project` | `PAGETRAY_PROJECT` | 無し | `page list`・`page put`・`file list`・`file put` の project |
| `--role` | `PAGETRAY_ROLE` | `human` | `page put`・`file put` で置く人の役割 |
| — | `PAGETRAY_NO_DELETE` | 無し | 付いていれば、消す命令を止める |
| — | `BROWSER` | macOS の `open` | `page view --web` で browser を開く命令 |

環境変数の proxy（`HTTP_PROXY` など）は使わない。

## 呼び方

- 生成した操作（`GetHealth`・`ListPages` などの定義の道）は、定義から作った client（`crates/pagetray-client/`）で呼ぶ。
- 手の道（ページの本文・file の中身・新着の知らせ・DB の写し・`api` の命令）は、生成した client に無いので、同じ HTTP の client（`aws-smithy-http-client` の hyper の作りの connector。`Connector::builder().build_http()`）で直に呼ぶ。ureq は使わない。訳: 生成した client が hyper と tokio を引くので、ureq を足すと HTTP の client が 2 つになり、TLS の無い HTTP/1.1・接続を使い回さない・proxy を使わない・`Host` の決まりを 2 か所で守ることになる。1 つにすれば、決まりは `crates/cli/src/client.rs` の 1 か所で、依存も増えない。
- HTTP の設定は、置き場が castlist を呼ぶ時（[API の仕様](api.md) の「送り手」）と同じ: TLS の無い HTTP/1.1（features は `default-client` だけ）・接続を使い回さない（`pool_max_idle_per_host(0)`）・送り直さない（`RetryConfig::disabled()`）・環境変数の proxy を使わない・`Host: 127.0.0.1:<port>`（port は `--url` の物。無ければ付けない）で送り `Origin` は付けない（置き場は `Host` の port を外して 127.0.0.1・localhost でない要求を断る。CLI は browser でないので、つなぐ先の名がどうでも届くようにする。置き場は答えの `url`・`Location` を `Host` で組むので、port を付けて開ける link にする）。
- 待つのは 1 度に 10 秒（つなぐ・送る・答えを読み終えるまで）。`page watch` だけは、答えの頭も本文も待ち続ける。`backup` は、答えの頭まで（置き場が写しを作る間。置き場は 5 秒で 503 を返す）を 10 秒と数え、本文は切れ目ごとに 10 秒（10 秒何も届かなければ、止まった置き場として code 3）で、全部で何秒かかってもよい。訳: 置き場の時間切れも答えの頭までで、送る間は数えない。DB の写しは大きく、読み終えるまでを 10 秒と数えると、大きな DB の写しが取れなくなる。待ち続けると、途中で止まった置き場を待って戻らないので、届かない間だけを数える。

## 終わりの code

| code | 訳 |
|---|---|
| `0` | できた |
| `1` | 置き場が断った（`4xx`・`5xx`。無いページ `404`・消したページ `410` を含む）。file に書けない時・`PAGETRAY_NO_DELETE` で止めた時・`file put` が置く前に止めた時（`--reviewed` が無い・秘密の形がある）も |
| `2` | 使い方の誤り（`file put` の、置けない種類・大きすぎる・中身が拡張子と合わない file を含む） |
| `3` | 置き場が応えない（つながらない・10 秒で答えない・答えが読めない・`backup` の本文が 10 秒届かない・`page watch` の流れが途中で切れた） |
| `4` | 置き場の API が、この CLI の話す形でない: 生成した操作か、CLI が自分で組む手の道に、生成した層が 404 `UnknownOperationException` を返した（この CLI の呼ぶ操作を知らない、定義より古い置き場）。`api` の命令の道では code 1（上の「直に呼ぶ」） |
| `130`・`143` | `backup` を止める合図（SIGINT・SIGTERM）で止めた（書きかけの写しは消した） |

誤りの訳は stderr に 1 行で出る。stdout には、頼んだ物だけが出る。

- API には版の header が無く、形の版は定義の release の tag との `smithy diff` が守る（[API の仕様](api.md) の「共通の決まり」）。壊す形の替わりは無いので、置き場の版が CLI より古くて困るのは、CLI の呼ぶ操作が置き場に無い時だけ。それを code 1 の断りとせず、code 4 の「置き場を上げる」と分けて出す。

## `roster page` との関わり

`roster` を使う project では、担当は `roster page`（put・path・list・delete）を使う。`roster page` は、project・Key・役割・控えの folder・消してよい人という `roster` の決まりを持ち、同じ API を呼ぶ。`pagetray` は、その決まりを持たない道具で、`roster` を通さない場面（人の手元・`roster` の project でない folder）で使う。どちらで置いたページも、同じ置き場に入り、どちらからも探せる。
