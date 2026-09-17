/**
 * 合計列樣式測試
 *
 * 「從尾端數」是這組邏輯的核心。用固定索引的話，
 * 不含稅報價（少一列稅額）會讓粗體套到錯的列上，
 * 而且產生的 PDF 看起來只是「總計沒變粗」，不會有任何錯誤。
 */

import { describe, expect, it } from 'vitest';
import {
  isInKeepTogetherBlock,
  keepTogetherStartIndex,
  resolveRowStyle,
  type RowStyleOverride,
} from '../src/plugins/rowStyles.js';

/** 報價單典型設定：小計上緣加框線，總計粗體 */
const QUOTATION_STYLES: RowStyleOverride[] = [
  { fromEnd: 2, borderTopWidth: 0.5, borderColor: '#000000' }, // 小計
  { fromEnd: 1 }, // 稅額，無特別樣式
  { fromEnd: 0, fontName: 'NotoSansCJKtc-Bold', fontSize: 12 }, // 總計
];

describe('從尾端數的列樣式', () => {
  it('最後一列取 fromEnd 0', () => {
    const style = resolveRowStyle(9, 10, QUOTATION_STYLES);
    expect(style?.fontName).toBe('NotoSansCJKtc-Bold');
    expect(style?.fontSize).toBe(12);
  });

  it('倒數第三列取 fromEnd 2', () => {
    const style = resolveRowStyle(7, 10, QUOTATION_STYLES);
    expect(style?.borderTopWidth).toBe(0.5);
  });

  it('明細列沒有樣式', () => {
    expect(resolveRowStyle(0, 10, QUOTATION_STYLES)).toBeNull();
    expect(resolveRowStyle(6, 10, QUOTATION_STYLES)).toBeNull();
  });

  it('明細行數改變時樣式跟著移動', () => {
    // 同一組設定，5 列與 100 列的表格，總計都該是最後一列
    for (const length of [5, 20, 100]) {
      const style = resolveRowStyle(length - 1, length, QUOTATION_STYLES);
      expect(style?.fontName, `${length} 列時總計應為粗體`).toBe('NotoSansCJKtc-Bold');
    }
  });

  it('少一列合計時樣式不會錯位', () => {
    // 不含稅報價：只有小計與總計兩列。
    // 用固定索引的話「總計」會套到「小計」的樣式。
    const noTax: RowStyleOverride[] = [
      { fromEnd: 1, borderTopWidth: 0.5 },
      { fromEnd: 0, fontName: 'NotoSansCJKtc-Bold' },
    ];

    expect(resolveRowStyle(9, 10, noTax)?.fontName).toBe('NotoSansCJKtc-Bold');
    expect(resolveRowStyle(8, 10, noTax)?.borderTopWidth).toBe(0.5);
  });
});

describe('邊界', () => {
  it('沒有設定時回 null', () => {
    expect(resolveRowStyle(0, 10, undefined)).toBeNull();
    expect(resolveRowStyle(0, 10, [])).toBeNull();
  });

  it('索引超出範圍時回 null', () => {
    expect(resolveRowStyle(10, 10, QUOTATION_STYLES)).toBeNull();
  });

  it('空表格不崩潰', () => {
    expect(resolveRowStyle(0, 0, QUOTATION_STYLES)).toBeNull();
  });

  it('fromEnd 超過表格長度時不套用', () => {
    // 只有兩列的表格，fromEnd: 2 指向不存在的位置
    const style = resolveRowStyle(0, 2, [{ fromEnd: 2, fontSize: 99 }]);
    expect(style).toBeNull();
  });
});

describe('多個設定指向同一列', () => {
  it('後宣告的覆蓋先宣告的', () => {
    const style = resolveRowStyle(0, 1, [
      { fromEnd: 0, fontSize: 10, fontColor: '#111111' },
      { fromEnd: 0, fontSize: 14 },
    ]);

    expect(style?.fontSize).toBe(14);
    expect(style?.fontColor, '未覆蓋的鍵應保留').toBe('#111111');
  });

  it('undefined 不會蓋掉已設定的值', () => {
    // 「只想加粗體」的設定不該把字級清掉
    const style = resolveRowStyle(0, 1, [
      { fromEnd: 0, fontSize: 12 },
      { fromEnd: 0, fontName: 'Bold', fontSize: undefined },
    ]);

    expect(style?.fontSize).toBe(12);
    expect(style?.fontName).toBe('Bold');
  });
});

describe('尾端不分頁的區塊判斷', () => {
  it('最後三列屬於保護區塊', () => {
    for (const i of [7, 8, 9]) {
      expect(isInKeepTogetherBlock(i, 10, 3), `第 ${i} 列`).toBe(true);
    }
  });

  it('明細列不屬於保護區塊', () => {
    for (const i of [0, 5, 6]) {
      expect(isInKeepTogetherBlock(i, 10, 3), `第 ${i} 列`).toBe(false);
    }
  });

  it('未啟用時全部為否', () => {
    expect(isInKeepTogetherBlock(9, 10, 0)).toBe(false);
    expect(isInKeepTogetherBlock(9, 10, undefined)).toBe(false);
  });

  it('起始索引', () => {
    expect(keepTogetherStartIndex(10, 3)).toBe(7);
    expect(keepTogetherStartIndex(3, 3), '涵蓋整個表格時無需保護').toBeNull();
    expect(keepTogetherStartIndex(2, 3), '保護範圍大於表格時無需保護').toBeNull();
    expect(keepTogetherStartIndex(10, 0)).toBeNull();
  });
});
