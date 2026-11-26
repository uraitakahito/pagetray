// メッセージの一覧（/messages）を、本物の browser で見る。HTML の字は crates/server/tests/messages.rs（Rust）が見るので、
// ここは browser でしか見えない物（script の動き・本文が HTML として動かないこと）と、人が押してたどる道を見る。
// 「まだ届いていない n 件」は置き場の全部を数えるので、まだ届いていないにした記録は、試験の終わりまでに届いたにする。
import { expect, test } from '@playwright/test';
import { choice, collectErrors, fresh, messageRow, messageRows, openLive, place, putMessage, rows, toolbar, undelivered, type Message } from './helpers';

const message = (change: Partial<Message> = {}): Message => ({
  heading: 'request',
  from: 't-1.swe.1',
  to: 'tpm',
  text: '本文',
  sentAt: '2026-11-01T00:00:00Z',
  state: 'delivered',
  ...change,
});

test('ページの一覧とメッセージの一覧は、操作の帯の link で行き来する', async ({ page, request }) => {
  await putMessage(request, 'msg-link', 'm-1', message());
  await page.goto('/');
  await toolbar(page).getByRole('link', { name: 'メッセージ', exact: true }).click();
  await expect(page).toHaveURL(/\/messages$/);
  await expect(page).toHaveTitle(/メッセージの記録$/);
  await expect(messageRow(page, 'msg-link', 'm-1')).toBeVisible();
  await toolbar(page).getByRole('link', { name: 'ページ', exact: true }).click();
  await expect(page).toHaveURL(/\/$/);
  await expect(page).toHaveTitle(/ページの置き場$/);
});

test('行: 送った時刻・Key・見出し・送り手と受け手・状態の字・届いた時刻・渡した時・渡し直しの数。絞り込める。console に誤りが無い', async ({ page, request }) => {
  const errors = collectErrors(page);
  await putMessage(
    request,
    'msg-rows',
    'm-1',
    message({ key: 'A-1', heading: 'answer', from: 'pm.1', to: 't-1.swe.1', text: 'りんごにする', deliveredAt: '2026-11-01T00:00:07Z', via: 'session-start', deliveries: 3 }),
  );
  await putMessage(request, 'msg-rows', 'm-2', message({ sentAt: '2026-11-02T00:00:00Z', state: 'expired', text: 'みかん' }));
  await putMessage(request, 'msg-rows', 'm-3', message({ sentAt: '2026-11-03T00:00:00Z', state: 'carried', to: 'pm.1', text: 'ぶどう' }));

  await openLive(page, '/messages?project=msg-rows');
  await expect(messageRows(page)).toHaveCount(3);
  await expect(messageRows(page).locator('.state')).toHaveText(['ほかの道具で運んだ', '期限切れ', '届いた']);
  const row = messageRow(page, 'msg-rows', 'm-1');
  // UTC の 00:00 は、日本の時刻で 09:00。
  await expect(row.locator('.top time')).toHaveText('11-01 09:00');
  await expect(row.locator('.top')).toContainText('A-1');
  await expect(row.locator('.name')).toHaveText('answer');
  await expect(row.locator('.route')).toHaveText('pm.1 → t-1.swe.1');
  await expect(row.locator('.who')).toContainText('届いた 11-01 09:00');
  await expect(row.locator('.who')).toContainText('渡した時: session-start');
  await expect(row.locator('.who')).toContainText('渡し直し 2 度');
  await expect(row.locator('pre')).toHaveText('りんごにする');

  // 絞り込み: 候補を押すと合う行だけになり、もう一度押すと外れる。
  await choice(page, '受け手', 'pm.1').click();
  await expect(page).toHaveURL(/[?&]to=pm\.1/);
  await expect(messageRows(page)).toHaveCount(1);
  await expect(messageRows(page).locator('pre')).toHaveText('ぶどう');
  await choice(page, '受け手', 'pm.1').click();
  await expect(messageRows(page)).toHaveCount(3);
  await choice(page, '状態', '期限切れ').click();
  await expect(page).toHaveURL(/[?&]state=expired/);
  await expect(messageRows(page).locator('pre')).toHaveText('みかん');
  expect(errors).toEqual([]);
});

test('本文: HTML として動かない。長い本文は頭だけ見え、開くと全部', async ({ page, request }) => {
  const errors = collectErrors(page);
  const evil = '<img src=x onerror="window.__pwned = 1"><script>window.__pwned = 2</script><b>太字</b>';
  const long = `頭の行\n${'あ'.repeat(400)}終わり`;
  await putMessage(request, 'msg-text', 'm-1', message({ text: evil, from: '<i>from</i>' }));
  await putMessage(request, 'msg-text', 'm-2', message({ sentAt: '2026-11-02T00:00:00Z', text: long }));

  await page.goto('/messages?project=msg-text');
  const row = messageRow(page, 'msg-text', 'm-1');
  await expect(row.locator('pre')).toHaveText(evil);
  await expect(row.locator('.route')).toHaveText('<i>from</i> → tpm');
  await expect(page.locator('#list img, #list b, #list i, #list script')).toHaveCount(0);
  expect(await page.evaluate(() => (window as any).__pwned)).toBeUndefined();

  const folded = messageRow(page, 'msg-text', 'm-2');
  await expect(folded.locator('summary')).toContainText('頭の行');
  await expect(folded.locator('pre')).toBeHidden();
  await folded.locator('summary').click();
  await expect(folded.locator('pre')).toBeVisible();
  await expect(folded.locator('pre')).toHaveText(long);
  expect(errors).toEqual([]);
});

test('新着の知らせ: 置かれると押さずに頭に入り、書き足されると状態と数が替わる。開いた本文は開いたまま', async ({ page, request }) => {
  const errors = collectErrors(page);
  const long = `長い本文\n${'あ'.repeat(400)}`;
  await putMessage(request, 'msg-live', 'm-1', message({ text: long }));
  await openLive(page, '/messages?project=msg-live');
  await expect(messageRows(page)).toHaveCount(1);
  await expect(undelivered(page)).toBeHidden();
  await messageRow(page, 'msg-live', 'm-1').locator('summary').click();
  // 画面ごと読み直したら消える印。
  await page.evaluate(() => ((window as any).__kept = true));

  // ほかの project の記録は、一覧の列に入らない（数は置き場の全部なので増える）。
  await putMessage(request, 'msg-live-other', 'm-1', message({ sentAt: '2026-11-02T00:00:00Z', state: 'waiting' }));
  await expect(undelivered(page)).toHaveText('まだ届いていない 1 件');
  await expect(messageRows(page)).toHaveCount(1);

  await putMessage(request, 'msg-live', 'm-2', message({ sentAt: '2026-11-03T00:00:00Z', state: 'waiting', text: '新しく置いた' }));
  await expect(messageRows(page)).toHaveCount(2);
  await expect(messageRows(page).first().locator('pre')).toHaveText('新しく置いた');
  await expect(messageRows(page).first().locator('.state')).toHaveText('まだ届いていない');
  await expect(undelivered(page)).toHaveText('まだ届いていない 2 件');
  await expect(page).toHaveTitle('まだ届いていない 2 件 ・ メッセージの記録');
  // 絞り込みの列も新しくなる。
  await expect(choice(page, '状態', 'まだ届いていない').locator('.n')).toHaveText('2');
  // 開いていた本文は、差し替えても開いたまま。
  await expect(messageRow(page, 'msg-live', 'm-1').locator('details pre')).toBeVisible();

  // 書き足すと、押さずに状態が替わる。
  await putMessage(request, 'msg-live', 'm-2', message({ sentAt: '2026-11-03T00:00:00Z', state: 'delivered', deliveredAt: '2026-11-03T00:00:09Z', text: '新しく置いた' }));
  await expect(messageRow(page, 'msg-live', 'm-2').locator('.state')).toHaveText('届いた');
  await expect(messageRow(page, 'msg-live', 'm-2').locator('.who')).toContainText('届いた 11-03 09:00');
  await expect(undelivered(page)).toHaveText('まだ届いていない 1 件');
  await putMessage(request, 'msg-live-other', 'm-1', message({ sentAt: '2026-11-02T00:00:00Z', state: 'delivered' }));
  await expect(undelivered(page)).toBeHidden();
  await expect(page).toHaveTitle('メッセージの記録');
  await expect(fresh(page)).toBeHidden();
  expect(await page.evaluate(() => (window as any).__kept)).toBe(true);
  expect(errors).toEqual([]);
});

test('ページの一覧は、メッセージの記録の知らせでは新着を数えない。メッセージの一覧は、ページの知らせでは読み直さない', async ({ page, request }) => {
  await place(request, { project: 'msg-pages', key: 'K-1', kind: 'plan', role: 'tpm' }, '<title>一枚目</title>');
  await openLive(page, '/?project=msg-pages');
  await expect(rows(page)).toHaveCount(1);
  await putMessage(request, 'msg-pages', 'm-1', message());
  // 次に置いたページは、押さずに入る（知らせの流れは生きている）。メッセージの記録は、新着に数えていない。
  await place(request, { project: 'msg-pages', key: 'K-1', kind: 'plan', role: 'tpm' }, '<title>二枚目</title>');
  await expect(rows(page)).toHaveCount(2);
  await expect(page).toHaveTitle('(1) ページの置き場');

  await openLive(page, '/messages?project=msg-pages');
  await expect(messageRows(page)).toHaveCount(1);
  let pulls = 0;
  page.on('request', (r) => {
    if (new URL(r.url()).pathname === '/messages') pulls += 1;
  });
  await place(request, { project: 'msg-pages', key: 'K-1', kind: 'plan', role: 'tpm' }, '<title>三枚目</title>');
  await putMessage(request, 'msg-pages', 'm-2', message({ sentAt: '2026-11-02T00:00:00Z' }));
  await expect(messageRows(page)).toHaveCount(2);
  // 読み直したのは、メッセージの記録の知らせの 1 度だけ。
  expect(pulls).toBe(1);
});
