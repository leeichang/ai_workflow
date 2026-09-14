# Temporal PoC 驗證報告

- 日期：2026-09-14
- 對應任務：[實作計畫](../../系統規劃/04_報價單流程實作計畫.md) 任務 0.2
- 目的：驗證決議 D-02「不自研 Workflow Runtime，改用 Temporal」是否成立
- 結論：**三項核心驗證全部通過，決議成立，可進入 Phase 1**

---

## 1. 摘要

| 項目 | 結果 |
|---|---|
| 測試總數 | 30 |
| 通過 | 30 |
| 失敗 | 0 |
| 執行時間 | 12.6 秒 |
| 環境 | Temporal Server 1.25.2（docker）+ PostgreSQL 16 + Python 3.12 + temporalio 1.32.0 |

執行指令：

```bash
cd poc/temporal
docker compose up -d
.venv/bin/python -m pytest -q
```

---

## 2. 核心驗證結果

任務 0.2 指定三項。若任一項不成立，整個架構決策需重評估。

### 2.1 並簽與 REJECT 立即中斷

**測試**：`test_one_reject_cancels_others_immediately`

三人並簽，第一人拒絕時流程立即結束，另外兩人的待辦被取消，不等其他人回應。

驗證內容：

| 斷言 | 結果 |
|---|---|
| 判定為 REJECT | 通過 |
| 只有一筆簽核結果，未等待其他人 | 通過 |
| 其餘兩個 task 出現在 `cancelled_task_ids` | 通過 |
| Activity 真的把 task 狀態改為 CANCELLED | 通過 |
| `cancel_human_tasks` 確實被呼叫 | 通過 |

最後兩項是關鍵。只驗證回傳值不夠，必須確認取消動作真的執行了，否則正式環境會留下永遠不會被處理的待辦。

### 2.2 動態簽核人

**測試**：`test_dynamic_approvers_by_amount`（四個參數化案例）

金額超過一百萬時自動加簽財務長。

| 金額 | 簽核人數 | 含財務長 | 結果 |
|---|---|---|---|
| 900,000 | 2 | 否 | 通過 |
| 1,000,000 | 2 | 否 | 通過 |
| 1,000,001 | 3 | 是 | 通過 |
| 5,000,000 | 3 | 是 | 通過 |

刻意測邊界值。「大於」與「大於等於」的差異在真實專案常引起爭議，規格寫的是 `total > 1000000`，一百萬整不加簽。

### 2.3 狀態持久性

**測試**：`test_state_survives_worker_restart`

Worker 在等待簽核期間關閉，重啟後流程狀態完整保留。

執行序列：

1. 第一個 Worker 啟動流程，三人待簽
2. 第一人簽核同意，狀態為 1/3
3. **關閉第一個 Worker**
4. 啟動第二個 Worker
5. 查詢狀態，仍為 1/3，第一人的簽核未丟失
6. 其餘兩人簽完，流程正常結束

Temporal UI 顯示該流程 `Workers 3`，證實跨越多個 Worker 執行。截圖見 `screenshots/temporal-poc/02-restart-test-summary.png`。

---

## 3. 其他驗證

| 測試 | 驗證內容 | 結果 |
|---|---|---|
| `test_timeout_when_nobody_responds` | 七天無人回應判定為 TIMEOUT，待辦被取消 | 通過 |
| `test_long_wait_does_not_block` | 等待三十天不佔用資源，時間跳過後仍可簽核 | 通過 |
| `test_duplicate_signal_is_idempotent` | 同一 task 重複送三次 Signal 只計算一次 | 通過 |
| `test_empty_resolution_fails_explicitly` | 簽核人為空時明確失敗，錯誤型別為 `RESOLVE_EMPTY` 且標記不可重試 | 通過 |
| `test_three_way_approval_all_success` | 全部同意判定為 CONTINUE，無人被取消 | 通過 |

`test_policy_matrix` 涵蓋 Approval Policy Engine 十五個組合，加四個獨立測試，共十九項，全部通過。矩陣涵蓋 ALL、ANY、N_OF_M 三種完成策略與 ALL_SUCCESS、ANY_REJECT、MAJORITY 三種結果策略的交叉組合，以及逾期與完成條件的優先順序。

---

## 4. 過程中發現的問題

全部是實作或測試寫法問題，非 Temporal 能力限制。記錄下來供正式實作參考。

### 4.1 dataclass 的 Enum 經序列化後被拆成字元陣列

**現象**：`Completion.ALL` 傳入 Workflow 後變成 `['A','L','L']`，比較永遠失敗。

**原因**：Temporal 以 JSON 傳遞 dataclass。`str` 為基底的 Enum 在某些反序列化路徑被當成字元序列展開。

**處置**：所有含 Enum 的 dataclass 加 `__post_init__` 正規化。

```python
def _to_enum(enum_cls, value):
    if isinstance(value, enum_cls):
        return value
    if isinstance(value, (list, tuple)):
        value = "".join(value)
    return enum_cls(str(value))
```

**正式版建議**：改用 Pydantic 並由 `schemas/*.json` 產生型別，此問題自然消失。這也是任務 1.4 之前要做型別產生的另一個理由。

### 4.2 Activity 回傳值需明確指定 result_type

**現象**：`execute_activity` 回傳的 `list[Participant]` 實際是 `list[dict]`，存取屬性時報 `'dict' object has no attribute 'display'`。

**處置**：呼叫時明確傳 `result_type=list[Participant]`。`client.start_workflow` 同樣需要 `result_type`，否則 `handle.result()` 回 dict。

**正式版建議**：所有 `execute_activity` 與 `start_workflow` 一律明確指定 `result_type`，列入 code review 檢查項。

### 4.3 ApplicationError 的位置與參數

**現象**：`workflow.ApplicationError` 不存在。

**正確用法**：

```python
from temporalio.exceptions import ApplicationError

raise ApplicationError(
    "簽核人解析結果為空，請確認部門主管或角色已指派人員",
    type="RESOLVE_EMPTY",     # 第一個位置參數是 message，type 需具名
    non_retryable=True,
)
```

測試斷言需查 `__cause__`，不是頂層例外：

```python
cause = ei.value.__cause__
assert cause.type == "RESOLVE_EMPTY"
assert cause.non_retryable
```

### 4.4 time-skipping 測試環境的時鐘干擾

**現象**：跨 Worker 重啟與失敗路徑測試在 time-skipping 環境下逾時。

**原因**：該環境會在 Workflow 等待時自動快轉虛擬時鐘，導致客戶端的真實時間逾時判斷失準。

**處置**：需要穩定時鐘的測試改連真實 Temporal 伺服器（docker compose）。逾期測試仍用 time-skipping，讓七天瞬間過完。

**正式版建議**：測試分兩組。純邏輯與逾期用 time-skipping，涉及重啟或失敗路徑用真實伺服器。CI 兩者都跑。

### 4.5 真實伺服器暴露了測試環境遮蔽的錯誤

`ApplicationError` 的 API 誤用在測試環境不報錯，換到真實伺服器才浮現。

**啟示**：不能只靠內建測試環境。整合測試必須對真實 Temporal 執行，這點寫進[測試策略](../../系統規劃/09_測試策略.md)的 L4 層級。

---

## 5. 架構約束驗證

PoC 刻意遵守以下約束並證明可行。正式版必須維持。

| 約束 | 為何必要 | PoC 是否遵守 |
|---|---|---|
| Workflow 內無 IO、不讀時鐘、不用亂數 | Temporal 靠重放事件還原狀態，非決定性會導致重放不一致 | 是 |
| 簽核人解析在 Activity | 需查資料庫 | 是 |
| Policy Engine 是純函數 | 可在 Workflow 內直接呼叫，不需往返 Activity | 是，且十九個測試證明無副作用 |
| Human Task 建立與取消都是 Activity | 寫入資料庫 | 是 |
| 時間由 `workflow.now()` 取得 | 重放時回傳原始時間 | 是 |

---

## 6. 尚未驗證

以下留待正式實作階段，本 PoC 不涵蓋。

| 項目 | 何時驗證 | 風險 |
|---|---|---|
| Rust 經 gRPC 控制 Temporal | 任務 1.3 | 中。決議 D-02 的另一個風險點，Temporal 無正式 Rust SDK |
| Workflow 版本演進（patching） | 任務 4.1 | 低。Temporal 原生支援 |
| 多租戶 workflow_id 前綴 | 任務 1.3 | 低 |
| Activity 重試與外部系統 idempotency | 任務 3.5 | 中。Temporal 負責重試，冪等性仍是自己的責任 |
| PostgreSQL RLS 租戶隔離 | 任務 1.1 | 高。這是安全機制，需獨立驗證 |

---

## 7. 結論

**決議 D-02 成立。不自研 Workflow Runtime，改用 Temporal。**

三項核心驗證全部通過，且過程證明了幾件事：

1. 並簽、動態簽核人、逾期、取消這些企業流程的核心需求，Temporal 都能直接支撐，不需自己實作狀態機。
2. 狀態持久性是免費得到的。自研要做到同等水準，需自行處理事件持久化、重放、冪等性，這正是「Runtime 黑洞」的來源。
3. 遇到的五個問題全是 SDK 使用細節，一天內可解決。若自研，同等功能的除錯週期會以月計。

**建議進入 Phase 1。** 但需注意剩餘風險中，Rust 經 gRPC 控制 Temporal（任務 1.3）與 PostgreSQL RLS（任務 1.1）兩項仍未驗證，前者是 D-02 的另一半，後者是安全機制。兩者都應在 Phase 1 早期完成。

---

## 8. 附件

| 檔案 | 說明 |
|---|---|
| `screenshots/temporal-poc/01-workflow-list.png` | Temporal UI 執行清單。Failed 為空解析測試的預期失敗，Timed Out 為逾期測試 |
| `screenshots/temporal-poc/02-restart-test-summary.png` | 重啟測試的流程詳情。`Workers 3` 證實跨多個 Worker 執行 |
| `poc/temporal/` | 完整程式碼與測試 |
