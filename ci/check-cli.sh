#!/usr/bin/env bash
# release に付ける CLI の binary に、入ってよい物だけが入っているかを見る。1 つでも違えば exit 1（release に付けない）。
#   1. 形: macOS の arm64 の実行 file
#   2. つながる library: ci/cli-libs.txt と同じ（OS の物だけ）
#   3. 版: --version が Cargo.toml の版と同じ
#   4. 秘密の走査: binary の中の字（ci/scan-binary.sh。image の binary と同じ決まり）。
#      source の走査は ci/check-image.sh がする（同じ commit に、CI の image と release の check で必ず流れる）
# 使い方: ci/check-cli.sh <binary>
set -euo pipefail

binary=${1:?使い方: ci/check-cli.sh <binary>}
here=$(cd "$(dirname "$0")" && pwd)

fail=0
ng() {
  echo "NG: $*" >&2
  fail=1
}
say() {
  echo "$*"
  if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then echo "$*" >>"$GITHUB_STEP_SUMMARY"; fi
}

# --- 1. 形 ---
kind=$(file -b "$binary")
say "### CLI の binary"
say '```'
say "$kind"
say "$(wc -c <"$binary" | tr -d ' ') bytes"
case "$kind" in
  "Mach-O 64-bit executable arm64"*) ;;
  *) ng "macOS の arm64 の実行 file でない: $kind" ;;
esac

# --- 2. つながる library ---
libs=$(otool -L "$binary" | tail -n +2 | awk '{print $1}' | LC_ALL=C sort)
say "$libs"
say '```'
[ "$libs" = "$(LC_ALL=C sort "$here/cli-libs.txt")" ] || ng "つながる library が ci/cli-libs.txt と違う"

# --- 3. 版 ---
want="pagetray $(sed -n 's/^version = "\(.*\)"$/\1/p' "$here/../Cargo.toml" | head -1)"
got=$("$binary" --version)
[ "$got" = "$want" ] || ng "--version が「${got}」（Cargo.toml の版なら「${want}」）"

# --- 4. 秘密の走査 ---
"$here/scan-binary.sh" "$binary" || ng "binary の走査に落ちた"

if [ "$fail" -ne 0 ]; then
  echo "CLI の確かめに落ちた。この binary は出さない。" >&2
  exit 1
fi
say "CLI の確かめ: 通った（${got}）"
