# Schema 契約

三方共用的唯一真相。Rust serde、Python Pydantic、TypeScript 型別皆由此產生。

**規則：Schema 先定版，設計器只是它的編輯器。** 設計器不得反向決定 Schema 結構。詳見 [07_實作順序調整.md](../docs/系統規劃/07_實作順序調整.md) 保險一。

---

## 檔案

| 檔案 | 內容 |
|---|---|
| `workflow-dsl.schema.json` | 流程定義。節點、邊、簽核人解析、Join 策略、逾期、退回 |
| `form.schema.json` | 表單定義。欄位含 `ui` / `data` / `workflow` 三個 key |
| `fixtures/quotation_approval_v1.json` | 報價單流程，含條件分支、外部簽核、退回 |
| `fixtures/purchase_approval_v1.json` | 採購流程，含 composite resolver 金額門檻加簽 |
| `fixtures/quotation_form_v1.json` | 報價單表單，含明細表格、計算欄位、外部隱藏欄位 |

---

## 兩層驗證

JSON Schema 只能驗單一節點的結構，驗不了跨節點的語意。因此分兩層。

### 第一層：JSON Schema（結構）

驗欄位型別、必填、列舉值、格式。由 `ajv` 執行。

```bash
cd schemas/tools && npm run validate    # 正向：fixture 應通過
cd schemas/tools && npm run negative    # 負向：16 個錯誤案例應被擋
```

### 第二層：圖結構（語意）

驗跨節點的關係。參考實作在 `tools/graph-check.mjs`。

```bash
cd schemas/tools && npm run graph           # 正向
cd schemas/tools && npm run graph-negative  # 負向：13 個案例
```

正式版在 Rust（`server/crates/domain/src/workflow/validator.rs`，任務 4.1）。
**兩者必須對同一組 fixture 產生相同的錯誤碼集合。**

| 錯誤碼 | 規則 |
|---|---|
| WF-E001 | 恰一個 trigger，至少一個 end |
| WF-E002 | 所有節點可從 trigger 到達，且可到達某個 end |
| WF-E003 | condition 須有 when=true 與 when=false 兩條出邊 |
| WF-E004 | goto 目標須在該節點的上游路徑 |
| WF-E005 | resolver 的角色須存在於租戶 |
| WF-E006 | 表達式須可解析 |
| WF-E007 | action 須存在於 Registry；寫入外部系統者須有 `idempotency_key` |
| WF-E008 | `participant=external` 的 resolver 必須是 `external_contacts` |

---

## 一個容易踩到的設計細節

流程有兩種控制流：

1. `edges` 陣列 — 正常前進路徑
2. `on_reject` / `on_failure` 的 `goto` — 退回路徑

**可達性分析（E002）必須包含兩者**，否則只靠 goto 進入的節點會被誤判為孤立。報價單的 `revise` 節點就是這種情況，它的入邊來自三個不同節點的 `on_reject`。

**上游判斷（E004）只能看 `edges`**，不能含 goto。否則 goto 會讓目標自動成為上游，規則形同虛設。

`tools/graph-check.mjs` 的 `buildAdjacency` 同時建立兩組鄰接表處理這件事。實作 Rust 版時必須沿用相同邏輯。

---

## 新增 fixture 時

1. 放進 `fixtures/`，檔名含 `approval` 者會被圖驗證自動掃到。
2. 跑 `npm test` 確認兩層驗證皆過。
3. 若新 fixture 用到新的 action 或角色，需同步更新 `graph-check.mjs` 頂端的 `KNOWN_ROLES` 與 `ACTION_REGISTRY`（正式版查 DB，此處為模擬）。

---

## 型別產生（尚未實作）

規劃中，任務 1.4 之前完成：

| 目標 | 工具 |
|---|---|
| Rust | `typify` 或手寫 serde struct + fixture 往返測試 |
| Python | `datamodel-code-generator` 產 Pydantic |
| TypeScript | `json-schema-to-typescript` |

CI 需驗證三方對同一組 fixture 序列化結果一致。
