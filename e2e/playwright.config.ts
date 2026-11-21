// 一覧の UI の試験の設定。同じ commit の pagetray-server を、空いている port と一時の DB で起こして向ける。
//   PAGETRAY_E2E_SERVER   起こす binary（既定は ../target/debug/pagetray-server。先に cargo build --bin pagetray-server）
//   PAGETRAY_E2E_PORT     待ち受ける port（既定 18801）
//   PAGETRAY_E2E_CHANNEL  この機械に入っている browser を使う時の名（chrome など）。無ければ Playwright の Chromium
import { defineConfig } from '@playwright/test';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const port = Number(process.env.PAGETRAY_E2E_PORT ?? 18801);
const server = process.env.PAGETRAY_E2E_SERVER ?? '../target/debug/pagetray-server';
const db = join(tmpdir(), `pagetray-e2e-${port}.db`);

export default defineConfig({
  testDir: 'tests',
  // 新着の印は、ほかの試験が置いたページも数える。だから 1 本ずつ順に流す。
  fullyParallel: false,
  workers: 1,
  // やり直さない（時々落ちる試験を隠さない）。
  retries: 0,
  forbidOnly: !!process.env.CI,
  reporter: process.env.CI ? [['list'], ['html', { open: 'never' }]] : 'list',
  use: {
    baseURL: `http://127.0.0.1:${port}`,
    channel: process.env.PAGETRAY_E2E_CHANNEL,
    // 時刻は browser の地域の時刻で出る。機械によって変わらないように決めておく。
    timezoneId: 'Asia/Tokyo',
    locale: 'ja-JP',
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
  },
  webServer: {
    // 前の回の DB を消してから起こす（試験は、いつも空の置き場から始まる）。
    command: `rm -f "${db}" "${db}-wal" "${db}-shm" && exec "${server}"`,
    env: { PAGETRAY_LISTEN: `127.0.0.1:${port}`, PAGETRAY_DB: db },
    url: `http://127.0.0.1:${port}/healthz`,
    reuseExistingServer: false,
  },
});
