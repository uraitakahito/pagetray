// 文書の本（docs/book）の試験の設定。先に ci/docs-build.sh で本を作っておく。作った本を、そのまま配って見る。
//   PAGETRAY_E2E_DOCS_PORT  配る port（既定 18803）
//   PAGETRAY_E2E_CHANNEL    この機械に入っている browser を使う時の名（chrome など）。無ければ Playwright の Chromium
import { defineConfig } from '@playwright/test';

const port = Number(process.env.PAGETRAY_E2E_DOCS_PORT ?? 18803);

export default defineConfig({
  testDir: 'docs-tests',
  retries: 0,
  forbidOnly: !!process.env.CI,
  reporter: process.env.CI ? [['list'], ['html', { open: 'never' }]] : 'list',
  use: {
    baseURL: `http://127.0.0.1:${port}`,
    channel: process.env.PAGETRAY_E2E_CHANNEL,
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
  },
  webServer: {
    command: `python3 -m http.server ${port} --bind 127.0.0.1 --directory ../docs/book`,
    url: `http://127.0.0.1:${port}/index.html`,
    reuseExistingServer: false,
  },
});
