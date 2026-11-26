# pagetray

ページ（HTML）の置き場のサーバー。置かれたページを DB（SQLite）に持ち、番号を付け、絞って探せる一覧の UI を出す。
この Mac の中（`127.0.0.1`）だけで使う物で、`roster page` の命令（[roster](https://github.com/uraitakahito/roster)）が、ここの API を `curl` で呼ぶ。

A local-only server that stores HTML pages in SQLite, numbers them, and serves a searchable list UI.

## ドキュメント

使い方・API の仕様・release の出し方・開発の手引きは、docs の site にある。

- **日本語** — <https://uraitakahito.github.io/pagetray/>
- **English** — <https://uraitakahito.github.io/pagetray/en/>

site は [`docs/`](docs/)（mdBook）から作り、release の tag（`v*`）を打った時に GitHub Pages に出す。版ごとに変えたことは [CHANGELOG.md](CHANGELOG.md)。

## 動かす

Apple Container（`container` CLI）で、release の image を動かす。`<version>` は [release](https://github.com/uraitakahito/pagetray/releases) の版。

```sh
container volume create pagetray-data-v1    # 初めての時だけ
container run -d --name pagetray \
  -p 127.0.0.1:8801:8080 \
  -p 127.0.0.1:8800:8080 \
  -v pagetray-data-v1:/data \
  -m 200M -c 1 \
  ghcr.io/uraitakahito/pagetray:v<version>
curl -s http://127.0.0.1:8801/healthz    # {"status":"ok","version":"<version>"}
```

`-p` には必ず `127.0.0.1:` を書く（書かないと LAN に開く）。`8800` は前の置き場の port で、前に置いたページの URL を同じ path で開けるように、同じ container が待ち受ける。0.8.x の volume `pagetray-data` では起こさない（1.0.0 で、新しい volume `pagetray-data-v1` に移した。site の「[release を出す](https://uraitakahito.github.io/pagetray/release.html)」の「切り替え（済み）」）。digest の確かめ・送り手を castlist に聞く設定・確かめる・DB の写しを取る・写しから戻す・止める・版を上げるは、site の「[動かす](https://uraitakahito.github.io/pagetray/run.html)」にある。

## CLI

`pagetray` は、置き場を端末から操作する命令（探す・読む・置く・消す・DB の写し）。release に macOS（arm64）用の binary が付いている。入れ方と命令の一覧は、site の「[CLI](https://uraitakahito.github.io/pagetray/cli.html)」にある。

```sh
gh release download v<version> -R uraitakahito/pagetray -p 'pagetray-v<version>-aarch64-apple-darwin.tar.gz*'
shasum -a 256 -c pagetray-v<version>-aarch64-apple-darwin.tar.gz.sha256
tar -xzf pagetray-v<version>-aarch64-apple-darwin.tar.gz -C ~/.local/bin pagetray
pagetray status
```

## docs を手元で見る

道具の入れ方は、site の「[開発](https://uraitakahito.github.io/pagetray/development.html)」の「この本を直す」にある。

```sh
ci/docs-build.sh
python3 -m http.server -d docs/book    # http://localhost:8000/
```

## License

[MIT](LICENSE)
