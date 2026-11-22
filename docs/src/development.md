# 開発

| ファイル | 中身 |
|---|---|
| `src/main.rs` | 設定を読む・起こす・止める（binary の名は `pagetray-server`） |
| `src/lib.rs` | 道筋と、この Mac の中からの要求だけを受ける守り |
| `src/db.rs` | 接続・移行・SQL |
| `src/api.rs` | ページと file を置く・探す・消す・DB の写し |
| `src/castlist.rs` | 置かれる時に、送り手を castlist に聞く（HTTP/1.1 を TCP の上で直に話す。1 秒まで待つ）。聞く先と、画面の link の頭の設定を読む |
| `src/pages.rs` | ページの本文を返す |
| `src/files.rs` | file の中身を返す |
| `src/ui.rs`・`templates/index.html` | 一覧の UI（askama の template。build の時に検査される） |
| `src/events.rs` | 新着の push（SSE） |
| `src/text.rs` | HTML から題と検索用の字を取り出す |
| `crates/api/` | API の仕様のうち、サーバーと CLI が共に使う物（API の版・header の名・引数の字の決まり・path の形・ページと file の記録の型・受ける file の種類） |
| `crates/cli/` | CLI（binary の名は `pagetray`）。置き場の API を HTTP で呼ぶ。試験は、同じ commit のサーバーを起こして打つ |
| `e2e/` | 一覧の UI と、この本を、本物の browser で見る試験（Playwright・TypeScript）。image にも CLI にも入らない。流し方は下の「一覧の UI の試験」と「この本を直す」の「本の試験」 |
| `migrations/` | 表を作る SQL（起きる時に、まだ当てていない物を当てる）。できる表は [DB の作り](db.md) |
| `.sqlx/` | SQL の検査用の file（commit する） |
| `Containerfile`・`.dockerignore` | image の作り方（静的な binary を、空の image に 1 つだけ置く）と、build の中に入れる物 |
| `ci/` | 出す image と、release に付ける CLI に、入ってよい物だけが入っているか・動くかを見る script と、その決まり。この本を build する script（`ci/docs-build.sh`）もここ |
| `docs/` | この本（mdBook）。原文は `docs/src/` の日本語で、`docs/src/api.md` が、呼ぶ側との API の仕様と、API の版の決まり。直し方は下の「この本を直す」 |
| `.claude/roles/config.jsonc` | roster の `tpm` に向けた、環境 `pagetray` の宣言 |

Rust は `rustup` で入れる（Homebrew の `rust` は `rust-toolchain.toml` を読まない）。版は `rust-toolchain.toml` が決め、この repo の中で `cargo` を打つと、その版が入って使われる。

```sh
brew install rustup                              # keg-only。PATH に /opt/homebrew/opt/rustup/bin を足す
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
SQLX_OFFLINE=true cargo test --locked --workspace
PAGETRAY_DB=/tmp/pages.db PAGETRAY_LISTEN=127.0.0.1:18801 cargo run --bin pagetray-server   # 手元で起こす
```

GitHub Actions でも同じ順に流す（`.github/workflows/ci.yml`）。試験は、試験ごとに一時の DB を作る。workflow の `image` は、`Containerfile` を arm64 の runner で build し、[image に入る物](release.md#image-に入る物) を確かめ（`ci/check-image.sh`）、空の image の中で起きて置いて読めること・止める合図で終わること・起こし直しても残ることを見る（`ci/smoke-image.sh`）。release の workflow も、同じ 2 つを push の前に流す。workflow の `cli` は、release に付ける CLI を macOS（arm64）の runner で、出す時と同じ script（`ci/build-cli.sh`）で作り、binary の形・つながる library・版・秘密の走査を確かめ（`ci/check-cli.sh`）、その binary を同じ commit のサーバーに向けて打つ。

API の形（URL・引数の名・JSON の欄）を変える時は、先に [API の仕様](api.md#api-の版を上げる時上げない時) の「API の版を上げる時・上げない時」を見る。変えたら、roster の `tpm page`（`cli/src/commands/page.ts`）と、その偽の置き場（`cli/test/helpers.ts` の `FakeStore`）も合わせて直す。roster の CI が、この repo の決めた版に向けて、つなぎの試験を流す。

- **SQL を変えた時**: SQL は `query!` が build の時に `.sqlx/` と突き合わせる。SQL か `migrations/` を変えたら、一時の DB を作って作り直し、commit する（忘れると CI の `cargo sqlx prepare --check` が落ちる）。`sqlx-cli` の版は workflow の `SQLX_CLI_VERSION` と合わせる。

  ```sh
  cargo install sqlx-cli@0.9.1 --no-default-features --features sqlite
  export DATABASE_URL=sqlite://$PWD/target/dev.db
  cargo sqlx database create && cargo sqlx migrate run
  cargo sqlx prepare
  ```

- **当てた移行の SQL は変えない**: 動いている DB が、当てた SQL の中身を覚えている。表を変える時は `migrations/` に次の番号の file を足し、`tests/migrations.rs` の一覧に 1 行足す（前からある行が変わると、試験が落ちる）。
- **表を変えたら、章も直す**: 表・列・型・空を許すか・主キーと一意と索引を変えたら、同じ PR で [DB の作り](db.md) と、表を足したなら [用語集](glossary.md) も直す。`tests/schema_doc.rs` が、移行を当てた DB と章を突き合わせ、食い違えば落ちる。
- **版を上げる**: `Cargo.toml` の `[workspace.package]` の `version` を上げ（サーバーと `crates/` の crate が同じ版になる。`Cargo.lock` も変わる）、`CHANGELOG.md` に、API の版が変わったかを書く。
- **crate を上げる**: `.cargo/config.toml` の `global-min-publish-age` が、公開から 2 日たっていない版を選ばせない（Cargo 1.100 から）。上げる時は `cargo update` → 試験 → PR。`cargo install` はこの決まりを見ないので、道具は版を書いて入れる。
- **弱点と license の見張り**: workflow の `advisories` が `cargo deny check advisories licenses`（設定は `deny.toml`）を、PR ごとと週に 1 度流す。`deny.toml` に無い license の crate が入ると落ちる。
- **workflow の action**: commit の SHA で固定する（tag は後から中身が替わりうる）。
- **main を守る決まり**: GitHub の ruleset「main: CI が通らないと merge できない」（Settings → Rules → Rulesets）が、`main` への変更に、CI の `test`・`image`・`advisories`・`docs`・`cli` の 5 つが通っていることを求める。PR は、5 つが通るまで merge できない。repo の admin は、この決まりを bypass できるので、`main` への直の push は通る（GitHub の必須の check には「PR だけに効かせる」選び方が無いので、直の push を通すには bypass になる）。admin は、わざと選べば（画面の bypass の印・`gh pr merge --admin`）PR も CI を通さずに merge できてしまうので、使わない。workflow の job の名を変えたら、ruleset の check の名も合わせて直す（合わないと、PR がいつまでも merge できない）。

## 一覧の UI の試験

一覧の UI は、HTML の字を `tests/ui.rs` が見て、browser でしか見えない物（script の動き・色・幅での並び）を `e2e/` の試験が見る。`e2e/` は [Playwright](https://playwright.dev/) の試験（TypeScript）で、同じ commit の `pagetray-server` を一時の DB で起こし、本物の browser（Chromium）で開く。Node が要るのはこの folder の中だけで、image にも CLI にも入らない。

```sh
cargo build --bin pagetray-server
cd e2e
pnpm install --frozen-lockfile
pnpm exec playwright install chromium      # 初めの 1 度だけ。試験用の Chromium を入れる
pnpm test
PAGETRAY_E2E_CHANNEL=chrome pnpm test      # この機械の Chrome を使う（Chromium を入れなくてよい）
```

- **CI**: workflow の `ui` が PR ごとに流す。落ちると、画面の絵と操作の記録が artifact（`ui-report`）に残る。`ui` は、main を守る決まりの必須の check に、まだ入れていない（時々落ちないことを見てから足す）。
- **読むだけの試験**: 題に `@smoke` が付いた試験（`e2e/tests/smoke.spec.ts`）は、何も置かず、何が置かれていても通る物だけを見る。`PAGETRAY_E2E_URL=http://127.0.0.1:8801 pnpm test` と打つと、サーバーを起こさず、動いている置き場に向けて、この試験だけが流れる（置く試験は、設定が流させない）。版を上げた後の確かめに使う（[動かす](run.md#確かめる) の「確かめる」）。
- **試験を足す**: `e2e/tests/` に `*.spec.ts` を足す。ページは `helpers.ts` の `place` で置き、区画（操作の帯・絞り込みの列・一覧の列・本文の枠と、Key の画面の file の枠・上げる枠）は `helpers.ts` の関数で選ぶ。file は `placeFile` で置き、上げる枠の試験は `input[type=file]` に `setInputFiles` で file を渡す。試験は同じ置き場に 1 本ずつ順に流れるので、試験ごとに別の project の名を使う。新着を見る試験（一覧の列の差し替えと新着の印）は、`openLive` で開いてから置く（知らせにつながる前に置いたページの知らせは届かない）。新着の印は一覧の列を下へ送っている時に出るので、`scrollList` で送ってから置く。大きな file は試験の中で作り、repo に置かない。
- **返事の状態を見る試験**（`e2e/tests/reply.spec.ts`）: 返事待ちの数は、絞り込みに関わらず置き場の全部を数え、tab の題にも出る。試験は同じ置き場に順に流れるので、返事待ちにしたページは、試験の終わりまでに答えたにする（残すと、後の試験の tab の題が変わる）。絞り込みの列の見出し「返事」は、状態を書いたページが 1 枚でもあると出るので、この file は、見出しの並びを見る `list.spec.ts` より後に流れる名にしてある。
- **送り手を見る試験**（`e2e/tests/list.spec.ts` の終わり）: 試験の置き場は、次の port の偽の castlist に送り手を聞き、一覧の UI の link の頭（`PAGETRAY_CASTLIST_LINK_URL`）は、聞く先と別の値（`helpers.ts` の `CASTLIST_LINK`。開かない）にする（`playwright.config.ts`）。偽の castlist は `helpers.ts` の `startCastlist` で、送り手を書いて置く間だけ立てる。送り手のある行が 1 つでもあると、絞り込みの列に見出し「送り手」が出るので、見出しの並びを見る試験より後に置く。
- **template を変えた時**: `templates/index.html` を変えたら、この試験も流す。試験が選ぶ手がかり（区画の id の `toolbar`・`filters`・`list`・`viewer`・`files`・`upload`、`#fresh`・`#waiting`、`li[data-path]`・`.reply`、絞り込みの列の見出しの字など）を変える時は、同じ PR で `helpers.ts` も直す。
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
- **語を足す・替える**: 先に [用語集](glossary.md) を直し、文書・註・code の名をそれに合わせる。用語集に書いた id が画面に在るか・「使わない言い方」が戻っていないかは、試験（`tests/terms.rs`）が見る。
- **章を足す**: `docs/src/` に file を置き、`SUMMARY.md` に 1 行足す。CLI の章は `docs/src/cli.md`（「使う」の節の、動かす と API の仕様 の間）。
- **repo の file を指す link**: 本の外の file（`CHANGELOG.md` など）は、GitHub の URL で書く。章どうしは、`run.md` のように file の名で書く。
- **本の試験**: `e2e/docs-tests/` の試験（Playwright）が、作った本を本物の browser で開き、どの章も開いて console に誤りが無いこと・図が描けること・検索の枠が開いて日本語と英語の語が当たること・言葉の切り替えで同じ章へ移ることを見る。`ci/docs-build.sh` の後に、`e2e/` で `pnpm test:docs` を打つ（道具の入れ方は上の「一覧の UI の試験」と同じ）。CI では job `docs` が流す。章を足したら、`e2e/docs-tests/docs.spec.ts` の章の一覧にも足す（目次と合わないと落ちる）。
- **図**: ```` ```mermaid ```` の塊で書く。構文を間違えても build は通る。描けるかは、本の試験が見る（図のある章は `docs.spec.ts` の `FIGURES` に並べる。図を足した章は、そこにも足す）。
- **検索と言葉の切り替え**: `docs/theme/site.js` は、mdBook の画面の `#mdbook-menu-bar` と `path_to_root` に頼っている。mdBook を上げても検索の枠が開くこと・言葉が切り替わることは、本の試験が見る。
