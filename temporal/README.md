# Temporal 部署設定

此目錄放 Temporal Server 的**部署設定**，不放原始碼。原始碼在 `reference/temporal-server/`（唯讀參考）。

## 授權

Temporal Server 為 MIT，可自行部署於商業 SaaS 而無授權費。詳見 [決議紀錄 D-02](../docs/需求規劃/202609/02_決議紀錄.md)。

真正的成本不是授權，是運維：備份、升級、TLS、retention、archival、災難復原。

## 預計檔案

| 檔案 | 用途 | 階段 |
|---|---|---|
| `dynamicconfig/development.yaml` | 開發用動態設定 | Phase 1 |
| `dynamicconfig/production.yaml` | 正式環境設定 | Phase 5 |
| `namespace-init.sh` | 建立 Namespace 與設定 retention | Phase 1 |

## Namespace 策略

MVP 採**單一 Namespace**，租戶隔離靠 workflow_id 前綴：

```text
{tenant_id}:{business_object}:{instance_id}
例：a1b2c3:quotation:QT-2026-0001
```

Enterprise 客戶若要求更強隔離，再改為每租戶一個 Namespace。此時 `temporal-client` 的 Namespace 解析需改為查表，其餘不動。

## Retention

預設 30 天。超過後 Event History 被清除，但業務資料在 PostgreSQL，不受影響。

長流程（報價往返可能數週）需確認 retention 大於最長流程週期，否則進行中的流程會被清除。Phase 1 需實測。

## 與本專案的關係

Rust 端經 gRPC 控制（`server/crates/temporal-client/`），Python 端以 `temporalio` SDK 執行 Workflow 與 Activity（`python-ai/`）。

Temporal 只存流程執行狀態，**不是業務資料庫**。業務真相在 PostgreSQL。
