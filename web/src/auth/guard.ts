/**
 * 路由守衛
 *
 * 未登入者一律導向登入頁，唯一例外是標了 meta.public 的路由。
 * 預設拒絕而非預設允許：新增頁面時忘記標註，結果是「被擋下來」
 * 而不是「未登入者看得到」。
 *
 * 這道守衛擋的是使用者，不是攻擊者——前端程式碼在使用者手上，
 * 改掉就繞過了。真正的授權在後端 Actor extractor。
 */

import type { Router } from 'vue-router'
import { useSession } from './useSession'

export function installAuthGuard(router: Router): void {
  router.beforeEach((to) => {
    const { isAuthenticated } = useSession()
    const isPublic = to.meta.public === true

    if (!isAuthenticated.value && !isPublic) {
      return {
        path: '/login',
        // 記下原本要去的地方，登入後送回去。
        // 只存 fullPath（站內相對路徑），登入頁還會再驗一次。
        query: { redirect: to.fullPath },
      }
    }

    // 已登入還去登入頁，多半是按了上一頁或存了書籤。
    // 讓他停在登入頁沒有意義，送回首頁。
    if (isAuthenticated.value && to.path === '/login') {
      return { path: '/' }
    }

    return true
  })
}
