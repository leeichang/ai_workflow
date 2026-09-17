/**
 * HTTP client 底層
 *
 * 職責：
 *   附加認證標頭、統一錯誤轉換、日期欄位解析。
 *
 * 用原生 fetch 而非 axios：
 *   MSW 攔截 fetch 最直接，且不需要額外的 adapter 設定。
 *   我們的需求也不複雜，不值得多一層抽象。
 */

import { ApiError, type ApiErrorBody } from './types'

const BASE_URL = import.meta.env.VITE_API_BASE_URL ?? '/api'

/**
 * token 存取
 *
 * 預設回傳 null。實際的來源由 auth/useSession.ts 在模組載入時
 * 用 setTokenGetter 接上——client 不該知道 token 存在哪裡。
 */
let tokenGetter: () => string | null = () => null

export function setTokenGetter(fn: () => string | null): void {
  tokenGetter = fn
}

interface RequestOptions {
  method?: 'GET' | 'POST' | 'PUT' | 'DELETE' | 'PATCH'
  body?: unknown
  /** 不附加認證標頭，登入端點用 */
  anonymous?: boolean
  signal?: AbortSignal
}

export async function request<T>(path: string, options: RequestOptions = {}): Promise<T> {
  const { method = 'GET', body, anonymous = false, signal } = options

  const headers: Record<string, string> = {}
  if (body !== undefined) {
    headers['Content-Type'] = 'application/json'
  }
  if (!anonymous) {
    const token = tokenGetter()
    if (token) {
      headers['Authorization'] = `Bearer ${token}`
    }
  }

  const response = await fetch(`${BASE_URL}${path}`, {
    method,
    headers,
    body: body === undefined ? undefined : JSON.stringify(body),
    signal,
  })

  // 204 No Content 沒有 body
  if (response.status === 204) {
    return undefined as T
  }

  const text = await response.text()

  // 解析可能失敗：反向代理或負載平衡器出錯時常回 HTML 而非 JSON。
  // 直接讓 JSON.parse 拋錯會使呼叫端拿到 SyntaxError 而非 ApiError，
  // 錯誤處理邏輯就失效了。
  let parsed: unknown = null
  let parseFailed = false
  if (text) {
    try {
      parsed = JSON.parse(text)
    } catch {
      parseFailed = true
    }
  }

  if (!response.ok) {
    throw new ApiError(response.status, normalizeError(parsed, response.status))
  }

  if (parseFailed) {
    // 2xx 但內容不是 JSON，屬於伺服器行為異常
    throw new ApiError(response.status, {
      code: 'INTERNAL_ERROR',
      message: '伺服器回應格式非預期',
    })
  }

  return parsed as T
}

/**
 * 把非預期的錯誤格式也轉成統一結構
 *
 * 後端正常會回 { code, message }，但反向代理或負載平衡器
 * 可能回 HTML 或純文字。前端不該因此崩潰。
 */
function normalizeError(parsed: unknown, status: number): ApiErrorBody {
  if (
    parsed !== null &&
    typeof parsed === 'object' &&
    'code' in parsed &&
    'message' in parsed
  ) {
    return parsed as ApiErrorBody
  }

  const fallbackCode = status === 401 ? 'UNAUTHORIZED' : status >= 500 ? 'INTERNAL_ERROR' : 'BAD_REQUEST'
  return {
    code: fallbackCode,
    message: `伺服器回應異常（HTTP ${status}）`,
  }
}

/** ISO 8601 字串轉 Date，供列表與詳情使用 */
export function parseDate(value: string | null): Date | null {
  return value === null ? null : new Date(value)
}
