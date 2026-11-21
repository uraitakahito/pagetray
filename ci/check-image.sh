#!/usr/bin/env bash
# 出す image に、入ってよい物だけが入っているかを見る。1 つでも違えば exit 1（release は push しない）。
#   1. 中身の一覧: layer を取り出し、file の一覧が ci/image-contents.txt と同じ
#   2. 設定: 環境変数・label・Entrypoint・port などが、下の決まりと同じ
#   3. 秘密の走査: binary の中の字（ci/scan-binary.sh）と、source（gitleaks）
# 使い方: ci/check-image.sh <image> [source の folder（既定は repo の一番上）]
set -euo pipefail

image=${1:?使い方: ci/check-image.sh <image> [source の folder]}
here=$(cd "$(dirname "$0")" && pwd)
source_dir=${2:-$(dirname "$here")}

# 決まり。名前や設定を変える時は、ここと ci/image-contents.txt を一緒に直す。
BIN=pagetray-server
WANT_ENV='["PAGETRAY_DB=/data/pages.db","PAGETRAY_LISTEN=0.0.0.0:8080"]'
DEFAULT_PATH='PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin'
WANT_LAYERS=2

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
fail=0
ng() {
  echo "NG: $*" >&2
  fail=1
}
say() {
  echo "$*"
  if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then echo "$*" >>"$GITHUB_STEP_SUMMARY"; fi
}

# --- 1. 中身の一覧 ---
mkdir "$work/save" "$work/rootfs"
docker save "$image" | tar -x -C "$work/save"
mapfile -t layers < <(jq -r '.[0].Layers[]' "$work/save/manifest.json")
[ "${#layers[@]}" -eq "$WANT_LAYERS" ] || ng "layer が ${#layers[@]} 枚（決まりは $WANT_LAYERS 枚）"
for layer in "${layers[@]}"; do tar -xf "$work/save/$layer" -C "$work/rootfs"; done
(cd "$work/rootfs" && find . -mindepth 1 -printf '%y %p\n' | LC_ALL=C sort -k2) >"$work/contents.txt"
say "### image の中身（$image）"
say '```'
say "$(cat "$work/contents.txt")"
say '```'
diff -u "$here/image-contents.txt" "$work/contents.txt" || ng "中身が ci/image-contents.txt と違う"

# --- 2. 設定 ---
config=$(docker inspect --format '{{json .Config}}' "$image")
say "### image の設定"
say '```json'
say "$(jq '{Env, Labels, Entrypoint, Cmd, ExposedPorts, Volumes, User, WorkingDir}' <<<"$config")"
say '```'
check() {
  local name=$1 want=$2 got
  got=$(jq -c "$3" <<<"$config")
  [ "$got" = "$want" ] || ng "$name: $got（決まりは $want）"
}
check 環境変数 "$WANT_ENV" '[.Env[]? | select(startswith("PATH=") | not)] | sort'
check PATH '[]' "[.Env[]? | select(startswith(\"PATH=\")) | select(. != \"$DEFAULT_PATH\")]"
check label '[]' '[(.Labels // {}) | keys[] | select(IN("org.opencontainers.image.source", "org.opencontainers.image.version", "org.opencontainers.image.revision") | not)]'
check Entrypoint "[\"/$BIN\"]" '.Entrypoint'
check Cmd '[]' '.Cmd // []'
check port '["8080/tcp"]' '(.ExposedPorts // {}) | keys'
check volume '[]' '(.Volumes // {}) | keys'
check user '""' '.User // ""'

# --- 3. 秘密の走査 ---
# binary の中の字は ci/scan-binary.sh が見る（release に付ける CLI にも、同じ script を掛ける）。
"$here/scan-binary.sh" "$work/rootfs/$BIN" || ng "binary の走査に落ちた"
if command -v gitleaks >/dev/null; then
  gitleaks dir --no-banner --redact "$source_dir" || ng "gitleaks が source で見つけた"
else
  ng "gitleaks が無い（走査を飛ばさない）"
fi

if [ "$fail" -ne 0 ]; then
  echo "image の確かめに落ちた。この image は出さない。" >&2
  exit 1
fi
say "image の確かめ: 通った"
