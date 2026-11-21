#!/usr/bin/env bash
# この本（docs/）を作る: 日本語を docs/book/ に、英語を docs/book/en/ に、検索の索引を docs/book/pagefind/ に。
# 要る道具（PATH に）: mdbook・mdbook-mermaid・mdbook-gettext・pagefind_extended か pagefind。
# 版は .github/workflows/docs.yml の env。手元での入れ方は docs/src/development.md の「この本を直す」。
set -euo pipefail
cd "$(dirname "$0")/../docs"

rm -rf book
# 原文（日本語）は site の root に出る。
mdbook build
# 訳は /<言葉>/ に出る。po/<言葉>.po に訳の無い所は、原文のまま出る。
MDBOOK_BOOK__LANGUAGE=en MDBOOK_OUTPUT__HTML__SITE_URL=/pagetray/en/ mdbook build -d book/en

# 日本語を語に切れるのは extended の方（Homebrew と npm の pagefind は extended）。
# 印刷用のページ（全部の章が 1 枚）・目次だけのページ・404 は、索引に入れない。作る間だけ脇へ除ける。
pagefind=$(command -v pagefind_extended || command -v pagefind)
aside=$(mktemp -d)
trap 'rm -rf "$aside"' EXIT
for dir in . en; do
  mkdir -p "$aside/$dir"
  mv book/"$dir"/{print,toc,404}.html "$aside/$dir/"
done
"$pagefind"
for dir in . en; do
  mv "$aside/$dir"/*.html book/"$dir"/
done

# 出した物に、要る物があるか。
for f in index.html run.html cli.html api.html glossary.html db.html en/index.html en/run.html en/cli.html en/glossary.html en/db.html pagefind/pagefind.js pagefind/pagefind-ui.js; do
  [ -s "book/$f" ] || { echo "docs/book/$f が無い" >&2; exit 1; }
done
for lang in ja en; do
  ls book/pagefind/pagefind."$lang"_*.pf_meta >/dev/null 2>&1 || { echo "検索の索引に $lang が無い" >&2; exit 1; }
done
# 英語の本の本文に、日本語が残っていないか（訳の項に出ない所: コードの塊の中の字など）。
if perl -CSD -0777 -ne '
    next unless m{<main>(.*?)</main>}s;
    (my $text = $1) =~ s/<[^>]+>//g;
    for (split /\n/, $text) { if (/[\p{Hiragana}\p{Katakana}\p{Han}]/) { print "$ARGV: $_\n"; $n = 1 } }
    END { exit !$n }' book/en/*.html; then
  echo "英語の本に、日本語が残っている（上の行）。訳すか、原文を英字で書く。" >&2
  exit 1
fi
echo "docs/book に作った"
