# pdfme 基準版本

table-plus plugin 依賴 pdfme 的公開型別與公開行為。上游改動時以本文件對照。

## 基準

| 項目 | 值 |
|---|---|
| 套件版本 | 實測於 **5.5.10**（package.json 宣告 ^5.4.2，npm 解析到 5.5.10）|
| 參考原始碼 | `reference/pdfme`（唯讀，不進版控） |
| 查證日期 | 2026-09-16 |
| 注意 | `reference/pdfme` 的版本比實際安裝的舊，函式名已不同（`placeUnitsOnPages` → `placeRowsOnPages`）。以 `node_modules` 內的原始碼為準 |

## 依賴的上游行為

以下行為若在上游改變，plugin 會失效且**不會有編譯錯誤**，必須靠渲染測試發現。

| 依賴 | 檔案 | 說明 |
|---|---|---|
| `getDynamicHeights` 回傳「表頭 + 各 body 列」的高度陣列 | `tables/dynamicTemplate.ts` | `showHead` 時第一個元素是表頭高度 |
| 分頁由 `placeUnitsOnPages` 依高度累加決定 | `common/dynamicTemplate.ts:103` | 放不下時**先換頁再放該列**，不是取餘數 |
| 列高只影響版面配置，不影響繪製 | `tables/pdfRender.ts:45` | `drawRow` 用 table 模型的 `row.height` |
| `EPSILON` 用於浮點比較 | `common/dynamicTemplate.ts` | 值須與上游一致，否則會多換一頁 |
| 座標語意：繪製為絕對、分頁為流動 | `common/dynamicTemplate.ts` | 見下方說明 |

**座標語意**是實作中最違反直覺的一點，實測才理清：

- **繪製時是絕對座標**，不會自動避讓。明細變長時合計不會被推開，
  而是直接疊在明細上。
- **分頁時卻是依序流動的**。前面的 schema 佔掉空間後，
  後面的即使 y 值放得下，也會被擠到下一頁——實測時簽章區
  設在 y=262（頁面可容納到 282）仍被推到空白的第二頁。

結論：表格之後的所有 schema，y 都必須相對於明細高度計算。
用固定值會同時踩到版面重疊與多出空白頁兩個問題。

第二項是實作時踩過的坑。初版用 `y % pageContentHeight` 推進，
與框架的實際行為差 7mm，只在明細恰好 49 列時出錯。
測試改為掃過 120 種長度 × 4 種列高 × 3 種版面才抓到。

## 官方已支援，不需自行實作

2026-09-14 clone 原始碼查證，修正先前依 GitHub issues 做出的判斷：

| 能力 | 位置 |
|---|---|
| `repeatHead` 跨頁重複表頭 | `tables/types.ts:29`、`propPanel.ts:27` |
| `columnStyles.alignment` 逐欄對齊 | `tables/types.ts:36-38` |

## 本 plugin 補齊的能力

| 能力 | 狀態 | 實作 |
|---|---|---|
| `keepLastRowsTogether` 最後 n 列不分頁 | **已完成** | `pagination.ts`，包裝 `getDynamicHeights` |
| `rowStyles` 從尾端數的列樣式 | **已完成**（改用拆表） | `summarySplit.ts` |

### rowStyles 為何改用拆表而非分叉

原訂計畫（06 文件 4.2 節）是複製 `tables/` 目錄改 `cellStyles()`。
**實測後放棄。**

原因：那 10 個檔案（2007 行）另外 import 了 11 個 pdfme 內部模組——
`../box.js`、`../constants.js`、`../shapes/line.js`、`../shapes/rectAndEllipse.js`、
`../splitRange.js`、`../text/constants.js`、`../text/helper.js`、
`../text/pdfRender.js`、`../text/types.js`、`../text/uiRender.js`、`../utils.js`。
這些都**不在 `@pdfme/schemas` 的公開匯出**（只有 `pdf`、`ui`、`propPanel`、`icon`）。
等於要 vendor 半個 schemas 套件，升級時全部得重對。

改用的作法：**把一張邏輯表格拆成兩個 table schema**。

```
items            showHead: true，明細樣式
items__summary   showHead: false，合計樣式，只佔右側兩欄
```

兩者都是官方 table，欄寬以同一組百分比推算確保對齊。
不碰任何內部實作，升級時不需重對。

代價是合計成為獨立 schema，位置需由呼叫端依明細高度計算
（見「依賴的上游行為」第五項）。

## 升級程序

1. 更新 `reference/pdfme` 至新版
2. 比對上表「依賴的上游行為」四項是否仍成立
3. 跑 `npm test`（邏輯測試）與渲染回歸測試
4. 更新本文件的基準版本
