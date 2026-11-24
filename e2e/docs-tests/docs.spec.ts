// 文書の本を、本物の browser で見る。build と訳の検査（ci/docs-build.sh・ci/docs-po.sh）では見えない、
// docs/theme/site.js が足す物（検索の枠・言葉の切り替え）と、図（Mermaid）が描けるかを見る。
import { expect, test, type Page } from '@playwright/test';

/** 目次（docs/src/SUMMARY.md）の章。章を足したら、ここにも足す（下の試験が、目次と合っているかを見る）。 */
const CHAPTERS = ['index', 'run', 'cli', 'api', 'glossary', 'development', 'release', 'db', 'next'];
/** 図（Mermaid）のある章。図を足したら、ここにも足す。 */
const FIGURES = ['index', 'db'];
const LANGS = [
  { lang: 'ja', root: '/' },
  { lang: 'en', root: '/en/' },
];

// mdBook の script（book.js）は、ページを去る時の受け口（unload）を使う。新しい Chromium はこれを止めて、
// console に知らせを出す。本の動きには関わらず、この repo では直せない（mdBook の側の物）ので、この 1 つだけ除く。
const KNOWN = ['Permissions policy violation: unload is not allowed in this document.'];

function collectErrors(page: Page): string[] {
  const errors: string[] = [];
  page.on('console', (message) => {
    if (message.type() === 'error' && !KNOWN.includes(message.text())) errors.push(message.text());
  });
  page.on('pageerror', (error) => errors.push(String(error)));
  return errors;
}

/** 検索の枠を開いて語を入れ、当たった章の link を返す。 */
async function search(page: Page, word: string) {
  const dialog = page.locator('dialog.search-dialog');
  if (!(await dialog.isVisible())) await page.locator('button.search-open').click();
  await expect(dialog).toBeVisible();
  const input = dialog.locator('input');
  await input.fill(word);
  // 結果は少し遅れて替わる。この語の結果になったのを待つ（前の語の結果を読まない）。
  await expect(dialog.locator('.pagefind-ui__message')).toContainText(word);
  return dialog.locator('.pagefind-ui__result-link');
}

for (const { lang, root } of LANGS) {
  test(`${lang}: どの章も開き、言葉が合っていて、console に誤りが無い`, async ({ page }) => {
    const errors = collectErrors(page);
    // 目次に出る章が、上の一覧と同じ。
    // toc.html は枠の中に出す用のページなので、browser では開かず、中の link だけを読む。
    const toc = await (await page.request.get(`${root}toc.html`)).text();
    const listed = [...new Set([...toc.matchAll(/href="([a-z-]+)\.html"/g)].map((found) => found[1]))].sort();
    expect(listed).toEqual([...CHAPTERS].sort());
    for (const chapter of CHAPTERS) {
      await page.goto(`${root}${chapter}.html`);
      await expect(page.locator('html')).toHaveAttribute('lang', lang);
      await expect(page.locator('main h1').first()).toBeVisible();
    }
    expect(errors).toEqual([]);
  });

  for (const chapter of FIGURES) {
    test(`${lang}: ${chapter} の図（Mermaid）が描ける`, async ({ page }) => {
      const errors = collectErrors(page);
      await page.goto(`${root}${chapter}.html`);
      const figures = page.locator('pre.mermaid');
      await expect(figures).not.toHaveCount(0);
      for (const figure of await figures.all()) {
        // 描けると、塊の中が svg に替わる。構文の誤りは、誤りの絵（class に error）になる。
        await expect(figure.locator('svg')).toBeVisible();
        await expect(figure.locator('svg[aria-roledescription="error"]')).toHaveCount(0);
      }
      expect(errors).toEqual([]);
    });
  }
}

test('ja: 検索の枠が開き、日本語の語が章に当たる。無い語は当たらない', async ({ page }) => {
  await page.goto('/index.html');
  for (const word of ['置き場', '写し', '起こす']) {
    const hits = await search(page, word);
    await expect(hits.first(), word).toBeVisible();
    // 日本語の本の検索は、日本語の章だけを出す。
    for (const href of await hits.evaluateAll((links) => links.map((link) => link.getAttribute('href')!))) {
      expect(href).not.toContain('/en/');
    }
  }
  // 本に無い語（英字の並びは、頭の字が合う語に当たることがあるので、漢字の語で見る）。
  const none = await search(page, '鸚鵡');
  await expect(none).toHaveCount(0);
});

test('ja: 「/」のキーでも検索の枠が開く。当たった章へ行ける', async ({ page }) => {
  await page.goto('/index.html');
  await page.locator('main').press('/');
  await expect(page.locator('dialog.search-dialog')).toBeVisible();
  const hits = await search(page, 'DB の写し');
  await hits.first().click();
  await expect(page).toHaveURL(/\/[a-z]+\.html/);
  await expect(page.locator('html')).toHaveAttribute('lang', 'ja');
});

test('en: 英語の語が英語の章に当たる。日本語の語は当たらない', async ({ page }) => {
  await page.goto('/en/index.html');
  for (const word of ['backup', 'restore']) {
    const hits = await search(page, word);
    await expect(hits.first(), word).toBeVisible();
    for (const href of await hits.evaluateAll((links) => links.map((link) => link.getAttribute('href')!))) {
      expect(href).toContain('/en/');
    }
  }
  const none = await search(page, '置き場');
  await expect(none).toHaveCount(0);
});

test('言葉の切り替え: 同じ章の、もう一方の言葉へ移る', async ({ page }) => {
  await page.goto('/run.html');
  await page.locator('a.lang-link[lang="en"]').click();
  await expect(page).toHaveURL(/\/en\/run\.html$/);
  await expect(page.locator('html')).toHaveAttribute('lang', 'en');
  // 今の言葉への link は出ない。
  await expect(page.locator('a.lang-link')).toHaveCount(1);
  await page.locator('a.lang-link[lang="ja"]').click();
  await expect(page).toHaveURL(/\/run\.html$/);
  await expect(page).not.toHaveURL(/\/en\//);
  await expect(page.locator('html')).toHaveAttribute('lang', 'ja');
});
