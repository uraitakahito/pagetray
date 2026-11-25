#!/usr/bin/env bash
# 定義（model/）から、server の骨（crates/pagetray-sdk/）と、castlist を呼ぶ crate（crates/castlist-client/）と、
# pagetray を呼ぶ crate（crates/pagetray-client/。人が打つ CLI が使う）を作り直す。
# 生成した物は repo に入れ、手で直さない。
#   ci/gen.sh           castlist の定義（model/castlist/）を下の tag から取り直し、3 つの crate を作り直して置き換える
#   ci/gen.sh --check   castlist から取った定義が下の digest と合うか、作り直した物と入れてある物が同じかを見る（CI が流す）。違えば exit 1。
#                       castlist には取りに行かない（入れてある物を digest で見る）
#   ci/gen.sh --diff    定義を、定義の入った最新の release の tag（v*。版の後ろに - の付く物（rc など）は除く）と比べ、壊す変わり（DANGER 以上）があれば exit 1（CI が流す）。
#                       定義の入った tag がまだ無ければ、比べずに通す。pre-release の間は、まだ誰も使っていない形なので比べない
# 要る物: curl・unzip・shasum か sha256sum・rustfmt（rustup。rust-toolchain.toml の版）。Smithy の CLI は JRE を同梱しているので、Java は要らない。
# 生成器の版は model/smithy-build.json に書く。そこに名指した Maven の物は、公開から 2 日たっていなければ使わない
# （crate と pnpm の待ちに合わせた物。名指した物だけを見て、それが引く物は見ない）。
# 作るのは repo の中（model/build。git は無視）。字の並びは rust-toolchain.toml の版の rustfmt で整える（下の 5）。
# 作り方は castlist の ci/gen.sh と同じ（違うのは、castlist から取った定義と、作る crate が 3 つなこと）。
# macOS の bash（3.2）でも動く書き方にする。
set -euo pipefail

SMITHY_VERSION=1.74.1
# release の digest（https://github.com/smithy-lang/smithy/releases/tag/1.74.1）。
# 版は model/smithy-build.json の trait の package（smithy-aws-traits など）と同じにする。上げる時は 2 か所と digest を一緒に替える。
sha_for() {
  case "$1" in
    darwin-aarch64) echo ebb5e931936aa503055b4620c4981319f2b060656482553ada0151235ff0f1ac ;;
    darwin-x86_64) echo 06b5fa6660fd3ab0281511af1d04f3128e10fa6220c552bc2b94ff18d660886d ;;
    linux-aarch64) echo c563b5387c66fe04b8c459178dfcc08316798bc107c03fd92474dd8a0c028850 ;;
    linux-x86_64) echo 6b50c0b9f2443400bb7115b9badb54b3609816ec5fb4f625954eadd2f6b2f52a ;;
    *) return 1 ;;
  esac
}
# castlist の定義（ai.common と ai.castlist）を固める所。ここだけに書く。上げる時は tag と 2 つの digest を一緒に替え、
# ci/gen.sh を流して取った定義と 3 つの crate を作り直し、同じ commit に入れる。
CASTLIST_TAG=v1.0.0-rc.1
castlist_sha_for() {
  case "$1" in
    common.smithy) echo 6425614b79d58d7149f17396cebc3eb30e0e6c511516957d82004eb329fb66d0 ;;
    castlist.smithy) echo 1ecf61ca8243b8bdc1b84a149eb41b8ac0d0570349fccd36f4385482f0ffa53d ;;
    *) return 1 ;;
  esac
}
CASTLIST_FILES="common.smithy castlist.smithy"
CASTLIST_RAW="https://raw.githubusercontent.com/uraitakahito/castlist/$CASTLIST_TAG/model/model"
MIN_AGE_SECONDS=$((2 * 24 * 60 * 60))
# つなぎの一時の失敗（github.com や Maven Central への時間切れ）は、少し待って送り直す（castlist の AI-36 の台帳 #153 と同じ）。
# 送り直すのは時間切れ・5xx・つなぐ前の断り。404 などの書き違いは早く落とす（--retry-all-errors は使わない）。
CURL_RETRY="--retry 5 --retry-connrefused --retry-delay 5 --connect-timeout 20"

root=$(cd "$(dirname "$0")/.." && pwd)
# 作る crate: 生成器の plugin の名（同じ plugin を 2 度使う物は、smithy-build.json の `<plugin>::<名>` の名）と、置き場。
crates="rust-server-codegen:crates/pagetray-sdk rust-client-codegen:crates/castlist-client pagetray-client:crates/pagetray-client"
check=false
diff_mode=false
case "${1:-}" in
  --check) check=true ;;
  --diff) diff_mode=true ;;
  "") ;;
  *) echo "使い方: ci/gen.sh [--check|--diff]" >&2; exit 2 ;;
esac

ng() {
  echo "NG: $*" >&2
  exit 1
}
sha256() { if command -v sha256sum >/dev/null; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi; }
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# 1. castlist から取った定義（model/castlist/）。--check は入れてある物を digest で見るだけ。ほかは tag から取り直してから見る。
copies="$root/model/castlist"
if ! $check && ! $diff_mode; then
  mkdir -p "$copies"
  for f in $CASTLIST_FILES; do
    curl -fsSL $CURL_RETRY -o "$work/$f" "$CASTLIST_RAW/$f"
    got=$(sha256 "$work/$f")
    want=$(castlist_sha_for "$f")
    [ "$got" = "$want" ] || ng "castlist の $CASTLIST_TAG の $f が、固めた digest と違う（${got}。$want のはず）。model/castlist/ は替えない"
  done
  for f in $CASTLIST_FILES; do cp "$work/$f" "$copies/$f"; done
fi
for f in $CASTLIST_FILES; do
  [ -f "$copies/$f" ] || ng "castlist から取った定義が無い: model/castlist/${f}（ci/gen.sh を流す）"
  got=$(sha256 "$copies/$f")
  want=$(castlist_sha_for "$f")
  [ "$got" = "$want" ] || ng "model/castlist/$f が castlist の $CASTLIST_TAG の物と違う（${got}。$want のはず）"
done
extra=$(cd "$copies" && ls | grep -vx -e common.smithy -e castlist.smithy || true)
[ -z "$extra" ] || ng "model/castlist/ に、取った定義のほかの file がある: $extra"

# 2. Smithy の CLI を取る（版と digest を確かめ、cache に置く）。
os=$(uname -s | tr '[:upper:]' '[:lower:]')
arch=$(uname -m)
[ "$arch" = arm64 ] && arch=aarch64
platform="$os-$arch"
want=$(sha_for "$platform") || ng "Smithy の CLI の無い機械: $platform"
cache="${XDG_CACHE_HOME:-$HOME/.cache}/pagetray-smithy/$SMITHY_VERSION-$platform"
smithy="$cache/smithy-cli-$platform/bin/smithy"
# digest は取りに行った時に見る。cache（CI の actions/cache。key は ci/gen.sh の hash なので、版と digest を替えれば取り直す）から
# 戻した CLI は、そのまま使う。
if [ ! -x "$smithy" ]; then
  mkdir -p "$cache"
  zip="$cache/cli.zip"
  curl -fsSL $CURL_RETRY -o "$zip" "https://github.com/smithy-lang/smithy/releases/download/$SMITHY_VERSION/smithy-cli-$platform.zip"
  got=$(sha256 "$zip")
  [ "$got" = "$want" ] || ng "Smithy の CLI の digest が違う（${got}。$want のはず）"
  unzip -q -o "$zip" -d "$cache"
  rm -f "$zip"
fi

# 3. 生成器（Maven の物）が、公開から 2 日たっているか。
now=$(date -u +%s)
deps=$(sed -n 's/^ *"\([a-zA-Z0-9._-]*:[a-zA-Z0-9._-]*:[0-9][a-zA-Z0-9._-]*\)",\{0,1\}$/\1/p' "$root/model/smithy-build.json")
[ -n "$deps" ] || ng "model/smithy-build.json に Maven の依存が無い"
for dep in $deps; do
  group=${dep%%:*}
  rest=${dep#*:}
  artifact=${rest%%:*}
  version=${rest#*:}
  pom="https://repo1.maven.org/maven2/$(echo "$group" | tr . /)/$artifact/$version/$artifact-$version.pom"
  modified=$(curl -fsSI $CURL_RETRY "$pom" | tr -d '\r' | sed -n 's/^[Ll]ast-[Mm]odified: //p')
  [ -n "$modified" ] || ng "$dep の公開の時刻を読めない（${pom}）"
  if date -u -d "$modified" +%s >/dev/null 2>&1; then
    published=$(date -u -d "$modified" +%s)
  else
    published=$(LC_ALL=C date -u -j -f '%a, %d %b %Y %H:%M:%S GMT' "$modified" +%s)
  fi
  [ $((now - published)) -ge $MIN_AGE_SECONDS ] || ng "$dep は公開から 2 日たっていない（${modified}）"
done

# 4. 壊す変わりを見る（--diff）。
if $diff_mode; then
  base=""
  for tag in $(git -C "$root" tag --list 'v*' --sort=-version:refname | grep -v -- '-'); do
    if git -C "$root" cat-file -e "$tag:model/smithy-build.json" 2>/dev/null; then
      base=$tag
      break
    fi
  done
  if [ -z "$base" ]; then
    echo "定義の入った release の tag がまだ無いので、比べない"
    exit 0
  fi
  (cd "$root/model" && "$smithy" diff --mode git --old "$base" --severity DANGER) || ng "定義が $base から壊す形に変わった（上の DANGER・ERROR）"
  echo "定義は $base から壊す形に変わっていない"
  exit 0
fi

# 5. 作る（repo の中の model/build で。git は無視する）。
build="$root/model/build"
rm -rf "$build"
log="$build.log"
(cd "$root/model" && "$smithy" build --quiet >"$log" 2>&1) || {
  cat "$log" >&2
  rm -f "$log"
  ng "smithy build が落ちた"
}
rm -f "$log"
bad=0
for pair in $crates; do
  plugin=${pair%%:*}
  dir=${pair#*:}
  made="$build/smithy/source/$plugin"
  out="$root/$dir"
  [ -f "$made/Cargo.toml" ] || ng "$plugin の crate が作られていない"
  # 字の並びを整える。生成器も cargo fmt を流すが、作る folder が workspace の外の扱いになって落ちる（warning で続く）ので、
  # ここで rustfmt を直に流す。repo の中で打つので、rust-toolchain.toml の版の rustfmt が使われる。
  (cd "$root" && find "$made" -name '*.rs' -print0 | xargs -0 rustfmt --edition 2021) || ng "rustfmt が落ちた（${plugin}）"
  # 生成した crate には clippy を掛けない。server の骨は server の crate が引くので、`cargo clippy` は骨も workspace の member として見る。
  # 生成器が付ける断り（lib.rs の頭の allow）は、新しい Rust の lint に追いつかないので、clippy の lint をまとめて外す 1 行を、頭に足す。
  lib="$made/src/lib.rs"
  [ -f "$lib" ] || ng "$plugin の lib.rs が無い"
  { echo '#![allow(clippy::all)]'; cat "$lib"; } >"$lib.new" && mv "$lib.new" "$lib"
  # server の骨は、答えにだけ使う形の決まり（ai.common#ActorSummary の id・kind の @pattern）にも、要求を断る時の関数
  # （as_validation_exception_field）を作り、使われずに dead_code の warning になる。生成した物は直さないので、骨だけ外す。
  if [ "$plugin" = rust-server-codegen ]; then
    { echo '#![allow(dead_code)]'; cat "$lib"; } >"$lib.new" && mv "$lib.new" "$lib"
  fi
  # 呼ぶ側の crate は、既定で https の client（rustls と aws-lc）を引く。pagetray が castlist を呼ぶのも、人が打つ CLI が
  # pagetray を呼ぶのも、同じ機械の中の http:// だけなので、既定の features から外す（TLS の crate を build せず、
  # license の見張りにも入れない）。HTTP の client は、使う側（crates/server・crates/cli-next）が設定で渡す。
  if [ "$plugin" = rust-client-codegen ] || [ "$plugin" = pagetray-client ]; then
    manifest="$made/Cargo.toml"
    grep -qx 'default = \["rt-tokio", "rustls", "default-https-client"\]' "$manifest" \
      || ng "$plugin の Cargo.toml の既定の features が、思った形でない（ci/gen.sh を見直す）"
    sed 's/^default = \["rt-tokio", "rustls", "default-https-client"\]$/default = ["rt-tokio"]/' "$manifest" >"$manifest.new" \
      && mv "$manifest.new" "$manifest"
  fi
  if $check; then
    if ! diff -r "$made" "$out" >"$work/diff.txt"; then
      head -50 "$work/diff.txt" >&2
      echo "NG: $dir/ が定義と合わない。ci/gen.sh を流して、作り直した物を commit する" >&2
      bad=1
    else
      echo "$dir/ は定義（model/）と合っている"
    fi
  else
    rm -rf "$out"
    mkdir -p "$(dirname "$out")"
    cp -R "$made" "$out"
    echo "作り直した: $dir/"
  fi
done
[ "$bad" = 0 ] || exit 1
