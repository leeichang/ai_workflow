/**
 * 報價單模板
 *
 * 依設計稿 docs/UI 設計/.../pdf/code.html 的 PDF 預覽實作。
 *
 * 模板以程式產生而非純 JSON，因為欄寬、合計樣式等需要由同一組
 * 常數推導，手寫 JSON 容易讓兩處不一致。
 *
 * 設計器存的仍是 JSON（使用者可調的部分），這裡負責把它展開成
 * pdfme 吃得下的形式。
 */

import type { RowStyleOverride } from '../plugins/rowStyles.js';
import { summaryStylesFrom } from '../plugins/rowStyleRender.js';
import { FONT_BOLD, FONT_REGULAR } from '../fonts.js';

const PAGE = { width: 210, height: 297, padding: [15, 15, 15, 15] as number[] };
const CONTENT_WIDTH = PAGE.width - PAGE.padding[1] - PAGE.padding[3];
const LEFT = PAGE.padding[3];

const INK = '#1E293B';
const MUTED = '#64748B';
const BORDER = '#CBD5E1';
const HEAD_BG = '#F1F5F9';

/** 明細表欄位。與設計稿 _9 右側「欄位設定」一致。 */
export const ITEM_COLUMNS = ['品名與規格', '數量', '單位', '單價(NT$)', '金額(NT$)'];
const COLUMN_PERCENTAGES = [46, 11, 9, 16, 18];
const COLUMN_ALIGNMENT: Record<number, string> = {
  0: 'left',
  1: 'right',
  2: 'center',
  3: 'right',
  4: 'right',
};

/**
 * 合計表只佔右側兩欄（單價欄與金額欄）
 *
 * 寬度以明細表的欄寬百分比推算，確保「金額」欄右緣與明細表對齊——
 * 對不齊的話兩張表看起來像是不相干的兩個區塊。
 */
const SUMMARY_SPAN = COLUMN_PERCENTAGES[3] + COLUMN_PERCENTAGES[4];
const SUMMARY_WIDTH = (CONTENT_WIDTH * SUMMARY_SPAN) / 100;
const SUMMARY_LEFT = LEFT + CONTENT_WIDTH - SUMMARY_WIDTH;
/**
 * 合計表與明細表之間的間距
 *
 * pdfme 的座標語意比想像中微妙，實測後才理清：
 *
 *   繪製時：位置是絕對座標，**不會**自動避讓。明細變長時合計不會
 *           被推開，而是直接疊在明細上（實測看到合計壓在第二、三列）。
 *   分頁時：卻又是**依序流動**的。前面的 schema 佔掉空間後，
 *           後面的即使 y 值放得下，也會被擠到下一頁。
 *
 * 兩者合起來的結論是：表格之後的所有 schema，y 都必須相對於
 * 明細高度計算。用固定值會同時踩到兩個問題——版面重疊，
 * 以及最後一個區塊被推到空白的第二頁。
 */
/**
 * 頁尾區塊的固定起點
 *
 * 明細為單一表格時，pdfme 的流動式分頁會自動把頁尾往下推，
 * 因此這裡給的是「最短情況」的位置，不需要依明細高度計算。
 */
const FOOTER_Y = 150;

const SUMMARY_PERCENTAGES = [
  (COLUMN_PERCENTAGES[3] / SUMMARY_SPAN) * 100,
  (COLUMN_PERCENTAGES[4] / SUMMARY_SPAN) * 100,
];

/** 合計列樣式：小計上緣加線，總計粗體放大 */
export const SUMMARY_ROW_STYLES: RowStyleOverride[] = [
  { fromEnd: 2, borderTopWidth: 0.4, borderColor: INK },
  { fromEnd: 0, fontName: FONT_BOLD, fontSize: 11 },
];

function cellStyle(fontName: string, fontSize: number, extra: Record<string, unknown> = {}) {
  return {
    fontName,
    fontSize,
    alignment: 'left',
    verticalAlignment: 'middle',
    lineHeight: 1.35,
    characterSpacing: 0,
    fontColor: INK,
    backgroundColor: '',
    borderColor: BORDER,
    borderWidth: { top: 0.1, right: 0.1, bottom: 0.1, left: 0.1 },
    padding: { top: 2, right: 3, bottom: 2, left: 3 },
    ...extra,
  };
}

export interface QuotationTemplateOptions {
  /** 明細表起始 y。上方的抬頭與客戶區塊高度固定。 */
  tableY?: number;
  /** 合計列數。預設 3（小計、稅額、總計）。 */
  summaryRowCount?: number;
  /** 最後 n 列不分頁。預設同 summaryRowCount。 */
  keepLastRowsTogether?: number;
}

/**
 * 產生報價單模板
 *
 * 明細與合計同在 `items` 一張表內，合計列的樣式由 rowStyleRender.ts
 * 在繪製時套用。其餘為靜態或單值欄位。
 */
export function buildQuotationTemplate(options: QuotationTemplateOptions = {}) {
  const tableY = options.tableY ?? 96;

  const summaryRowCount = options.summaryRowCount ?? 3;
  const keep = options.keepLastRowsTogether ?? summaryRowCount;

  const detailBodyStyles = cellStyle(FONT_REGULAR, 9, {
    alternateBackgroundColor: '#FAFAFA',
  });

  // 合計區塊上緣的分隔線：取 rowStyles 中 fromEnd 最大者（小計列）
  const withBorder = SUMMARY_ROW_STYLES.filter((r) => r.borderTopWidth);
  const topBorder = withBorder.length
    ? {
        width: withBorder.reduce((a, b) => (a.fromEnd >= b.fromEnd ? a : b)).borderTopWidth!,
        color: withBorder.reduce((a, b) => (a.fromEnd >= b.fromEnd ? a : b)).borderColor ?? INK,
      }
    : null;

  const schemas = [
    // ── 抬頭 ──────────────────────────────────────────
    {
      // y 必須在上邊距之內。放在 padding 之上（例如 y=10，而 padding 為 15）
      // 會讓 pdfme 算出負的頁索引，在 placeRowsOnPages 崩潰於
      // `pages[currentPageIndex].push`，錯誤訊息是
      // 「Cannot read properties of undefined (reading 'push')」，
      // 完全看不出與座標有關。
      name: 'form_identifier',
      type: 'text',
      content: '',
      position: { x: LEFT, y: PAGE.padding[0] },
      width: CONTENT_WIDTH,
      height: 4,
      fontName: FONT_REGULAR,
      fontSize: 6.5,
      alignment: 'right',
      fontColor: MUTED,
    },
    {
      name: 'company_name',
      type: 'text',
      content: '',
      position: { x: LEFT, y: 21 },
      width: 110,
      height: 7,
      fontName: FONT_BOLD,
      fontSize: 13,
      fontColor: INK,
    },
    {
      name: 'company_info',
      type: 'text',
      content: '',
      position: { x: LEFT, y: 29 },
      width: 110,
      height: 12,
      fontName: FONT_REGULAR,
      fontSize: 7.5,
      lineHeight: 1.45,
      fontColor: MUTED,
    },
    {
      name: 'doc_title',
      type: 'text',
      content: '報 價 單',
      position: { x: 140, y: 21 },
      width: 55,
      height: 10,
      fontName: FONT_BOLD,
      fontSize: 20,
      alignment: 'right',
      fontColor: INK,
    },
    {
      name: 'doc_title_en',
      type: 'text',
      content: 'QUOTATION',
      position: { x: 140, y: 32 },
      width: 55,
      height: 4,
      fontName: FONT_REGULAR,
      fontSize: 8,
      alignment: 'right',
      characterSpacing: 1.5,
      fontColor: MUTED,
    },
    {
      name: 'doc_meta',
      type: 'text',
      content: '',
      position: { x: 115, y: 38 },
      width: 80,
      height: 14,
      fontName: FONT_REGULAR,
      fontSize: 8.5,
      alignment: 'right',
      lineHeight: 1.5,
      fontColor: INK,
    },
    {
      name: 'header_rule',
      type: 'line',
      position: { x: LEFT, y: 50 },
      width: CONTENT_WIDTH,
      height: 0.4,
      color: INK,
    },

    // ── 買賣雙方 ───────────────────────────────────────
    {
      name: 'customer_block',
      type: 'text',
      content: '',
      position: { x: LEFT, y: 55 },
      width: 95,
      height: 34,
      fontName: FONT_REGULAR,
      fontSize: 8.5,
      lineHeight: 1.6,
      fontColor: INK,
    },
    {
      name: 'terms_block',
      type: 'text',
      content: '',
      position: { x: 112, y: 55 },
      width: 83,
      height: 34,
      fontName: FONT_REGULAR,
      fontSize: 8.5,
      lineHeight: 1.6,
      fontColor: INK,
    },

    // ── 明細表 ────────────────────────────────────────
    {
      name: 'items',
      type: 'table',
      position: { x: LEFT, y: tableY },
      width: CONTENT_WIDTH,
      height: 30,
      showHead: true,
      // 官方支援。明細跨頁時每頁重複欄位名稱。
      repeatHead: true,
      // 合計列不被拆頁，由 rowStyleRender.ts 在繪製時保證。
      keepLastRowsTogether: keep,
      head: ITEM_COLUMNS,
      headWidthPercentages: COLUMN_PERCENTAGES,
      content: '[]',
      tableStyles: { borderColor: BORDER, borderWidth: 0.1 },
      headStyles: cellStyle(FONT_BOLD, 8.5, {
        alignment: 'center',
        backgroundColor: HEAD_BG,
      }),
      bodyStyles: detailBodyStyles,
      columnStyles: { alignment: COLUMN_ALIGNMENT },

      // 合計列留在同一張表內。拆成第二張 table 會讓它的位置
      // 必須預先算出，而真實位置要等排版後才知道——循環相依。
      // 由 rowStyleRender.ts 在繪製時分兩段處理。
      summaryRowCount: summaryRowCount,
      summaryStyles: summaryStylesFrom(SUMMARY_ROW_STYLES),
      summaryTopBorder: topBorder,
    },

    // ── 頁尾 ──────────────────────────────────────────
    {
      name: 'amount_in_words',
      type: 'text',
      content: '',
      position: { x: LEFT, y: FOOTER_Y },
      width: CONTENT_WIDTH,
      height: 8,
      fontName: FONT_BOLD,
      fontSize: 10,
      fontColor: INK,
    },
    {
      name: 'terms',
      type: 'text',
      content: '',
      position: { x: LEFT, y: FOOTER_Y + 10 },
      width: CONTENT_WIDTH,
      height: 22,
      fontName: FONT_REGULAR,
      fontSize: 7.5,
      lineHeight: 1.6,
      fontColor: '#374151',
    },
    {
      name: 'signature_customer',
      type: 'text',
      content: '客戶簽章確認 (Customer Signature & Stamp)：',
      position: { x: LEFT, y: FOOTER_Y + 38 },
      width: 88,
      height: 6,
      fontName: FONT_REGULAR,
      fontSize: 8,
      fontColor: INK,
    },
    {
      name: 'signature_sales',
      type: 'text',
      content: '業務代表 / 廠務核准 (Approved)：',
      position: { x: 107, y: FOOTER_Y + 38 },
      width: 88,
      height: 6,
      fontName: FONT_REGULAR,
      fontSize: 8,
      fontColor: INK,
    },
  ];

  return {
    basePdf: { ...PAGE },
    schemas: [schemas],
  };
}
