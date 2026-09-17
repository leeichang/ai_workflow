/**
 * 報價單渲染
 *
 * 把業務資料變成 PDF bytes。無狀態純函數，不碰資料庫——
 * 租戶隔離由呼叫端保證（見 server.ts 的說明）。
 */

import { generate } from '@pdfme/generator';
import { text, image, line, rectangle, table, barcodes } from '@pdfme/schemas';
import { loadFonts } from './fonts.js';
import { createTableWithSummary } from './plugins/rowStyleRender.js';
import { buildQuotationTemplate } from './template/quotation.js';
import {
  buildInputs,
  buildRows,
  buildSummaryRows,
  type QuotationData,
} from './template/mapper.js';

export const PLUGINS = {
  text,
  image,
  line,
  rectangle,
  qrcode: barcodes.qrcode,
  // 取代官方 table，讓合計列能有自己的樣式（見 rowStyleRender.ts）。
  // 名稱維持 'table'，既有模板不必改 type。
  table: createTableWithSummary() as unknown as typeof table,
};

/**
 * 渲染報價單
 *
 * 明細與合計同在一張 table 內，合計列的樣式由 rowStyleRender.ts
 * 在繪製時分兩段套用。
 */
export async function renderQuotation(data: QuotationData): Promise<Uint8Array> {
  const rows = buildRows(data);
  const font = await loadFonts();

  const inputs = {
    ...buildInputs(data),
    // 明細與合計同一張表。合計列的樣式由 rowStyleRender 在繪製時套用。
    items: JSON.stringify([...rows, ...buildSummaryRows(data)]),
  };

  const template = buildQuotationTemplate();

  return renderPdf(template, [inputs], font);
}

/**
 * 以任意模板渲染
 *
 * 給設計器預覽用。模板來自使用者編輯的內容，因此不假設結構。
 */
export async function renderTemplate(
  template: unknown,
  inputs: Record<string, unknown>[],
): Promise<Uint8Array> {
  const font = await loadFonts();
  return renderPdf(template, inputs, font);
}

/**
 * 產生 PDF
 *
 * 合計列的樣式與「不被拆頁」都由 rowStyleRender.ts 在繪製時處理，
 * 分頁完全交給官方，這裡不做額外干預。
 *
 * 曾經試過的兩條路都失敗，記錄以免重蹈：
 *   包裝 plugin 的 getDynamicHeights —— pdfme 的 Plugin 型別沒有這個
 *     欄位，generate() 內部是寫死的 case 'table'，覆寫不會被呼叫。
 *   先自行 getDynamicTemplate 展開再交給 generate —— 會被二次分頁，
 *     40 列的報價單從 3 頁變 6 頁且合計消失。
 */
async function renderPdf(
  template: unknown,
  inputs: Record<string, unknown>[],
  font: Awaited<ReturnType<typeof loadFonts>>,
): Promise<Uint8Array> {
  return generate({
    template: template as never,
    inputs: inputs as never,
    options: { font },
    plugins: PLUGINS,
  });
}
