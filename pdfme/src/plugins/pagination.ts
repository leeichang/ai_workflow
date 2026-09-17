/**
 * 尾端不分頁的分頁計算
 *
 * 需求：明細跨頁時，小計、稅額、總計三列不可被拆到兩頁。
 * 使用者看到上一頁只有「小計」、下一頁才出現「總計」會誤以為金額有誤。
 *
 * 作法：官方的分頁由 getDynamicHeightsForTable 回傳的「每列高度」驅動，
 * 框架依高度累加決定在哪裡換頁。因此不需要改分頁演算法本身——
 * 只要在保護區塊的第一列放大高度，讓它在當前頁放不下，
 * 框架自然會把整個區塊推到下一頁。
 *
 * 為何不直接改分頁迴圈：
 *   那需要複製 dynamicTemplate.ts 整段邏輯，包含 repeatHead 的處理。
 *   上游修 bug 時我們拿不到。改成「調整輸入」則只依賴公開行為。
 *   見 06_單據套版設計器選型.md 4.2 節「plugin 只依賴公開型別」。
 *
 * 放大列高是否會讓那一列被畫得比較高：
 *   不會。查證 pdfRender.ts 的 drawRow 用的是 table 模型裡的 row.height，
 *   而 heights 只進到 common/dynamicTemplate.ts 決定「這一塊放哪一頁」。
 *   副作用是該頁的表格 schema 高度會比實際內容略高，在表格後方
 *   留下一段空白。由於保護區塊本來就要換頁，那段空白落在頁尾，
 *   視覺上等同於「這一頁提早結束」。
 */

/** 與 common/dynamicTemplate.ts 的 EPSILON 一致，避免浮點誤差造成多換一頁 */
const EPSILON = 1e-9;

export interface PageMetrics {
  /** 每頁可用內容高度（扣除上下邊距） */
  pageContentHeight: number;
  /** 表格起始的 y（相對於頁面內容區頂端） */
  startOffsetY: number;
  /** 重複表頭的高度，不重複時為 0 */
  headerHeight: number;
}

/**
 * 調整列高，使尾端保護區塊不被拆頁
 *
 * heights 為 body 每一列的高度（不含表頭）。
 * 回傳同長度的陣列，其中保護區塊的第一列可能被放大。
 *
 * 放大而非插入空白列：插入會改變列數，而列數是 body 資料的長度，
 * 動到它會讓 __bodyRange 的索引對不上實際資料。
 */
export function inflateForKeepTogether(
  heights: number[],
  keepStartIndex: number | null,
  metrics: PageMetrics,
): number[] {
  if (keepStartIndex === null || keepStartIndex <= 0) return [...heights];
  if (keepStartIndex >= heights.length) return [...heights];

  const { pageContentHeight, startOffsetY, headerHeight } = metrics;
  if (pageContentHeight <= 0) return [...heights];

  const blockHeight = heights
    .slice(keepStartIndex)
    .reduce((sum, h) => sum + h, 0);

  // 保護區塊本身就超過一整頁時放棄保護。硬要不分頁會變成無限迴圈：
  // 推到下一頁仍放不下，再推到下下頁，永遠找不到位置。
  if (blockHeight > pageContentHeight - headerHeight) {
    return [...heights];
  }

  // 模擬累加，找出保護區塊起始列落在該頁的哪個位置。
  //
  // 必須與 common/dynamicTemplate.ts 的 placeRowsOnPages 同樣語意：
  // 放不下時「先換頁再放這一列」，而不是「放完再取餘數」。
  // 初版用 y % pageContentHeight 推進，在明細 49 列時算出的剩餘空間
  // 比實際多 7mm，保護區塊照樣被拆頁——測試掃過 60 種長度才抓到。
  let y = startOffsetY;
  for (let i = 0; i < keepStartIndex; i++) {
    const h = heights[i] ?? 0;
    if (y + h > pageContentHeight + EPSILON) {
      y = headerHeight;
    }
    y += h;
  }

  const remaining = pageContentHeight - y;
  if (blockHeight <= remaining) {
    // 整個區塊放得下，不需調整
    return [...heights];
  }

  // 放不下。把起始列的高度撐到「剛好超過本頁剩餘空間」，
  // 框架就會在它之前換頁，整個區塊隨之移到下一頁。
  const result = [...heights];
  result[keepStartIndex] = remaining + 0.01;
  return result;
}
