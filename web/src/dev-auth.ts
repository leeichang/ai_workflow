/**
 * 開發用自動登入
 *
 * 登入頁尚未實作，此模組在開發模式下自動以 seed 帳號登入，
 * 讓畫面能載入資料。正式版由登入頁取代。
 *
 * import.meta.env.DEV 為 false 時完全不執行，不會進 production bundle。
 */
import { login } from './api/auth'

const DEV_CREDENTIALS = {
  tenant_code: 'demo',
  email: 'designer@demo.local',
  password: 'demo1234',
}

export async function ensureDevToken(): Promise<void> {
  if (!import.meta.env.DEV) return
  if (localStorage.getItem('access_token')) return

  try {
    const res = await login(DEV_CREDENTIALS)
    localStorage.setItem('access_token', res.access_token)
    console.info('[dev] 已自動登入為', res.user.name)
  } catch (e) {
    console.warn('[dev] 自動登入失敗，請確認 API 已啟動並執行過 server/seed.sh', e)
  }
}
