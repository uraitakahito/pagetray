#!/usr/bin/env bash
# 確かめた CLI の binary（ci/check-cli.sh を通った物）を、release に付ける tar にする。
#   <出す folder>/pagetray-<tag>-aarch64-apple-darwin.tar.gz と、その .sha256
# tar に入れるのは ci/cli-contents.txt の file だけ（binary と LICENSE）。作った人の名が残らないように、
# 持ち主を root に固定し、macOS が足す付け足し（._* の file・拡張属性）を入れない。作った tar を読み直して確かめる。
# 使い方: ci/pack-cli.sh <binary> <tag> <出す folder>
set -euo pipefail

binary=${1:?使い方: ci/pack-cli.sh <binary> <tag> <出す folder>}
tag=${2:?tag（v0.3.0 など）}
out=${3:?出す folder}
here=$(cd "$(dirname "$0")" && pwd)
name="pagetray-${tag}-aarch64-apple-darwin.tar.gz"

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
cp "$binary" "$stage/pagetray"
cp "$here/../LICENSE" "$stage/LICENSE"
chmod 755 "$stage/pagetray"
chmod 644 "$stage/LICENSE"
mkdir -p "$out"
# shellcheck disable=SC2046
COPYFILE_DISABLE=1 tar -czf "$out/$name" \
  --uid 0 --gid 0 --uname root --gname wheel --no-xattrs --no-mac-metadata \
  -C "$stage" $(cat "$here/cli-contents.txt")

fail=0
ng() {
  echo "NG: $*" >&2
  fail=1
}
# 中身の一覧が、決まりと同じか。
tar -tzf "$out/$name" | LC_ALL=C sort >"$stage/contents.txt"
diff -u "$here/cli-contents.txt" "$stage/contents.txt" || ng "tar の中身が ci/cli-contents.txt と違う"
# 持ち主の名が root/wheel だけか（作った人の名が無い）。
owners=$(tar -tvzf "$out/$name" | awk '{print $3 "/" $4}' | LC_ALL=C sort -u)
[ "$owners" = "root/wheel" ] || ng "tar の持ち主が root/wheel でない: ${owners}"
# 入れた binary が、確かめた物と同じか。
mkdir "$stage/check"
tar -xzf "$out/$name" -C "$stage/check"
cmp -s "$binary" "$stage/check/pagetray" || ng "tar の中の binary が、確かめた物と違う"
if [ "$fail" -ne 0 ]; then
  rm -f "$out/$name"
  echo "tar の確かめに落ちた。release に付けない。" >&2
  exit 1
fi

(cd "$out" && shasum -a 256 "$name" >"$name.sha256")
echo "作った: ${out}/${name}（$(wc -c <"$out/$name" | tr -d ' ') bytes）"
cat "$out/$name.sha256"
