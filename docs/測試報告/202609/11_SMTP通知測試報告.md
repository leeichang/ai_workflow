# SMTP 通知測試報告

- 日期：2026-09-17
- 範圍：郵件寄送層、通知 API 的實際送出、端到端實機驗證
- 環境：本機 PostgreSQL 17、Temporal 1.25.2、Rust API `:3001`、Gmail SMTP
- 相關決議：[D-07d](../../需求規劃/202609/02_決議紀錄.md)（沙箱通知的處理）

---

## 1. 起因：測試報告 10 留下的最大缺口

報告 10 的「已知限制」寫著：

> **提醒的實際發送是最大的缺口**——目前流程會在正確的時間點呼叫
> `send_notification`，但那個 Activity 只寫稽核記錄，沒有真的寄出。

逾時前提醒做完了，時間點算得對，收件人解析得出來，
稽核記錄也寫了——**但沒有人會收到信**。

本次補上這一段。

---

## 2. 測試結果總覽

| 元件 | 數量 | 本次新增 |
|------|------|----------|
| Rust | 161 | **+7** |
| Python | 155 | +1 |
| 前端單元／元件 | 330 | — |
| pdfme | 57 | — |
| E2E（Playwright） | 51 | — |
| **合計** | **754** | **+8** |

```
cargo test: 161 passed (14 suites, 3.11s)
```

---

## 3. 郵件寄送層

### 3.1 抽成 trait 而非直接呼叫 lettre

`server/crates/http-public/src/mailer.rs`

```rust
#[async_trait::async_trait]
pub trait Transport: Send + Sync {
    async fn send(&self, mail: &Mail) -> SendOutcome;
}
```

理由寫在檔頭註解裡：**測試不該真的寄信，
而「寄信失敗時流程怎麼走」正是最需要測的部分。**

直接呼叫 lettre 的話，這幾件事測不到：位址無效、連線被拒、
部分收件人失敗。而那些正是生產環境最常發生的。

### 3.2 三態結果，不是布林

```rust
pub enum SendOutcome {
    Sent { accepted: usize },
    Disabled,
    Failed { reason: String },
}
```

**分開「停用」與「失敗」**：前者是預期的設定狀態（開發環境沒設 SMTP），
後者要寫進稽核讓人查。

用布林的話兩者都是 `false`，稽核記錄會塞滿開發環境的假失敗，
真正的失敗反而被淹沒。

### 3.3 逐封寄送

```rust
// 逐封寄送而非一次多個收件人：一個位址無效不該讓其他人也收不到。
// 簽核提醒尤其如此——三個簽核人其中一個 email 打錯，
// 另外兩個仍應收到。
for addr in &mail.to {
```

部分成功仍回 `Sent`，但把失敗的記進 warning。

### 3.4 測試（7 條）

| 測試 | 驗證 |
|------|------|
| `disabled_mailer_does_not_send` | 未設定時回 `Disabled`，不是 `Failed` |
| `enabled_mailer_sends` | 正常路徑 |
| **`empty_recipients_is_failure_not_send`** | **沒有收件人回 `Failed`，且不呼叫傳輸層** |
| `failure_is_reported` | 失敗原因傳得出來 |
| `config_requires_host` | 沒有 `SMTP_HOST` 是停用不是錯誤 |
| `config_without_credentials_is_disabled` | 半套設定停用而非讓 API 啟動失敗 |
| `from_defaults_to_username` | `SMTP_FROM` 留空時用 username，預設埠 587 |

第三條的理由寫在測試裡：

```rust
// 沒有收件人卻回報成功，會讓稽核記錄看起來正常而實際沒人收到
```

這正是報告 10 那個缺口的一般化——**稽核顯示成功但實際沒送達**
是這類功能最危險的失敗模式。

---

## 4. 通知 API

`server/crates/http-public/src/internal.rs` 的 `send_notification`
從「只記稽核」改成真的送出。

### 4.1 收件人解析

```rust
/// 把 user id 轉成 email，非 UUID 的值視為 email 直接使用
///
/// 外部聯絡人的「id」就是 email 本身（resolver 型別 external_contacts），
/// 他們不在 app_user 裡，查不到是正常的。
```

UUID 走 `persistence::participant::emails_of` 查表，
含 `@` 的字串直接當 email 用。

### 4.2 LINE 明確標示為未支援

```rust
} else {
    // LINE 尚未實作。明確標示而非假裝送出。
    crate::mailer::SendOutcome::Disabled
};
```

不是靜默跳過。稽核會記 `status: disabled`，
查得出來「這則通知本來要走 LINE 但沒送」。

### 4.3 稽核記錄的內容

```jsonc
{
  "node_id": "...",
  "kind": "reminder",
  "channel": ["email"],
  "to": ["<uuid>"],           // 原始的參與者 id
  "recipients": ["a@b.com"],  // 解析後的實際收件人
  "status": "sent",
  "detail": "1 封"
}
```

`to` 與 `recipients` 都記，因為解析本身可能出錯——
只記其中一個的話，查不出是「解析錯了」還是「寄錯了」。

---

## 5. 實機驗證：兩封信與一個 bug

這一段是本次報告的重點。**單元測試全綠，但功能是壞的。**

### 5.1 第一封信：標題錯了

設定好 SMTP，觸發一次逾時前提醒，信確實寄到
`leeichang@gmail.com`。但標題是：

```
【流程通知】
```

預期應該是：

```
【待辦提醒】1 天 後到期
```

### 5.2 原因：Activity 沒有轉發 `kind`

`subject_of` 依 `body.kind` 決定標題：

```rust
fn subject_of(body: &NotificationBody) -> String {
    match body.kind.as_deref() {
        Some("reminder") => { /* 待辦提醒 */ }
        _ => "【流程通知】".into(),
    }
}
```

Workflow 有傳 `kind`，Rust 端也會處理，
但中間的 Python Activity 沒有把它放進請求 body：

```python
# python-ai/activities/human_task.py，修正後
"template_key": req.get("template_key"),
"business_object": req.get("business_object") or {},
# 提醒與一般通知的信件內容不同，kind 決定用哪個標題。
# 漏傳的話提醒信會顯示成通用的「流程通知」，
# 收信的人不知道是催簽核還是別的事。
"kind": req.get("kind"),
"remaining": req.get("remaining"),
```

### 5.3 為什麼測試沒抓到

Python 端的測試驗的是「有沒有呼叫 `send_notification`」，
沒有驗請求 body 的欄位內容。Rust 端的測試直接構造
`NotificationBody`，`kind` 是測試自己填的。

**兩邊各自測都通過，接起來是壞的。** 這就是報告 10
「已知限制」列的缺口——沒有真的寄一封信，就不會發現。

### 5.4 第二封信：正確

修正後重新觸發：

```
標題：【待辦提醒】1 天 後到期

流程節點：manager_approval
單據編號：<instance_id>
剩餘時間：1 天

請至系統處理：http://localhost:3040/tasks
```

稽核記錄：

```jsonc
{
  "kind": "reminder",
  "status": "sent",
  "recipients": ["leeichang@gmail.com"]
}
```

### 5.5 `humanize`：ISO duration 轉人話

```rust
/// 信件裡寫「P1D 後到期」使用者看不懂。只處理常見的幾種，
/// 其餘原樣輸出——寧可顯示 ISO 字串，也不要算錯。
```

不寫通用的 parser 是刻意的。通用 parser 要處理
`P1Y2M3DT4H5M6S` 這種組合，寫錯的機率遠高於它帶來的價值——
而「算錯的剩餘時間」比「看不懂的 ISO 字串」糟糕得多。

---

## 6. 過程中發現的問題

### 6.1 `.env` 的密碼含空格未加引號

`start_service.sh` 用 `set -a && . .env` 載入環境變數。
Gmail 應用程式密碼是四組四字元、以空格分隔：

```bash
SMTP_PASSWORD=abcd efgh ijkl mnop   # 壞
```

shell 把第二個 token 當成指令執行：

```
command not found: efgh
```

而 `SMTP_PASSWORD` 只拿到第一段。症狀是 API 日誌顯示
「已設定 SMTP_HOST 但缺少帳號或密碼，郵件功能停用」，
但 `.env` 裡明明有值。

修正：加引號。

```bash
SMTP_PASSWORD="abcd efgh ijkl mnop"
```

### 6.2 `resolver.user_id` 需要 UUID 不是 email

測試流程時把 resolver 設成 email，流程 FAILED：

```
NoParticipants
```

`resolve_participants` 查的是 `app_user.id`。這與 §4.1 的
「含 `@` 直接當 email」是不同層——後者是**通知**的收件人解析，
前者是**參與者**解析，走的是不同路徑。

### 6.3 半套 SMTP 設定不讓 API 掛掉

```rust
if username.is_empty() || password.is_empty() {
    tracing::warn!("已設定 SMTP_HOST 但缺少帳號或密碼，郵件功能停用");
    return None;
}
```

設計選擇：啟動時讓整個 API 掛掉更糟。
中小企業的部署現場，郵件設定錯誤不該讓整套系統起不來——
流程還是要能跑，只是不寄信。

---

## 7. 安全事項

設定過程中 Gmail 應用程式密碼曾出現在對話紀錄裡。

處理：

1. 立即告知使用者撤銷該密碼
2. 使用者確認「舊的已經撤銷，mail 也修改」
3. 新密碼只寫進 `.env`
4. 確認 `.env` 在 `.gitignore` 且未被 git 追蹤

**給後續開發者**：不要把密碼貼給 AI agent。
對話紀錄會留存，撤銷是唯一的補救。直接寫進 `.env`。

---

## 8. 已知限制

| 項目 | 現況 |
|------|------|
| LINE Messaging API | 未實作。`channel: ["line"]` 會回 `disabled` 並記稽核 |
| 通知模板 | `template_key` 已傳（如 `task_reminder`），但模板系統未建。目前標題與內文是寫死的分支 |
| HTML 信件 | 只送純文字（`ContentType::TEXT_PLAIN`）|
| 退信處理 | 無。SMTP 接受後就算成功，之後的退信不會回到系統 |
| 寄送佇列 | 無。同步寄送，SMTP 慢會拖慢 API 回應 |
| 沙箱的通知 | 見 [D-07d](../../需求規劃/202609/02_決議紀錄.md)：標題要加「【沙箱模擬】」前綴、改寄測試者。**尚未實作** |

「通知模板」是下一個該做的——目前 `subject_of` 與 `text_of`
是 `match` 分支，加一種通知就要改程式碼。

---

## 9. 結論

- 郵件寄送層抽成 trait，7 條測試涵蓋停用、失敗、空收件人、設定缺漏
- 通知 API 從「只記稽核」改成真的送出，稽核同時記原始 `to` 與解析後 `recipients`
- LINE 明確回 `disabled`，不假裝送出
- **實機寄出兩封信，第一封抓到 Activity 漏傳 `kind` 的 bug**
- **754 個測試全數通過**（本次 +8）

最值得記下來的是 §5.3：Python 與 Rust 兩邊的測試各自都通過，
接起來卻是壞的。**單元測試驗「有沒有呼叫」，沒驗「傳了什麼」。**

這與報告 10 §3.1 記錄的那個註解（「可由對帳程序修正」，
而那個程序從未實作）是同一類問題——**測試與註解都可能在說謊，
只有實機驗證不會。**
