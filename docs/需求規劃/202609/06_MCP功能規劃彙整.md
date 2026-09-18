# Workflow 系統支援 MCP：多 AI 意見彙整

- 日期：2026-09-18
- 提問（6 個 AI 同一題）：**「如果我的工作流系統要支援 MCP 需要規劃那些功能？」**
- 原則：**不刪減任何一方的觀點**；分歧處並列，再給判斷
- 與既有文件的關係：
  - [01_多AI意見彙整與最佳建議.md](01_多AI意見彙整與最佳建議.md)：產品定位與 Runtime 選型（結論：Temporal）
  - [04_AI時代表單與流程功能彙整.md](04_AI時代表單與流程功能彙整.md)：表單功能清單與 AI 初審設計
  - **本文**：把 MCP（Model Context Protocol）當成系統的對外／對內能力介面，要規劃哪些功能
  - 架構對照基準：[系統規劃/01_系統架構.md](../../系統規劃/01_系統架構.md)

---

## 0. 來源與取得狀態

| 編號 | 來源 | 主要貢獻 | 狀態 |
|---|---|---|---|
| K | Kimi | 角色定位先決、Client 端連線管理、能力探索與 schema 版本、跨切面治理（命名衝突、灰度、注入防護） | 取得 |
| D | DeepSeek（帶網路搜尋，引用 99 個網頁） | 協議層完整、MCP **Tasks** 擴展、OAuth 2.1 雙重角色、稽核雜湊鏈、Skills / MCP Apps 擴展 | 取得（引用的是 **2025-11-25** 規格） |
| L | Claude | Server 端務實清單、**資料存取層不可直接暴露 DB**、分頁與 token 用量、唯讀 vs 寫入 tool 分離與 dry-run | 取得 |
| G | Gemini Pro | 控制平面（Control Plane）定位、三原語完整 API 對照、Resources **訂閱觸發工作流**、Agentic 執行迴圈 | 取得 |
| C | ChatGPT | 最完整（18 節）：MCP Gateway、Tool Registry 資料模型、Permission / Risk Engine、MCP Task、UI 設計、五階段 MVP 順序 | 取得 |
| X | Grok | **唯一對齊最新規格 `2026-07-28`（MCP 2.0，無狀態化）**；MRTR、Tool annotations、catalog 爆炸、CIMD / EMA、Schema 演進、刻意不做清單、五個最易漏的坑 | 取得（使用者手動貼上） |

註 1：C 的對話接在先前「SME Workflow Engine / AI 表單產生器 / n8n」討論串之後，因此最貼近本專案脈絡，具體到資料表與 UI。
註 2：**X 的規格版本比其他五家新一代**。凡 X 與其他來源衝突處，本文一律以 X 為準並標注，理由見 §1。

---

## 1. 規格版本：`2026-07-28`（MCP 2.0）與雙版本窗口（X 獨有，最關鍵）

其他五家的回答建立在 `2025-11-25` 或更早的規格上（D 的搜尋結果明確引用 `specification/2025-11-25`）。X 指出核心已改成**無狀態 request/response**：

| 項目 | 舊（`2025-11-25` 及更早） | 新（`2026-07-28`） |
|---|---|---|
| 連線模型 | `initialize` handshake + `Mcp-Session-Id` 有狀態 session | **無 handshake、無 session**，每筆 request 自帶 `_meta`（`protocolVersion`、`clientInfo`、`clientCapabilities`） |
| 能力探索 | initialize 階段協商 | 可選 `server/discover` 探活與能力宣告 |
| Transport | HTTP + SSE 雙向長連線 | **Streamable HTTP POST-only**，必帶 `Mcp-Method`、`Mcp-Name` header |
| 負載均衡 | 需 session affinity | 無狀態後可 round-robin；跨呼叫狀態用**明確 handle（jobId / runId）**，不靠連線 |
| 人機互動 | Elicitation 靠長連線 SSE | **MRTR（Multi Round-Trip Requests）**：server 回 `resultType: "input_required"`，client 帶 `inputResponses` 重試 |
| 長任務取結果 | `tasks/result` | **poll**（`tasks/get`），`tasks/result` 已不用 |

**功能生命週期（Active / Deprecated / Removed）** — X 明列新實作**不要再做**：

- Roots
- Sampling
- Logging（當一等公民）
- **DCR（Dynamic Client Registration）**
- legacy SSE
- 依賴 session 的雙向長連線設計
- 把每個內部 API 都暴露成獨立 MCP tool

**雙版本窗口**：至少 **12 個月**要相容舊版，因為生態上仍有大量 `2025-11-25` client；只實作 2.0 會接不上 Claude Desktop / 舊 SDK。

> ⚠️ **這一節直接推翻其他來源的兩項建議**：
> 1. D 與 K 都把 **DCR（動態客戶端註冊）** 列為授權必做項 —— 在新規格已 deprecated，見 §5.1
> 2. K 描述的 Elicitation（server 反向要輸入）與 G 的 SSE 長連線，在新規格改用 MRTR 重試語意，見 §5.4

---

## 2. 共識骨架：先決定角色，再談功能

六家一致：**MCP 支援不是單一功能，是先選角色。**

| 角色 | 意義 | 誰主張 | 典型對標（X 提供） |
|---|---|---|---|
| **MCP Client / Host** | 工作流去連外部 MCP Server，畫布上直接用 GitHub / Slack / DB / 內部系統的工具 | K、G、D、**X（建議先做）** | n8n MCP Client、Dify MCP 工具、LangChain `langchain.mcp` |
| **MCP Server** | 把工作流暴露給 Claude / Cursor / ChatGPT，讓外部 Agent 搜尋、觸發、編輯流程 | L、C | n8n instance-level MCP |
| **雙向** | 兩者都做，系統成為企業內部的「Agent 能力樞紐」 | K、G、C、**X（最終都會兩邊做）** | — |

X 的定位最精準：**Server 是生態入口，Client 是立即能用的產品能力。**
C 的戰略敘述最強：不要只做「MCP Client 可以連 MCP Server」，而要把 MCP 設計成平台的**標準外部能力層（Capability Layer）**。

---

## 3. 協議底座（兩個角色都要）

X：「這層不做，後面原語全是空中樓閣。」

| 項目 | 內容 | 來源 |
|---|---|---|
| JSON-RPC 2.0 | 完整的請求／回應、通知、標準錯誤碼 | D、G、X |
| 版本協商 | 新版 `_meta` 自帶；舊版 `initialize` 握手 | X（新）、K / D / G（舊） |
| timeout / retry / **idempotency** | 協議層基礎設施 | X、D |
| 進度追蹤 / 取消 | Progress、Cancellation | D、X |
| **Catalog cache** | `tools/list` / `prompts/list` / `resources/list` / `resources/read` 的 `ttlMs`、`cacheScope`；**list 必須穩定排序**，否則上游 prompt cache 失效 | X |
| Pagination | `cursor` | X、L |
| **Subscriptions** | `subscriptions/listen` 訂閱 `tools/list_changed` 等；動態工具目錄不能只靠啟動時掃一次 | X、K、G |
| 心跳與重連 | Ping、斷線重連、連線狀態監控（**新規格無狀態後大幅弱化**） | G、K |
| 連線池 / 健康檢查 | Client 端連多個 Server 的基本工程 | K |

### 3.1 Transport 選型（**原有分歧，X 給出定論**）

| 來源 | 主張 |
|---|---|
| **X** | **Streamable HTTP POST-only（遠端主力）**，必帶 `Mcp-Method` / `Mcp-Name` header，讓 gateway 不拆 body 就能路由與授權；stdio 給本機開發（Claude Desktop / Cursor） |
| D | stdio + Streamable HTTP（官方推薦的生產首選） |
| K | stdio、Streamable HTTP（主流）、SSE（舊版相容） |
| C | Gateway 至少支援 Streamable HTTP，本地 Agent 再支援 stdio；舊 SSE 只做相容 |
| G | Stdio + **SSE**（需實作 SSE 客戶端接推播、HTTP POST 發請求）；未提 Streamable HTTP |
| L | stdio vs HTTP/SSE；多租戶 SaaS 用 HTTP+SSE 或「新的 Streamable HTTP」，要處理連線狀態管理 |

**判斷：Streamable HTTP POST-only 為主，stdio 為輔，legacy SSE 只在雙版本窗口內做相容。** G 的 SSE-first 反映舊規格；X 的 header 路由設計（`Mcp-Method` / `Mcp-Name`）是 C 的 MCP Gateway 構想能成立的關鍵前提——沒有 header，gateway 得拆 JSON body 才知道要路由到哪。

---

## 4. 三大原語（Tools / Resources / Prompts）

### 4.1 Tools（做事，會改變狀態）

| 功能 | 內容 | 來源 |
|---|---|---|
| 動態發現 | `tools/list` 取得工具清單與 JSON Schema 參數定義 | D、G、K、X |
| 執行 | `tools/call`，解析目標 Server 並路由，回傳結構化輸出 | D、G、X |
| Schema 轉換 | MCP Tool Schema → 工作流 UI 節點表單（Vue 3）／ LLM function calling 格式 | G |
| 參數映射 | 上游節點輸出 → tool input schema，需 JSON Schema 驗證與型別轉換；X 補：靜態填值 / 上游輸出 / LLM 填 三種綁定 | K、X |
| 輸出正規化 | content blocks（text / image / resource）轉成工作流變數，後續節點才接得上 | K、X |
| **Tool annotations** | `readOnly` / `destructive` / `openWorld` → **UI 風險標示與預設審批策略** | **X** |
| **並行 tool call** | 並行、超時、重試、冪等 key | X |
| Schema 快取與版本 | 快取 schema、偵測變更、版本管理 | K、X |
| **描述品質** | tool description 品質直接決定 LLM 用不用得對；JSON Schema 要嚴格定義以減少無效呼叫 | K、L |
| 註冊為流程步驟 | 把 tool 註冊成工作流步驟，傳結構化輸入、消費結構化輸出 | D、C |

> **X 的 Tool annotations 比 C 的自訂 `risk_level` 更根本**：annotation 是協議層標準欄位（來自 server 宣告），`risk_level` 是本地治理欄位。正解是兩層——annotation 當預設值來源，本地 registry 可覆寫加嚴。

### 4.2 Resources（唯讀資料，提供上下文）

| 功能 | 內容 | 來源 |
|---|---|---|
| API | `resources/list`、`resources/templates/list`、`resources/read`（text / blob 注入 context） | G、D、X |
| URI 設計 | 需命名規則；C 舉例 `erp://product/123`、`erp://sales-order/SO202609001` | L、C |
| 資源模板 | RFC 6570 URI 模板描述參數化資源 | D |
| **定位** | 當 RAG / 系統提示 / 節點輸入，**而不是每次都讓模型再 call 一次 tool** | X |
| **分頁與大檔** | 分塊、摘要、引用 URI，不要整份塞進 LLM；工單批次查詢要考慮 token 用量 | L、X |
| **訂閱觸發** | `notifications/resources/updated`：外部 DB / MES 資料變動時，**觸發工作流的下一步** | G、X |

### 4.3 Prompts（提示模板）

| 功能 | 內容 | 來源 |
|---|---|---|
| API | `prompts/list`、`prompts/get`，工作流變數可填入 | G、D、X |
| 對應到 | slash command、流程模板、Agent 系統提示庫 | X |
| 用途 | 把常用流程封裝成模板，一鍵觸發標準化工作流 | K、L、D |
| **邊界** | 這是 **user-controlled**，不要自動當 tool 亂調 | **X** |
| 進階 | 把 AI Agent / Workflow / Approval Prompt 全部變成 MCP Prompt，例：`/analyze_cost_variance` | C |

---

## 5. 長時間任務與非同步（對本專案最關鍵）

企業流程是「建立採購單 → 等主管核准 → 等供應商回覆 → 等到貨 → IQC → 入庫」，不適合同步 tool call（C）。X：「工作流本質就是長任務，這層幾乎必做。」

| 方案 | 內容 | 來源 |
|---|---|---|
| **Tasks extension** | `io.modelcontextprotocol/tasks`；`tasks/get`、`tasks/update`、`tasks/cancel`（**poll，不再靠 `tasks/result`**） | X（新規格）、D（舊規格版本） |
| 狀態機 | 任務是持久化狀態機，攜帶底層執行狀態，支援延遲結果檢索 | D |
| **MCP task ↔ 工作流 run** | 對應狀態、進度、中途 elicitation、**durable handle**；逾時、重試、補償、可恢復 | **X** |
| 非同步回傳 | 先回 task id（handle），再輪詢；不要讓呼叫方卡住 | K、X |
| 狀態設計 | `running` / `waiting` / `approval_required` / `completed` / `failed` | C |
| 三種執行模式 | 同步、非同步、查詢模式 | D |

**共識：工作流引擎天然就是長時間任務引擎，MCP Task 是它的對外投影，不是另一套引擎。**

### 5.4 Elicitation / HITL（**新舊規格語意不同**）

| 來源 | 描述 |
|---|---|
| **X（新）** | **MRTR**：server 回 `resultType: "input_required"` → 工作流進入 **Wait for Human** 狀態 → 渲染表單（boolean / 字串 / 枚舉 / 結構化 schema）→ 帶 `inputResponses` **重試原操作**；需支援逾時、拒絕、委派給另一個審批人。LangChain 把它對成 Graph interrupt，工作流引擎應對成「人工節點」 |
| K（舊） | 支援 MCP 的 Elicitation（server 反向向使用者要輸入） |
| G | 破壞性 tool 呼叫前，工作流必須能暫停等人工確認（UI 或 LINE / Telegram） |
| D | 提供清晰 UI 讓使用者檢視與授權活動 |
| L | 唯讀 vs 寫入 tool 分離；寫入類加確認機制或 **dry-run 模式** |

> **X 的第 4 個坑點值得單獨記下**：「elicitation 不是『發個 Slack』，是**原操作未完成，帶答案重試**。狀態機要能掛起再續。」——這正是 Temporal 的 Signal 語意。

---

## 6. MCP Server 側：把工作流暴露出去（X 的拆法最細）

### 6.1 兩種暴露粒度（X，抄 n8n）

1. **Instance-level**：一個 MCP endpoint，可搜尋、觸發、編輯被標記的流程
2. **Workflow-level**：單一流程當一個（或一組）tool，適合精心設計的 Agent API

### 6.2 建議暴露的工具面（X）

| 類別 | 例子 | 注意 |
|---|---|---|
| 發現 | `search_workflows`、`get_workflow` | 不要一次 dump 全部 schema |
| 執行 | `run_workflow`、`get_run`、`cancel_run` | 長流程用 Tasks，不要同步卡死 |
| 輸入 | 依流程 JSON Schema 動態生成 tool input | **敏感欄位不要進 schema 描述** |
| 編輯（可選、高風險） | `create_workflow`、`update_node` | 必須強授權 + 審計 + 沙箱 |
| 資料 | 讀業務實體 / data table | 最小權限 |

C 的版本（動詞更貼 ERP）：`create_purchase_order`、`approve_purchase_order`、`get_inventory`、`get_product_cost`、`create_work_order`、`analyze_cost_variance`、`send_notification`、`create_invoice`。
L 的版本：建立任務、指派、查詢狀態、觸發流程節點。

### 6.3 Server 側其他規劃項

| 項目 | 內容 | 來源 |
|---|---|---|
| 可見性開關 | 每個流程「是否對 MCP 可見」的開關與 scope | X |
| Run ↔ Task | 把流程 run 對成 MCP Task（輪詢進度、可取消） | X |
| 執行中要人確認 | 對 client 發 elicitation（走 MRTR） | X |
| 回傳格式 | **結構化 JSON + 人類可讀摘要**，方便模型接著推理 | X |
| **工具合併** | 流程很多時**不要 1 workflow = 1 tool**；用 **search / execute / docs 三件套**（Cloudflare Code Mode 那套），否則模型 context 會炸 | **X** |
| 反向暴露 | 把自身流程暴露給 Claude Desktop 等，成為企業內部 Agent 能力樞紐 | G、C |

---

## 7. Client 側：工作流「消費」外部能力（X §2 最完整）

| 項目 | 內容 | 來源 |
|---|---|---|
| Server 註冊 | URL / stdio command、header、OAuth、環境變數、啟停 | X、K（類似 `mcp.json` 的設定介面） |
| 健康檢查 | 連線健康檢查、`server/discover`、能力協商 | X |
| 目錄同步 | Tools / Resources / Prompts 分開快取，支援手動刷新與 `list_changed` | X、K |
| **工具過濾** | allowlist、按 tag / 風險等級隱藏；**避免把 200+ 個 tool 全塞進 context（token 爆炸是第一個產品坑）** | **X**、D（允許清單） |
| **多 server 命名空間** | `github.search_issues` vs `jira.search_issues`，避免 name collision | X、K（多 server 加前綴） |
| 多租戶隔離 | 不同團隊可用不同 server；catalog / credential / run 全部隔離 | K、X |
| Agentic 執行迴圈 | LLM 回 tool call → 攔截解析目標 server → `tools/call` → 結果封裝成 Tool Message 回送 LLM 續推理 | G |
| 上下文組裝 | 呼叫 LLM 前打包所有已連 server 的 tools / resources 描述 | G |
| 多代理路由 | Agent A 只能存取「報價單查詢 MCP」，Agent B 才可存取「庫存修改 MCP」 | G |

---

## 8. 授權、安全、治理

X：「MCP 把 tool 當任意程式執行，host 必須當它是**不可信**的。」

### 8.1 身分與授權（**X 與 D/K 衝突，以 X 為準**）

| 項目 | 內容 | 來源 |
|---|---|---|
| 基礎 | OAuth 2.1 + **Protected Resource Metadata** + **Resource Indicators** | X、D、C |
| **Client 註冊優先序** | ① **預註冊 client** ② **CIMD（Client ID Metadata Documents）** ③ DCR 只當 fallback（**已 deprecated**） | **X** |
| ~~DCR 必做~~ | ~~Dynamic Client Registration + RFC 8414 授權伺服器發現~~ | ~~D、K~~（舊規格，已被 X 推翻） |
| Issuer 校驗 | 校驗 `iss`（**RFC 9207**）；client credentials 綁定 issuer，**不可跨 server 重用** | X |
| 企業模式 | **EMA（Enterprise Managed Authorization）** | X |
| 雙重角色 | MCP Server 同時是 OAuth Client（對第三方 AS）與 OAuth Authorization Server（對 MCP Client） | D |
| 較輕方案 | 每個 tenant / user 的 OAuth 或 API token | L、K |
| 本機 stdio | 行程隔離、環境變數注入、**不要把 secrets 寫進流程 JSON** | X、C |
| Credential 管理 | Workflow → Credential Reference → **Secret Manager** → MCP Client | C |

### 8.2 執行治理

| 項目 | 內容 | 來源 |
|---|---|---|
| 預設要同意 | 每個 tool call 預設需同意；依 annotation 分流：**只讀自動 / 寫入確認 / destructive 強制審批** | X |
| 作用域 | 哪個流程能調哪個 server、哪個 tool | X、G |
| Tool-level 權限 | 哪些 tool 對哪些使用者 / 客戶端可見（例：Developer 不能呼叫財務 tool） | L、K |
| 最小權限 | 每個 tool 只授予完成其功能所需的最小權限 | D |
| **參數紅線** | 阻擋把 secrets / PII 當參數送出 | **X** |
| 注入與攻擊面 | **SSRF / 路徑穿越 / 命令注入（stdio server 尤其危險）**；tool description 注入防護 | X、K |
| Rate limit | 速率限制、並發上限、**成本配額**；避免 Agent 迴圈呼叫 | X、L、K |
| 沙箱 | 沙箱執行、參數白名單 | K |
| 灰度與停用 | 灰度發佈、停用機制 | K |

### 8.3 信任模型（X）

- **Tool description 不可信**，除非 server 在 allowlist
- 遠端 MCP 與本機 MCP 分開策略
- 供應鏈：server 來源、版本鎖定、hash / 簽名（後期）

### 8.4 Permission / Risk Engine（C）

C 認為這是「企業 Workflow」與一般 MCP 工具最大的差異：AI 呼叫 `create_purchase_order` **不能直接執行**：

```
AI → MCP Tool → Permission Engine → Risk Engine → Approval Policy → Workflow → Execute
```

| Tool | Risk | 行為 |
|---|---|---|
| 查詢庫存 / 查詢客戶 | Low | 直接執行 |
| 建立報價單 | Medium | 可執行 |
| 建立 PO | Medium | 主管核准 |
| 修改產品成本 | High | 人工核准 |
| 發出付款 | Critical | 多人核准 |

> 與 X 的 Tool annotations 合併使用：annotation（`readOnly` / `destructive` / `openWorld`）當**預設風險來源**，本地 `risk_level` 當**可加嚴的覆寫**。

### 8.5 資料存取邊界（L 單獨提出，但很重要）

> 避免直接把資料庫操作暴露成 tool，中間要有一層業務邏輯驗證。

與本專案既有的「AI 不決定下一節點、不直接寫入 ERP」職責邊界一致，見 [系統規劃/01_系統架構.md](../../系統規劃/01_系統架構.md) §1.1。

---

## 9. 工作流引擎要補的執行語意（X §5，其他家都沒整理成這樣）

X：協議接上了，引擎本身還缺這些，否則 MCP 只是「多一個 HTTP 節點」。

1. **Agent 節點**：動態選 tool、多輪 call、把 MCP catalog 當 toolbelt
2. **確定性節點**：選定某個 MCP tool，參數用變數綁定（**自動化場景比 Agent 更常用**）
3. **人工等待**：對齊 elicitation / 審批
4. **非同步 run**：對齊 Tasks，支援 poll、resume、cancel
5. **補償 / rollback**：destructive tool 失敗後怎麼收
6. **Schema 演進**：上游 tool schema 變了，已發布流程怎麼**告警、凍結、遷移**
7. **Context 預算**：tool 列表過長時的檢索、分組、延遲載入
8. **可觀測性**：trace 裡看到 MCP method、name、latency、token、cache hit
9. **Gateway**：統一出口，按 `Mcp-Method` / `Mcp-Name` 路由、授權、審計

C 對第 1、2 點的對應主張：MCP Tool 就是 Workflow Node 的一種標準化形式，節點型別統一為
`Native / HTTP / Python / SQL / AI / MCP Tool / n8n Workflow / External Workflow`。

---

## 10. 可觀測性、錯誤處理與運維

| 項目 | 內容 | 來源 |
|---|---|---|
| 全鏈路追蹤 | 從使用者輸入、推理、決策到執行的完整事件流 | D |
| **MCP 專屬 trace 欄位** | MCP method、name、latency、token、**cache hit** | X |
| 稽核日誌 | 誰、哪個流程、哪個 server、哪個 tool、參數摘要、結果碼、耗時 | X、G、L、C |
| 不可竄改 | 稽核記錄建議雜湊鏈結，可匯出 JSON / CSV 供合規審查 | D |
| 成本統計 | 每次 tool 呼叫的成本與延遲 | K、X |
| **錯誤語意分離** | MCP 協議層 error 與工具執行失敗（`isError`）要區分，對應到工作流的**重試 / 補償節點** | K |
| 錯誤可讀性 | 錯誤格式要讓 Agent 理解失敗原因，據以重試或改變策略 | L |
| 測試工具 | MCP Inspector 部署前驗證；Mock Server 讓開發者不依賴真實服務驗證流程邏輯 | D |
| 監控告警 | 追蹤存取模式、資源使用量，偵測異常行為 | D |

C 的稽核紀錄格式範例：

```
2026-09-18 07:30
User: John
Agent: Purchase-Agent
Workflow: PO-Approval
Tool: create_purchase_order
Arguments: {...}
Approval: Manager-Jack
Result: SUCCESS
Duration: 2.3 sec
```

---

## 11. 資料模型（C 提供，唯一給到表級的來源）

```
mcp_server            mcp_tool             mcp_permission
mcp_server_version    mcp_resource         mcp_policy
mcp_connection        mcp_prompt           mcp_tool_execution
mcp_credential        mcp_subscription     mcp_audit_log
mcp_task              mcp_marketplace_item
```

C 認為最重要的五張：`mcp_tool`、`mcp_permission`、`mcp_policy`、`mcp_task`、`mcp_tool_execution`。

`mcp_tool` 欄位建議：

```
id, server_id, name, description,
input_schema, output_schema, category,
risk_level, requires_approval,
timeout, retry_policy, enabled, version
```

**依 X 需補的欄位**：`annotations`（readOnly / destructive / openWorld）、`namespace`（多 server 前綴）、`schema_hash`（Schema 演進偵測用）、`catalog_ttl_ms`、`sort_key`（list 穩定排序用）。

X 另指出 Client / Server 共用 runtime 時，**credential、catalog、run store 要共用，但權限邊界必須分開**。

---

## 12. UI 規劃（C 提供）

```
MCP Center                 Tools                      Permissions
─────────────────          ─────────────────          ─────────────────
My MCP Server              🔧 get_inventory           AI Agent: Purchase Agent
● Production MCP           🔧 get_product_cost
  23 Tools                 🔧 create_purchase_order   Allowed:
  18 Resources             🔧 approve_purchase_order    ✓ get_inventory
   6 Prompts               🔧 analyze_cost_variance     ✓ get_supplier
[ Endpoint ]                                          Require Approval:
https://xxx.com/mcp        Risk                         ⚠ create_purchase_order
[ Copy Configuration ]     LOW 12 / MED 7 / HIGH 4    Denied:
                                                        ✕ approve_payment
```

Client 端另需 Connector 管理介面（C、K、X）：每個 MCP Server 記錄 Name、Description、Icon、Endpoint、Transport、Authentication、Available Tools / Resources / Prompts、Permission、Status。

---

## 13. 擴展生態

| 擴展 | 內容 | 來源 |
|---|---|---|
| **Tasks** | 長時間工作（見 §5） | D、X |
| **Skills over MCP** | 提供結構化指令供 Agent 工作流使用；X：把「**怎麼用這套流程**」寫成可發現的 skill | D、X |
| **MCP Apps** | 在對話中內嵌互動式 UI：圖表、**表單**、審批 UI、影片播放器 | D、X |

> **MCP Apps 與本專案的表單引擎高度重疊**：若 MCP Apps 可在 Claude 對話內嵌表單與審批 UI，等於本專案的 @form-create 表單有了第二個渲染宿主。列為待評估項（Q-07）。

---

## 14. 分歧點對照與判斷

### 14.1 先做 Client 還是先做 Server？（**主要分歧，X 加入後票數反轉**）

| 來源 | 主張 | 理由 |
|---|---|---|
| **X** | **先 Client**，再 Server | Server 是生態入口，**Client 是立即能用的產品能力**。但其 P0 同時含一個**最小 Server**（`run_workflow` + `get_run`） |
| K | **先 Client** | 先打通「在流程裡呼叫外部 tool」，再加審批與權限達到生產可用，最後把自身能力包成 Server |
| G | **先 Client**（SSE + Tools） | 能立即帶來最大業務價值 |
| C | **先 Server** | Phase 1 就做 Streamable HTTP + Auth + `tools/list` + `tools/call` + Tool Registry + RBAC + Audit；Phase 2 才做 Client |
| D | 先協議 + 三原語 + 雙 transport，再安全治理，最後進階功能 | 以協議完整度分期，未站邊 |
| L | 未談順序，回答通篇以 Server 為主 | — |

票數：**Client-first 3（X / K / G）vs Server-first 2（C / L）**。

**修正後的判斷**：我先前只看五家時判斷「先 Server」。X 加入後要調整為 —— **兩邊都做最小面，但 Server 面刻意做小**：

- **Server 面**只做 **workflow-level** 粒度的 `run_workflow` + `get_run`（X 的 P0），不做 instance-level 搜尋 / 編輯。理由：本專案已有 Business Object / 簽核 / ERP 寫回，暴露出去是**用既有資產換新入口**，成本極低。
- **Client 面**做 Streamable HTTP + `tools/list` / `tools/call` + 固定工具節點。理由：X / K / G 一致指出這是**立即可用的產品能力**，且對「已有 ERP 的 SME」而言，接 Odoo / LINE / Google Drive 的 MCP 比被 Claude 呼叫更早產生營收價值。
- **不做**：instance-level 編輯流程、Marketplace、Agent 自主選 tool —— 這些都在 X 的 P2。

### 14.2 MCP 是獨立模組，還是介面層？（C 主張，X 的 Gateway 設計佐證）

C 明確反對做成獨立模組：

```
Business Object → Business Action → Policy → Workflow → Execution
                                                            │
                                            ┌───────────────┼───────────────┐
                                          REST             UI              MCP
```

> MCP 是 Workflow Engine 的一個標準 API / Capability Interface，而不是 Workflow Engine 本身。

X 的架構圖同樣把 MCP Gateway 放在 runtime 之上、與 runtime 分離：

```text
[ Claude / Cursor / 內部 Agent ]
                 │  MCP Server 角色
                 ▼
┌──────────────────────────────────────────┐
│  MCP Gateway                             │
│  路由(Mcp-Method/Name) / OAuth / 審計     │
├──────────────────────────────────────────┤
│  Workflow Runtime                        │
│  節點執行 │ Agent loop │ HITL │ Tasks    │
├────────────┬─────────────────────────────┤
│ 把流程當    │  MCP Client 連外部 servers  │
│ tool 暴露   │  GitHub / Slack / 內部系統  │
└────────────┴─────────────────────────────┘
```

**判斷：C 與 X 一致，採之。** 本專案既有架構已是 Business Object → Action → Workflow，MCP 應與 REST API、Vue UI、Portal 並列為第四個介面，共用同一套 Business Action 與 RBAC；否則會出現「REST 有權限檢查、MCP 沒有」的雙軌漏洞。

### 14.3 授權：DCR 還是 CIMD？（**D/K 與 X 直接衝突**）

- D、K（`2025-11-25` 規格）：**DCR 動態客戶端註冊必做**
- X（`2026-07-28` 規格）：**DCR 已 deprecated**，優先預註冊 client，其次 CIMD，DCR 只當 fallback

**判斷：以 X 為準。** 但雙版本窗口內（至少 12 個月）舊 client 仍可能走 DCR，故 DCR 列為「相容路徑」而非「主路徑」。

---

## 15. 各家建議的落地順序對照

| 階段 | X（Grok，最新規格） | C（ChatGPT） | K（Kimi） | G（Gemini） | D（DeepSeek） |
|---|---|---|---|---|---|
| **P0 / 1** | Streamable HTTP client、連線管理 + `server/discover`、Tools list/call → Agent 節點 + 固定工具節點、OAuth / Bearer、基本審計與允許清單、**一個把指定工作流當 tool 的 MCP Server（`run_workflow` + `get_run`）** | MCP Server：Streamable HTTP、Auth、tools/list、tools/call、Tool Registry、RBAC、Audit | Client + Streamable HTTP，打通流程內呼叫外部 tool | Client 基礎（SSE）+ Tools | JSON-RPC 通訊層、三大原語、stdio + HTTP |
| **P1 / 2** | Resources / Prompts、Catalog cache + list_changed、**Elicitation → 人工節點（MRTR）**、Tasks ↔ 非同步 run、進度 / 取消 / 逾時、**工具搜尋 / 分組避免 catalog 爆炸** | MCP Client：連外部 Server、發現、Credential、OAuth | 加審批與權限，達到生產可用 | Resources、Prompts、stdio 子行程管理、安全審核 | OAuth 2.1、稽核日誌、權限控制、使用者同意 |
| **P2 / 3** | **雙版本相容與協定升級策略**、MCP Gateway（header 路由、集中授權）、Instance-level 搜尋 / 編輯、Skills、MCP Apps、企業 SSO / EMA、供應鏈、多租戶配額、**Schema 變更偵測與流程凍結** | Workflow 整合：MCP Tool / Resource / Prompt Node、Approval、Retry、Timeout、Compensation | 自身能力包成 MCP Server 對外開放 | — | Tasks、工具註冊表、Skills / Apps、可觀測性 |
| 4 | — | Enterprise：多租戶、Policy Engine、Risk Level、Audit、Rate Limit、Secret Manager、Observability | — | — | — |
| 5 | — | AI-native：AI Agent、Tool selection、Resource retrieval、HITL、Tasks、長流程、MCP Apps | — | — | — |

### 15.1 刻意不做（X，新系統）

- Sampling、Roots、Logging 當一等公民
- 依賴 session 的雙向長連線設計
- 把每個內部 API 都暴露成獨立 MCP tool

---

## 16. 規劃時最容易漏的五件事（X，全文最實用的一段）

1. **Token / catalog 爆炸**：200 個 tool dump 進 prompt，Agent 正確率會崩。要**搜尋式發現**，不要全量注入。
2. **長任務**：部署、審批、批次處理不能用同步 `tools/call`。沒 Tasks / run handle，遠端 client 會 timeout。
3. **Schema 不穩定**：MCP tool 一改，**已上線流程全滅**。要版本、快照、相容檢測。
4. **HITL 語意**：elicitation 不是「發個 Slack」，是「**原操作未完成，帶答案重試**」。狀態機要能掛起再續。
5. **雙版本窗口**：生態上還有大量 `2025-11-25` client；只實作 2.0 會接不上 Claude Desktop / 舊 SDK。

---

## 17. 對照本專案架構的落點（本文判斷，非 AI 來源）

依 [系統規劃/01_系統架構.md](../../系統規劃/01_系統架構.md) 的既有分層：

| MCP 功能 | 落在哪一層 | 說明 |
|---|---|---|
| MCP Server endpoint（`/mcp`） | **Rust (Axum)** | 與現有 Auth / Tenant / RBAC / Audit 同層，天然共用；不可另建繞過權限的路徑 |
| MCP Gateway（header 路由） | **Rust (Axum) 前置中介層** | X 的 `Mcp-Method` / `Mcp-Name` header 讓路由與授權不需拆 body；初期可與 endpoint 同進程，之後再抽出 |
| Tool 定義 = Business Action | **Rust Business Object API** | 一個 Business Action 同時暴露成 REST、UI 與 MCP tool，三介面共用同一套驗證 |
| Tool Registry（`mcp_tool` 等表） | **PostgreSQL（RLS by tenant_id）** | annotations（協議來源）+ risk_level（本地覆寫）雙層 |
| MCP Client / `tools/call` | **Python Worker 的新 Activity** | 例如 `mcp.call_tool`，直接繼承既有 `idempotency_key`、Retry、Timeout 機制 |
| MCP Tool 節點 | **DSL 新增 node type `mcp_tool`** | 與 `ai_task` 同屬 Phase 2；比照現行原則，**DSL 不得含 MCP transport 詞彙**（transport / `_meta` / JSON-RPC 留在 Worker 內） |
| 長時間任務 / MCP Task | **Temporal** | X 的「durable handle（jobId / runId）」= Temporal 的 workflow_id；`tasks/get` 應是 **Temporal Query 的投影**，**不要再造第二套狀態機** |
| Elicitation / MRTR | **Temporal Signal + 既有 `human_approval` 節點** | `input_required` → 建 Human Task → 使用者填表 → Signal → 帶 `inputResponses` 續跑。X 的「掛起再續」就是 Signal 語意，本專案不需新機制 |
| HITL 審批 | **既有 `human_approval` + Approver Resolver** | destructive annotation 的 tool 強制插入 `human_approval`，不做第二套審批 |
| 補償 / rollback | **既有 `on_reject.goto` 不夠用** | destructive tool 失敗需要補償節點，DSL 目前無此型別 → 見 Q-09 |
| **Schema 演進** | **衝突點：DSL 發布即 immutable** | 已發布流程綁定的外部 tool schema 若變更，流程會靜默失效。需 `schema_hash` 快照 + 啟動時比對 + 告警 / 凍結 → 見 Q-10 |
| 稽核 | **既有 Audit + `mcp_tool_execution`** | 記錄 Agent 身分、tool、參數摘要、核准人、結果、耗時、cache hit |
| Credential | **Secret Manager，DSL 只存 reference** | 呼應 C 與 X：workflow node 絕不直接保存 token |
| 外部 Portal token vs MCP token | **分離** | Portal 的 magic link token 與 MCP 的 OAuth token 權限模型不同，不可共用 |
| Catalog 快取與過濾 | **Rust 側 + PostgreSQL** | X 的 `ttlMs` / `cacheScope` / 穩定排序；工具過濾 allowlist |

---

## 18. 待決事項

| 編號 | 問題 | 影響 |
|---|---|---|
| Q-01 | MCP 排在哪一期？現況連 `ai_task` 都在 Phase 2，MCP 若進 MVP 會排擠報價單流程 | 排程 |
| Q-02 | 先 Client 還是先 Server？（§14.1，修正後傾向**兩邊各做最小面**，Server 只做 workflow-level） | 架構順序 |
| Q-03 | Tool 粒度：Business Action 一對一，還是走 X 的 **search / execute / docs 三件套**？流程一多，1 workflow = 1 tool 會讓 context 爆炸 | Registry 設計、LLM 可用性 |
| Q-04 | 認證：MVP 用 API key + tenant 綁定，還是直接上 OAuth 2.1 + **預註冊 client / CIMD**？（DCR 已 deprecated，不列選項） | 工期差異大 |
| Q-05 | **Rust 生態的 MCP SDK 成熟度需驗證**，且要確認是否支援 `2026-07-28` 無狀態規格（比照 01 文件指出 Temporal 無官方 Rust SDK 的風險）。若不成熟，MCP Server 是否改由 Python 側承載？ | 技術風險 |
| Q-06 | Resources 訂閱觸發工作流（G、X）是否納入？等於多一條事件觸發來源 | Trigger 模型 |
| Q-07 | MCP Apps 與既有 @form-create 表單引擎重疊，是否評估以 MCP Apps 當第二渲染宿主？ | 產品邊界 |
| Q-08 | **雙版本窗口**：是否同時支援 `2026-07-28` 與 `2025-11-25`？只做新版接不上現有 Claude Desktop / 舊 SDK；兩版都做工期翻倍 | 相容性 vs 工期 |
| Q-09 | DSL 目前只有 `on_reject.goto` 一種回跳，**destructive tool 失敗的補償 / rollback 節點型別缺席**，是否補 `compensation` 節點？ | DSL 擴充 |
| Q-10 | **Schema 演進 vs 發布即 immutable**：已發布流程綁的外部 tool schema 變更後，要告警、凍結還是自動遷移？這是 X 點名「MCP tool 一改，已上線流程全滅」的坑 | 版本治理（與 05 文件的版本化議題相關） |

---

## 19. 原始來源連結

| 編號 | 連結 |
|---|---|
| K | https://www.kimi.ai/share/1a0b1c87-2ca2-8040-8000-0000d18823f2 |
| D | https://chat.deepseek.com/share/ftpl2epl49diwph4sh |
| L | https://claude.ai/share/fe68422e-d93b-4b00-84f0-d4ab6d10cdb4 |
| X | https://grok.com/share/bGVnYWN5_8d24e7dc-ae95-4b2c-a526-0cd83acb1b3e （分享頁被 Cloudflare 擋，內容由使用者手動提供） |
| G | https://share.gemini.google/2JIHdbclstEn |
| C | https://chatgpt.com/share/6aac7d7f-d8cc-83ee-984e-539dfa68e734 |
