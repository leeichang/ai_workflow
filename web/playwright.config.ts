import { defineConfig, devices } from '@playwright/test'

/**
 * E2E 設定
 *
 * 不用 webServer 自動啟動：這套系統需要 PostgreSQL、Temporal、
 * Rust API、渲染服務同時在跑，Playwright 起得了前端也起不了那些。
 * 測試前需自行啟動服務，連不上時第一個測試會明確失敗。
 *
 * workers 設為 1：測試共用同一個資料庫，平行執行會互相干擾
 * （例如一個在核准待辦，另一個正在數待辦有幾筆）。
 */
export default defineConfig({
  testDir: './e2e',
  timeout: 30_000,
  fullyParallel: false,
  workers: 1,
  reporter: [['list'], ['html', { outputFolder: 'e2e-report', open: 'never' }]],
  use: {
    baseURL: 'http://localhost:3040',
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
    viewport: { width: 1440, height: 900 },
    locale: 'zh-TW',
  },
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
  ],
})
