// Key の画面の file の枠と上げる枠を、本物の browser で見る。HTML の字は tests/ui.rs が、
// 守り（どの Origin から上げられるか）は tests/api.rs が見る。ここは script の動きと、人がたどる道を見る。
import { expect, test, type Page } from '@playwright/test';
import { PNG, collectErrors, fileRows, files, openLive, place, placeFile, upload, viewer } from './helpers';

const choose = (page: Page, name: string, mimeType: string, buffer: Buffer) =>
  upload(page).locator('input[type=file]').setInputFiles({ name, mimeType, buffer });
const reviewed = (page: Page) => upload(page).getByLabel('秘密（token・鍵・個人の情報・ほかの人の画面）が写っていないか見た');
const send = (page: Page) => upload(page).getByRole('button', { name: '送る' });
const say = (page: Page) => upload(page).locator('form > .say');

test('上げる枠: 図を選ぶと見本が出て、見た印を付けるまで送れない。送ると file の枠に出て、参照の path が出る', async ({ page, request }) => {
  const errors = collectErrors(page);
  await openLive(page, '/up/U-1/');
  await expect(files(page)).toContainText('この Key の file は無い');
  await expect(send(page)).toBeDisabled();

  await choose(page, 'Screen Shot 2026.PNG', 'image/png', PNG);
  // 名の語は CLI と同じ作り方。見本は、選んだ図そのもの。
  await expect(upload(page).locator('.preview')).toContainText('Screen Shot 2026.PNG（70 B）→ screen-shot-2026.png');
  const sample = upload(page).locator('.preview img');
  await expect(sample).toBeVisible();
  expect(await sample.evaluate((img: HTMLImageElement) => img.naturalWidth)).toBeGreaterThan(0);
  await expect(send(page)).toBeDisabled();

  await reviewed(page).check();
  await expect(send(page)).toBeEnabled();
  await reviewed(page).uncheck();
  await expect(send(page)).toBeDisabled();
  await reviewed(page).check();
  await send(page).click();

  await expect(say(page)).toHaveText('上げた。ページからは files/screen-shot-2026-1.png で参照する');
  await expect(fileRows(page)).toHaveCount(1);
  const row = fileRows(page).first();
  await expect(row).toHaveAttribute('data-file', '/up/U-1/files/screen-shot-2026-1.png');
  await expect(row.locator('.name')).toHaveText('screen-shot-2026-1.png');
  await expect(row.locator('.who')).toContainText('70 B ・ product-manager ・');
  await expect(row.locator('code.ref')).toHaveText('files/screen-shot-2026-1.png');
  await expect(row.locator('.name')).toHaveAttribute('target', '_blank');
  expect(await row.locator('img').evaluate((img: HTMLImageElement) => img.complete && img.naturalWidth)).toBeGreaterThan(0);
  // 送った後は、次の file を選ぶまで送れない。
  await expect(send(page)).toBeDisabled();
  await expect(reviewed(page)).not.toBeChecked();

  // 置き場は、引数でなく一覧の UI からと見て、役割を決めた。
  const listed = await (await request.get('/api/files?project=up&key=U-1')).json();
  expect(listed.files.map((f: { role: string; workspace: string }) => [f.role, f.workspace])).toEqual([['product-manager', '']]);

  // 上げた図を参照するページは、本文の枠の中で図を出す。
  const path = await place(request, { project: 'up', key: 'U-1', kind: 'result', role: 'tpm' }, '<title>図</title><img id="shot" src="files/screen-shot-2026-1.png">');
  await page.goto(`/up/U-1/?open=${encodeURIComponent(path)}`);
  const shot = viewer(page).locator('iframe').contentFrame().locator('#shot');
  await expect(shot).toBeVisible();
  await expect.poll(() => shot.evaluate((img: HTMLImageElement) => img.naturalWidth)).toBeGreaterThan(0);
  expect(errors).toEqual([]);
});

test('上げる枠: 字の file は頭の 20 行が見本に出る', async ({ page }) => {
  await page.goto('/up-text/T-1/');
  const lines = Array.from({ length: 30 }, (_, i) => `${i + 1} 行目`);
  await choose(page, 'notes.log', 'text/plain', Buffer.from(lines.join('\n')));
  await expect(upload(page).locator('.preview pre')).toHaveText(lines.slice(0, 20).join('\n'));
  await reviewed(page).check();
  await send(page).click();
  await expect(say(page)).toHaveText('上げた。ページからは files/notes-1.log で参照する');
  await expect(fileRows(page).locator('.thumb')).toHaveText(['log']);
});

test('上げる枠: 受けない拡張子と大きすぎる物は、送る前に断る。置き場が断った訳も出す', async ({ page, request }) => {
  await page.goto('/up-bad/B-1/');
  await choose(page, 'movie.mp4', 'video/mp4', Buffer.from('not a movie'));
  await expect(say(page)).toContainText('.mp4 は上げられない。上げられるのは png・jpg・jpeg・gif・webp・pdf・txt・log・md・csv・json');
  await expect(say(page)).toHaveClass(/bad/);
  await expect(upload(page).locator('.preview')).toBeHidden();
  await reviewed(page).check();
  await expect(send(page)).toBeDisabled();

  await choose(page, 'huge.txt', 'text/plain', Buffer.alloc(16 * 1024 * 1024 + 1, 'a'));
  await expect(say(page)).toHaveText('16.0 MiB は大きすぎる。1 つ 16.0 MiB まで');
  await expect(send(page)).toBeDisabled();

  // 拡張子は合うが、中身が PNG でない物は、置き場が断る。
  await choose(page, 'fake.png', 'image/png', Buffer.from('これは字'));
  await reviewed(page).check();
  await send(page).click();
  await expect(say(page)).toHaveText('上げられなかった（415）: 中身が .png の形でない');
  await expect(send(page)).toBeEnabled();
  expect((await (await request.get('/api/files?project=up-bad')).json()).files).toEqual([]);
});

test('上げる枠: file を枠に落としても選べる', async ({ page }) => {
  await page.goto('/up-drop/D-1/');
  const dropped = await page.evaluateHandle((text) => {
    const data = new DataTransfer();
    data.items.add(new File([text], 'Dropped Notes.md', { type: 'text/markdown' }));
    return data;
  }, '# 落とした\n本文');
  await upload(page).locator('.drop').dispatchEvent('drop', { dataTransfer: dropped });
  await expect(upload(page).locator('.preview')).toContainText('Dropped Notes.md（21 B）→ dropped-notes.md');
  await expect(upload(page).locator('.preview pre')).toHaveText('# 落とした\n本文');
  await reviewed(page).check();
  await send(page).click();
  await expect(say(page)).toHaveText('上げた。ページからは files/dropped-notes-1.md で参照する');
});

test('file の枠: ほかの所（CLI など）からこの Key に置かれた file は、読み直さなくても出る', async ({ page, request }) => {
  await openLive(page, '/up-live/L-1/');
  await placeFile(request, { project: 'up-live', key: 'L-2', name: 'other.png', role: 'tpm' }, 'image/png', PNG);
  await placeFile(request, { project: 'up-live', key: 'L-1', name: 'shot.png', role: 'software-engineer' }, 'image/png', PNG);
  await expect(fileRows(page)).toHaveCount(1);
  await expect(fileRows(page).locator('.name')).toHaveText('shot-1.png');
  await expect(fileRows(page).locator('.who')).toContainText('software-engineer');
});

test('上げる枠: script が無くても画面は出て、file の枠は読める', async ({ browser, request }) => {
  await placeFile(request, { project: 'up-nojs', key: 'J-1', name: 'shot.png', role: 'tpm' }, 'image/png', PNG);
  const context = await browser.newContext({ javaScriptEnabled: false });
  const page = await context.newPage();
  await page.goto('/up-nojs/J-1/');
  await expect(fileRows(page).locator('.name')).toHaveText('shot-1.png');
  await expect(upload(page)).toContainText('上げるには script が要る。');
  await expect(upload(page).locator('form')).toBeHidden();
  await context.close();
});
