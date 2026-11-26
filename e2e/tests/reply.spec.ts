// 返事の状態（返事待ち・答えた）の印と数を、本物の browser で見る。HTML の字は tests/ui.rs（Rust）が見る。
// 返事待ちの数は置き場の全部を数え、tab の題にも出るので、返事待ちにしたページは、試験の終わりまでに答えたにする。
// 見出し「返事」は、状態を書いたページがあると出る。見出しの並びを見る list.spec.ts より後に流れるよう、この名にしてある。
import { expect, test } from '@playwright/test';
import { choice, collectErrors, openLive, place, reply, replyMark, rows, scrollList, viewer, waiting } from './helpers';

const WAIT_LIGHT = 'rgb(138, 90, 0)'; // --wait: #8a5a00
const WAIT_DARK = 'rgb(224, 169, 58)'; // --wait: #e0a93a

test('返事の印: 返事待ちにすると、押さずに印と数が出る。答えたにすると替わり、数は消える。console に誤りが無い', async ({ page, request }) => {
  const errors = collectErrors(page);
  const confirm = await place(request, { project: 'reply', key: 'R-1', kind: 'confirm', role: 'software-engineer' }, '<title>案を選ぶ</title><p>確認の本文</p>');
  await place(request, { project: 'reply', key: 'R-1', kind: 'proposal', role: 'software-engineer' }, '<title>提案</title>');

  await openLive(page, `/?project=reply&open=${encodeURIComponent(confirm)}`);
  await expect(rows(page)).toHaveCount(2);
  await expect(waiting(page)).toBeHidden();
  await expect(replyMark(page, confirm)).toHaveCount(0);
  await expect(page).toHaveTitle('ページの置き場');
  // 本文の枠は、印が替わっても読み込み直さない。
  const frame = viewer(page).locator('iframe');
  await frame.evaluate((element) => ((element as HTMLIFrameElement & { kept?: boolean }).kept = true));

  await reply(request, confirm, 'waiting');
  await expect(replyMark(page, confirm)).toHaveText('返事待ち');
  await expect(waiting(page)).toBeVisible();
  await expect(waiting(page)).toHaveText('返事待ち 1 件');
  // 返事の状態は、新着には数えない。
  await expect(page).toHaveTitle('返事待ち 1 件 ・ ページの置き場');
  await expect(choice(page, '返事', '返事待ち').locator('.n')).toHaveText('1');
  await expect(rows(page).locator('.reply')).toHaveCount(1);

  await reply(request, confirm, 'answered');
  await expect(replyMark(page, confirm)).toHaveText('答えた');
  await expect(waiting(page)).toBeHidden();
  await expect(page).toHaveTitle('ページの置き場');
  await expect(choice(page, '返事', '返事待ち')).toHaveCount(0);
  await expect(choice(page, '返事', '答えた')).toBeVisible();
  expect(await frame.evaluate((element) => (element as HTMLIFrameElement & { kept?: boolean }).kept)).toBe(true);
  expect(errors).toEqual([]);
});

test('返事待ちの数: 押すと返事待ちだけに絞り、絞り込みの候補を押すと外れる。ほかの project の分も数える', async ({ page, request }) => {
  const a = await place(request, { project: 'reply-a', key: 'A-1', kind: 'confirm', role: 'software-engineer' }, '<title>a の確認</title>');
  const b = await place(request, { project: 'reply-b', key: 'B-1', kind: 'confirm', role: 'software-engineer' }, '<title>b の確認</title>');
  await place(request, { project: 'reply-a', key: 'A-1', kind: 'result', role: 'software-engineer' }, '<title>a の結果</title>');
  await reply(request, a, 'waiting');
  await reply(request, b, 'waiting');
  try {
    await page.goto('/?project=reply-a');
    await expect(rows(page)).toHaveCount(2);
    await expect(waiting(page)).toHaveText('返事待ち 2 件');

    await waiting(page).click();
    await expect(page).toHaveURL(/[?&]reply=waiting/);
    await expect(page).not.toHaveURL(/project=/);
    await expect(rows(page).locator('.title')).toHaveText(['b の確認', 'a の確認']);
    await expect(waiting(page)).toHaveAttribute('aria-current', 'true');
    await expect(choice(page, '返事', '返事待ち')).toHaveAttribute('aria-current', 'true');

    // 絞っている間に答えると、その行は一覧の列から外れる。
    await openLive(page, '/?reply=waiting');
    await reply(request, b, 'answered');
    await expect(rows(page).locator('.title')).toHaveText(['a の確認']);
    await expect(waiting(page)).toHaveText('返事待ち 1 件');

    await choice(page, '返事', '返事待ち').click();
    await expect(page).not.toHaveURL(/reply=/);
  } finally {
    await reply(request, a, 'answered');
    await reply(request, b, 'answered');
  }
});

test('返事の印: 一覧の列を下へ送っていても、見ている行は動かず、印だけが替わる', async ({ page, request }) => {
  const base = Date.parse('2026-11-05T00:00:00Z');
  const paths: string[] = [];
  for (let n = 1; n <= 40; n += 1) {
    const createdAt = new Date(base + n * 1000).toISOString().replace('.000Z', 'Z');
    paths.push(await place(request, { project: 'reply-scroll', key: 'S-1', kind: 'confirm', role: 'tpm', n: String(n), createdAt }, `<title>${n} 枚目</title>`));
  }
  await page.setViewportSize({ width: 1200, height: 500 });
  await openLive(page, '/?project=reply-scroll');
  await scrollList(page, 600);
  const target = paths[25];
  const row = page.locator(`li[data-path="${target}"]`);
  const before = (await row.boundingBox())!.y;
  try {
    await reply(request, target, 'waiting');
    await expect(replyMark(page, target)).toHaveText('返事待ち');
    expect(Math.abs((await row.boundingBox())!.y - before)).toBeLessThan(1);
  } finally {
    await reply(request, target, 'answered');
  }
});

for (const [colorScheme, color] of [['light', WAIT_LIGHT], ['dark', WAIT_DARK]] as const) {
  test(`色（${colorScheme}）: 返事待ちの印と数が決めた色で出る。印は字でも読める`, async ({ browser, request }) => {
    const context = await browser.newContext({ colorScheme });
    const page = await context.newPage();
    const confirm = await place(request, { project: `reply-color-${colorScheme}`, key: 'C-1', kind: 'confirm', role: 'tpm' }, '<title>色を見る</title>');
    await reply(request, confirm, 'waiting');
    try {
      await page.goto(`/?project=reply-color-${colorScheme}`);
      await expect(replyMark(page, confirm)).toHaveText('返事待ち');
      await expect(replyMark(page, confirm)).toHaveCSS('color', color);
      await expect(waiting(page)).toHaveCSS('color', color);
      await expect(waiting(page)).toHaveText(/^返事待ち \d+ 件$/);
    } finally {
      await reply(request, confirm, 'answered');
      await context.close();
    }
  });
}

test('script が無くても: 返事の印と数は、サーバーが組んだ HTML に出ている', async ({ browser, request }) => {
  const context = await browser.newContext({ javaScriptEnabled: false });
  const page = await context.newPage();
  const confirm = await place(request, { project: 'reply-nojs', key: 'N-1', kind: 'confirm', role: 'tpm' }, '<title>script 無し</title>');
  await reply(request, confirm, 'waiting');
  try {
    await page.goto('/?project=reply-nojs');
    await expect(replyMark(page, confirm)).toHaveText('返事待ち');
    await expect(waiting(page)).toHaveText('返事待ち 1 件');
  } finally {
    await reply(request, confirm, 'answered');
    await context.close();
  }
});
