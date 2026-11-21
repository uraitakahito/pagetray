# pagetray

ページ（HTML）の置き場のサーバー。置かれたページを DB（SQLite）に持ち、番号を付け、絞って探せる一覧の UI を出す。
この Mac の中（`127.0.0.1`）だけで使う物で、[skills](https://github.com/uraitakahito/skills) の `tpm page` の命令が、ここの口を `curl` で呼ぶ。

もとは skills の repo の `pagestore/` にあった物を、履歴ごと分けた。版ごとに変えたことは [CHANGELOG.md](CHANGELOG.md)。

## 動かす

Apple Container（`container` CLI）で、release の image を動かす。`127.0.0.1` だけで待ち受け、LAN にもインターネットにも開かない。

| 物 | 値 |
|---|---|
| image | `ghcr.io/uraitakahito/pagetray:v<版>`（[release](https://github.com/uraitakahito/pagetray/releases) に digest がある。arm64） |
| container | `pagetray`（`127.0.0.1:8801` と `127.0.0.1:8800` → 中の 8080。メモリは Apple Container の下限の 200 MiB、CPU は 1） |
| URL | `http://127.0.0.1:8801/`（一覧の UI。ページは `/<project>/<key>/<kind>-<n>.html`）。`8800` は、前の置き場（Nginx。今は無い）の port。前に置いたページの URL を同じ path で開けるように、同じ container が待ち受ける |
| DB | 名前つきの volume `pagetray-data`（中の `/data/pages.db`）。Mac の folder は mount しない |
| 設定 | 環境変数 `PAGETRAY_DB`（既定 `/data/pages.db`）・`PAGETRAY_LISTEN`（既定 `0.0.0.0:8080`）。image の既定のままでよい |

skills の `tpm` を使う Mac では、container と volume に触る命令を `tpm infra run pagetray -- <命令>` で打つ（この repo の `.claude/roles/config.jsonc` と、skills の設定が、環境 `pagetray` を「TPM に相談」の手続きで宣言している）。下の命令は、その頭を省いて書いてある。使う版は、skills の `.pagetray-version`（skills の cli と合わせてある版）。

### 起こす

```sh
container image pull ghcr.io/uraitakahito/pagetray:v<版>
container image inspect ghcr.io/uraitakahito/pagetray:v<版> | grep -o '"digest" *: *"sha256:[0-9a-f]*"'   # release の digest がこの中にあるか
container volume create pagetray-data                           # 初めての時だけ
container run -d --name pagetray \
  -p 127.0.0.1:8801:8080 \
  -p 127.0.0.1:8800:8080 \
  -v pagetray-data:/data \
  -m 200M -c 1 \
  ghcr.io/uraitakahito/pagetray:v<版>
```

- digest の見方: `container image list` に出る digest は、Apple Container が付ける別の値。release の digest は、`container image inspect` の `variants` の中の `digest` に出る。
- `-p` には必ず `127.0.0.1:` を書く。`-p 8801:8080` と書くと LAN に開く。
- 8801 が使われていたら別の port にする。skills の担当が使う URL は、環境変数 `TPM_PAGES_URL` で合わせる。
- 起きる時に、まだ当てていない移行の SQL（`migrations/`）を DB に当てる。

### 確かめる

1. `container list` で `pagetray` が `running`。
2. `lsof -nP -iTCP:8801 -iTCP:8800 -sTCP:LISTEN` が `127.0.0.1:8801` と `127.0.0.1:8800` だけを出す（`*:` で始まる行があれば LAN に開いている）。
3. `curl -s http://127.0.0.1:8801/healthz` と `curl -s http://127.0.0.1:8800/healthz` が `ok` を返し、`curl -s http://127.0.0.1:8801/api/version` が、起こした版と `"api":1` を返す。browser で `http://127.0.0.1:8801/` が一覧を出す。
4. `container logs pagetray` の頭に `pagetray <版> listen=0.0.0.0:8080 db=/data/pages.db` が出ている。
5. `container stats --no-stream pagetray` のメモリが数 MiB〜20 MiB ほど（初めて起こした直後と、たくさん置いた直後は、OS が file を覚えている分で 50 MiB を超えることがある。起こし直すと戻る）。

### DB の写しを取る

**版を上げる前と、volume に触る前**に取る。動かしたまま取れる。

```sh
curl -fsS -o ~/pagetray-$(date +%Y%m%d-%H%M).db http://127.0.0.1:8801/api/backup
sqlite3 ~/pagetray-<日時>.db 'select count(*) from pages'    # 開けて、件数が合うか
```

写しには、置かれたページの本文がみな入っている。この repo にも CI にも置かない。

### 写しから戻す

volume を失った・DB が壊れた時。戻す間、ページは置けない。

```sh
container stop pagetray
container run --rm \
  -v pagetray-data:/data -v "<写しのある folder>:/in:ro" \
  docker.io/library/rust:1.100.0-alpine \
  sh -c 'rm -f /data/pages.db-wal /data/pages.db-shm && cp /in/<写しの file> /data/pages.db'
container start pagetray
```

- 動かす image は空で `cp` も無いので、file を volume に入れる時だけ、shell のある image を借りる（上は `Containerfile` の build の段と同じ物。何でもよい）。
- 起こしたら「確かめる」をし、`curl -s 'http://127.0.0.1:8801/api/pages?limit=1'` で一番新しいページを見る。
- 写しが無ければ、空の volume で起こす。skills の `tpm page import` が、控えの folder のページを同じ番号で入れ直す（置いた役割の記録は戻らない）。

### 止める・Mac を起こし直した後

- 止めている間、ページは置けず、一覧の UI も開けない（番号をサーバーが決めるため）。止めるのは `container stop pagetray`。DB は volume に残る。
- Mac を起こし直すと止まったままになる。`container system start` の後に `container start pagetray` を打ち、「確かめる」をする。
- volume（`pagetray-data`）は消さない。

### 版を上げる

1. [CHANGELOG.md](CHANGELOG.md) で、口の版が変わるか・移行の SQL が足されるかを見る（移行が足された版は、当てた後に前の版へ戻せない）。
2. 新しい版の image を pull し、digest を release の値と見比べる。
3. 「DB の写しを取る」をする。
4. `container stop pagetray`・`container delete pagetray` の後、新しい版で「起こす」の `container run` を打ち、「確かめる」をする（volume はそのまま付く）。
5. 駄目なら、`container delete pagetray` の後、前の版で `container run` を打つ（移行が足されていなければ、同じ volume で起きる。足されていたら、3 の写しから戻す）。
6. 古い image は `container image delete` で消す。

## release を出す

image は public で、だれでも pull して中を見られる。**出すのは release の workflow（`.github/workflows/release.yml`）だけ**で、手元から `push` しない（手元の build には、commit していない file が入りうる）。

1. `Cargo.toml` の `version` を上げ、`CHANGELOG.md` に節を足す PR を merge する。
2. main のその commit に tag を打って push する: `git tag v<版> && git push origin v<版>`。
3. workflow の `check` が、tag と版が合うか・image の中身の一覧・設定・秘密の走査（下の「image に入る物」）・動くかを見る。**1 つでも落ちれば、`push` は動かない。**
4. `push` が、確かめたその image（作り直さない）を `ghcr.io/uraitakahito/pagetray:v<版>` に置き、release に digest と中身の一覧を書く。同じ tag がもうあれば置かない。
5. release の中身の一覧が「`pagetray` と空の `data` だけ」であることを見る（この時、image はもう public。違っていたら、すぐ下の「機微な物が出てしまった時」をする）。
6. skills の `.pagetray-version` を上げる PR を出す（つなぎの試験が、その image に向けて通る）。

- 出した tag は打ち直さない（digest が変わり、書き留めた値と合わなくなる）。直す時は、次の版を出す。
- **機微な物が出てしまった時**: GitHub の package の画面で、その版を消す。秘密なら、すぐ替える（消しても、pull した人の手元には残る前提で動く）。直して次の版を出し、`ci/` の決まりに、同じ物を止める形を足す。
- **置いた時から public**: この repo は public で、image に repo を指す label を付けて置くので、package は初めの release から、だれでも pull できる（v0.2.0 で確かめた。private の間に人が見る、という段は無い）。人が見られるのは、tag を打つ前だけ。だから、3 の確かめを push の前に置いてあり、決まりを緩める PR（`ci/` の file・`.dockerignore`・`Containerfile`）は、よく見てから merge する。

## 口

呼ぶ側が頼ってよい形と、口の版の決まりは [docs/src/api.md](docs/src/api.md)。ここは早見。

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
| `ci/` | 出す image に、入ってよい物だけが入っているか・動くかを見る script と、その決まり |
| `docs/` | 文書の本（mdBook）。`docs/src/api.md` が、呼ぶ側との口の約束と、口の版の決まり。作り方は `docs/src/development.md` の「この本を直す」 |
| `.claude/roles/config.jsonc` | skills の `tpm` に向けた、環境 `pagetray` の宣言 |

Rust は `rustup` で入れる（Homebrew の `rust` は `rust-toolchain.toml` を読まない）。版は `rust-toolchain.toml` が決め、この repo の中で `cargo` を打つと、その版が入って使われる。

```sh
brew install rustup                              # keg-only。PATH に /opt/homebrew/opt/rustup/bin を足す
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
SQLX_OFFLINE=true cargo test --locked
PAGETRAY_DB=/tmp/pages.db PAGETRAY_LISTEN=127.0.0.1:18801 cargo run   # 手元で起こす
```

GitHub Actions でも同じ順に流す（`.github/workflows/ci.yml`）。試験は、試験ごとに一時の DB を作る。workflow の `image` は、`Containerfile` を arm64 の runner で build し、下の「image に入る物」を確かめ（`ci/check-image.sh`）、空の image の中で起きて置いて読めること・止める合図で終わること・起こし直しても残ることを見る（`ci/smoke-image.sh`）。release の workflow も、同じ 2 つを push の前に流す。

口の形（URL・引数の名・JSON の欄）を変える時は、先に [docs/src/api.md](docs/src/api.md) の「口の版を上げる時・上げない時」を見る。変えたら、skills の `tpm page`（`cli/src/commands/page.ts`）と、その偽の置き場（`cli/test/helpers.ts` の `FakeStore`）も合わせて直す。skills の CI が、この repo の決めた版に向けて、つなぎの試験を流す。

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
- **main を守る決まり**: GitHub の ruleset「main: CI が通らないと merge できない」（Settings → Rules → Rulesets）が、`main` への変更に、CI の `test`・`image`・`advisories` の 3 つが通っていることを求める。PR は、3 つが通るまで merge できない。repo の admin は、この決まりを bypass できるので、`main` への直の push は通る（GitHub の必須の check には「PR だけに効かせる」選び方が無いので、直の push を通すには bypass になる）。admin は、わざと選べば（画面の bypass の印・`gh pr merge --admin`）PR も CI を通さずに merge できてしまうので、使わない。workflow の job の名を変えたら、ruleset の check の名も合わせて直す（合わないと、PR がいつまでも merge できない）。

## image に入る物

image は外に出す物なので、入ってよい物を決めてあり、CI（`ci/check-image.sh`）が毎回確かめる。1 つでも違えば落ちる。

| 見る物 | 決まり | 書いてある所 |
|---|---|---|
| build の中に入る物 | 名指した source だけ（履歴・CI の設定・試験・手元の file は入れない） | `.dockerignore` |
| image の file | binary 1 つと、空の `/data` だけ。layer は 2 枚 | `ci/image-contents.txt` |
| 設定 | 環境変数は DB の path と待ち受けの 2 つだけ。label は `org.opencontainers.image.source`・`version`・`revision` だけ。開ける port は 8080 だけ | `ci/check-image.sh` の頭 |
| binary の中の字 | home の path・mail の形・秘密の鍵や token の頭が無い。gitleaks も掛ける（source にも） | `ci/strings-deny.txt`・`ci/strings-allow.txt` |

- build に秘密は要らない。`Containerfile` に `ARG` を書かず、`--build-arg`・`--secret` を使わない。label は、release の workflow が `--label` で付ける 3 つだけ。
- 出どころの記録（provenance・SBOM）は付けない。付ける時は、中身を見てから。
- release の workflow は、確かめる job（書き込みの権限が無い）と、置く job（`packages: write`）に分かれている。置く job は、確かめた image の file を受け取って、そのまま置く。
- 本物の DB やその写しを、この repo にも CI にも置かない（`.gitignore` と `.dockerignore` が `*.db` を外す）。
- 決まりを変える時（file を足す・通す字を足す）は、PR で見える形で上の file を直す。

## License

[MIT](LICENSE)
