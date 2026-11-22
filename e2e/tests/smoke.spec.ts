// 読むだけの試験。動いている置き場（PAGETRAY_E2E_URL）に向けても流せるように、
// 何も置かず、何が置かれていても通る物だけを見る。版を上げた後の確かめに使う（docs/src/run.md の「確かめる」）。
// 流している間にほかの人がページを置いても落ちないように、数は「前と後の間」で見る。
import { expect, test } from '@playwright/test';
import { collectErrors, filters, openLive, pageLink, place, rows, viewer } from './helpers';

type Listed = { pages: { path: string }[] };

test.beforeAll(async ({ request }) => {
  // 自分で起こした空の置き場では、見る物が無い。1 枚だけ置く（動いている置き場には置かない）。
  if (!process.env.PAGETRAY_E2E_URL) {
    await place(request, { project: 'smoke', key: 'S-1', kind: 'result', role: 'tpm' }, '<title>読むだけの試験の材料</title><p>本文</p>');
  }
});

test('@smoke 一覧の UI が出る。API の答えと合う。絞れる。選ぶと本文の枠に出る', async ({ page, request }) => {
  const errors = collectErrors(page);
  const listed = async () => ((await (await request.get('/api/pages?limit=200')).json()) as Listed).pages;

  expect((await (await request.get('/api/version')).json()).api).toBe(1);

  const before = await listed();
  // 新着の知らせ（/api/events）につながることも、ここで見ている。
  await openLive(page, '/');
  const after = await listed();

  // 一覧の列: 行の数は API の答えと合い、一番上の行は API も知っているページ。
  const count = await rows(page).count();
  expect(count).toBeGreaterThanOrEqual(before.length);
  expect(count).toBeLessThanOrEqual(after.length);
  test.skip(count === 0, '置き場が空（見る行が無い）');
  const top = await rows(page).first().getAttribute('data-path');
  expect(after.map((listedPage) => listedPage.path)).toContain(top);
  await expect(rows(page).first().locator('time')).toHaveText(/^\d\d-\d\d \d\d:\d\d$/);

  // 一覧の UI そのものの console に、誤りが無い（ここより後は、置かれたページの中の誤りも混ざるので見ない）。
  expect(errors).toEqual([]);

  // 絞り込みの列: project の候補を押すと、行の数が、その候補の件数と合う（200 件まで）。
  const project = filters(page).locator('h2:text-is("project") + ul a').first();
  await project.click();
  await expect(page).toHaveURL(/[?&]project=/);
  const chosen = filters(page).locator('h2:text-is("project") + ul a[aria-current="true"]');
  const shown = Number(await chosen.locator('.n').innerText());
  await expect(rows(page)).toHaveCount(Math.min(shown, 200));

  // 本文の枠: 行を選ぶと、そのページが出る。
  const row = rows(page).first();
  const path = (await row.getAttribute('data-path'))!;
  await pageLink(row).click();
  await expect(page.locator('body')).toHaveAttribute('data-viewer', path);
  const frame = viewer(page).locator('iframe[title="選んだページ"]');
  await expect(frame).toHaveAttribute('src', path);
  await expect(frame.contentFrame().locator('body')).toBeAttached();
});
