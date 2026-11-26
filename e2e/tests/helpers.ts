// 試験の材料を置く・一覧の UI の区画を選ぶ。
import { expect, type APIRequestContext, type Locator, type Page } from '@playwright/test';
import { createServer, type Server } from 'node:http';

export type Placing = {
  project: string;
  key: string;
  kind: string;
  role: string;
  workspace?: string;
  /** 取り込みの時だけ: 番号と置いた時刻を決める。 */
  n?: string;
  createdAt?: string;
  /** 送り手の付随する id。書けば、置き場が castlist（試験では startCastlist の偽物）に聞く。 */
  actorIssuer?: string;
  actorRef?: string;
};

/** query の引数（値の無い物は付けない）。 */
const query = (data: Record<string, string | undefined>) =>
  new URLSearchParams(Object.entries(data).filter((pair): pair is [string, string] => pair[1] !== undefined));

/** API でページを 1 枚置き、置き場の中の path を返す（browser の外から送るので Origin は付かない）。 */
export async function place(request: APIRequestContext, to: Placing, html: string): Promise<string> {
  const { project, key, kind, ...rest } = to;
  const response = await request.post(`/api/projects/${project}/keys/${key}/kinds/${kind}/pages?${query(rest)}`, {
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
  const { project, key, ...rest } = to;
  const response = await request.post(`/api/projects/${project}/keys/${key}/files?${query(rest)}`, {
    data: body,
    headers: { 'content-type': type },
  });
  expect(response.ok(), await response.text()).toBeTruthy();
  return response.json();
}

/** ページの path（`place` が返した物）を、project・Key・種類・番号に分ける。 */
const pageOf = (path: string) => {
  const [, project, key, kind, n] = /^\/([^/]+)\/([^/]+)\/(.+)-(\d+)\.html$/.exec(path)!;
  return { project, key, kind, n };
};

/** ページ 1 枚の道（`/api/projects/{p}/keys/{k}/kinds/{kind}/pages/{n}`）。 */
const pageApi = (path: string) => {
  const { project, key, kind, n } = pageOf(path);
  return `/api/projects/${project}/keys/${key}/kinds/${kind}/pages/${n}`;
};

/** API で、ページ 1 枚の返事の状態を書く（browser の外から送るので Origin は付かない）。`path` は `place` が返した物。 */
export async function reply(request: APIRequestContext, path: string, state: 'waiting' | 'answered'): Promise<void> {
  const response = await request.put(`${pageApi(path)}/reply`, { data: { state } });
  expect(response.ok(), await response.text()).toBeTruthy();
}

/** API で、ページ 1 枚の返事の状態を、待ちの相手を名指して書く。 */
export async function replyOn(request: APIRequestContext, path: string, state: 'waiting' | 'answered', on: 'pm' | 'review' | 'tpm'): Promise<void> {
  const response = await request.put(`${pageApi(path)}/reply`, { data: { state, on } });
  expect(response.ok(), await response.text()).toBeTruthy();
}

/** API で、レビューの印（判定・見た物）を書く。 */
export async function reviewMarks(request: APIRequestContext, path: string, marks: { verdict?: string; targets?: string[] }): Promise<void> {
  const response = await request.post(`${pageApi(path)}/review`, { data: marks });
  expect(response.ok(), await response.text()).toBeTruthy();
}

/** 偽の castlist の port（試験の置き場の次の port。playwright.config.ts が置き場に渡す）。 */
export const CASTLIST_PORT = Number(process.env.PAGETRAY_E2E_PORT ?? 18801) + 1;

/**
 * 一覧の UI の、castlist のアクターの画面への link の頭（playwright.config.ts が `PAGETRAY_CASTLIST_LINK_URL` で渡す）。
 * 聞く先（偽の castlist）と違う所にして、link が聞く先から作られていないことを見る。開かないので、待ち受ける物は無い。
 */
export const CASTLIST_LINK = `http://localhost:${CASTLIST_PORT + 1}`;

export type Actor = { id: string; name: string; kind: string };

/**
 * 偽の castlist を立てる。`POST /api/resolve`（`asOf`）に、`actors` にある付随する id はそのアクター、ほかは無しと答える
 * （無しの物は `actor` を付けない）。
 * 置き終えたら close する（ほかの試験は、送り手を書かないので、castlist に聞かない）。
 */
export async function startCastlist(actors: Record<string, Actor>): Promise<Server> {
  const server = createServer((req, res) => {
    let body = '';
    req.on('data', (chunk) => (body += chunk));
    req.on('end', () => {
      const asked = JSON.parse(body || '{}');
      const results = (asked.refs ?? []).map((ref: string) => (actors[ref] ? { ref, actor: actors[ref] } : { ref }));
      res.writeHead(200, { 'content-type': 'application/json', connection: 'close' });
      res.end(JSON.stringify({ asOf: new Date().toISOString(), results }));
    });
  });
  await new Promise<void>((resolve) => server.listen(CASTLIST_PORT, '127.0.0.1', resolve));
  return server;
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
/**
 * 行の、ページを本文の枠に開く link（行のすぐ下の a）。行には送り手の link（`.sender a.actor`）も付くことがあるので、
 * 行の中の link を役割だけで選ばず、この関数で選ぶ。
 */
export const pageLink = (row: Locator) => row.locator(':scope > a');
export const fresh = (page: Page) => page.locator('#fresh');
/** 操作の帯の、返事待ちの数。 */
export const waiting = (page: Page) => page.locator('#waiting');
/** 行の、返事の印。 */
export const replyMark = (page: Page, path: string) => list(page).locator(`li[data-path="${path}"] .reply`);
/** 操作の帯の、レビュー待ちの数と、上げた数。 */
export const reviewWaiting = (page: Page) => page.locator('#review-waiting');
export const escalated = (page: Page) => page.locator('#escalated');
/** 行の、判定の印（行の上の段の物。見られたページの「レビュー:」の物は入らない）。 */
export const verdictMark = (page: Page, path: string) => list(page).locator(`li[data-path="${path}"] .top .verdict`);
/** 行の下の link の並び（`targets` は見た物・見る物、`reviews` はそのページを見たレビュー）。 */
export const links = (page: Page, path: string, which: 'targets' | 'reviews') => list(page).locator(`li[data-path="${path}"] .links.${which}`);

/**
 * 一覧の列を、頭から y px 下へ送る。幅が広い時は真ん中の列（.middle）が、狭い時は画面全体が送られる。
 */
export async function scrollList(page: Page, y: number): Promise<void> {
  await page.evaluate((y) => {
    const list = document.getElementById('list')!;
    const box = list.parentElement!;
    if (box.scrollHeight > box.clientHeight) box.scrollTop = y;
    else window.scrollTo(0, list.getBoundingClientRect().top + window.scrollY + y);
  }, y);
}

/** 一覧の列の頭が、見えている所の頭にあるか（頭へ送られているか）。 */
export const listAtTop = (page: Page) =>
  page.evaluate(() => {
    const list = document.getElementById('list')!;
    const box = list.parentElement!;
    const top = box.scrollHeight > box.clientHeight ? box.getBoundingClientRect().top : 0;
    return list.getBoundingClientRect().top >= top - 1;
  });
// Key の画面だけの区画: file の枠（files）・上げる枠（upload）。
export const files = (page: Page) => page.locator('#files');
export const fileRows = (page: Page) => files(page).locator('li[data-file]');
export const upload = (page: Page) => page.locator('#upload');

/**
 * 絞り込みの列の候補。見出し（返事・送り手・担当者（役割）・workspace・種類・Key・project）と、候補の名で選ぶ。
 * link の先では選ばない（ほかの見出しの候補も、今の条件を link の先に持つ）。
 */
export const choice = (page: Page, heading: string, label: string) =>
  filters(page)
    .locator(`h2:text-is("${heading}") + ul a`)
    .filter({ has: page.locator('span:first-child', { hasText: new RegExp(`^${escape(label)}`) }) });

const escape = (text: string) => text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
