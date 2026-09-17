/**
 * 登入狀態
 *
 * 模組層級的單一狀態（非 pinia）：專案其餘部分用的是 composable 模式，
 * 為了登入引入狀態管理套件並不划算。狀態宣告在模組層級，
 * 所有 useSession() 取得同一份。
 *
 * token 存 localStorage 而非 sessionStorage：使用者關掉分頁再開回來
 * 不該被要求重新登入。代價是 XSS 時 token 會被讀走——這需要靠
 * CSP 與輸入清理防範，換 sessionStorage 並不能解決。
 */

import { computed, readonly, ref } from 'vue'
import * as authApi from '@/api/auth'
import { setTokenGetter } from '@/api/client'
import type { LoginRequest, UserInfo } from '@/api/types'

export const STORAGE_KEY = 'workflow.session'

/** 存進 localStorage 的結構 */
interface StoredSession {
  access_token: string
  user: UserInfo
}

// ── 模組層級狀態 ────────────────────────────────────────

const token = ref<string | null>(null)
const user = ref<UserInfo | null>(null)

/**
 * 讀出 JWT 的到期時間
 *
 * 只解析不驗章——簽章驗證是後端的事，前端解 exp 的目的是避免
 * 帶著明知已過期的 token 去打 API，讓使用者看到一連串 401。
 * 解不出來時回傳 null，當作「不確定」而非「已過期」，
 * 讓後端去判斷。
 */
function readExpiry(jwt: string): number | null {
  const parts = jwt.split('.')
  if (parts.length !== 3) return null
  try {
    const payload: unknown = JSON.parse(
      new TextDecoder().decode(
        Uint8Array.from(
          atob(parts[1].replace(/-/g, '+').replace(/_/g, '/')),
          (c) => c.charCodeAt(0),
        ),
      ),
    )
    if (
      payload !== null &&
      typeof payload === 'object' &&
      'exp' in payload &&
      typeof (payload as { exp: unknown }).exp === 'number'
    ) {
      return (payload as { exp: number }).exp
    }
    return null
  } catch {
    return null
  }
}

function isExpired(jwt: string): boolean {
  const exp = readExpiry(jwt)
  if (exp === null) return false
  return exp * 1000 <= Date.now()
}

function persist(session: StoredSession): void {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(session))
}

function clearStorage(): void {
  localStorage.removeItem(STORAGE_KEY)
}

// client.ts 改讀 session 的 token，而不是自己去翻 localStorage。
// 登入後不需要額外通知 client，它每次請求都會重新取。
setTokenGetter(() => token.value)

// ── 對外介面 ────────────────────────────────────────────

export function useSession() {
  /**
   * 從 localStorage 回復登入狀態
   *
   * 在 app 啟動時呼叫一次。內容毀損或 token 已過期都當作未登入，
   * 並清掉 storage——留著只會讓下次啟動重複失敗一次。
   */
  function restore(): void {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (raw === null) return

    let parsed: StoredSession
    try {
      parsed = JSON.parse(raw) as StoredSession
    } catch {
      clearStorage()
      return
    }

    if (
      typeof parsed?.access_token !== 'string' ||
      parsed?.user === undefined ||
      parsed?.user === null
    ) {
      clearStorage()
      return
    }

    if (isExpired(parsed.access_token)) {
      clearStorage()
      return
    }

    token.value = parsed.access_token
    user.value = parsed.user
  }

  /**
   * 登入
   *
   * 失敗時拋出 ApiError，且不動既有狀態——已登入的使用者
   * 在別處登入失敗，不該因此被踢出目前的工作階段。
   */
  async function login(input: LoginRequest): Promise<void> {
    const result = await authApi.login(input)
    token.value = result.access_token
    user.value = result.user
    persist({ access_token: result.access_token, user: result.user })
  }

  /**
   * 登出
   *
   * 純前端清除。後端的 JWT 是無狀態的，沒有 session 可以作廢，
   * token 會一直有效到 exp。要做到立即失效需要黑名單或改用
   * refresh token，那是後續的工作。
   */
  function logout(): void {
    token.value = null
    user.value = null
    clearStorage()
  }

  /** 測試用：重置狀態 */
  function reset(): void {
    token.value = null
    user.value = null
  }

  /**
   * 角色判斷
   *
   * admin 視為擁有所有角色，與後端 Actor::is_admin 的行為一致。
   * 兩邊若不一致，畫面會藏起使用者其實有權限做的操作。
   */
  function hasRole(role: string): boolean {
    const roles = user.value?.roles
    if (roles === undefined) return false
    return roles.includes('admin') || roles.includes(role)
  }

  return {
    token: readonly(token),
    user: readonly(user),
    isAuthenticated: computed(() => token.value !== null),
    restore,
    login,
    logout,
    reset,
    hasRole,
  }
}
