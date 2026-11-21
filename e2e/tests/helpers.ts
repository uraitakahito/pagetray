// 試験の材料を置く・一覧の UI の区画を選ぶ。
import { expect, type APIRequestContext, type Page } from '@playwright/test';

export type Placing = {
  project: string;
  key: string;
  kind: string;
  role: string;
  workspace?: string;
  /** 取り込みの時だけ: 番号と置いた時刻を決める。 */
  n?: string;
  created_at?: string;
};

/** API でページを 1 枚置き、置き場の中の path を返す（browser の外から送るので Origin は付かない）。 */
export async function place(request: APIRequestContext, to: Placing, html: string): Promise<string> {
  const response = await request.post(`/api/pages?${new URLSearchParams(to)}`, {
    data: html,
    headers: { 'content-type': 'text/html' },
  });
  expect(response.ok(), await response.text()).toBeTruthy();
  return (await response.json()).path;
}

/** API で file を 1 つ置き、記録を返す（browser の外から送るので Origin は付かない）。 */
export async function placeFile(
  request: APIRequestContext,
  to: { project: string; key: string; name: string; role: string },
  type: string,
  body: Buffer,
): Promise<{ path: string; role: string }> {
  const response = await request.post(`/api/files?${new URLSearchParams(to)}`, { data: body, headers: { 'content-type': type } });
  expect(response.ok(), await response.text()).toBeTruthy();
  return response.json();
}

/** 1×1 の PNG。 */
export const PNG = Buffer.from(
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==',
  'base64',
);

/** 一覧の UI を開き、新着の知らせ（/api/events）につながるまで待つ。つながる前に置いたページの知らせは届かない。 */
export async function openLive(page: Page, url: string): Promise<void> {
  const live = page.waitForResponse((r) => r.url().endsWith('/api/events') && r.status() === 200);
  await page.goto(url);
  await live;
}

/** console の誤りと、script の例外を集める。 */
export function collectErrors(page: Page): string[] {
  const errors: string[] = [];
  page.on('console', (message) => {
    if (message.type() === 'error') errors.push(message.text());
  });
  page.on('pageerror', (error) => errors.push(String(error)));
  return errors;
}

// 区画: 操作の帯（toolbar）・絞り込みの列（filters）・一覧の列（list）・本文の枠（viewer）。template の id で選ぶ。
export const toolbar = (page: Page) => page.locator('#toolbar');
export const filters = (page: Page) => page.locator('#filters');
export const list = (page: Page) => page.locator('#list');
export const rows = (page: Page) => list(page).locator('li[data-path]');
export const viewer = (page: Page) => page.locator('#viewer');
export const fresh = (page: Page) => page.locator('#fresh');
// Key の画面だけの区画: file の枠（files）・上げる枠（upload）。
export const files = (page: Page) => page.locator('#files');
export const fileRows = (page: Page) => files(page).locator('li[data-file]');
export const upload = (page: Page) => page.locator('#upload');

/**
 * 絞り込みの列の候補。見出し（担当者（役割）・workspace・種類・Key・project）と、候補の名で選ぶ。
 * link の先では選ばない（ほかの見出しの候補も、今の条件を link の先に持つ）。
 */
export const choice = (page: Page, heading: string, label: string) =>
  filters(page)
    .locator(`h2:text-is("${heading}") + ul a`)
    .filter({ has: page.locator('span:first-child', { hasText: new RegExp(`^${escape(label)}`) }) });

const escape = (text: string) => text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
