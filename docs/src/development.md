# 開発

| ファイル | 中身 |
|---|---|
| `src/main.rs` | 設定を読む・起こす・止める（binary の名は `pagetray-server`） |
| `src/lib.rs` | 道筋と、この Mac の中からの要求だけを受ける守り |
| `src/db.rs` | 接続・移行・SQL |
| `src/api.rs` | 置く・探す・消す・DB の写し |
| `src/pages.rs` | ページの本文を返す |
| `src/ui.rs`・`templates/index.html` | 一覧の UI（askama の template。build の時に検査される） |
| `src/events.rs` | 新着の push（SSE） |
| `src/text.rs` | HTML から題と検索用の字を取り出す |
| `crates/api/` | 口の約束のうち、サーバーと CLI が共に使う物（口の版・header の名・引数の字の決まり・path の形・ページの記録の型） |
| `crates/cli/` | CLI（binary の名は `pagetray`）。置き場の口を HTTP で呼ぶ。試験は、同じ commit のサーバーを起こして打つ |
| `migrations/` | 表を作る SQL（起きる時に、まだ当てていない物を当てる） |
| `.sqlx/` | SQL の検査用の file（commit する） |
| `Containerfile`・`.dockerignore` | image の作り方（静的な binary を、空の image に 1 つだけ置く）と、build の中に入れる物 |
| `ci/` | 出す image と、release に付ける CLI に、入ってよい物だけが入っているか・動くかを見る script と、その決まり。この本を build する script（`ci/docs-build.sh`）もここ |
| `docs/` | この本（mdBook）。原文は `docs/src/` の日本語で、`docs/src/api.md` が、呼ぶ側との口の約束と、口の版の決まり。直し方は下の「この本を直す」 |
| `.claude/roles/config.jsonc` | skills の `tpm` に向けた、環境 `pagetray` の宣言 |

Rust は `rustup` で入れる（Homebrew の `rust` は `rust-toolchain.toml` を読まない）。版は `rust-toolchain.toml` が決め、この repo の中で `cargo` を打つと、その版が入って使われる。

```sh
brew install rustup                              # keg-only。PATH に /opt/homebrew/opt/rustup/bin を足す
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
SQLX_OFFLINE=true cargo test --locked --workspace
PAGETRAY_DB=/tmp/pages.db PAGETRAY_LISTEN=127.0.0.1:18801 cargo run --bin pagetray-server   # 手元で起こす
```

GitHub Actions でも同じ順に流す（`.github/workflows/ci.yml`）。試験は、試験ごとに一時の DB を作る。workflow の `image` は、`Containerfile` を arm64 の runner で build し、[image に入る物](release.md#image-に入る物) を確かめ（`ci/check-image.sh`）、空の image の中で起きて置いて読めること・止める合図で終わること・起こし直しても残ることを見る（`ci/smoke-image.sh`）。release の workflow も、同じ 2 つを push の前に流す。workflow の `cli` は、release に付ける CLI を macOS（arm64）の runner で、出す時と同じ script（`ci/build-cli.sh`）で作り、binary の形・つながる library・版・秘密の走査を確かめ（`ci/check-cli.sh`）、その binary を同じ commit のサーバーに向けて打つ。

口の形（URL・引数の名・JSON の欄）を変える時は、先に [口の約束](api.md#口の版を上げる時上げない時) の「口の版を上げる時・上げない時」を見る。変えたら、skills の `tpm page`（`cli/src/commands/page.ts`）と、その偽の置き場（`cli/test/helpers.ts` の `FakeStore`）も合わせて直す。skills の CI が、この repo の決めた版に向けて、つなぎの試験を流す。

- **SQL を変えた時**: SQL は `query!` が build の時に `.sqlx/` と突き合わせる。SQL か `migrations/` を変えたら、一時の DB を作って作り直し、commit する（忘れると CI の `cargo sqlx prepare --check` が落ちる）。`sqlx-cli` の版は workflow の `SQLX_CLI_VERSION` と合わせる。

  ```sh
  cargo install sqlx-cli@0.9.1 --no-default-features --features sqlite
  export DATABASE_URL=sqlite://$PWD/target/dev.db
  cargo sqlx database create && cargo sqlx migrate run
  cargo sqlx prepare
  ```

- **当てた移行の SQL は変えない**: 動いている DB が、当てた SQL の中身を覚えている。表を変える時は `migrations/` に次の番号の file を足し、`tests/migrations.rs` の一覧に 1 行足す（前からある行が変わると、試験が落ちる）。
- **版を上げる**: `Cargo.toml` の `[workspace.package]` の `version` を上げ（サーバーと `crates/` の crate が同じ版になる。`Cargo.lock` も変わる）、`CHANGELOG.md` に、口の版が変わったかを書く。
- **crate を上げる**: `.cargo/config.toml` の `global-min-publish-age` が、公開から 2 日たっていない版を選ばせない（Cargo 1.100 から）。上げる時は `cargo update` → 試験 → PR。`cargo install` はこの決まりを見ないので、道具は版を書いて入れる。
- **弱点と license の見張り**: workflow の `advisories` が `cargo deny check advisories licenses`（設定は `deny.toml`）を、PR ごとと週に 1 度流す。`deny.toml` に無い license の crate が入ると落ちる。
- **workflow の action**: commit の SHA で固定する（tag は後から中身が替わりうる）。
- **main を守る決まり**: GitHub の ruleset「main: CI が通らないと merge できない」（Settings → Rules → Rulesets）が、`main` への変更に、CI の `test`・`image`・`advisories`・`docs`・`cli` の 5 つが通っていることを求める。PR は、5 つが通るまで merge できない。repo の admin は、この決まりを bypass できるので、`main` への直の push は通る（GitHub の必須の check には「PR だけに効かせる」選び方が無いので、直の push を通すには bypass になる）。admin は、わざと選べば（画面の bypass の印・`gh pr merge --admin`）PR も CI を通さずに merge できてしまうので、使わない。workflow の job の名を変えたら、ruleset の check の名も合わせて直す（合わないと、PR がいつまでも merge できない）。

## この本を直す

この本は [mdBook](https://rust-lang.github.io/mdBook/) で作り、release の tag（`v*`）を打った時に、workflow（`.github/workflows/docs.yml`）が GitHub Pages に出す。PR と main への push では、build できるかと、英語の訳が原文と合っているかを見る（job の名は `docs`）。

| ファイル | 中身 |
|---|---|
| `docs/book.toml` | 本の設定。組み込みの検索は切ってある（日本語が引けないため） |
| `docs/src/` | 原文（日本語）。目次は `SUMMARY.md` |
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
- **章を足す**: `docs/src/` に file を置き、`SUMMARY.md` に 1 行足す。CLI の章は `docs/src/cli.md`（「使う」の節の、動かす と 口の約束 の間）。
- **repo の file を指す link**: 本の外の file（`CHANGELOG.md` など）は、GitHub の URL で書く。章どうしは、`run.md` のように file の名で書く。
- **図**: ```` ```mermaid ```` の塊で書く。構文を間違えても build は通るので、変えたら browser で見る。
- **検索と言葉の切り替え**: `docs/theme/site.js` は、mdBook の画面の `#mdbook-menu-bar` と `path_to_root` に頼っている。mdBook を上げたら、検索の枠が開くこと・言葉が切り替わることを browser で見る。
