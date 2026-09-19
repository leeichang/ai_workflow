# 資料庫 Migration

## 套用

```bash
docker compose -f deploy/docker-compose.yml up -d postgres
export DATABASE_URL="postgres://app:app_dev_only@localhost:5433/workflow"
for f in server/migrations/*.sql; do psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -f "$f"; done
```

編號順序即執行順序。`0000_roles.sql` 必須最先執行。

## 兩台以上平行開發時的編號

兩台機器各自新增 migration 時很容易撞號。2026-09-19 就同時出現了
三組：`0006`、`0007`、`0008` 各有兩份。

那一次剛好沒出事——同號的兩份動的是不同的表，互不依賴，
glob 排序下的執行順序恰好可行。但那是運氣，不是設計：

- 同號檔案的相對順序取決於檔名後半的字典序，沒有人會去檢查它
- 只要有一次真的交叉依賴（A 建表、B 改那張表），
  新環境從零跑就會失敗，而既有環境完全看不出問題

規則：

1. 新增 migration 前先 `git pull`，用**當前最大編號 + 1**
2. 真的撞號時，**後合併的那一方改自己的編號**往後移，
   不要改已經合併進來的那一份
3. 改編號要同時 grep 程式碼與文件裡的檔名引用

改編號不影響已套用的環境（`psql -f` 不記錄檔名），
但新環境從零跑的順序會因此確定。

## 兩個連線角色

| 角色 | 用途 | superuser | 受 RLS 約束 |
|---|---|---|---|
| `app` | 執行 migration、維運 | 是 | **否** |
| `workflow_app` | 應用程式連線 | 否 | 是 |

**應用程式必須用 `workflow_app` 連線。**

### 為何需要分開

PostgreSQL 的 superuser 會完全繞過 Row Level Security，連 `FORCE ROW LEVEL SECURITY` 都擋不住。

這在開發期間實測發現：用容器預設的 `app` 角色連線時，租戶 A 查得到租戶 B 的資料，未設定 `app.tenant_id` 也能看到全部。policy 寫得正確，但因為是 superuser 所以完全無效。

這類問題不會有任何錯誤訊息，只會安靜地讓租戶隔離失效。

## 租戶隔離機制

所有帶 `tenant_id` 的表都啟用 RLS 並套用相同 policy：

```sql
using (tenant_id = current_setting('app.tenant_id', true)::uuid)
with check (tenant_id = current_setting('app.tenant_id', true)::uuid)
```

`current_setting` 的第二個參數 `true` 表示未設定時回 `null` 而非報錯。`null` 與任何值比較皆為 false，查詢自然回空。這是刻意的 fail-closed 設計：忘記設定 `tenant_id` 時看不到任何資料，而不是看到全部。

應用程式每個請求需在交易內設定：

```sql
BEGIN;
SET LOCAL app.tenant_id = '<從 JWT 取得>';
-- 業務查詢
COMMIT;
```

`SET LOCAL` 只在交易內有效，交易結束自動清除，不會洩漏到連線池的下一個請求。這點很重要，用 `SET` 而非 `SET LOCAL` 會造成跨請求污染。

## 驗證

```bash
./server/migrations/verify_rls.sh
```

任何修改 RLS、新增帶 `tenant_id` 的表、或調整連線角色之後，都必須重跑此腳本。

## 表清單

| Migration | 內容 |
|---|---|
| `0000_roles.sql` | 資料庫角色與權限 |
| `0001_tenant_identity.sql` | 租戶、部門、使用者、角色 |
| `0002_form_definition.sql` | 表單定義與版本 |
| `0003_audit.sql` | 稽核事件 |

## 設計原則

**發布後 immutable。** 已發布的表單版本不可修改內容，由 trigger `form_version_immutable` 阻擋。要改就升版。

**稽核 append-only。** `audit_event` 不可 UPDATE 或 DELETE，由 trigger 與權限雙重保證。

**草稿與版本同表。** `form_definition_version.version` 為 `null` 代表草稿，部分唯一索引保證每個表單最多一份草稿。分兩張表會讓「取當前可編輯內容」需要 union，且容易漏同步。
