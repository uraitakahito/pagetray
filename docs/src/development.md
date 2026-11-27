# 開発

| ファイル | 中身 |
|---|---|
| `crates/server/` | server（package と binary `pagetray-server`）。image（`Containerfile`）に `/pagetray-server` の名で入る |
| `crates/server/src/main.rs` | 起こす binary。設定を読む・起こす・止める |
| `crates/server/src/lib.rs`・`guard.rs` | 道筋（生成した骨と手の道を 1 つの Router に並べる）と、この Mac の中からの要求だけを受ける守りの層 |
| `crates/server/src/db.rs` | 接続・移行・SQL。ほかの版の server の DB を断る所も |
| `crates/server/src/pages.rs`・`files.rs`・`list.rs`・`search.rs`・`marks.rs`・`remove.rs` | ページと file を置く・探す・印を書く・消す（生成した操作の handler）と、ページの本文・file の中身を返す道 |
| `crates/server/src/messages.rs` | メッセージの記録を置く・書き足す・探す・絞り込みの候補を数える（生成した操作の handler） |
| `crates/server/src/castlist.rs` | 置かれる時に、送り手を castlist に聞く（castlist の定義から作った client で。1 秒まで待つ）。聞く先と、画面の link の頭の設定を読む |
| `crates/server/src/hand.rs` | 手の道の DB の写し（`GET /api/backup`）と記録（`GET /api/logs`） |
| `crates/server/src/ui.rs`・`ui/messages.rs`・`facets.rs`・`templates/index.html`・`templates/messages.html` | 一覧の UI と、メッセージの一覧（askama の template。build の時に検査される） |
| `crates/server/src/events.rs` | 新着の push（SSE） |
| `crates/server/src/text.rs` | HTML から題と検索用の字を取り出す |
| `crates/server/migrations/` | 表を作る SQL（起きる時に、まだ当てていない物を当てる）。できる表は [DB の作り](db.md) |
| `crates/cli/` | 人が打つ CLI（package と binary `pagetray`。[CLI](cli.md)）。`crates/pagetray-client/` で呼ぶ。release に付く。試験は、同じ commit の server を起こして打つ |
| `model/`・`ci/gen.sh` | API の Smithy の定義と、そこから crate を作る script（下の「定義と生成」） |
| `crates/pagetray-sdk/`・`crates/castlist-client/`・`crates/pagetray-client/` | 定義から作った server の骨と、castlist を呼ぶ crate と、pagetray を呼ぶ crate（CLI が使う）。手で直さない |
| `e2e/` | 一覧の UI と、この本を、本物の browser で見る試験（Playwright・TypeScript）。image にも CLI にも入らない。流し方は下の「一覧の UI の試験」と「この本を直す」の「本の試験」 |
| `Containerfile`・`.dockerignore` | image の作り方（静的な binary を、空の image に 1 つだけ置く）と、build の中に入れる物 |
| `ci/` | 出す image と、release に付ける CLI に、入ってよい物だけが入っているか・動くかを見る script と、その決まり。この本を build する script（`ci/docs-build.sh`）もここ |
| `docs/` | この本（mdBook）。原文は `docs/src/` の日本語で、`docs/src/api.md` が、呼ぶ側との API の仕様。直し方は下の「この本を直す」 |
| `.claude/roles/config.jsonc` | `roster` に向けた、環境 `pagetray` の宣言 |

Rust は `rustup` で入れる（Homebrew の `rust` は `rust-toolchain.toml` を読まない）。版は `rust-toolchain.toml` が決め、この repo の中で `cargo` を打つと、その版が入って使われる。

```sh
brew install rustup                              # keg-only。PATH に /opt/homebrew/opt/rustup/bin を足す
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
PAGETRAY_DB=/tmp/pages.db PAGETRAY_LISTEN=127.0.0.1:18801 cargo run --bin pagetray-server   # 手元で起こす
```

GitHub Actions でも同じ順に流す（`.github/workflows/ci.yml`）。試験は、試験ごとに一時の DB を作る。workflow の `image` は、`Containerfile` を arm64 の runner で build し、[image に入る物](release.md#image-に入る物) を確かめ（`ci/check-image.sh`）、空の image の中で起きて API で置いて読めること・断る答えが restJson1 の形であること・止める合図で終わること・起こし直しても残ることを見る（`ci/smoke-image.sh <image>`）。終わりに、出した v1.0.0 の image（digest で固める）が書いた volume を、この image で開いて読めるかも見る。release の workflow も、同じ 2 つを push の前に流す。workflow の `cli` は、release に付ける CLI を macOS（arm64）の runner で、出す時と同じ script（`ci/build-cli.sh`）で作り、binary の形・つながる library・版・秘密の走査を確かめ（`ci/check-cli.sh`）、その binary を同じ commit のサーバーに向けて打つ。

API の形（URL・引数の名・JSON の欄）を変える時は、定義（`model/model/pagetray.smithy`）を直す。壊す変わり（前からある呼ぶ側が困る変わり）は、CI の `ci/gen.sh --diff` が落とす（下の「定義と生成」と、[API の仕様](api.md) の「共通の決まり」の「版」）。変えたら、`roster page`（roster の `cli/src/commands/page.ts`）と、その偽の置き場（`cli/test/helpers.ts` の `FakeStore`）も合わせて直す。roster の CI が、この repo の決めた版に向けて、つなぎの試験を流す。

- **SQL**: SQL は動く時に組む形（`query`・`query_as`）だけを使うので、build の時の検査の file（`.sqlx/`）は要らない。
- **当てた移行の SQL は変えない**: 動いている DB が、当てた SQL の中身を覚えている。表を変える時は `crates/server/migrations/` に次の番号の file を足し、`crates/server/tests/migrations.rs` の `UNRELEASED` に名を足す（出したら `RELEASED` に移し、SHA-256 を書く）。出した file が 1 byte でも替わるか、どちらにも名指さない file があると、試験が落ちる。
- **表を変えたら、章も直す**: 表・列・型・空を許すか・主キーと一意と索引を変えたら、同じ PR で [DB の作り](db.md) と、表を足したなら [用語集](glossary.md) も直す。`crates/server/tests/schema_doc.rs` が、移行を当てた DB と章を突き合わせ、食い違えば落ちる。
- **記録（`crates/server/src/runlog.rs`）**: 終わり方の印と、大きさで回す記録の file（[動かす](run.md) の「落ちた後に調べる」）。ここだけは、tokio の糸を塞ぐ同期の書き方をする（`panic = "abort"` で、落ちる直前の行を残すため）。上限（`LOG_FILE_MAX`・`LOG_FILES`・`RUNS_MAX`）を変えたら、「落ちた後に調べる」の表も直す。binary を起こして終わり方を見る試験は `crates/server/tests/runlog.rs` と `crates/server/tests/server.rs`（試験ごとに空の folder を作り、その中に DB を置く。記録が DB の隣の `logs/` にできるため）、本物の image での確かめは `ci/smoke-image.sh`。log に出す行は、今までの行と同じく英語の `名=値` の形で書く。
- **版を上げる**: `Cargo.toml` の `[workspace.package]` の `version` を上げ（server と CLI が同じ版になる。`Cargo.lock` も変わる）、`CHANGELOG.md` に、API の形が替わったかを書く。
- **crate を上げる**: `.cargo/config.toml` の `global-min-publish-age` が、公開から 2 日たっていない版を選ばせない（Cargo 1.100 から）。上げる時は `cargo update` → 試験 → PR。`cargo install` はこの決まりを見ないので、道具は版を書いて入れる。
- **弱点と license の見張り**: workflow の `advisories` が `cargo deny --workspace check advisories licenses`（設定は `deny.toml`。workspace の member をみな見る）を、PR ごとと週に 1 度流す。`deny.toml` に無い license の crate が入ると落ちる。許す一覧の外の license は、名指した crate 1 つだけに、訳と一緒に許す（今は server が引く `minicbor` の BlueOak-1.0.0 だけ）。
- **workflow の action**: commit の SHA で固定する（tag は後から中身が替わりうる）。
- **main を守る決まり**: GitHub の ruleset「main: CI が通らないと merge できない」（Settings → Rules → Rulesets）が、`main` への変更に、CI の `test`・`image`・`advisories`・`docs`・`cli` の 5 つが通っていることを求める。PR は、5 つが通るまで merge できない。repo の admin は、この決まりを bypass できるので、`main` への直の push は通る（GitHub の必須の check には「PR だけに効かせる」選び方が無いので、直の push を通すには bypass になる）。admin は、わざと選べば（画面の bypass の印・`gh pr merge --admin`）PR も CI を通さずに merge できてしまうので、使わない。workflow の job の名を変えたら、ruleset の check の名も合わせて直す（合わないと、PR がいつまでも merge できない）。job `ui` は、まだ必須の check ではない。

## 定義と生成

API は Smithy の定義から作る。server は定義から作った骨（`crates/pagetray-sdk/`）に処理を入れた物で、image（`Containerfile` は `-p pagetray-server` だけを作る）に入る（[release を出す](release.md)）。形の決まりは [API の仕様](api.md)。

- 定義は `model/model/pagetray.smithy`。共通の形 `ai.common` と castlist の定義は、castlist の release の tag から取った `model/castlist/`（tag と digest は `ci/gen.sh` の 1 か所。手で直さない）。
- 生成した crate（`crates/pagetray-sdk/`・`crates/castlist-client/`・`crates/pagetray-client/`）は `ci/gen.sh` が作る。**手で直さない**。定義を替えたら `ci/gen.sh` を流し、作り直した物も同じ commit に入れる。workspace の member だが、既定の命令（`default-members`）の外。中の protocol の試験は `cargo test --locked -p pagetray_sdk`・`-p castlist_client`・`-p pagetray_client` で流す。定義の例の引数の値は、呼ぶ側が逃がす字（`:` など）を `%3A` と逃がして書く（逃がさずに書くと、呼ぶ側の crate の試験が合わずに落ちる。server の骨の試験は、逃がしを戻してから比べるので、どちらでも通る）。
- 生成器の版は `model/smithy-build.json`（Maven の物）と `ci/gen.sh`（Smithy の CLI と digest）に書く。`smithy-build.json` に名指した Maven の物は、公開から 2 日たっていなければ `ci/gen.sh` が断る。
- CI の job `model` が `ci/gen.sh --check`（取った定義が digest と合い、作り直した物が入れてある物と同じか）と `ci/gen.sh --diff`（定義の入った最新の release（rc を除く）から壊す形に変わっていないか）を流す。
- server の試験は `crates/server/tests/`、CLI の試験は `crates/cli/tests/`。試験だけの commit を先に作り、落ちるのを見てから実装を足す。

## 一覧の UI の試験

一覧の UI は、HTML の字を `crates/server/tests/ui.rs`（メッセージの一覧は `crates/server/tests/messages.rs`）が見て、browser でしか見えない物（script の動き・色・幅での並び）を `e2e/` の試験が見る。`e2e/` は [Playwright](https://playwright.dev/) の試験（TypeScript）で、同じ commit の `pagetray-server` を一時の DB で起こし、本物の browser（Chromium）で開く。Node が要るのはこの folder の中だけで、image にも CLI にも入らない。

```sh
cargo build --locked --bin pagetray-server
cd e2e
pnpm install --frozen-lockfile
pnpm exec playwright install chromium      # 初めの 1 度だけ。試験用の Chromium を入れる
pnpm test
PAGETRAY_E2E_CHANNEL=chrome pnpm test      # この機械の Chrome を使う（Chromium を入れなくてよい）
```

- **起こす binary**: 既定は `../target/debug/pagetray-server`。ほかの binary に向ける時は `PAGETRAY_E2E_SERVER=<path>` を付ける。
- **CI**: workflow の `ui` が PR ごとに流す。落ちると、画面の絵と操作の記録が artifact（`ui-report`）に残る。`ui` は、main を守る決まりの必須の check に、まだ入れていない（時々落ちないことを見てから足す）。
- **読むだけの試験**: 題に `@smoke` が付いた試験（`e2e/tests/smoke.spec.ts`）は、何も置かず、何が置かれていても通る物だけを見る。`PAGETRAY_E2E_URL=http://127.0.0.1:8801 pnpm test` と打つと、サーバーを起こさず、動いている置き場に向けて、この試験だけが流れる（置く試験は、設定が流させない）。版を上げた後の確かめに使う（[動かす](run.md#確かめる) の「確かめる」）。
- **試験を足す**: `e2e/tests/` に `*.spec.ts` を足す。ページは `helpers.ts` の `place` で置き、区画（操作の帯・絞り込みの列・一覧の列・本文の枠と、Key の画面の file の枠・上げる枠）は `helpers.ts` の関数で選ぶ。file は `placeFile` で置き、上げる枠の試験は `input[type=file]` に `setInputFiles` で file を渡す。試験は同じ置き場に 1 本ずつ順に流れるので、試験ごとに別の project の名を使う。新着を見る試験（一覧の列の差し替えと新着の印）は、`openLive` で開いてから置く（知らせにつながる前に置いたページの知らせは届かない）。新着の印は一覧の列を下へ送っている時に出るので、`scrollList` で送ってから置く。大きな file は試験の中で作り、repo に置かない。
- **返事の状態を見る試験**（`e2e/tests/reply.spec.ts`）: 返事待ちの数は、絞り込みに関わらず置き場の全部を数え、tab の題にも出る。試験は同じ置き場に順に流れるので、返事待ちにしたページは、試験の終わりまでに答えたにする（残すと、後の試験の tab の題が変わる）。絞り込みの列の見出し「返事」は、状態を書いたページが 1 枚でもあると出るので、この file は、見出しの並びを見る `list.spec.ts` より後に流れる名にしてある。
- **レビューの印を見る試験**（`e2e/tests/review.spec.ts`）: 返事の状態を見る試験と同じく、レビュー待ち・上げたにしたページは、試験の終わりまでに答えたにする。絞り込みの列の見出し「判定」は、判定を書いたページが 1 枚でもあると出るので、この file も `list.spec.ts` より後に流れる名にしてある。
- **メッセージの一覧を見る試験**（`e2e/tests/messages.spec.ts`）: メッセージの記録は `helpers.ts` の `putMessage` で置き、行は `messageRow`・`messageRows`、まだ届いていない数は `undelivered` で選ぶ。まだ届いていない数は、絞り込みに関わらず置き場の全部を数え、tab の題にも出るので、まだ届いていない（`waiting`）にした記録は、試験の終わりまでに届いた（`delivered`）にする。操作の帯に釦を足す時は、狭い幅（800px）の試験も見る（釦が増えて操作の帯が 2 段になると、一覧の列が動く。差し替えの script は、操作の帯の替わりも入れて、見ている行を同じ所に置く）。
- **送り手を見る試験**（`e2e/tests/list.spec.ts` の終わり）: 試験の置き場は、次の port の偽の castlist に送り手を聞き、一覧の UI の link の頭（`PAGETRAY_CASTLIST_LINK_URL`）は、聞く先と別の値（`helpers.ts` の `CASTLIST_LINK`。開かない）にする（`playwright.config.ts`）。偽の castlist は `helpers.ts` の `startCastlist` で、送り手を書いて置く間だけ立てる。送り手のある行が 1 つでもあると、絞り込みの列に見出し「送り手」が出るので、見出しの並びを見る試験より後に置く。
- **template を変えた時**: `templates/index.html`・`templates/messages.html` を変えたら、この試験も流す。試験が選ぶ手がかり（区画の id の `toolbar`・`filters`・`list`・`viewer`・`files`・`upload`、`#fresh`・`#waiting`・`#review-waiting`・`#escalated`、`li[data-path]`・`.reply`・`.verdict`・`.links`、絞り込みの列の見出しの字など）を変える時は、同じ PR で `helpers.ts` も直す。
- **Node の依存**: 依存は `@playwright/test` の 1 つで、版は `package.json` にそのまま書く（`^` を付けない）。`pnpm-lock.yaml` を commit し、CI は `--frozen-lockfile` で入れる。`pnpm-workspace.yaml` の `minimumReleaseAge` が、公開から 2 日たっていない版を入れさせない。弱点は、workflow の `advisories` が `pnpm audit` で、PR ごとと週に 1 度見る。版は手で上げる（Playwright を上げると browser も替わるので、試験が通るのを見る）。`package.json` の版を書き替え、`pnpm install` → 試験 → PR。

## この本を直す

この本は [mdBook](https://rust-lang.github.io/mdBook/) で作り、release の tag（`v*`）を打った時に、workflow（`.github/workflows/docs.yml`）が GitHub Pages に出す。PR と main への push では、build できるか・英語の訳が原文と合っているか・作った本が browser で動くか（下の「本の試験」）を見る（job の名は `docs`）。

| ファイル | 中身 |
|---|---|
| `docs/book.toml` | 本の設定。組み込みの検索は切ってある（日本語が引けないため） |
| `docs/src/` | 原文（日本語）。目次は `SUMMARY.md`。語の決まりは `glossary.md`（用語集） |
| `docs/po/en.po` | 英語の訳。原文の段落・表の升・コードの塊ごとに 1 項 |
| `docs/theme/site.js`・`site.css` | 上の帯に足す、言葉の切り替えと検索の枠 |
| `docs/pagefind.yml` | 検索（[Pagefind](https://pagefind.app/)）の索引に入れる物 |
| `docs/mermaid.min.js`・`mermaid-init.js` | 図（Mermaid）を描く script。`mdbook-mermaid install docs` が置いた物 |
| `ci/docs-po.sh` | `en.po` を原文に合わせる。`--check` を付けると、合っているかを見るだけ |
| `ci/docs-build.sh` | 日本語の本を `docs/book/` に、英語の本を `docs/book/en/` に作り、検索の索引を `docs/book/pagefind/` に作る |

道具は、workflow の `env` に書いた版を入れる（上げる時は、workflow の版と checksum を一緒に上げる）。

```sh
cargo install mdbook@0.5.4 mdbook-mermaid@0.17.1 --locked
cargo install mdbook-i18n-helpers --locked \
  --git https://github.com/google/mdbook-i18n-helpers --rev b202ae5b952e8f5fd3df72021af6f8c94463636e
brew install pagefind gettext
ci/docs-build.sh
python3 -m http.server -d docs/book
```

`ci/docs-build.sh` が本を `docs/book/` に作る。最後の行で配り、`http://localhost:8000/` で見る。

- **mdbook-i18n-helpers は commit を名指して入れる**: 公開済みの 0.4.0 は、訳した本で囲み（`> [!WARNING]` など）を崩す。直し（[#344](https://github.com/google/mdbook-i18n-helpers/pull/344)）を含む版が出たら、版の名指しに替える。
- **原文を直したら、同じ PR で英語も直す**: `ci/docs-po.sh` を流すと、足した段落が `docs/po/en.po` に空の訳で入り、変えた段落に `#, fuzzy` の印が付く。空の所と印の所を訳し、印の行を消す。`ci/docs-po.sh --check` が通れば済み（CI も同じ検査を流す。原文にあって訳に無い段落・`fuzzy` の残り・訳に残った日本語があると落ちる）。
- **コードの塊**: 訳に出るのは、コードの中の註（`#` の行）と、引用符の中の字だけ。命令の中の置き換えの名は訳せないので、`<version>` のように英字で書く。行の後ろに付けた註は、取り出されないことがある（`ci/docs-build.sh` が、英語の本に日本語が残っていれば落ちる）。
- **訳さない物も `msgstr` に書く**: 命令・URL だけの項は、原文をそのまま写す。link の先の file の名（`run.md` など）は変えない。見出しを指す `#…` は、英語の本では見出しの id が訳した題から作られるので、訳した見出しの id に書き替える（`release.md#image-に入る物` → `release.md#what-goes-into-the-image`）。
- **語を足す・替える**: 先に [用語集](glossary.md) を直し、文書・註・code の名をそれに合わせる。用語集に書いた id が画面に在るか・「使わない言い方」が戻っていないかは、試験（`crates/server/tests/terms.rs`）が見る。
- **章を足す**: `docs/src/` に file を置き、`SUMMARY.md` に 1 行足す。CLI の章は `docs/src/cli.md`（「使う」の節の、動かす と API の仕様 の間）。
- **repo の file を指す link**: 本の外の file（`CHANGELOG.md` など）は、GitHub の URL で書く。章どうしは、`run.md` のように file の名で書く。
- **本の試験**: `e2e/docs-tests/` の試験（Playwright）が、作った本を本物の browser で開き、どの章も開いて console に誤りが無いこと・図が描けること・検索の枠が開いて日本語と英語の語が当たること・言葉の切り替えで同じ章へ移ることを見る。`ci/docs-build.sh` の後に、`e2e/` で `pnpm test:docs` を打つ（道具の入れ方は上の「一覧の UI の試験」と同じ）。CI では job `docs` が流す。章を足したら、`e2e/docs-tests/docs.spec.ts` の章の一覧にも足す（目次と合わないと落ちる）。
- **図**: ```` ```mermaid ```` の塊で書く。構文を間違えても build は通る。描けるかは、本の試験が見る（図のある章は `docs.spec.ts` の `FIGURES` に並べる。図を足した章は、そこにも足す）。
- **検索と言葉の切り替え**: `docs/theme/site.js` は、mdBook の画面の `#mdbook-menu-bar` と `path_to_root` に頼っている。mdBook を上げても検索の枠が開くこと・言葉が切り替わることは、本の試験が見る。
