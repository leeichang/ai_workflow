# Temporal PoC

對應[實作計畫](../../docs/系統規劃/04_報價單流程實作計畫.md)任務 0.2。

**目的**：在投入 20 週開發前，驗證 Temporal 能否承擔本平台最難的三件事。若任一項不成立，整個架構決策（決議 D-02）需重新評估。

**這是驗證用專案，不是產品程式碼。** 驗證完成後保留作為參考，正式實作在 `python-ai/`。

---

## 執行

```bash
cd poc/temporal
python3 -m venv .venv
.venv/bin/pip install temporalio pytest pytest-asyncio pytest-timeout
docker compose up -d          # 真實 Temporal + PostgreSQL
.venv/bin/python -m pytest -q
```

**結果（2026-09-14）：30 passed in 12.6s。** 報告見 [測試報告](../../docs/測試報告/202609/01_Temporal_PoC驗證報告.md)。

Temporal UI 在 http://localhost:8080 。

### 為何需要 Docker

大部分測試用 `WorkflowEnvironment.start_time_skipping()`，會自動下載輕量測試伺服器，不需 Docker。

但跨 Worker 重啟與失敗路徑測試必須連真實伺服器。time-skipping 環境會在 Workflow 等待時快轉虛擬時鐘，導致這類測試逾時。

真實伺服器還揭露了一個測試環境遮蔽的 API 誤用（`ApplicationError` 的匯入位置），詳見測試報告第 4.5 節。

---

## 驗證項目

### 1. 並簽與 REJECT 立即中斷

`test_one_reject_cancels_others_immediately`

三人並簽，第一人拒絕時流程立即結束，另外兩人的待辦被取消。不等其他人回應。

這驗證了 `ANY_REJECT` 策略的正確語意，以及取消 Activity 確實被呼叫。自研 Runtime 時，這裡最容易出的 bug 是「拒絕後仍等待其他人」或「取消時機錯誤導致重複執行」。

### 2. 動態簽核人

`test_dynamic_approvers_by_amount`

金額超過一百萬時自動加簽財務長。四個參數化案例涵蓋邊界：

| 金額 | 簽核人數 | 含 CFO |
|---|---|---|
| 900,000 | 2 | 否 |
| 1,000,000 | 2 | 否（等於不算超過）|
| 1,000,001 | 3 | 是 |
| 5,000,000 | 3 | 是 |

邊界值刻意測試，因為「大於」與「大於等於」的差異在真實專案中常造成爭議。

### 3. 狀態持久性

`test_state_survives_worker_restart`

Worker 在等待簽核期間被關閉，重啟後流程狀態完整保留，已簽核的結果不丟失。

這是 Temporal 最核心的價值。自研 Runtime 要做到這點，需要自己處理狀態持久化、事件重放、冪等性，是「Runtime 黑洞」的主要來源。

### 其他驗證

| 測試 | 驗證什麼 |
|---|---|
| `test_timeout_when_nobody_responds` | 逾期處理。time-skipping 讓七天瞬間過完 |
| `test_long_wait_does_not_block` | 等待三十天不佔用資源 |
| `test_duplicate_signal_is_idempotent` | 重複 Signal 不重複計算。前端重試或網路重送都可能發生 |
| `test_empty_resolution_fails_explicitly` | 簽核人解析為空時明確失敗，不靜默卡住 |
| `test_policy.py` | Policy Engine 十九個案例，含完成策略、結果策略、逾期交互 |

---

## 架構約束（正式版必須維持）

PoC 刻意遵守這些約束，證明它們可行：

| 約束 | 原因 |
|---|---|
| Workflow 內無 IO、不讀時鐘、不用亂數 | Temporal 靠重放事件還原狀態，非決定性程式碼會導致重放結果不一致 |
| 簽核人解析在 Activity | 需查資料庫，屬於 IO |
| Policy Engine 是純函數 | 可在 Workflow 內直接呼叫，不需往返 Activity |
| Human Task 建立與取消都是 Activity | 寫入資料庫 |
| 時間由 `workflow.now()` 取得 | 重放時回傳原始時間，非當下時間 |

`workflow.py` 的 `with workflow.unsafe.imports_passed_through():` 是必要的。Temporal 會沙箱化 Workflow 模組以偵測非決定性，此語法標示這些匯入是安全的。

---

## 檔案

| 檔案 | 內容 | 正式版落點 |
|---|---|---|
| `models.py` | 資料模型 | 由 `schemas/*.json` 產生 Pydantic |
| `policy.py` | Approval Policy Engine | `python-ai/policy/engine.py` |
| `workflow.py` | 並簽 Workflow | `python-ai/workflows/interpreter.py` 的一部分 |
| `activities.py` | Activity，記憶體模擬 | `python-ai/activities/`，改呼叫 Rust API |
| `test_policy.py` | 純函數測試 | `python-ai/tests/test_policy.py` |
| `test_workflow.py` | Workflow 測試 | `python-ai/tests/test_interpreter.py` |

---

## 尚未驗證

以下留給正式實作階段：

| 項目 | 何時驗證 |
|---|---|
| Rust 經 gRPC 控制 Temporal | 任務 1.3。這是 D-02 的另一個風險點 |
| Workflow 版本演進（patching） | 任務 4.1 |
| 真實 Temporal Server（非測試環境） | 任務 1.4 |
| 多租戶 workflow_id 前綴 | 任務 1.3 |
| Activity 重試與 idempotency | 任務 3.5 |
