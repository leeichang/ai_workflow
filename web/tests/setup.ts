/**
 * 測試環境設定
 *
 * MSW 以 onUnhandledRequest: 'error' 啟動：
 * 任何未被 handler 攔截的請求都會讓測試失敗。
 * 這可防止測試悄悄打到真實網路，或因為漏寫 handler 而假性通過。
 */
import { afterAll, afterEach, beforeAll } from 'vitest'
import { server } from './msw/server'
import { setTokenGetter } from '../src/api/client'

beforeAll(() => {
  server.listen({ onUnhandledRequest: 'error' })
  // 測試預設帶 token，個別測試可覆寫
  setTokenGetter(() => 'test-token')
})

afterEach(() => {
  server.resetHandlers()
  setTokenGetter(() => 'test-token')
})

afterAll(() => server.close())
