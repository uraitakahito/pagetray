# release を出す

image と、release に付ける CLI の binary は public で、だれでも取って中を見られる。**出すのは release の workflow（`.github/workflows/release.yml`）だけ**で、手元から `push` せず、release に file を手で付けない（手元の build には、commit していない file と、自分の home の path が入りうる）。

1. `Cargo.toml` の `[workspace.package]` の `version` を上げ、`CHANGELOG.md` に節を足す PR を merge する。
2. main のその commit に tag を打って push する: `git tag v<version> && git push origin v<version>`。
3. workflow の `check` が、tag と版が合うか・image の中身の一覧・設定・秘密の走査（下の「image に入る物」）・動くかを見る。`check-cli` が、CLI の binary の形・つながる library・版・秘密の走査（下の「release に付ける CLI に入る物」）・動くかを見る。**1 つでも落ちれば、`push` は動かず、image も CLI も出ない。**
4. `push` が、確かめたその image（作り直さない）を `ghcr.io/uraitakahito/pagetray:v<version>` に置き、release に digest と中身の一覧を書き、確かめたその CLI の tar と checksum を付ける。同じ tag がもうあれば置かない。
5. release の中身の一覧が「`pagetray-server` と空の `data` だけ」であることと、付いた file が `pagetray-v<version>-aarch64-apple-darwin.tar.gz` とその `.sha256` の 2 つだけであることを見る（この時、image も CLI ももう public。違っていたら、すぐ下の「機微な物が出てしまった時」をする）。
6. roster の `.pagetray-version` を上げる PR を出す（つなぎの試験が、その image に向けて通る）。

- 出した tag は打ち直さない（digest が変わり、書き留めた値と合わなくなる）。直す時は、次の版を出す。
- **rc（作り直しの間。AI-36）**: tag に `-rc.` の付く物（`v<版>-rc.<n>`。`Cargo.toml` の版も `CHANGELOG.md` の節も `<版>-rc.<n>`）は、image を新しい server の `Containerfile.next` で作り、`ci/smoke-image.sh <image> next`（新しい API の形）で確かめ、GitHub の pre-release にする。ほかの tag は、今までどおり `Containerfile` の今の server。release に付ける CLI は、どちらの tag でも今の CLI（`crates/cli`）。確かめと置く job の分け方・版と CHANGELOG の突き合わせ・main の上にあるか・同じ tag を置き直さないことは、どちらも同じ。 **手順 6（roster の `.pagetray-version` を上げる）はしない**: 上げると、今の API を話す roster のつなぎの試験が落ち、運用の image を作り直すと新しい server が今の volume で起きずに置き場が止まる。上げるのは切り替え（AI-36 の段 8）の時、新しい volume と一緒に。
- **rc を動かす時と切り替え**: rc の image の中の binary も `/pagetray-server`（`pagetray-next` を、この名で置く）で、環境変数と port も同じなので、[動かす](run.md) の命令で替わるのは image の版と、**volume** だけ。
  1. **volume は新しい物（`pagetray-data-v1`）にする。新しい server を、今の volume（`pagetray-data`）で起こさない。** 今の DB は新しい server の形でない（新しい server は、今の server の移行を当てた DB を、何も書かずに断って止まる）。今の volume は、戻す時のために、そのまま残す。
  2. 先にデータを移す: 今の server の `GET /api/backup` で写しを取り（[動かす](run.md) の「DB の写しを取る」）、同じ rc の image に `migrate --from <写し> --to /data/pages.db` を付けて、新しい volume と写しの folder を付けて起こす（例 `container run --rm -v pagetray-data-v1:/data -v "<backup-dir>:/in:ro" ghcr.io/uraitakahito/pagetray:v<版>-rc.<n> migrate --from /in/<backup-file> --to /data/pages.db`）。写しは `-wal`・`-shm` の無い 1 つの file で渡す。表ごとの行の数と digest・本文の SHA-256 などの突き合わせ（[新しい API の形](next.md) の「データ」）が 1 つでも合わなければ、何も足さずに 1 で終わる（新しい volume に作りかけた DB は消す）。**1 で終わったら 3 に進まない（rc を起こさない）。** 訳を直してから、もう 1 度 2 をする。出た行は、予行の時と見比べられるように残す。
  3. 移し終えてから、新しい volume で rc の image を起こす。
- **段 8 までに今の server を直して出す時**: 版は workspace で 1 つ（`Cargo.toml` の `[workspace.package]`）なので、main の版は rc のまま。今の server の直しを出すには、版を `0.8.<n>` にして `CHANGELOG.md` に節を足す PR を merge し、`v0.8.<n>` を打つ（`-rc.` の無い tag は今の `Containerfile` で作る）。`CHANGELOG.md` の `## 0.8.<n>` の節は、いちばん上（`## 1.0.0-rc.<n>` の上。file は出した順）に置く。その merge の後は main の版が `0.8.<n>` になるので、次の rc の PR で版を `1.0.0-rc.<n+1>` にし、rc の節を足して打つ。
- **機微な物が出てしまった時**: GitHub の package の画面で、その版を消す。CLI の方なら、release の画面で付いた file を消す。秘密なら、すぐ替える（消しても、pull した人の手元には残る前提で動く）。直して次の版を出し、`ci/` の決まりに、同じ物を止める形を足す。
- **置いた時から public**: この repo は public で、image に repo を指す label を付けて置くので、package は初めの release から、だれでも pull できる（v0.2.0 で確かめた。private の間に人が見る、という段は無い）。人が見られるのは、tag を打つ前だけ。だから、3 の確かめを push の前に置いてあり、決まりを緩める PR（`ci/` の file・`.dockerignore`・`Containerfile`）は、よく見てから merge する。CLI の binary も、release に付いた時から public。

## image に入る物

image は外に出す物なので、入ってよい物を決めてあり、CI（`ci/check-image.sh`）が毎回確かめる。1 つでも違えば落ちる。rc の image（`Containerfile.next`）も同じ決まり（binary の名も `pagetray-server`）で、CI の job `image-next` が毎回確かめる。

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
