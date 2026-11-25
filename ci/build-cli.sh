#!/usr/bin/env bash
# release に付ける CLI（pagetray）を作る。CI の job `cli` と release の workflow が、同じこの script を使う。
# 作った binary は target/release/pagetray。
#
# Rust は、source の path を binary に埋め込む（panic の文など）。そのままだと、build した人の home の path
# （macOS の runner なら /Users/runner、手元なら自分の名）が外に出る。home と repo の path を、決まった名に付け替える。
# 付け替えが漏れたら、ci/check-cli.sh の走査が /Users/ で落とす。
# build に秘密は要らない。環境変数で値を渡さず、--locked で lock のとおりに作る。
set -euo pipefail
cd "$(dirname "$0")/.."

export RUSTFLAGS="--remap-path-prefix=$HOME=/h --remap-path-prefix=$PWD=/src"
# 1.0.0 から、出す CLI は新しい server に向けた物（package pagetray-cli-next・binary pagetray）。
cargo build --release --locked -p pagetray-cli-next
echo "作った: target/release/pagetray"
