# release を出す

image と、release に付ける CLI の binary は public で、だれでも取って中を見られる。**出すのは release の workflow（`.github/workflows/release.yml`）だけ**で、手元から `push` せず、release に file を手で付けない（手元の build には、commit していない file と、自分の home の path が入りうる）。

1. `Cargo.toml` の `[workspace.package]` の `version` を上げ、`CHANGELOG.md` に節を足す PR を merge する。
2. main のその commit に tag を打って push する: `git tag v<version> && git push origin v<version>`。
3. workflow の `check` が、tag と版が合うか・image の中身の一覧・設定・秘密の走査（下の「image に入る物」）・動くかを見る。`check-cli` が、CLI の binary の形・つながる library・版・秘密の走査（下の「release に付ける CLI に入る物」）・動くかを見る。**1 つでも落ちれば、`push` は動かず、image も CLI も出ない。**
4. `push` が、確かめたその image（作り直さない）を `ghcr.io/uraitakahito/pagetray:v<version>` に置き、release に digest と中身の一覧を書き、確かめたその CLI の tar と checksum を付ける。同じ tag がもうあれば置かない。
5. release の中身の一覧が「`pagetray-server` と空の `data` だけ」であることと、付いた file が `pagetray-v<version>-aarch64-apple-darwin.tar.gz` とその `.sha256` の 2 つだけであることを見る（この時、image も CLI ももう public。違っていたら、すぐ下の「機微な物が出てしまった時」をする）。
6. skills の `.pagetray-version` を上げる PR を出す（つなぎの試験が、その image に向けて通る）。

- 出した tag は打ち直さない（digest が変わり、書き留めた値と合わなくなる）。直す時は、次の版を出す。
- **機微な物が出てしまった時**: GitHub の package の画面で、その版を消す。CLI の方なら、release の画面で付いた file を消す。秘密なら、すぐ替える（消しても、pull した人の手元には残る前提で動く）。直して次の版を出し、`ci/` の決まりに、同じ物を止める形を足す。
- **置いた時から public**: この repo は public で、image に repo を指す label を付けて置くので、package は初めの release から、だれでも pull できる（v0.2.0 で確かめた。private の間に人が見る、という段は無い）。人が見られるのは、tag を打つ前だけ。だから、3 の確かめを push の前に置いてあり、決まりを緩める PR（`ci/` の file・`.dockerignore`・`Containerfile`）は、よく見てから merge する。CLI の binary も、release に付いた時から public。

## image に入る物

image は外に出す物なので、入ってよい物を決めてあり、CI（`ci/check-image.sh`）が毎回確かめる。1 つでも違えば落ちる。

| 見る物 | 決まり | 書いてある所 |
|---|---|---|
| build の中に入る物 | 名指した source だけ（履歴・CI の設定・試験・手元の file は入れない） | `.dockerignore` |
| image の file | binary 1 つ（`pagetray-server`）と、空の `/data` だけ。layer は 2 枚 | `ci/image-contents.txt` |
| 設定 | 環境変数は DB の path と待ち受けの 2 つだけ。label は `org.opencontainers.image.source`・`version`・`revision` だけ。開ける port は 8080 だけ | `ci/check-image.sh` の頭 |
| binary の中の字 | home の path・mail の形・秘密の鍵や token の頭が無い。gitleaks も掛ける（source にも） | `ci/strings-deny.txt`・`ci/strings-allow.txt` |

- build に秘密は要らない。`Containerfile` に `ARG` を書かず、`--build-arg`・`--secret` を使わない。label は、release の workflow が `--label` で付ける 3 つだけ。
- 出どころの記録（provenance・SBOM）は付けない。付ける時は、中身を見てから。
- release の workflow は、確かめる job（書き込みの権限が無い）と、置く job（`packages: write`）に分かれている。置く job は、確かめた image の file を受け取って、そのまま置く。
- 本物の DB やその写しを、この repo にも CI にも置かない（`.gitignore` と `.dockerignore` が `*.db` を外す）。
- 決まりを変える時（file を足す・通す字を足す）は、PR で見える形で上の file を直す。

## release に付ける CLI に入る物

CLI（`pagetray`。macOS の arm64 用）も外に出す物なので、image と同じに、入ってよい物を決めてある。CI の job `cli` が PR ごとに、release の `check-cli` が出す前に、同じ script で確かめる。1 つでも違えば落ちる。

| 見る物 | 決まり | 書いてある所 |
|---|---|---|
| build | home と repo の path を、決まった名に付け替えて作る（Rust は source の path を binary に埋め込むので、そのままだと build した人の home の path が出る）。秘密は渡さない | `ci/build-cli.sh` |
| binary の形 | macOS の arm64 の実行 file。つながる library は OS の物だけ。`--version` が `Cargo.toml` の版と同じ | `ci/check-cli.sh`・`ci/cli-libs.txt` |
| binary の中の字 | image の binary と同じ決まり（home の path・mail の形・秘密の鍵や token の頭が無い。gitleaks も掛ける）。同じ script を使う | `ci/scan-binary.sh`・`ci/strings-deny.txt`・`ci/strings-allow.txt` |
| tar の中身 | `pagetray` と `LICENSE` だけ。持ち主は `root`/`wheel` に固定し、macOS が足す付け足しの file を入れない | `ci/pack-cli.sh`・`ci/cli-contents.txt` |
| 動くか | その binary を、同じ commit のサーバーに向けて打つ（CLI の試験） | `crates/cli/tests/` |

- `check-cli` は、書き込みの権限も秘密も持たない。`push` は、渡された tar の checksum を見てから、作り直さずに release に付ける。
- CLI は、認証の token も設定の file も持たない。build の時に binary に焼き込む値は、版だけ。
- 署名は linker が付ける ad-hoc の物だけで、公証はしていない。出どころの記録（provenance）も付けない。
