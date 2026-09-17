/**
 * 合計列樣式：在同一張表格內讓最後幾列有自己的樣式
 *
 * 官方 table 的 cellStyles() 只讀 head / body / alternate / column
 * 四個樣式來源，沒有以列索引為鍵的切點（alternateRowStyles 也只是
 * `rowIndex % 2` 的隔行變色）。
 *
 * 前一版的作法是把合計拆成獨立的第二張 table。可行，但合計成為
 * 獨立 schema 後位置必須預先算出，而真實位置只有排版後才知道——
 * 排版又受它自己影響，形成循環相依。實測 40 列時合計不是壓在明細上，
 * 就是被推到多出來的空白頁。
 *
 * 改用的作法：**包裝 table.pdf，把同一頁的表格分兩段繪製。**
 *   第一段 明細列，用原本的 bodyStyles
 *   第二段 合計列，用覆蓋後的 bodyStyles，緊接在第一段下方
 *
 * 關鍵是 pdfme 已經在 schema.__bodyRange 告訴我們「這一頁涵蓋哪幾列」，
 * 因此不需要自己算分頁——只需在收到的那一頁範圍內判斷是否含合計列。
 * 分頁仍完全由官方處理。
 */

import type { Plugin } from '@pdfme/common';
import { table as officialTable, getDynamicHeightsForTable } from '@pdfme/schemas';
import type { RowStyleOverride } from './rowStyles.js';

/** 本頁的列範圍。pdfme 在跨頁時寫入。 */
interface BodyRange {
  start: number;
  end: number;
}

interface TableRenderSchema {
  name: string;
  position: { x: number; y: number };
  showHead?: boolean;
  bodyStyles?: Record<string, unknown>;
  headStyles?: Record<string, unknown>;
  __bodyRange?: BodyRange;
  /** 尾端幾列套用 summaryStyles */
  summaryRowCount?: number;
  /** 合計列的樣式覆蓋 */
  summaryStyles?: Record<string, unknown>;
  /** 合計區塊上緣的框線 */
  summaryTopBorder?: { width: number; color: string };
}

function parseRows(value: unknown): string[][] {
  if (typeof value !== 'string' || !value) return [];
  try {
    const parsed: unknown = JSON.parse(value);
    return Array.isArray(parsed) ? (parsed as string[][]) : [];
  } catch {
    // 值壞掉時當成空表格。單據少了明細仍看得出是哪一張單，
    // 整份 PDF 產不出來則什麼都看不到。
    return [];
  }
}

/**
 * 建立支援合計列樣式的 table plugin
 *
 * 不改分頁，不碰官方內部實作，只在繪製時把一頁拆成兩段。
 */
export function createTableWithSummary(): Plugin<never> {
  const base = officialTable as unknown as {
    pdf: (arg: unknown) => Promise<void>;
    ui: unknown;
    propPanel: unknown;
    icon?: string;
  };

  return {
    ...base,
    pdf: async (arg: unknown) => {
      const a = arg as { schema: TableRenderSchema; value: unknown };
      const schema = a.schema;
      const summaryCount = schema.summaryRowCount ?? 0;

      if (summaryCount <= 0) return base.pdf(arg);

      const allRows = parseRows(a.value);
      if (allRows.length === 0) return base.pdf(arg);

      const range = schema.__bodyRange ?? { start: 0, end: allRows.length };
      const pageRows = allRows.slice(range.start, range.end);
      if (pageRows.length === 0) return base.pdf(arg);

      // 合計列在整份資料中的起始索引
      const summaryStart = allRows.length - summaryCount;

      // 本頁全是明細，照原樣畫
      if (range.start + pageRows.length <= summaryStart) return base.pdf(arg);

      const detailCount = Math.max(0, summaryStart - range.start);
      const detailRows = pageRows.slice(0, detailCount);
      const summaryRows = pageRows.slice(detailCount);

      // 明細段實際佔多高，用官方的高度計算問出來，不自行估算。
      // 初版以字級推算，遇到含規格說明的兩行列就會少算，
      // 合計整塊往上壓在最後一筆明細上（實測可見文字疊字）。
      const detailSchema = { ...schema, __bodyRange: undefined };
      let offsetY = 0;

      if (detailRows.length > 0) {
        await base.pdf({
          ...(arg as object),
          value: JSON.stringify(detailRows),
          // 清掉 __bodyRange，否則官方會再依它切一次
          schema: detailSchema,
        });

        offsetY = await measureTableHeight(arg, detailSchema, detailRows);
      } else if (schema.showHead) {
        offsetY = await measureTableHeight(arg, detailSchema, []);
      }

      const summaryStyles: Record<string, unknown> = {
        ...schema.bodyStyles,
        ...schema.summaryStyles,
        // 合計列不套隔行變色，否則小計與總計底色不同看起來像分組
        alternateBackgroundColor: '',
      };

      if (schema.summaryTopBorder) {
        summaryStyles.borderWidth = {
          ...(schema.bodyStyles?.borderWidth as Record<string, number>),
          top: schema.summaryTopBorder.width,
        };
        summaryStyles.borderColor = schema.summaryTopBorder.color;
      }

      await base.pdf({
        ...(arg as object),
        value: JSON.stringify(summaryRows),
        schema: {
          ...schema,
          __bodyRange: undefined,
          showHead: false,
          position: { ...schema.position, y: schema.position.y + offsetY },
          bodyStyles: summaryStyles,
        },
      });
    },
  } as unknown as Plugin<never>;
}

/**
 * 問出一段表格實際佔多高
 *
 * 用官方的 getDynamicHeightsForTable，它回傳「表頭 + 每列」的高度，
 * 加總即為該段表格的總高。自行以字級估算會在多行儲存格上失準——
 * 規格說明造成的兩行列會被算成一行，合計因而疊在明細上。
 */
async function measureTableHeight(
  arg: unknown,
  schema: TableRenderSchema,
  rows: string[][],
): Promise<number> {
  const a = arg as {
    basePdf: unknown;
    options: unknown;
    _cache: Map<string | number, unknown>;
  };

  const heights = await getDynamicHeightsForTable(JSON.stringify(rows), {
    schema: schema as never,
    basePdf: a.basePdf as never,
    options: a.options as never,
    _cache: a._cache,
  });

  return heights.reduce((sum, h) => sum + h, 0);
}

/** 由 rowStyles 設定推出合計段的樣式覆蓋 */
export function summaryStylesFrom(
  overrides: RowStyleOverride[] | undefined,
): Record<string, unknown> {
  if (!overrides || overrides.length === 0) return {};

  // 取 fromEnd 0（最後一列，通常是總計）的字型設定套用於整段。
  // 拆兩段後整段共用一份樣式，無法逐列不同——實務上小計與稅額
  // 樣式相同，只有總計再加粗，取最顯著的那一組即可。
  const last = overrides
    .filter((o) => o.fromEnd === 0)
    .reduce<RowStyleOverride | undefined>((acc, o) => ({ ...(acc ?? {}), ...o }), undefined);

  const styles: Record<string, unknown> = {};
  if (last?.fontName) styles.fontName = last.fontName;
  if (last?.fontSize) styles.fontSize = last.fontSize;
  if (last?.fontColor) styles.fontColor = last.fontColor;
  if (last?.backgroundColor !== undefined) styles.backgroundColor = last.backgroundColor;

  return styles;
}
