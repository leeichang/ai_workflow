# Temporal 整合測試報告

- 日期：2026-09-15
- 範圍：實作順序第 4 項「Temporal 整合」，對應實作計畫任務 1.3
- 環境：Temporal 1.25.2（Docker）、本機 PostgreSQL 17、API `:3001`
- 前置：PoC 已於 2026-09-14 驗證 Temporal 本身的能力（30 測試通過）

---

## 1. 測試結果總覽

| 層級 | 內容 | 數量 | 結果 |
|------|------|------|------|
| L1 單元 | Payload 編碼 `payload.rs` | 9 | 全數通過 |
| L1 單元 | Workflow ID 租戶前綴 `workflow_id.rs` | 6 | 全數通過 |
| L4 整合 | Rust ↔ Python 互通 `tests/interop.rs` | 9 | 全數通過 |
| L4 整合 | 既有 API 測試（迴歸） | 39 | 全數通過 |
| L5 手動 | 經真實 API 端到端 | 6 個情境 | 全數通過 |

後端合計 **125 個測試，通過 125，失敗 0**（本次新增 24）。
PoC 的 30 個測試重跑仍全綠。

```bash
cd poc/temporal && docker compose up -d          # Temporal Server
python-ai/.venv/bin/python python-ai/worker.py   # Python Worker
cd server && cargo test
```

---

## 2. 本階段要解決的風險

決議 D-02 選用 Temporal，但 Temporal **沒有官方 Rust SDK**。
本平台的 Rust API 必須直接以 gRPC 控制流程，這帶來一個
實作計畫早已標記的風險（見計畫書風險表）：

> Temporal gRPC 由 Rust 直呼，Payload 編碼與 Python 不相容 → Phase 1 卡關

不相容的症狀特別難查：Workflow 會收到 `None` 或型別錯誤的參數，
而不是明確的解碼失敗。單元測試各自測自己那一端都會通過，
只有真的跑一次跨語言呼叫才看得出來。

因此本階段的核心產出不是功能，而是**這個風險已被關閉的證據**。

---

## 3. Payload 編碼相容性

### 3.1 Python 端的規則

查 `reference/temporal-sdk-python/tests/test_converter.py` 確認：

| 值 | encoding | data |
|----|----------|------|
| `None` | `binary/null` | 空 |
| `{"a":1}` | `json/plain` | JSON 位元組 |
| `"str"` | `json/plain` | `"str"`（含引號）|
| `1234` | `json/plain` | `1234` |
| `True` | `json/plain` | `true` |

`None` 是特例：不是 `json/plain` 的字串 `"null"`，而是獨立的
`binary/null` 編碼且 data 為空。送錯的話 Python 會解出字串
而非 `None`，且不會有任何錯誤。

### 3.2 驗證方式

寫了一個 Python 探針 Workflow（`python-ai/workflows/interop_probe.py`），
由 Rust 啟動並送入刻意涵蓋各種型別的 payload：

```json
{
  "amount": 1234567.89,
  "approved": true, "rejected": false,
  "note": null,
  "customer": "台灣精密工業股份有限公司",
  "items": [{ "sku": "A-001", "qty": 3, "price": 100.5 }],
  "nested": { "level2": { "level3": ["深層", 42, null, true] } }
}
```

Rust 再以 Query 取回 Python 收到的內容，逐型別比對後確認整份相等。
浮點數、布林、`null`、非 ASCII 字串、巢狀混合型別陣列全部無損。

三個方向都測：
- **啟動參數** Rust → Python
- **Signal 參數** Rust → Python（五種型別各一次）
- **Query 參數與結果** 雙向往返

**結論：編碼相容，D-02 的這個風險已關閉。**

---

## 4. 租戶隔離

`workflow_id` 採三段式：`{tenant_id}:{business_object}:{instance_id}`。

這不只是命名慣例，而是安全邊界。Signal 與 Cancel 都以
`workflow_id` 定位流程，若呼叫端能自由指定完整 id，
A 租戶就能對 B 租戶的流程送 Signal。

因此：

- 對外 API 只收 `business_object` 與 `instance_id`
- `tenant_id` 從 JWT 取得後由後端組合，呼叫端無從干預
- 兩段識別碼都禁止包含 `:`，否則可構造出跨租戶的 id
- 取消前額外以 `belongs_to()` 再驗一次歸屬（縱深防禦）

六個單元測試守住這組規則，含「拒絕含分隔符的識別碼」
與「格式異常的 id 不視為任何租戶所有」。

資料庫端，新增的 `workflow_instance` 與 `human_task` 兩張表
都套用與既有表相同的 RLS policy 與 `FORCE ROW LEVEL SECURITY`。
`verify_rls.sh` 9 項全數通過，其中最後兩項會掃描所有帶
`tenant_id` 的表，新表漏設會被抓出來。

---

## 5. 實作中發現並修正的缺陷

### 5.1 GET 請求卡住 20 秒

**現象**：`GET /instances/{id}` 耗時 20 秒才回應。

**原因**：取得流程狀態用了 `get_result()`，它以 long poll 等待
流程結束。流程還在跑時，Temporal 會 hold 住連線直到伺服器端
逾時（實測 20 秒）。這對「等流程跑完」是正確行為，
對一個該立刻回應的 GET 則是災難。

第一次修正嘗試把 `wait_new_event` 設為 `false`，**但沒有用**——
搭配 `CloseEvent` 過濾時，Temporal 仍會 long poll 到逾時。
測試斷言「peek 不該等待」把這個假修正抓了出來：

```
peek 不該等待，實際耗時 20.0086795s
```

**最終修正**：改用 `DescribeWorkflowExecution`，這是單純的狀態
查詢，一定立即回應。代價是它不帶回傳值，需要輸出時仍用
`get_result()`，但那時流程已結束，long poll 會立即返回。

修正後同一個請求耗時 **32 毫秒**，整個 interop 測試套件
從 20 秒降到 0.37 秒。

### 5.2 探針 Workflow 沒有處理取消

**現象**：取消測試失敗，流程看似沒有結束。

**追查**：Temporal UI 顯示流程狀態其實是 `Completed`，
且 `WorkflowExecutionCancelRequested` 事件確實存在。
問題出在測試本身——我用「Query 失敗」判斷流程已結束，
但 **Temporal 對已結束的流程仍會回應 Query**（重放最後狀態）。

**兩處修正**：

1. Python 端明確 catch `asyncio.CancelledError`。不 catch 的話
   流程會以「失敗」結束而非「已取消」，且不執行任何清理。
   正式的 Interpreter 會在這裡取消待辦、寫稽核紀錄。
2. 測試改用結束事件判斷，並補上一個對照組
   「未取消的流程正常結束時不帶取消標記」——
   少了它，即使在「從未收到取消」的情況下測試也可能通過。

取消後的最終狀態是 `COMPLETED` 而非 `CANCELLED`，這是正確的：
取消是「請求」而非強制終止，Workflow 攔下後做完清理再正常回傳。
強制終止要用 `TerminateWorkflowExecution`，那會跳過清理。

### 5.3 proto 模組宣告手寫易漏

protoc 只編譯 `WorkflowService`，但會連帶產生它依賴的全部
package——實測 **29 個**，遠多於直覺。手寫巢狀 `include_proto!`
清單漏掉任何一個都是編譯錯誤，而錯誤訊息指向產生的程式碼，
不容易看出是清單漏了。

改為在 `build.rs` 掃描產物自動生成模組樹。

---

## 6. 兩個系統的一致性

啟動流程牽涉本地資料庫與 Temporal，兩者沒有分散式交易。
順序刻意設計為**先寫資料庫，再啟動 Temporal**：

| 順序 | 中途失敗的後果 |
|------|----------------|
| 先寫 DB 再啟動 | 留下 RUNNING 但實際沒跑的孤兒紀錄，可由對帳程序偵測清除 |
| 先啟動再寫 DB | 流程已在跑但系統不知道它存在，人工待辦會憑空出現，無線索可追 |

寧可留孤兒，不可留幽靈。啟動失敗時另外把該筆標記為 `FAILED`，
讓孤兒一眼可辨，不必等對帳程序。

`workflow_instance` 對 `(tenant_id, temporal_workflow_id)` 設唯一約束，
重複點擊送出會得到 409 而非兩張簽核單。Temporal 端另有一層
保護：`request_id` 由 `workflow_id` 推導而非隨機值，
重送同一請求不會啟動第二個流程。兩層各自有測試。

---

## 7. 端到端驗證

經真實 API、真實 PostgreSQL、真實 Temporal：

| # | 情境 | 結果 |
|---|------|------|
| 1 | `POST /instances` 啟動報價單流程 | 201，workflow_id 帶正確租戶前綴 |
| 2 | 相同單號重複啟動 | 409「單號 QT-2026-9001 已有進行中的流程」 |
| 3 | 未發布的流程 | 404 |
| 4 | `GET /instances` 清單 | 正確回傳，狀態 RUNNING |
| 5 | `POST /instances/{id}/cancel` | 取消請求送出，回應說明狀態稍後更新 |
| 6 | `GET /instances/{id}` | 32 毫秒回應，live_status 與本地狀態並列 |

另驗證降級行為：把 `TEMPORAL_TARGET` 指向不存在的位址後重啟，

- API 正常啟動，記 WARN 並印出嘗試連線的位址
- `GET /forms` 仍回 200（表單設計、權限矩陣不受影響）
- `POST /instances` 回 **503** 並說明「流程引擎未連線」

這是刻意的設計：為了 Temporal 讓整個系統起不來並不合理。
代價是設定打錯時不會立刻發現，因此連線失敗記為 WARN 而非 DEBUG。

---

## 8. 交付內容

| 檔案 | 內容 |
|------|------|
| `server/crates/temporal-client/` | gRPC client，含 proto（MIT，含 LICENSE） |
| `server/crates/temporal-client/src/payload.rs` | 編碼相容層 |
| `server/crates/temporal-client/src/workflow_id.rs` | 租戶前綴 |
| `server/crates/temporal-client/build.rs` | tonic 生成 + 模組樹自動產生 |
| `server/migrations/0005_workflow_instance.sql` | 實例與人工待辦表 |
| `server/crates/persistence/src/instance.rs` | 實例持久化 |
| `server/crates/http-public/src/instances.rs` | 實例 API |
| `python-ai/worker.py` | Temporal Worker |
| `python-ai/workflows/interop_probe.py` | 互通探針 Workflow |

proto 檔（920KB、67 個）刻意複製進 crate 而非指向 `reference/`：
後者不納入版控，少了它 clone 下來的專案無法建置。

---

## 9. 已知限制與下一步

| 項目 | 現況 |
|------|------|
| `DslInterpreter` Workflow | **尚未實作**。API 已會啟動它，但 Worker 尚未註冊，流程啟動後不會前進 |
| 人工待辦 Activity | 表已建好，Activity 未實作 |
| 收件匣 API | 未實作 |
| 決策 Signal | client 已支援 signal，端點未實作 |
| 流程結束回寫 | `finish()` 已備妥，但無人呼叫 |
| Workflow 版本演進（patching） | 未驗證 |
| Retention 與最長流程週期 | 未實測 |

**本階段完成的是控制面與相容性驗證，不是可執行的業務流程。**
`DslInterpreter`（任務 1.4）、Policy Engine（1.5）、
人工待辦（1.7）與收件匣（1.8）是下一階段的內容，
屆時才會出現「建立 → 主管簽 → 完成」的完整往返。

---

## 10. 結論

Temporal 的 Rust gRPC 控制面完成並通過驗證。
決議 D-02 中「Rust 直呼 gRPC，Payload 與 Python 不相容」
的風險已關閉——以真實 Temporal Server 跑過三個方向的
跨語言往返，各種型別無損。

實作過程修正 3 個缺陷，其中 GET 卡住 20 秒的問題
第一次修正無效，是靠測試斷言抓出假修正。

後端 125 個測試全數通過，PoC 30 個測試重跑仍全綠。

下一步為實作順序第 5 項「表單流程執行整合」，
即實作 `DslInterpreter` 與人工待辦，讓流程真正跑得完。
