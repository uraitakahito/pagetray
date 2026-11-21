#!/usr/bin/env bash
# 外に出す binary（image の中のサーバー・release に付ける CLI）の中の字に、あってはいけない形が無いかを見る。
#   1. ci/strings-deny.txt の形（home の path・mail の形・秘密の鍵や token の頭）が無い。通してよい字は ci/strings-allow.txt
#   2. gitleaks を、binary の中の字に掛ける
# 1 つでも当たれば exit 1。gitleaks が無い時も落とす（走査を飛ばさない）。
# macOS の bash（3.2）でも動く書き方にする。
# 使い方: ci/scan-binary.sh <binary>
set -euo pipefail

binary=${1:?使い方: ci/scan-binary.sh <binary>}
here=$(cd "$(dirname "$0")" && pwd)

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
fail=0
ng() {
  echo "NG: $*" >&2
  fail=1
}

# -a: file の全部を読む（macOS の strings は、既定では binary の一部の区画しか読まない）。
strings -a -n 6 "$binary" >"$work/strings.txt"
[ -s "$work/strings.txt" ] || ng "binary から字を取り出せない: $binary"
patterns() { grep -v -E '^(#|$)' "$1" || true; }
patterns "$here/strings-deny.txt" >"$work/deny"
patterns "$here/strings-allow.txt" >"$work/allow"
grep -E -f "$work/deny" "$work/strings.txt" >"$work/hits.txt" || true
if [ -s "$work/allow" ]; then
  grep -v -E -f "$work/allow" "$work/hits.txt" >"$work/left.txt" || true
else
  cp "$work/hits.txt" "$work/left.txt"
fi
if [ -s "$work/left.txt" ]; then
  # 当たった字そのものは log に出さない（public な log に写るので）。どの形に何行当たったかだけを出す。
  while IFS= read -r pattern; do
    n=$(grep -c -E -e "$pattern" "$work/left.txt" || true)
    [ "$n" -eq 0 ] || echo "  $pattern: $n 行" >&2
  done <"$work/deny"
  ng "binary の中に、あってはいけない形の字がある（手元で流すと中身が見られる: CHECK_IMAGE_SHOW=1 ci/scan-binary.sh ${binary}）"
  if [ -n "${CHECK_IMAGE_SHOW:-}" ]; then cat "$work/left.txt" >&2; fi
fi
if command -v gitleaks >/dev/null; then
  gitleaks dir --no-banner --redact "$work/strings.txt" || ng "gitleaks が binary の中の字で見つけた"
else
  ng "gitleaks が無い（走査を飛ばさない）"
fi

if [ "$fail" -ne 0 ]; then
  echo "binary の走査に落ちた: $binary" >&2
  exit 1
fi
echo "binary の走査: 通った（$(wc -l <"$work/strings.txt" | tr -d ' ') 行の字を見た）"
