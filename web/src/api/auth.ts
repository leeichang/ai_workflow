/**
 * 認證 API
 */

import { request } from './client'
import type { LoginRequest, LoginResponse, UserInfo } from './types'

/**
 * 登入
 *
 * anonymous 為 true，因為登入時還沒有 token。
 * 若誤附上舊的過期 token，後端會先擋在認證層而非進入登入邏輯。
 */
export function login(input: LoginRequest): Promise<LoginResponse> {
  return request<LoginResponse>('/auth/login', {
    method: 'POST',
    body: input,
    anonymous: true,
  })
}

export function me(): Promise<UserInfo> {
  return request<UserInfo>('/me')
}
