# release を出す

image は public で、だれでも pull して中を見られる。**出すのは release の workflow（`.github/workflows/release.yml`）だけ**で、手元から `push` しない（手元の build には、commit していない file が入りうる）。

1. `Cargo.toml` の `[workspace.package]` の `version` を上げ、`CHANGELOG.md` に節を足す PR を merge する。
2. main のその commit に tag を打って push する: `git tag v<版> && git push origin v<版>`。
3. workflow の `check` が、tag と版が合うか・image の中身の一覧・設定・秘密の走査（下の「image に入る物」）・動くかを見る。**1 つでも落ちれば、`push` は動かない。**
4. `push` が、確かめたその image（作り直さない）を `ghcr.io/uraitakahito/pagetray:v<版>` に置き、release に digest と中身の一覧を書く。同じ tag がもうあれば置かない。
5. release の中身の一覧が「`pagetray` と空の `data` だけ」であることを見る（この時、image はもう public。違っていたら、すぐ下の「機微な物が出てしまった時」をする）。
6. skills の `.pagetray-version` を上げる PR を出す（つなぎの試験が、その image に向けて通る）。

- 出した tag は打ち直さない（digest が変わり、書き留めた値と合わなくなる）。直す時は、次の版を出す。
- **機微な物が出てしまった時**: GitHub の package の画面で、その版を消す。秘密なら、すぐ替える（消しても、pull した人の手元には残る前提で動く）。直して次の版を出し、`ci/` の決まりに、同じ物を止める形を足す。
- **置いた時から public**: この repo は public で、image に repo を指す label を付けて置くので、package は初めの release から、だれでも pull できる（v0.2.0 で確かめた。private の間に人が見る、という段は無い）。人が見られるのは、tag を打つ前だけ。だから、3 の確かめを push の前に置いてあり、決まりを緩める PR（`ci/` の file・`.dockerignore`・`Containerfile`）は、よく見てから merge する。

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
