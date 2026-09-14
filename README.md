# Workflow Platform

給已有 ERP 的台灣製造業 SME 的 AI 營運流程層。不換 ERP，把 Excel / Email / LINE 上的簽核與對外文件往返，變成有 ERP 寫回、可稽核的正式流程。

第一場景：企業報價單（業務發起 → 內部審核 → 客戶 Portal 簽核 → Odoo 建立訂單）。
第二場景：採購簽核（人工填單 → 金額門檻動態並簽 → Odoo 建 PO）。

規劃文件見 [docs/系統規劃](docs/系統規劃/)。

---

## 目錄結構

| 目錄 | 用途 | 語言 / 技術 |
|---|---|---|
| `schemas/` | 三方共用契約。DSL、表單、節點屬性的 JSON Schema 與測試 fixture | JSON Schema |
| `openapi/` | API 契約。`public` 前端用、`portal` 客戶用、`internal` Python 呼叫 Rust 用 | OpenAPI 3 |
| `server/` | 控制層。身份、權限、業務資料、簽核人解析、Temporal gRPC 控制 | Rust + Axum |
| `python-ai/` | Workflow 解釋器、Approval Policy、所有 Activity、AI Task | Python + temporalio |
| `pdfme/` | 單據 PDF 渲染服務。無狀態，不連資料庫 | Node 20 + pdfme |
| `web/` | 內部應用。表單、收件匣、Designer、Monitor | Vue 3 + Vite |
| `portal/` | 客戶 Portal。magic link 進入，檢視與簽核 | Vue 3 + Vite |
| `templates/` | 系統預設模板。單據版面、通知文字、內建流程 | JSON / Jinja2 |
| `temporal/` | Temporal 部署設定。動態設定、Namespace 初始化 | YAML |
| `deploy/` | docker compose、備份還原腳本 | Shell / YAML |
| `poc/` | Phase 0 驗證用小專案，驗證後丟棄 | 混合 |
| `reference/` | 參考用開源原始碼，**不參與建置**，見下節 | 唯讀 |
| `docs/` | 需求與系統規劃文件 | Markdown |

`reference/` 已加入 `.gitignore`，不納入本專案版本控制。

---

## 架構速覽

```text
Vue 3（內部應用 + 客戶 Portal）
        ↓ HTTPS
Rust Axum（Auth / RBAC / 業務資料 / Resolver / Temporal gRPC Client）
        ↓ gRPC
Temporal Server（流程存活、Timer、Retry、Signal）
        ↓ Task Queue
Python Worker（DSL Interpreter / Policy Engine / Activities）
        ↓
PostgreSQL（RLS 租戶隔離）+ MinIO + Node 渲染服務 + Odoo
```

職責邊界見 [01_系統架構.md](docs/系統規劃/01_系統架構.md) 1.1 節。三條鐵律：

1. 任何元件不得直連 PostgreSQL，一律經 Rust 連線中介層，確保 `app.tenant_id` 生效。
2. AI 與外部工具只回傳結構化結果，流程決策由 Workflow Engine 負責。
3. 寫入外部系統的 Activity 必帶 `idempotency_key` 並經 `erp_write_log`。

---

## 開始開發

目前處於 Phase 0。尚無可執行程式碼。

Phase 0 三個任務見 [04_報價單流程實作計畫.md](docs/系統規劃/04_報價單流程實作計畫.md)：

| 任務 | 內容 | 產出 |
|---|---|---|
| 0.1 | pilot 訪談 | `docs/需求規劃/202609/03_pilot訪談紀錄.md` |
| 0.2 | Temporal PoC | `poc/temporal/` |
| 0.2b | pdfme Designer 與 plugin spike | `poc/pdfme-vue/` |
| 0.3 | DSL 第一版 Schema | `schemas/workflow-dsl.schema.json` |

---

## 參考原始碼

`reference/` 下的開源專案僅供查閱 API 與實作方式，不是相依套件。實際相依透過各語言的套件管理器安裝。

| 目錄 | 用途 | 授權 |
|---|---|---|
| `temporal-server` | Temporal Server 原始碼。查 Namespace、retention、archival 行為 | MIT |
| `temporal-api` | Temporal gRPC proto 定義。Rust tonic 生成的來源 | MIT |
| `temporal-sdk-python` | Python SDK。查 Workflow / Activity / Signal API | MIT |
| `temporal-samples-python` | 官方範例。並簽、Timer、Child Workflow 寫法 | MIT |
| `pdfme` | PDF 渲染與 Designer。自訂 table-plus plugin 的藍本 | MIT |
| `vue-flow` | 流程圖底層。自訂節點與佈局 | MIT |
| `form-create` | 表單 Renderer。自訂元件與 rule 格式 | MIT |
| `lowflow-design` | 簽核流程設計器。節點 UI 與屬性面板參考 | MIT |
| `axum` | Rust web 框架。middleware 與 extractor 用法 | MIT |
| `tonic` | Rust gRPC。Temporal client 實作參考 | MIT |
| `odoo-16` | **Odoo 16** 原始碼（sparse checkout：sale、purchase、account、base）。查 `sale.order`、`purchase.order` 欄位與 API | LGPL-3.0 |

整合目標為 **Odoo 16 社群版（Community Edition）**。Adapter 只用 XML-RPC / JSON-RPC 標準介面，不裝自訂 Odoo 模組。欄位差異集中於 `python-ai/adapters/odoo/v16.py`。

社群版沒有 Studio 與 Approvals（皆為企業版功能），這正是本平台補上的能力。規格不得依賴任何企業版模組。詳見 [決議 D-04b](docs/需求規劃/202609/02_決議紀錄.md)。

Odoo 16 的 model 檔案位置（已查證 commit `2175857`）：

| Model | 檔案 | 關鍵欄位 |
|---|---|---|
| `sale.order` | `addons/sale/models/sale_order.py` | `client_order_ref:87`、`validity_date:130` |
| `sale.order.line` | `addons/sale/models/sale_order_line.py` | — |
| `purchase.order` | `addons/purchase/models/purchase.py` | `partner_ref:88`、`date_planned:120` |

注意 `purchase.order` 定義在 `purchase.py`，不是 `purchase_order.py`。

**授權注意**：Odoo 為 LGPL-3.0，僅用於查閱欄位定義與 API 行為，不複製其程式碼。其餘皆為 MIT。複製 pdfme 的 table schema 作為 plugin 藍本時，需在檔案標頭保留其 MIT 聲明與來源 commit。

更新參考原始碼：

```bash
cd reference && for d in */; do git -C "$d" pull --depth 1 2>/dev/null; done
```
