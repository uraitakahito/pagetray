#!/usr/bin/env bash
# この本の訳（docs/po/en.po）を、原文（docs/src/）に合わせる。
#   ci/docs-po.sh           原文から段落を取り出し、en.po に足す・古い物に印を付ける。この後、en.po を訳す。
#   ci/docs-po.sh --check   原文のどの段落にも、古くない訳があるか・訳に日本語が残っていないかを見る（CI が流す）。en.po は変えない。
# 要る道具（PATH に）: mdbook・mdbook-mermaid・mdbook-xgettext・gettext（msgmerge・msgattrib・msgcmp・msgexec）・perl。
set -euo pipefail
cd "$(dirname "$0")/../docs"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
# 原文を、訳す単位（段落・表の升・コードの塊）に切り出す。行の番号は書かない（原文を 1 行足すたびに、en.po が丸ごと変わるため）。
MDBOOK_OUTPUT='{"xgettext": {"granularity": 0}}' mdbook build -d "$tmp" >/dev/null
pot="$tmp/messages.pot"

if [ "${1:-}" = "--check" ]; then
  # 原文にある段落が、en.po に無い・訳が空・古い（fuzzy）なら落ちる。
  if ! msgcmp po/en.po "$pot"; then
    echo "docs/po/en.po が原文と合っていない。ci/docs-po.sh を流し、足された所・fuzzy の所を訳す（fuzzy の印は消す）。" >&2
    exit 1
  fi
  # 原文を写しただけの訳を通さない: 英語の訳に、かな・漢字が残っていれば落ちる。
  if msgexec --input=po/en.po cat | perl -CSD -ne 'if (/[\p{Hiragana}\p{Katakana}\p{Han}]/) { print; $n = 1 } END { exit !$n }'; then
    echo "docs/po/en.po の訳に、日本語が残っている（上の行）。" >&2
    exit 1
  fi
  echo "docs/po/en.po は原文と合っている"
  exit 0
fi

if [ -f po/en.po ]; then
  msgmerge --quiet --update --backup=none po/en.po "$pot"
  # 原文から消えた段落の訳は、残さない。
  msgattrib --no-obsolete -o po/en.po po/en.po
else
  mkdir -p po
  msginit --no-translator --locale=en --input="$pot" --output-file=po/en.po
fi
msgfmt --statistics -o /dev/null po/en.po
