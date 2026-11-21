// 一覧の UI を、本物の browser で見る。HTML の字は tests/ui.rs（Rust）が見るので、
// ここは browser でしか見えない物（script の動き・色・並び）と、人が押してたどる道を見る。
// 試験は同じ置き場に順に置く。ほかの試験のページを見ないように、試験ごとに別の project を使う。
import { expect, test } from '@playwright/test';
import { choice, collectErrors, filters, fresh, list, openLive, place, rows, toolbar, viewer } from './helpers';

const NEW_LIGHT = 'rgb(31, 122, 77)'; // --new: #1f7a4d
const NEW_DARK = 'rgb(92, 193, 143)'; // --new: #5cc18f

test('一覧の列: 置いたページが新しい順に出る。時刻はこの機械の時刻。console に誤りが無い', async ({ page, request }) => {
  const errors = collectErrors(page);
  const at = (second: number) => `2026-11-01T00:00:0${second}Z`;
  await place(request, { project: 'rows', key: 'K-1', kind: 'plan', role: 'tpm', n: '1', created_at: at(1) }, '<title>計画 その一</title><p>りんご</p>');
  await place(request, { project: 'rows', key: 'K-1', kind: 'result', role: 'software-engineer', n: '1', created_at: at(2) }, '<p>題の無いページ</p>');
  await place(request, { project: 'rows', key: 'K-2', kind: 'brief', role: 'tpm', n: '1', created_at: at(3) }, '<title>依頼 その三</title><p>ぶどう</p>');

  await openLive(page, '/?project=rows');
  await expect(rows(page)).toHaveCount(3);
  await expect(rows(page).locator('.title')).toHaveText(['依頼 その三', '（題なし）', '計画 その一']);
  await expect(rows(page).locator('.name')).toHaveText(['brief-1', 'result-1', 'plan-1']);
  // UTC の 00:00 は、日本の時刻で 09:00。
  await expect(rows(page).first().locator('time')).toHaveText('11-01 09:00');
  await expect(viewer(page)).toContainText('ページを選ぶと、ここに 1 枚だけ出る');
  expect(errors).toEqual([]);
});

test('絞り込みの列: 候補を押すと合う行だけになり、もう一度押すと外れる', async ({ page, request }) => {
  await place(request, { project: 'pick-a', key: 'A-1', kind: 'plan', role: 'tpm', workspace: 'workspace:1' }, '<title>a1</title>');
  await place(request, { project: 'pick-a', key: 'A-2', kind: 'result', role: 'software-engineer', workspace: 'workspace:2' }, '<title>a2</title>');
  await place(request, { project: 'pick-b', key: 'B-1', kind: 'plan', role: 'tpm' }, '<title>b1</title>');

  await page.goto('/');
  await expect(filters(page).locator('h2')).toHaveText(['担当者（役割）', 'workspace', '種類', 'Key', 'project']);

  await choice(page, 'project', 'pick-a').click();
  await expect(page).toHaveURL(/[?&]project=pick-a/);
  await expect(rows(page).locator('.title')).toHaveText(['a2', 'a1']);
  await expect(choice(page, 'project', 'pick-a')).toHaveAttribute('aria-current', 'true');
  await expect(choice(page, 'project', 'pick-a').locator('.n')).toHaveText('2');

  // 条件は重なる。
  for (const [heading, label, title] of [
    ['Key', 'A-1', 'a1'],
    ['種類', 'result', 'a2'],
    ['担当者（役割）', 'tpm', 'a1'],
    ['workspace', 'workspace:2', 'a2'],
  ]) {
    await choice(page, heading, label).click();
    await expect(rows(page).locator('.title')).toHaveText([title]);
    // もう一度押すと、その条件だけ外れる。
    await choice(page, heading, label).click();
    await expect(rows(page)).toHaveCount(2);
  }

  await toolbar(page).getByRole('link', { name: '絞り込みを外す' }).click();
  await expect(page).not.toHaveURL(/project=/);
  await expect(toolbar(page).getByRole('link', { name: '絞り込みを外す' })).toHaveCount(0);
});

test('操作の帯: 題と本文の字で探せる。並びを替えられる', async ({ page, request }) => {
  const at = (second: number) => `2026-11-02T00:00:0${second}Z`;
  await place(request, { project: 'find', key: 'F-1', kind: 'plan', role: 'tpm', n: '1', created_at: at(1) }, '<title>古い方</title><p>みかん</p>');
  await place(request, { project: 'find', key: 'F-1', kind: 'result', role: 'tpm', n: '1', created_at: at(2) }, '<title>新しい方</title><p>ぶどう</p>');

  await page.goto('/?project=find');
  await toolbar(page).getByRole('searchbox', { name: '題と本文を探す' }).fill('みかん');
  await toolbar(page).getByRole('button', { name: '探す' }).click();
  // 絞り込みの条件は、探した後も残る。
  await expect(page).toHaveURL(/project=find/);
  await expect(rows(page).locator('.title')).toHaveText(['古い方']);

  await toolbar(page).getByRole('searchbox').fill('どこにも無い語');
  await toolbar(page).getByRole('searchbox').press('Enter');
  await expect(rows(page)).toHaveCount(0);
  await expect(list(page).locator('.empty')).toHaveText('合うページは無い');

  await page.goto('/?project=find');
  await expect(rows(page).locator('.title')).toHaveText(['新しい方', '古い方']);
  await toolbar(page).getByTitle('並びを替える').click();
  await expect(rows(page).locator('.title')).toHaveText(['古い方', '新しい方']);
});

test('一覧の列: 200 枚を越えると「続きを出す」で残りが出る', async ({ page, request }) => {
  const base = Date.parse('2026-11-03T00:00:00Z');
  for (let n = 1; n <= 201; n += 1) {
    const created_at = new Date(base + n * 1000).toISOString().replace('.000Z', 'Z');
    await place(request, { project: 'more', key: 'M-1', kind: 'result', role: 'tpm', n: String(n), created_at }, `<title>${n} 枚目</title>`);
  }
  await page.goto('/?project=more');
  await expect(rows(page)).toHaveCount(200);
  await expect(rows(page).first().locator('.title')).toHaveText('201 枚目');
  await list(page).getByRole('link', { name: '続きを出す' }).click();
  await expect(rows(page).locator('.title')).toHaveText(['1 枚目']);
  await expect(list(page).getByRole('link', { name: '続きを出す' })).toHaveCount(0);
});

test('操作の帯: ページが置かれると新着の印が出る。押すと一覧の列に出る', async ({ page, request }) => {
  await openLive(page, '/?project=fresh');
  await expect(fresh(page)).toBeHidden();
  await expect(page).toHaveTitle('ページの置き場');

  await place(request, { project: 'fresh', key: 'N-1', kind: 'result', role: 'tpm' }, '<title>一枚目</title>');
  await expect(fresh(page)).toBeVisible();
  await expect(fresh(page)).toHaveText('● 新着 1 件（読み直す）');
  await expect(page).toHaveTitle('(1) ページの置き場');
  // 自分では読み直さない（読んでいる所を動かさない）。
  await expect(rows(page)).toHaveCount(0);

  await place(request, { project: 'fresh', key: 'N-1', kind: 'result', role: 'tpm' }, '<title>二枚目</title>');
  await expect(fresh(page)).toHaveText('● 新着 2 件（読み直す）');
  await expect(page).toHaveTitle('(2) ページの置き場');

  await fresh(page).click();
  // 読み直しても、絞り込みの条件は残る。
  await expect(page).toHaveURL(/project=fresh/);
  await expect(rows(page).locator('.title')).toHaveText(['二枚目', '一枚目']);
  await expect(fresh(page)).toBeHidden();
});

test('一覧の列・本文の枠: 初めて開いた後に置かれたページに未読の印が付き、開くと消える', async ({ page, request }) => {
  await place(request, { project: 'unread', key: 'U-1', kind: 'plan', role: 'tpm' }, '<title>前からある</title>');
  await openLive(page, '/?project=unread');
  // 初めて開いた時より前のページは、未読にしない。
  await expect(rows(page)).toHaveCount(1);
  await expect(page.locator('li.unread')).toHaveCount(0);

  const path = await place(request, { project: 'unread', key: 'U-1', kind: 'result', role: 'tpm' }, '<title>後から置いた</title><p id="body">本文の字</p>');
  await fresh(page).click();
  const row = page.locator(`li[data-path="${path}"]`);
  await expect(row).toHaveClass(/unread/);
  await expect(page.locator('li.unread')).toHaveCount(1);

  await row.getByRole('link').click();
  await expect(page.locator('body')).toHaveAttribute('data-viewer', path);
  await expect(row.getByRole('link')).toHaveAttribute('aria-current', 'true');
  const frame = viewer(page).locator('iframe[title="選んだページ"]');
  await expect(frame).toHaveAttribute('sandbox', 'allow-scripts');
  await expect(frame.contentFrame().locator('#body')).toHaveText('本文の字');
  await expect(viewer(page).getByRole('link', { name: '別の tab で開く' })).toHaveAttribute('href', path);
  await expect(page.locator('li.unread')).toHaveCount(0);

  // 読み直しても、開いたページは既読のまま（browser が覚えている）。
  await page.goto('/?project=unread');
  await expect(page.locator('li.unread')).toHaveCount(0);
});

for (const [colorScheme, color] of [['light', NEW_LIGHT], ['dark', NEW_DARK]] as const) {
  test(`色（${colorScheme}）: 未読の印と新着の印が決めた色で出る。前の名（pagestore.*）で覚えた印も読む`, async ({ browser, request }) => {
    const context = await browser.newContext({ colorScheme });
    // 名を替える前の browser が覚えていた「初めて開いた時」。これより後のページは未読。
    await context.addInitScript(() => {
      if (!localStorage.getItem('pagetray.since')) localStorage.setItem('pagestore.since', '2000-01-01T00:00:00.000Z');
    });
    const page = await context.newPage();
    await place(request, { project: `color-${colorScheme}`, key: 'C-1', kind: 'plan', role: 'tpm' }, '<title>色を見る</title>');
    await openLive(page, `/?project=color-${colorScheme}`);

    await expect(page.locator('li.unread')).toHaveCount(1);
    expect(await page.evaluate(() => localStorage.getItem('pagetray.since'))).toBe('2000-01-01T00:00:00.000Z');
    const dot = await page.locator('li.unread .title').evaluate((title) => {
      const style = getComputedStyle(title, '::before');
      return { content: style.content, color: style.color };
    });
    expect(dot).toEqual({ content: '"● "', color });

    await place(request, { project: `color-${colorScheme}`, key: 'C-1', kind: 'result', role: 'tpm' }, '<title>新着</title>');
    await expect(fresh(page)).toBeVisible();
    await expect(fresh(page)).toHaveCSS('color', color);
    await context.close();
  });
}

test('幅: 900px 以下では、絞り込みの列・一覧の列・本文の枠が縦に 1 列に並ぶ', async ({ page, request }) => {
  await place(request, { project: 'narrow', key: 'W-1', kind: 'plan', role: 'tpm' }, '<title>幅を見る</title>');
  const top = async () => ({
    filters: (await filters(page).boundingBox())!,
    list: (await list(page).boundingBox())!,
    viewer: (await viewer(page).boundingBox())!,
  });

  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/?project=narrow');
  const wide = await top();
  // 広い時は横に並ぶ。
  expect(wide.list.x).toBeGreaterThanOrEqual(wide.filters.x + wide.filters.width - 1);
  expect(wide.viewer.x).toBeGreaterThanOrEqual(wide.list.x + wide.list.width - 1);

  await page.setViewportSize({ width: 800, height: 900 });
  const narrow = await top();
  expect(narrow.list.y).toBeGreaterThanOrEqual(narrow.filters.y + narrow.filters.height - 1);
  expect(narrow.viewer.y).toBeGreaterThanOrEqual(narrow.list.y + narrow.list.height - 1);
});
