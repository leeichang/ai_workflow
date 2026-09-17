/**
 * 合計列樣式：從尾端數的列樣式
 *
 * 官方 table 只支援 alternateRowStyles（隔行變色），沒有「指定第 n 列」
 * 的能力。報價單的小計、稅額、總計是明細表的最後三列，需要粗體與上緣框線。
 *
 * 為何以「從尾端數」而非固定索引：
 *   明細行數不固定，合計列的絕對位置會浮動。
 *   更關鍵的是不含稅報價時「稅額」那一列不會出現，
 *   固定索引會讓樣式套到錯的列上。
 *
 * 為何合計列放在表格內而非獨立的 text schema：
 *   獨立擺放會在明細跨頁時錯位——明細長度改變，合計的 y 座標就得跟著算，
 *   而設計時無從得知實際行數。放在表格內由分頁邏輯一併處理。
 *   見 docs/系統規劃/06_單據套版設計器選型.md 決議紀錄。
 */

export interface RowStyleOverride {
  /** 從尾端數的位置，0 為最後一列 */
  fromEnd: number;
  fontName?: string;
  fontSize?: number;
  fontColor?: string;
  backgroundColor?: string;
  /** 上緣框線寬度（mm）。小計列用它與明細分隔。 */
  borderTopWidth?: number;
  borderColor?: string;
}

export interface ResolvedRowStyle {
  fontName?: string;
  fontSize?: number;
  fontColor?: string;
  backgroundColor?: string;
  borderTopWidth?: number;
  borderColor?: string;
}

/**
 * 解析某一列的樣式
 *
 * rowIndex 為 body 內的索引（不含表頭），bodyLength 為 body 總列數。
 * 多個 override 指向同一列時，後宣告的覆蓋先宣告的——與 CSS 一致，
 * 讓使用者能先設通則再設特例。
 */
export function resolveRowStyle(
  rowIndex: number,
  bodyLength: number,
  overrides: RowStyleOverride[] | undefined,
): ResolvedRowStyle | null {
  if (!overrides || overrides.length === 0) return null;

  const fromEnd = bodyLength - 1 - rowIndex;
  if (fromEnd < 0) return null;

  let result: ResolvedRowStyle | null = null;

  for (const o of overrides) {
    if (o.fromEnd !== fromEnd) continue;

    const { fromEnd: _ignored, ...style } = o;
    // 只覆蓋有定義的鍵。undefined 會蓋掉底層樣式，
    // 讓「只想加粗體」的設定意外清掉字級與顏色。
    result = { ...(result ?? {}) };
    for (const [k, v] of Object.entries(style)) {
      if (v !== undefined) {
        (result as Record<string, unknown>)[k] = v;
      }
    }
  }

  return result;
}

/**
 * 判斷某一列是否屬於「不可分頁」的尾端區塊
 *
 * keepLastRowsTogether = 3 代表最後三列必須同頁。
 */
export function isInKeepTogetherBlock(
  rowIndex: number,
  bodyLength: number,
  keepLastRowsTogether: number | undefined,
): boolean {
  if (!keepLastRowsTogether || keepLastRowsTogether <= 0) return false;
  return rowIndex >= bodyLength - keepLastRowsTogether;
}

/**
 * 尾端保護區塊的起始列索引
 *
 * 回傳 null 代表沒有保護區塊，或保護範圍涵蓋整個 body
 * （此時無所謂「在它之前分頁」）。
 */
export function keepTogetherStartIndex(
  bodyLength: number,
  keepLastRowsTogether: number | undefined,
): number | null {
  if (!keepLastRowsTogether || keepLastRowsTogether <= 0) return null;
  if (keepLastRowsTogether >= bodyLength) return null;
  return bodyLength - keepLastRowsTogether;
}
