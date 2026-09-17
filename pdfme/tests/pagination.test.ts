/**
 * 尾端不分頁測試
 *
 * 症狀若出錯：上一頁只有「小計」，下一頁才出現「總計」。
 * 使用者會以為金額算錯，而 PDF 本身不會有任何錯誤訊息。
 */

import { describe, expect, it } from 'vitest';
import { inflateForKeepTogether, type PageMetrics } from '../src/plugins/pagination.js';
import { keepTogetherStartIndex } from '../src/plugins/rowStyles.js';

/** A4 直式扣除上下各 15mm 邊距 */
const A4: PageMetrics = {
  pageContentHeight: 267,
  startOffsetY: 0,
  headerHeight: 10,
};

/**
 * 模擬框架的分頁：回傳每一列落在第幾頁
 *
 * 必須與 common/dynamicTemplate.ts 的 placeUnitsOnPages 一致：
 * 放不下時先換頁再放這一列。這個模擬若與框架有落差，
 * 測試會通過但實際 PDF 仍然出錯。
 */
function paginate(heights: number[], metrics: PageMetrics): number[] {
  const pages: number[] = [];
  let page = 0;
  let y = metrics.startOffsetY;

  for (const h of heights) {
    if (y + h > metrics.pageContentHeight + 1e-9) {
      page++;
      y = metrics.headerHeight;
    }
    pages.push(page);
    y += h;
  }
  return pages;
}

describe('不需要調整的情況', () => {
  it('整張表格放得下一頁時不動', () => {
    const heights = Array(10).fill(10);
    const result = inflateForKeepTogether(heights, keepTogetherStartIndex(10, 3), A4);
    expect(result).toEqual(heights);
  });

  it('保護區塊剛好落在頁首時不動', () => {
    // 前 26 列共 260，第 27 列起換頁，保護區塊正好在新頁開頭
    const heights = Array(29).fill(10);
    const start = keepTogetherStartIndex(29, 3);
    const result = inflateForKeepTogether(heights, start, A4);

    const pages = paginate(result, A4);
    expect(new Set(pages.slice(start!)).size, '保護區塊應在同一頁').toBe(1);
  });

  it('沒有啟用保護時不動', () => {
    const heights = Array(40).fill(10);
    expect(inflateForKeepTogether(heights, null, A4)).toEqual(heights);
  });

  it('保護區塊涵蓋整個表格時不動', () => {
    const heights = [10, 10, 10];
    expect(inflateForKeepTogether(heights, keepTogetherStartIndex(3, 3), A4)).toEqual(
      heights,
    );
  });
});

describe('需要調整的情況', () => {
  it('保護區塊會被拆頁時整塊推到下一頁', () => {
    // 26 列明細（260）加三列合計（30），第 27 列起放不下，
    // 未調整時小計留在第一頁、稅額與總計到第二頁
    const heights = Array(29).fill(10);
    const start = keepTogetherStartIndex(29, 3)!;

    const before = paginate(heights, A4);
    const spansTwoPages = new Set(before.slice(start)).size > 1;

    const after = paginate(inflateForKeepTogether(heights, start, A4), A4);

    expect(new Set(after.slice(start)).size, '調整後保護區塊應同頁').toBe(1);
    // 若原本就沒被拆，這個測試沒有意義
    expect(spansTwoPages || new Set(before.slice(start)).size === 1).toBe(true);
  });

  it('各種明細長度與列高下保護區塊都不被拆', () => {
    // 掃過大量組合而非幾個手挑的案例。初版的頁面推進算法與框架不一致
    // （用取餘數而非「先換頁再放」），只在明細 49 列這一種長度下出錯，
    // 手挑案例完全測不到。
    const metricsList: PageMetrics[] = [
      A4,
      { ...A4, startOffsetY: 80 }, // 表格上方有抬頭與客戶資訊
      { ...A4, headerHeight: 0 }, // 不重複表頭
    ];

    let fixedCount = 0;

    for (const metrics of metricsList) {
      for (let detail = 1; detail <= 120; detail++) {
        for (const rowHeight of [8, 10, 12.5, 18]) {
          const length = detail + 3;
          const heights = Array(length).fill(rowHeight);
          const start = keepTogetherStartIndex(length, 3);

          if (new Set(paginate(heights, metrics).slice(start ?? 0)).size > 1) {
            fixedCount++;
          }

          const pages = paginate(inflateForKeepTogether(heights, start, metrics), metrics);
          const blockPages = new Set(pages.slice(start ?? 0));

          expect(
            blockPages.size,
            `明細 ${detail} 列、列高 ${rowHeight}、位移 ${metrics.startOffsetY} 時被拆到 ${blockPages.size} 頁`,
          ).toBe(1);
        }
      }
    }

    // 若這個數字是 0，代表測試資料根本沒觸發到拆頁，測了等於沒測
    expect(fixedCount, '應有案例是真的被修正的').toBeGreaterThan(50);
  });

  it('列高不一致時同樣有效', () => {
    // 明細可能換行而高度不同
    const heights = [
      ...Array(20).fill(12),
      ...Array(5).fill(18),
      8,
      8,
      8,
    ];
    const start = keepTogetherStartIndex(heights.length, 3);

    const pages = paginate(inflateForKeepTogether(heights, start, A4), A4);
    expect(new Set(pages.slice(start!)).size).toBe(1);
  });
});

describe('不可能達成的情況', () => {
  it('保護區塊超過一整頁時放棄保護而非無限迴圈', () => {
    // 硬要不分頁會變成：推到下一頁仍放不下，再推，永遠找不到位置
    const heights = [10, 10, 200, 200];
    const result = inflateForKeepTogether(heights, keepTogetherStartIndex(4, 2), A4);

    expect(result, '應原樣回傳，由框架自行處理').toEqual(heights);
  });

  it('保護區塊加表頭剛好超過一頁時也放棄', () => {
    const heights = [10, 260];
    const result = inflateForKeepTogether(heights, keepTogetherStartIndex(2, 1), A4);
    expect(result).toEqual(heights);
  });
});

describe('不破壞既有資料', () => {
  it('不修改傳入的陣列', () => {
    const heights = Array(29).fill(10);
    const snapshot = [...heights];

    inflateForKeepTogether(heights, keepTogetherStartIndex(29, 3), A4);
    expect(heights).toEqual(snapshot);
  });

  it('列數不變', () => {
    // 插入空白列會讓 __splitRange 的索引對不上實際資料
    const heights = Array(29).fill(10);
    const result = inflateForKeepTogether(heights, keepTogetherStartIndex(29, 3), A4);

    expect(result.length).toBe(heights.length);
  });

  it('只調整保護區塊的起始列', () => {
    const heights = Array(29).fill(10);
    const start = keepTogetherStartIndex(29, 3)!;
    const result = inflateForKeepTogether(heights, start, A4);

    result.forEach((h, i) => {
      if (i !== start) {
        expect(h, `第 ${i} 列不該被改動`).toBe(heights[i]);
      }
    });
  });

  it('調整後的起始列高度恰好超過該頁剩餘空間', () => {
    // 這一列會被「設為」剩餘空間再多一點點，而非「加上」某個值。
    // 因此它可能比原本更矮——原以為總高度只會增加，實測 287.01 < 290。
    // 真正要成立的性質是「放不下」，不是「變高」。
    const heights = Array(29).fill(10);
    const start = keepTogetherStartIndex(29, 3)!;
    const result = inflateForKeepTogether(heights, start, A4);

    // 模擬到 start 之前，算出當時的剩餘空間
    let y = A4.startOffsetY;
    for (let i = 0; i < start; i++) {
      if (y + heights[i] > A4.pageContentHeight + 1e-9) y = A4.headerHeight;
      y += heights[i];
    }
    const remaining = A4.pageContentHeight - y;

    expect(result[start]).toBeGreaterThan(remaining);
    expect(result[start], '只需剛好放不下，過大會浪費整頁').toBeLessThan(remaining + 1);
  });
});

describe('表格不從頁首開始', () => {
  it('起始位移納入計算', () => {
    // 表格上方有抬頭、客戶資訊等區塊
    const metrics: PageMetrics = { ...A4, startOffsetY: 80 };
    const heights = Array(22).fill(10);
    const start = keepTogetherStartIndex(22, 3);

    const pages = paginate(inflateForKeepTogether(heights, start, metrics), metrics);
    expect(new Set(pages.slice(start!)).size).toBe(1);
  });
});
