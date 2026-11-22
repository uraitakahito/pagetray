// 一覧の UI を、本物の browser で見る。HTML の字は tests/ui.rs（Rust）が見るので、
// ここは browser でしか見えない物（script の動き・色・並び）と、人が押してたどる道を見る。
// 試験は同じ置き場に順に置く。ほかの試験のページを見ないように、試験ごとに別の project を使う。
import { expect, test } from '@playwright/test';
import { CASTLIST_PORT, choice, collectErrors, filters, fresh, list, listAtTop, openLive, place, rows, scrollList, startCastlist, toolbar, viewer } from './helpers';

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

test('一覧の列: 続きを出して古い方を見ている時は差し替えず、新着の印を押すと頭の一覧に戻る', async ({ page, request }) => {
  const base = Date.parse('2026-11-04T00:00:00Z');
  for (let n = 1; n <= 201; n += 1) {
    const created_at = new Date(base + n * 1000).toISOString().replace('.000Z', 'Z');
    await place(request, { project: 'paged', key: 'P-1', kind: 'result', role: 'tpm', n: String(n), created_at }, `<title>${n} 枚目</title>`);
  }
  await page.goto('/?project=paged');
  const more = await list(page).getByRole('link', { name: '続きを出す' }).getAttribute('href');
  await openLive(page, more!);
  await expect(rows(page).locator('.title')).toHaveText(['1 枚目']);

  // ほかの project のページは数えない。
  await place(request, { project: 'paged-other', key: 'P-1', kind: 'result', role: 'tpm' }, '<title>ほかの project</title>');
  await place(request, { project: 'paged', key: 'P-1', kind: 'result', role: 'tpm' }, '<title>新しく置いた</title>');
  await expect(fresh(page)).toHaveText('● 新着 1 件（読み直す）');
  await expect(page).toHaveTitle('(1) ページの置き場');
  await expect(rows(page).locator('.title')).toHaveText(['1 枚目']);

  await fresh(page).click();
  await expect(page).toHaveURL(/project=paged/);
  await expect(page).not.toHaveURL(/[?&]at=/);
  await expect(rows(page).first().locator('.title')).toHaveText('新しく置いた');
  await expect(fresh(page)).toBeHidden();
});

test('一覧の列: 置かれると、押さずに頭に入る。数えるのは今の絞り込みに入ったページだけ。console に誤りが無い', async ({ page, request }) => {
  const errors = collectErrors(page);
  await openLive(page, '/?project=fresh');
  await expect(fresh(page)).toBeHidden();
  await expect(page).toHaveTitle('ページの置き場');
  // 画面ごと読み直したら消える印。
  await page.evaluate(() => ((window as any).__kept = true));

  // ほかの project のページは、一覧の列にも数にも入らない。
  await place(request, { project: 'other-of-fresh', key: 'N-1', kind: 'result', role: 'tpm' }, '<title>ほかの project</title>');
  await place(request, { project: 'fresh', key: 'N-1', kind: 'result', role: 'tpm' }, '<title>一枚目</title>');
  await expect(rows(page).locator('.title')).toHaveText(['一枚目']);
  await expect(page).toHaveTitle('(1) ページの置き場');
  // 頭が見えているので、新着の印は出ない。新しい行には未読の印が付き、時刻はこの機械の時刻で出る。
  await expect(fresh(page)).toBeHidden();
  await expect(rows(page).first()).toHaveClass(/unread/);
  await expect(rows(page).first().locator('time')).toHaveText(/^\d\d-\d\d \d\d:\d\d$/);
  // 絞り込みの列の件数も新しくなる。
  await expect(choice(page, 'project', 'fresh').locator('.n')).toHaveText('1');
  await expect(choice(page, 'project', 'other-of-fresh').locator('.n')).toHaveText('1');

  // 続けて置いても、同じ行は 2 度出ない。
  for (let n = 2; n <= 6; n += 1) {
    await place(request, { project: 'fresh', key: 'N-1', kind: 'result', role: 'tpm' }, `<title>${n} 枚目</title>`);
  }
  await expect(rows(page).locator('.title')).toHaveText(['6 枚目', '5 枚目', '4 枚目', '3 枚目', '2 枚目', '一枚目']);
  await expect(page).toHaveTitle('(6) ページの置き場');
  await expect(choice(page, 'project', 'fresh').locator('.n')).toHaveText('6');

  // 題と本文を探す（q）の条件も、サーバーがそのまま当てる。
  await openLive(page, '/?project=fresh&q=' + encodeURIComponent('りんご'));
  await expect(rows(page)).toHaveCount(0);
  await place(request, { project: 'fresh', key: 'N-2', kind: 'result', role: 'tpm' }, '<title>合わない</title><p>みかん</p>');
  await place(request, { project: 'fresh', key: 'N-2', kind: 'result', role: 'tpm' }, '<title>合う</title><p>りんご</p>');
  await expect(rows(page).locator('.title')).toHaveText(['合う']);
  await expect(page).toHaveTitle('(1) ページの置き場');
  expect(errors).toEqual([]);
});

test('本文の枠: 一覧の列が差し替わっても、開いているページは読み込み直さない', async ({ page, request }) => {
  const path = await place(request, { project: 'keep', key: 'K-1', kind: 'plan', role: 'tpm' }, '<title>読んでいる</title><p id="body">本文</p>');
  const loads: string[] = [];
  page.on('request', (r) => {
    if (r.url().endsWith(path)) loads.push(r.url());
  });
  await openLive(page, `/?project=keep&open=${encodeURIComponent(path)}`);
  const frame = viewer(page).locator('iframe[title="選んだページ"]').contentFrame();
  await expect(frame.locator('#body')).toHaveText('本文');
  await page.evaluate(() => ((window as any).__kept = true));
  await frame.locator('body').evaluate(() => ((window as any).__kept = true));
  const before = loads.length;

  await place(request, { project: 'keep', key: 'K-1', kind: 'result', role: 'tpm' }, '<title>後から置いた</title>');
  await expect(rows(page).locator('.title')).toHaveText(['後から置いた', '読んでいる']);
  expect(await page.evaluate(() => (window as any).__kept)).toBe(true);
  expect(await frame.locator('body').evaluate(() => (window as any).__kept)).toBe(true);
  expect(loads.length).toBe(before);
  // 開いている行の印も残る。
  await expect(page.locator(`li[data-path="${path}"] a`)).toHaveAttribute('aria-current', 'true');
});

for (const [name, width] of [['広い幅', 1280], ['狭い幅', 800]] as const) {
  test(`一覧の列（${name}）: 下へ送っている時は、見ている行が動かない。「上へ」を押すと頭へ行く`, async ({ page, request }) => {
    const project = `scroll-${width}`;
    const base = Date.parse('2026-11-05T00:00:00Z');
    for (let n = 1; n <= 40; n += 1) {
      const created_at = new Date(base + n * 1000).toISOString().replace('.000Z', 'Z');
      await place(request, { project, key: 'S-1', kind: 'result', role: 'tpm', n: String(n), created_at }, `<title>${n} 枚目</title>`);
    }
    await page.setViewportSize({ width, height: 600 });
    await openLive(page, `/?project=${project}`);
    await page.evaluate(() => ((window as any).__kept = true));
    await scrollList(page, 800);
    const watched = rows(page).filter({ hasText: '20 枚目' });
    const before = (await watched.boundingBox())!.y;

    await place(request, { project, key: 'S-1', kind: 'result', role: 'tpm' }, '<title>新しく置いた</title>');
    await expect(rows(page)).toHaveCount(41);
    await expect(fresh(page)).toHaveText('● 新着 1 件（上へ）');
    expect(Math.abs((await watched.boundingBox())!.y - before)).toBeLessThanOrEqual(1);

    await fresh(page).click();
    expect(await listAtTop(page)).toBe(true);
    await expect(rows(page).first().locator('.title')).toHaveText('新しく置いた');
    await expect(rows(page).first()).toBeInViewport();
    await expect(fresh(page)).toBeHidden();
    await expect(page).toHaveURL(new RegExp(`project=${project}$`));
    expect(await page.evaluate(() => (window as any).__kept)).toBe(true);
  });
}

test('一覧の列・本文の枠: 初めて開いた後に置かれたページに未読の印が付き、開くと消える', async ({ page, request }) => {
  await place(request, { project: 'unread', key: 'U-1', kind: 'plan', role: 'tpm' }, '<title>前からある</title>');
  await openLive(page, '/?project=unread');
  // 初めて開いた時より前のページは、未読にしない。
  await expect(rows(page)).toHaveCount(1);
  await expect(page.locator('li.unread')).toHaveCount(0);

  const path = await place(request, { project: 'unread', key: 'U-1', kind: 'result', role: 'tpm' }, '<title>後から置いた</title><p id="body">本文の字</p>');
  // 押さずに一覧の列に入る。
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
    await page.setViewportSize({ width: 1280, height: 600 });
    await place(request, { project: `color-${colorScheme}`, key: 'C-1', kind: 'plan', role: 'tpm' }, '<title>色を見る</title>');
    await openLive(page, `/?project=color-${colorScheme}`);

    await expect(page.locator('li.unread')).toHaveCount(1);
    expect(await page.evaluate(() => localStorage.getItem('pagetray.since'))).toBe('2000-01-01T00:00:00.000Z');
    const dot = await page.locator('li.unread .title').evaluate((title) => {
      const style = getComputedStyle(title, '::before');
      return { content: style.content, color: style.color };
    });
    expect(dot).toEqual({ content: '"● "', color });

    // 新着の印は、一覧の列を下へ送っている時に出る。
    for (let n = 2; n <= 40; n += 1) {
      await place(request, { project: `color-${colorScheme}`, key: 'C-1', kind: 'result', role: 'tpm' }, `<title>${n} 枚目</title>`);
    }
    await expect(rows(page)).toHaveCount(40);
    await scrollList(page, 800);
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

// 送り手のある行が出ると、絞り込みの列に「送り手」の見出しが足される。見出しの並びを見る試験より後に流す。
test('一覧の列: 送り手の castlist の名が行の下に出て、castlist のアクターの画面へ link する。付随する id は名の下に小さく。「送り手」で絞れる', async ({ page, request }) => {
  const errors = collectErrors(page);
  const actor = { id: '019a0000-0000-7000-8000-0000000000e2', name: 'e2e.swe.1', kind: 'ai' };
  const castlist = await startCastlist({ 'claude.session:e2e': actor });
  let sent = '';
  try {
    sent = await place(
      request,
      { project: 'senders', key: 'S-1', kind: 'plan', role: 'software-engineer', actor_issuer: 'tpm', actor_ref: 'claude.session:e2e' },
      '<title>送り手あり</title>',
    );
    await place(request, { project: 'senders', key: 'S-1', kind: 'note', role: 'tpm' }, '<title>送り手なし</title>');
  } finally {
    castlist.close();
  }

  await page.goto('/?project=senders');
  const row = list(page).locator(`li[data-path="${sent}"]`);
  const name = row.locator('.sender a.actor');
  const ref = row.locator('.sender small');
  await expect(name).toHaveText('e2e.swe.1');
  await expect(name).toHaveAttribute('href', `http://127.0.0.1:${CASTLIST_PORT}/actors/${actor.id}`);
  await expect(name).toHaveAttribute('target', '_blank');
  await expect(ref).toHaveText('claude.session:e2e');
  const nameBox = (await name.boundingBox())!;
  const refBox = (await ref.boundingBox())!;
  expect(refBox.y).toBeGreaterThanOrEqual(nameBox.y + nameBox.height - 1);
  const px = (el: typeof name) => el.evaluate((e) => parseFloat(getComputedStyle(e).fontSize));
  expect(await px(ref)).toBeLessThan(await px(name));
  await expect(rows(page).filter({ hasText: '送り手なし' }).locator('.sender')).toHaveCount(0);

  // 行を押すと、前と同じく本文の枠に出る（送り手の link は、行の link の外にある）。
  await row.locator('.title').click();
  await expect(viewer(page)).toContainText(sent);

  await choice(page, '送り手', 'e2e.swe.1').click();
  await expect(page).toHaveURL(new RegExp(`[?&]actor=${actor.id}`));
  await expect(rows(page).locator('.title')).toHaveText(['送り手あり']);
  expect(errors).toEqual([]);
});
