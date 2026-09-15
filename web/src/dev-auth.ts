/**
 * 開發用自動登入
 *
 * 登入頁尚未實作，此模組在開發模式下自動以 seed 帳號登入，
 * 讓畫面能載入資料。正式版由登入頁取代。
 *
 * import.meta.env.DEV 為 false 時完全不執行，不會進 production bundle。
 */
import { login, me } from './api/auth'

const TOKEN_KEY = 'access_token'

const DEV_CREDENTIALS = {
  tenant_code: 'demo',
  email: 'designer@demo.local',
  password: 'demo1234',
}

/**
 * 確保有可用的 token
 *
 * 不只檢查 token 是否存在，還要確認它仍有效。
 * API 重啟後 JWT 金鑰若改變，或 token 過期，舊值會留在
 * localStorage 讓後續請求全部 401，畫面看起來像壞掉。
 */
export async function ensureDevToken(): Promise<void> {
  if (!import.meta.env.DEV) return

  if (await hasValidToken()) return

  try {
    const res = await login(DEV_CREDENTIALS)
    localStorage.setItem(TOKEN_KEY, res.access_token)
    console.info('[dev] 已自動登入為', res.user.name)
  } catch (e) {
    localStorage.removeItem(TOKEN_KEY)
    console.warn(
      '[dev] 自動登入失敗。請確認 API 已啟動並執行過 server/seed.sh',
      e,
    )
  }
}

async function hasValidToken(): Promise<boolean> {
  if (!localStorage.getItem(TOKEN_KEY)) return false

  try {
    await me()
    return true
  } catch {
    // 失效就清掉，讓後續重新登入
    localStorage.removeItem(TOKEN_KEY)
    return false
  }
}
