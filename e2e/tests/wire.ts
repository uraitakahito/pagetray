// 試験が API を叩く時の道と欄の名を、向けた server に合わせる（playwright.config.ts の PAGETRAY_E2E_API）。
//   current  今の server（pagetray-server）: 置く道は /api/pages?…、引数は snake_case（既定）
//   next     作り直している新しい server（pagetray-next。AI-36）: 置く道は /api/projects/{p}/keys/{k}/…、引数は camelCase
// 画面（/・/{project}/・/{project}/{key}/）の URL と字・ページと file の URL は、どちらの server でも同じ。

/** 新しい server に向けているか。 */
export const next = process.env.PAGETRAY_E2E_API === 'next';

/** 引数の名（snake_case）を、向けた server の名にする。 */
export function params(data: Record<string, string | undefined>): URLSearchParams {
  const pairs = Object.entries(data).filter((pair): pair is [string, string] => pair[1] !== undefined);
  return new URLSearchParams(next ? pairs.map(([k, v]) => [k.replace(/_([a-z])/g, (_, c: string) => c.toUpperCase()), v]) : pairs);
}

/** ページを置く道（引数の project・key・kind は、新しい server では道に入る）。 */
export function placePath(to: Record<string, string | undefined>): string {
  if (!next) return `/api/pages?${params(to)}`;
  const { project, key, kind, ...rest } = to;
  return `/api/projects/${project}/keys/${key}/kinds/${kind}/pages?${params(rest)}`;
}

/** file を置く道。 */
export function placeFilePath(to: Record<string, string | undefined>): string {
  if (!next) return `/api/files?${params(to)}`;
  const { project, key, ...rest } = to;
  return `/api/projects/${project}/keys/${key}/files?${params(rest)}`;
}

/** ページの一覧の 1 頁の数の引数（今の server は limit、新しい server は maxResults）。 */
export const pageSize = (n: number): string => (next ? `maxResults=${n}` : `limit=${n}`);
