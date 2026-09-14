# UI 設計 Prompt（給 Stitch）

- 日期：2026-09-14
- 用途：貼給 [Google Stitch](https://stitch.withgoogle.com/) 產生畫面設計
- 對應：[07_實作順序調整.md](07_實作順序調整.md) 步驟 1、2、3、6

## 使用方式

1. 每個 prompt 獨立貼一次，不要合併。Stitch 一次處理一個畫面效果較好。
2. 先貼「共用設計語言」建立風格，再貼各畫面 prompt。
3. 產出後如需調整，用追加句子而非重寫整份。
4. Stitch 產出的是視覺稿與前端骨架，實際資料結構仍以 `schemas/` 下的 JSON Schema 為準。

---

## 0. 共用設計語言（先貼這段）

```
Design a desktop web application for Taiwanese small-to-medium manufacturing companies.
The users are salespeople, department managers, finance staff, and purchasing agents aged 30-55.
They are not technical. Many still use Excel and paper forms daily.

Visual style:
- Clean, dense, information-first. Similar to Linear or Notion, but more conservative.
- Light theme only. White background, subtle gray borders, no heavy shadows.
- Primary color: a calm blue (#2563EB). Success green, warning amber, danger red used sparingly.
- Typography: system sans-serif. Traditional Chinese text must render clearly at 14px.
- Generous use of tables and forms. Avoid large hero images or marketing-style layouts.
- Left sidebar navigation, top bar with breadcrumb and user menu.
- All labels in Traditional Chinese.

Layout frame for every screen:
- Left sidebar 240px wide, collapsible, with sections: 首頁, 我的待辦, 報價單, 採購申請, 設計器, 報表, 設定
- Top bar 56px: breadcrumb on left, search, notification bell, user avatar on right
- Main content area with a page title row and action buttons on the right
```

---

## 1. 表單設計器（Form Designer）

```
Design a form designer screen for a business workflow platform.
Screen title: 表單設計器

Three-column layout filling the viewport below the top bar:

LEFT PANEL (280px, white, scrollable) — 元件庫
A search box at top with placeholder 搜尋元件.
Below it, collapsible groups of draggable component cards. Each card is a
rounded rectangle with a small icon on the left and a Chinese label:
  基本欄位: 單行文字, 多行文字, 數字, 日期, 下拉選單, 多選, 核取方塊, 開關
  進階欄位: 明細表格, 關聯查詢, 人員選擇, 部門選擇, 檔案上傳, 唯讀顯示
  版面: 區塊標題, 分隔線, 說明文字
Cards show a subtle "grab" cursor affordance. Dragging a card shows a ghost preview.

CENTER CANVAS (flexible width, light gray #F8FAFC background) — 表單預覽
A white sheet centered with max-width 900px, padding 32px, representing the form.
It shows a live preview of a quotation form (報價單) with collapsible sections:
  客戶資訊 — two fields side by side: 客戶 (reference picker with search icon),
            簽核聯絡人 (multi-select showing 2 selected chips)
  報價條件 — three fields in a row: 報價單號 (read-only, gray), 有效期限 (date picker), 幣別 (dropdown)
            then 付款條件 (full width text input)
  報價明細 — a data table with columns: 品號, 品名, 數量, 單位, 單價, 折扣, 金額, 成本
            Numeric columns right-aligned. 金額 and 成本 columns have a small
            calculator icon meaning "computed". Three sample rows plus a
            dashed "+ 新增一列" row at the bottom.
  金額 — three fields: 表頭折扣率 (with % suffix), 小計 (read-only), 總計 (read-only, bold)
  內部資訊 — 毛利率 (read-only, with a small orange "客戶看不到" badge),
            內部備註 (textarea), 附件 (upload dropzone)

Each field in the canvas, when hovered, shows a thin blue outline and a small
floating toolbar at its top-right with three icons: 複製, 上下移動, 刪除.
The currently selected field has a solid blue outline.
Sections can be dragged to reorder, shown by a drag handle on the left edge.

RIGHT PANEL (340px, white, scrollable) — 欄位屬性
Header shows the selected field name, e.g. 「表頭折扣率」 with a small type badge 「數字」.
Below, three tabs: 基本, 資料, 規則

Tab 基本 (UI properties):
  標籤 (text input), 提示文字 (text input), 說明 (textarea),
  欄寬 (a 12-column visual selector — twelve small squares where the user
  clicks to set width, currently 4 of 12 highlighted),
  小數位數 (number stepper), 前綴 / 後綴 (two small inputs side by side),
  客戶是否可見 (toggle switch, currently off, with helper text 關閉後客戶 Portal 不顯示此欄位)

Tab 資料 (data binding):
  資料路徑 (text input showing quotation.discount_rate with a green checkmark
           meaning "valid path"),
  型別 (dropdown showing 小數),
  預設值, 最小值, 最大值 (three inputs),
  計算公式 (a code-style input with monospace font, showing an expression,
           and a small 驗證 button)

Tab 規則 (workflow rules):
  Three expression builders stacked. Each is a card with a title, a toggle
  to switch between 簡易 and 進階 mode, and the rule body.
  顯示條件 — currently 永遠顯示
  唯讀條件 — in 簡易 mode, showing a row of chips: [當前節點] [不等於] [開始, 修改]
             with an "+ 新增條件" button
  必填條件 — currently 永遠必填
  Below them: 可編輯角色 (multi-select chips showing 業務, 主管),
              可檢視角色 (multi-select chips)

TOP ACTION BAR of the page:
Left: breadcrumb 設計器 / 表單 / 報價單, then a version chip 「v1 草稿」
Right: buttons 預覽, 儲存草稿 (secondary), 發布 (primary blue)
An "undo / redo" pair of icon buttons sits to the left of 預覽.
```

---

## 2. 流程設計器（Workflow Designer）

```
Design a workflow designer screen for an approval process builder.
Screen title: 流程設計器

Important: the canvas uses AUTOMATIC TOP-DOWN LAYOUT, not free dragging.
Nodes are arranged vertically and centered. Users insert nodes at fixed
insertion points; they cannot drag nodes to arbitrary positions.
This is similar to DingTalk (釘釘) or Feishu approval flow builders, not like n8n.

Three-column layout:

LEFT PANEL (260px) — 節點庫
Groups of node type cards, each with a distinct colored icon:
  流程控制 (gray): 條件分支, 並行, 匯合
  人工 (blue): 人工簽核, 人工處理
  系統 (purple): 系統動作, 發送通知
  結束 (dark): 結束節點
Each card shows the node name and a one-line description in smaller gray text,
e.g. 人工簽核 / 指派給特定人員或角色進行核准

CENTER CANVAS (flexible, light gray background with a subtle dot grid) — 流程圖
A vertical flow rendered top-to-bottom, horizontally centered, showing:

  [開始] rounded pill, green, label 業務送出報價單
    |
    | ← a thin vertical connector line with a small circular "+" button
    |    centered on it. Hovering the "+" enlarges it and shows a tooltip 插入節點
    |
  [人工簽核] a white card 320px wide with: a blue left border 4px,
    a header row with icon + title 主管簽核, a body line 簽核人：申請人主管,
    and a footer line in small gray text 逾期 2 天升級至業務總監
    |
  [條件分支] a diamond-ish card, amber accent, title 折扣是否超過 15%,
    with the expression shown in monospace小字 quotation.discount_rate > 0.15
    |
    +---- the flow SPLITS into two vertical lanes side by side ----+
    |                                                              |
  left lane labeled 是 (green chip)              right lane labeled 否 (gray chip)
  [人工簽核] 財務簽核                              (empty lane with just a line)
    |                                                              |
    +---------------- lanes MERGE back into one line --------------+
    |
  [系統動作] purple accent card, title 產生 PDF 並寄給客戶,
    subtitle 動作：quotation.publish, small retry badge 重試 3 次
    |
  [人工簽核] card with an orange "外部" badge next to the title 客戶簽核,
    body 簽核人：報價單客戶聯絡人, footer 全部同意才通過 · 逾期 7 天通知業務
    |
  [系統動作] Odoo 建立訂單
    |
  [發送通知] Email 通知雙方
    |
  [結束] rounded pill, dark gray

Additionally, draw a RETURN PATH: a curved line on the far left margin going
from the 客戶簽核 node back up to a small node labeled [人工處理] 業務修改報價,
which then connects back into 主管簽核. Label this path 退回 in red small text.
The return line is dashed and red-tinted to distinguish it from the main flow.

Each node card when hovered shows a small toolbar at its top-right: 設定, 刪除.
The selected node has a blue glow outline.

RIGHT PANEL (340px) — 節點設定
Header: node name 「主管簽核」 with a type badge 人工簽核.
Sections stacked vertically, each a labeled group:

  簽核人
    A segmented control: 角色 | 指定人員 | 主管 | 部門主管 | 外部聯絡人 | 進階
    Currently 主管 is selected, showing below: 取得「申請人」的主管
    a helper line in gray: 執行時即時解析，人員異動自動跟隨

  簽核方式 (visible when multiple approvers)
    Radio group: 全部同意 (selected) / 任一同意 / 指定人數同意
    When 指定人數同意 is chosen a number input appears
    A second radio group: 全部通過才算成功 (selected) / 任一拒絕即退回

  逾期處理
    逾期時間 — a duration input: a number stepper and a unit dropdown (小時/天)
    逾期動作 — dropdown: 繼續等待 / 升級通知 / 自動同意 / 自動拒絕
    升級對象 — appears only when 升級通知 selected, a role picker

  退回設定
    Radio: 退回至指定節點 (selected) / 直接結束流程
    節點選擇 — a dropdown listing only upstream nodes, currently 業務修改報價
    A small info note: 只能退回流程上游節點

  表單
    使用表單 — dropdown, currently 報價單 (預設)

TOP ACTION BAR:
Left: breadcrumb 設計器 / 流程 / 報價單簽核流程, version chip 「v1 草稿」
Right: 模擬執行 (with a play icon), 驗證, 儲存草稿, 發布 (primary)

A VALIDATION BANNER variant: show an alternate state where a red banner appears
below the top bar reading 「發現 2 個問題」 with an expandable list:
  · 節點「財務簽核」的角色 finance_manager 不存在
  · 節點「結束」無法從開始節點到達
and the corresponding nodes in the canvas have red outlines with a warning icon badge.
```

---

## 3. 表單設計器 + 權限整合（Permission Matrix）

```
Design a permission configuration screen that combines form fields with workflow stages.
Screen title: 表單權限設定

The core of this screen is a MATRIX TABLE showing which role can read or edit
each field at each workflow stage. This is the most important visual element.

Layout: full width, no left component panel. A filter bar on top, matrix below.

FILTER BAR (sticky, white, below page title):
  表單 — dropdown, currently 報價單
  流程 — dropdown, currently 報價單簽核流程
  檢視模式 — segmented control: 依角色 (selected) | 依節點
  角色 — dropdown, currently 業務 (only shown in 依角色 mode)
  A legend on the right showing three small swatches:
    綠色 可編輯 · 藍色 唯讀 · 灰色 隱藏

MATRIX TABLE (the main element):
  A table with a frozen first column and horizontally scrollable body.

  First column (240px, frozen, light gray header): 欄位
    Rows grouped by section with a group header row spanning full width:
      客戶資訊, 報價條件, 報價明細, 金額, 內部資訊
    Each field row shows the field label and below it in small gray monospace
    the data path, e.g.
      表頭折扣率
      quotation.discount_rate

  Column headers: one column per workflow node, each 140px:
      開始
      主管簽核
      財務簽核
      客戶簽核  ← this header has an orange 外部 badge
      業務修改
    Each header is two lines: the node name, and below it a small gray node type.

  Cells: each cell is a three-state control shown as a small pill:
      可編輯 (green background, pencil icon)
      唯讀 (blue background, eye icon)
      隱藏 (gray background, crossed-eye icon)
    Clicking a cell cycles through the three states.
    Cells that are computed/read-only by nature show a lock icon and are
    not clickable, with a lighter appearance.

  Example content to render:
    客戶 row: 開始=可編輯, 主管簽核=唯讀, 財務簽核=唯讀, 客戶簽核=唯讀, 業務修改=可編輯
    毛利率 row: 開始=唯讀, 主管簽核=唯讀, 財務簽核=唯讀, 客戶簽核=隱藏, 業務修改=唯讀
      and this row has a small orange 客戶看不到 badge next to the field name
    內部備註 row: 客戶簽核 column shows 隱藏

  Bulk actions: hovering a column header reveals a small dropdown caret;
  hovering a row reveals one at the row's right edge. Show one open dropdown
  in the design with options: 整欄設為可編輯 / 整欄設為唯讀 / 整欄設為隱藏.

RIGHT DRAWER (380px, slides in when a cell is clicked, shown open in the design):
  Header: 表頭折扣率 · 財務簽核
  Body:
    權限 — three large radio cards side by side: 可編輯, 唯讀, 隱藏 (唯讀 selected)
    進階條件 — a toggle 使用條件式權限, currently ON, revealing:
      an expression builder showing chips: [報價金額] [大於] [1,000,000]
      with helper text 條件成立時套用上方權限，否則使用預設
    套用角色 — checkbox list: 業務 (checked), 主管 (checked), 財務 (checked),
               管理員 (checked, disabled with note 管理員永遠可編輯)
  Footer: 取消, 套用 (primary)

BOTTOM BAR (sticky):
  Left: a change summary 已修改 3 處權限設定
  Right: 還原, 儲存 (primary)

Also design a SECOND VARIANT of this screen in 依節點 mode, where the matrix
transposes: rows become roles (業務, 主管, 財務, 管理員, 外部客戶) and
columns become fields, with the node fixed by the filter bar.
```

---

## 4. 單據套版設計器（Document Template Designer）

```
Design a document template designer for creating printable quotation PDFs.
Screen title: 單據套版設計器

This is a WYSIWYG page-layout editor. The canvas represents an A4 portrait page.

Three-column layout:

LEFT PANEL (260px) — 元素與欄位
  Two tabs at top: 可用欄位 | 元素

  Tab 可用欄位 (default, this is the key differentiator):
    A search box, then a tree of draggable field chips grouped by source:
      報價單表頭: 報價單號, 報價日期, 有效期限, 幣別, 付款條件
      客戶: 客戶名稱, 統一編號, 地址, 聯絡人, 電話
      金額: 小計, 稅額, 總計, 表頭折扣率
      明細表格: (a single special chip labeled 報價明細表 with a table icon,
                and a note 拖入後可設定顯示欄位)
      公司: 公司名稱, 公司地址, 統一編號, Logo
    Each chip shows the Chinese label and a tiny gray data path below it.

  Tab 元素:
    文字, 圖片, 線條, 矩形, 條碼, QR Code, 頁碼

CENTER CANVAS (flexible, dark gray #374151 background) — 版面
  A white A4 page (ratio 210:297) centered, with:
    - A ruler along the top and left edges showing millimeters
    - Dashed blue margin guides inset 15mm from each edge
    - Zoom controls floating at the bottom center: − 100% + and 符合視窗
    - A page indicator at the bottom right: 第 1 頁，共 2 頁

  Content placed on the page representing a Taiwanese quotation:
    Top-left: a company logo placeholder box
    Top-right: 報價單 as a large bold title, below it 報價單號 and 報價日期
               shown as bound field boxes
    A horizontal rule
    Left block: 客戶名稱, 統一編號, 地址, 聯絡人 as label-value pairs
    Right block: 有效期限, 付款條件, 幣別
    Center: a table occupying most of the page with header row
            品名 | 數量 | 單位 | 單價 | 金額
            showing three sample data rows with placeholder values,
            numeric columns right-aligned, header row with gray background
    Below the table, right-aligned: 小計, 稅額, 總計 rows,
            with 總計 in bold and a top border line
    Bottom: a 備註與條款 text block with sample terms
    Footer: page number centered

  Bound field boxes are rendered with a light blue tint and a dashed blue
  border, with the field name shown. Static text has no tint.
  The selected element shows eight resize handles and a blue outline.
  When dragging, red alignment guide lines appear snapping to other elements.

RIGHT PANEL (320px) — 屬性
  Changes based on selection. Show the state where the TABLE is selected:

  Header: 報價明細表 with a table badge

  Section 欄位:
    A sortable list of the table's columns. Each row has a drag handle,
    the column name, a width input, an alignment segmented control
    (靠左 / 置中 / 靠右), and a delete icon.
    Rows shown: 品名 (auto, 靠左), 數量 (90px, 靠右), 單位 (70px, 置中),
                單價 (110px, 靠右), 金額 (120px, 靠右)
    Below: a 「+ 新增欄位」 button opening a dropdown of available line fields.

  Section 表頭:
    字型 (dropdown: 思源黑體 Bold), 字級 (number), 背景色 (color swatch),
    文字色 (color swatch), 對齊 (segmented)

  Section 內容:
    字型, 字級, 隔行變色 (toggle, ON, with a color swatch next to it),
    框線粗細 (number with mm unit), 儲存格內距 (four small inputs in a box shape)

  Section 分頁:
    換頁時重複表頭 (toggle, ON) — with helper text 明細超過一頁時，每頁都顯示欄位名稱
    合計列不分頁 (toggle, ON) with a number input 保留最後 3 列
      and helper text 避免小計、稅額、總計被拆到兩頁

  Section 合計列:
    A small list: 小計, 稅額, 總計 each with a style toggle 粗體 and
    a checkbox 上方加框線 (checked only for 小計)

TOP ACTION BAR:
  Left: breadcrumb 設計器 / 單據 / 報價單, version chip 「v2 草稿」
  Right: a 底稿模式 dropdown showing 標準 (可自動分頁) with an alternate
         option 固定底稿 (上傳 PDF)，
         then 預覽 PDF (with a download icon), 儲存草稿, 發布 (primary)

Also show a PREVIEW MODAL variant: a centered modal 900px wide displaying
a rendered PDF preview with page navigation, a 下載 button, and a note at the
bottom 「以範例資料渲染，實際內容依報價單資料而定」.
```

---

## 5. 追加 Prompt（視需要使用）

產出後若要微調，用這些追加句，不必重寫整份。

| 需求 | 追加句 |
|---|---|
| 畫面太空 | `Increase information density. Reduce vertical padding by half. Show more rows without scrolling.` |
| 中文顯示不佳 | `All text must be Traditional Chinese. Ensure Chinese characters are not clipped at 14px. Use a font stack that includes Noto Sans TC.` |
| 要行動版 | `Add a tablet variant at 1024px width where the right properties panel becomes a bottom sheet.` |
| 要空狀態 | `Add an empty state for this screen: an illustration, a title, one line of guidance, and a primary action button.` |
| 要載入狀態 | `Add a loading skeleton variant for the main content area.` |
| 顏色太鮮豔 | `Desaturate the accent colors. This is an enterprise tool used eight hours a day; avoid visual fatigue.` |

---

## 6. 產出後的對接注意

Stitch 產出的是視覺稿與前端骨架，不是可用程式碼。接進專案時注意：

1. **資料結構以 Schema 為準**。`schemas/form.schema.json` 與 `schemas/workflow-dsl.schema.json` 是唯一真相，Stitch 的欄位命名僅供參考。
2. **流程設計器的畫布不可改成自由拖曳**。決議 D-03 定為固定由上往下自動排版，底層是 Vue Flow 加 dagre。Stitch 若產出自由拖曳版本，只取視覺樣式。
3. **權限矩陣的儲存格狀態**對應 `form.schema.json` 的 `workflow.readonly_when`、`visible_when` 與 `editable_roles`。三態切換是 UI 糖衣，底層仍是表達式。
4. **單據套版設計器**的表格分頁選項對應 `repeatHead` 與 `keepLastRowsTogether`。前者官方已支援，後者需自訂 plugin（任務 2.6d）。
5. Stitch 產出多為 React 或 HTML，本專案是 Vue 3。取版面與樣式，元件需重寫。
