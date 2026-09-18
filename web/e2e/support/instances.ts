/**
 * E2E 啟動的流程實例追蹤與清理
 *
 * 為什麼需要這個：測試會啟動流程，但**不會把它跑完**——
 * 流程停在某個人工節點之後就沒有人去簽了。那個 Temporal workflow
 * 於是永遠活著並持續輪詢，而測試每跑一次就再增加幾個。
 *
 * 實測的後果（同一份程式碼、同一台機器，連續三次全量 E2E）：
 *
 *   第一次   403 秒
 *   第二次   437 秒
 *   第三次  1348 秒   ← 22 分鐘
 *
 * 而且每次失敗的是不同的測試——那是負載的特徵，不是缺陷的特徵。
 * 先前被記成「既有競態」的幾個 flake 其實都是這個問題。
 *
 * 加長 timeout 不能解決：那只會讓 22 分鐘變成 30 分鐘，
 * 累積照舊。要解決就得讓測試把自己啟動的東西收掉。
 *
 * 用 POST /instances/{id}/cancel 而非直接刪資料庫：
 * 那支端點會送取消請求給 Temporal，由 workflow 自己做清理
 * （取消待辦、寫稽核）後回報最終狀態。直接刪 DB 列會留下
 * Temporal 端的孤兒——已經發生過一次，34 個執行中只有 6 個有 DB 列。
 */

import type { APIRequestContext } from '@playwright/test'

const API = 'http://localhost:3001'

/** 本次執行啟動的實例。以 worker 為範圍，afterAll 時統一收掉。 */
const tracked: string[] = []

/** 記錄一個剛啟動的實例，供稍後清理 */
export function trackInstance(instanceId: string): void {
  tracked.push(instanceId)
}

/**
 * 取消所有追蹤中的實例
 *
 * 刻意不讓清理失敗影響測試結果：清理是衛生工作，
 * 不是被測的行為。某個實例已經結束（回 409）或服務暫時不通時，
 * 讓它過去就好——否則一個清理失敗會蓋掉真正的測試結果，
 * 反而更難查。
 *
 * 回傳成功取消的數量，供需要斷言的呼叫端使用。
 */
export async function cancelTrackedInstances(
  request: APIRequestContext,
  jwt: string,
): Promise<number> {
  let cancelled = 0

  for (const id of tracked) {
    try {
      const res = await request.post(`${API}/instances/${id}/cancel`, {
        headers: { Authorization: `Bearer ${jwt}` },
        data: { reason: 'E2E 清理' },
      })
      if (res.ok()) cancelled += 1
    } catch {
      // 清理失敗不影響測試結果，理由見上方註解
    }
  }

  tracked.length = 0
  return cancelled
}
