# pagetray

ページ（HTML）の置き場のサーバー。置かれたページを DB（SQLite）に持ち、番号を付け、絞って探せる一覧の UI を出す。
この Mac の中（`127.0.0.1`）だけで使う物で、[skills](https://github.com/uraitakahito/skills) の `tpm page` の命令が、ここの口を `curl` で呼ぶ。

もとは skills の repo の `pagestore/` にあった物を、履歴ごと分けた。版ごとに変えたことは [CHANGELOG.md](CHANGELOG.md)。

## 動かし方

今は、skills の docs の[運用](https://uraitakahito.github.io/skills/operations/)のページの「ページの置き場」にある（image を作る・起こす・確かめる・DB の写し・写しから戻す・image を上げる）。image は、この repo の一番上で作る。

```sh
container build --progress plain -t pagetray:$(git rev-parse --short HEAD) -f Containerfile .
```

## 口

呼ぶ側が頼ってよい形と、口の版の決まりは [docs/api.md](docs/api.md)。ここは早見。

| 口 | 中身 |
|---|---|
| `POST /api/pages?project=&key=&kind=&role=&workspace=` | 本文は HTML。番号を決めて `201` と JSON を返す。`n`・`created_at` は取り込みの時だけ付け、番号が使われていれば `409` |
| `GET /api/pages` | `project`・`key`・`kind`・`role`・`workspace`・`q`・`sort`・`limit`・`before`・`after` で絞る。`{"pages":[…]}` |
| `DELETE /api/pages?project=[&key=][&kind=][&n=]` | 合うページを消し、番号を表 `removed` に残して、消した path の一覧を返す。`n` には `key` と `kind` が要る。人は見分けない（だれが消してよいかは、呼ぶ側が決める） |
| `GET /<project>/<key>/<kind>-<n>.html` | 本文。無ければ `404`、消したページは `410` |
| `GET /`・`/<project>/`・`/<project>/<key>/` | 一覧の UI（絞った一覧） |
| `GET /api/events` | 置かれるたびに 1 件を push（SSE） |
| `GET /api/backup` | DB の写し |
| `GET /api/version` | サーバーの版と口の版（`{"version":"0.2.0","api":1}`）。どの答えにも header `Pagetray-Api` が付く |
| `GET /healthz` | 生きているか |

- **この Mac の中からだけ**: `Host` が `127.0.0.1`・`localhost` でない要求と、`Origin` の付いた書く要求は断る。
- **種類と役割の名**: 一覧は持たない。字の形（英小文字で始まり、英小文字・数字・`-` で 32 字まで）だけを見る。
- **一覧の UI**: サーバーが HTML を組んで返す。絞り込み・並べ替え・検索は link と form で、条件は URL に出る（script が無くても動く）。script がするのは、新着の印・未読の印（browser が覚える）・時刻をこの Mac の時刻で出すことだけ。選んだページは、右の枠（`sandbox` を付けた iframe）に 1 枚だけ出す。
- **番号**: 書く接続は 1 本で、1 つの SQL が「今の最大 ＋ 1」を取って足す。最大は、置いてあるページ（`pages`）と消したページ（`removed`）の両方から取るので、消した番号は 2 度使わない（古い URL が別のページを指さない）。取り込みで消した番号を名指すと `409`。置いたページを書き換える口は無い。
- **消す**: 1 つの取引で、`removed` に番号を写し、`pages_fts` と `pages` の行を消す。本文は残さない（戻せない）。

## 開発

| ファイル | 中身 |
|---|---|
| `src/main.rs` | 設定を読む・起こす・止める |
| `src/lib.rs` | 道筋と、この Mac の中からの要求だけを受ける守り |
| `src/db.rs` | 接続・移行・SQL |
| `src/api.rs` | 置く・探す・消す・DB の写し |
| `src/pages.rs` | ページの本文を返す |
| `src/ui.rs`・`templates/index.html` | 一覧の UI（askama の template。build の時に検査される） |
| `src/events.rs` | 新着の push（SSE） |
| `src/text.rs` | HTML から題と検索用の字を取り出す |
| `migrations/` | 表を作る SQL（起きる時に、まだ当てていない物を当てる） |
| `.sqlx/` | SQL の検査用の file（commit する） |
| `Containerfile`・`.dockerignore` | image の作り方（静的な binary を、空の image に 1 つだけ置く）と、build の中に入れる物 |
| `ci/` | 出す image に、入ってよい物だけが入っているかを見る script と、その決まり |

Rust は `rustup` で入れる（Homebrew の `rust` は `rust-toolchain.toml` を読まない）。版は `rust-toolchain.toml` が決め、この repo の中で `cargo` を打つと、その版が入って使われる。

```sh
brew install rustup                              # keg-only。PATH に /opt/homebrew/opt/rustup/bin を足す
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
SQLX_OFFLINE=true cargo test --locked
PAGETRAY_DB=/tmp/pages.db PAGETRAY_LISTEN=127.0.0.1:18801 cargo run   # 手元で起こす
```

GitHub Actions でも同じ順に流す（`.github/workflows/ci.yml`）。試験は、試験ごとに一時の DB を作る。workflow の `image` は、`Containerfile` を arm64 の runner で build し、下の「image に入る物」を確かめ、空の image の中で起きて置いて読めること・止める合図で終わること・起こし直しても残ることを見る。

口の形（URL・引数の名・JSON の欄）を変える時は、先に [docs/api.md](docs/api.md) の「口の版を上げる時・上げない時」を見る。変えたら、skills の `tpm page`（`cli/src/commands/page.ts`）と、その偽の置き場（`cli/test/helpers.ts` の `FakeStore`）も合わせて直す。skills の CI が、この repo の決めた版に向けて、つなぎの試験を流す。

- **SQL を変えた時**: SQL は `query!` が build の時に `.sqlx/` と突き合わせる。SQL か `migrations/` を変えたら、一時の DB を作って作り直し、commit する（忘れると CI の `cargo sqlx prepare --check` が落ちる）。`sqlx-cli` の版は workflow の `SQLX_CLI_VERSION` と合わせる。

  ```sh
  cargo install sqlx-cli@0.9.1 --no-default-features --features sqlite
  export DATABASE_URL=sqlite://$PWD/target/dev.db
  cargo sqlx database create && cargo sqlx migrate run
  cargo sqlx prepare
  ```

- **当てた移行の SQL は変えない**: 動いている DB が、当てた SQL の中身を覚えている。表を変える時は `migrations/` に次の番号の file を足し、`tests/migrations.rs` の一覧に 1 行足す（前からある行が変わると、試験が落ちる）。
- **版を上げる**: `Cargo.toml` の `version` を上げ（`Cargo.lock` も変わる）、`CHANGELOG.md` に、口の版が変わったかを書く。
- **crate を上げる**: `.cargo/config.toml` の `global-min-publish-age` が、公開から 2 日たっていない版を選ばせない（Cargo 1.100 から）。上げる時は `cargo update` → 試験 → PR。`cargo install` はこの決まりを見ないので、道具は版を書いて入れる。
- **弱点の見張り**: workflow の `advisories` が `cargo deny check advisories`（設定は `deny.toml`）を、PR ごとと週に 1 度流す。
- **workflow の action**: commit の SHA で固定する（tag は後から中身が替わりうる）。

## image に入る物

image は外に出す物なので、入ってよい物を決めてあり、CI（`ci/check-image.sh`）が毎回確かめる。1 つでも違えば落ちる。

| 見る物 | 決まり | 書いてある所 |
|---|---|---|
| build の中に入る物 | 名指した source だけ（履歴・CI の設定・試験・手元の file は入れない） | `.dockerignore` |
| image の file | binary 1 つと、空の `/data` だけ。layer は 2 枚 | `ci/image-contents.txt` |
| 設定 | 環境変数は DB の path と待ち受けの 2 つだけ。label は `org.opencontainers.image.source`・`version`・`revision` だけ。開ける port は 8080 だけ | `ci/check-image.sh` の頭 |
| binary の中の字 | home の path・mail の形・秘密の鍵や token の頭が無い。gitleaks も掛ける（source にも） | `ci/strings-deny.txt`・`ci/strings-allow.txt` |

- build に秘密は要らない。`Containerfile` に `ARG` を書かず、`--build-arg`・`--secret` を使わない。
- 本物の DB やその写しを、この repo にも CI にも置かない（`.gitignore` と `.dockerignore` が `*.db` を外す）。
- 決まりを変える時（file を足す・通す字を足す）は、PR で見える形で上の file を直す。
