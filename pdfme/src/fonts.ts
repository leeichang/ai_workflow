/**
 * CJK 字型註冊
 *
 * pdfme 預設的 Roboto 不含中文。未註冊時產生的 PDF 是豆腐字，
 * 而且**不會有任何錯誤**——設計時在瀏覽器看得到中文（瀏覽器有系統字型），
 * 產生的 PDF 卻是一排方框。
 *
 * 字型檔約 16MB 一個，共兩個。載入策略：
 *   Generator（本服務）常駐記憶體，啟動時載入一次
 *   Designer 端延遲載入，避免拖慢首屏
 */

import { readFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import type { Font } from '@pdfme/common';

const HERE = dirname(fileURLToPath(import.meta.url));
const FONT_DIR = join(HERE, '..', 'assets', 'fonts');

export const FONT_REGULAR = 'NotoSansCJKtc-Regular';
export const FONT_BOLD = 'NotoSansCJKtc-Bold';

let cached: Font | null = null;

/**
 * 載入字型
 *
 * 快取於模組層級。每次渲染都讀 32MB 的檔案會讓吞吐量掉到不可用。
 */
export async function loadFonts(): Promise<Font> {
  if (cached) return cached;

  const [regular, bold] = await Promise.all([
    readFile(join(FONT_DIR, `${FONT_REGULAR}.otf`)),
    readFile(join(FONT_DIR, `${FONT_BOLD}.otf`)),
  ]);

  cached = {
    [FONT_REGULAR]: { data: regular, fallback: true },
    [FONT_BOLD]: { data: bold },
  };

  return cached;
}

/**
 * 檢查字型檔是否存在
 *
 * 啟動時呼叫。缺字型就讓服務起不來，而不是等到第一張報價單
 * 印出豆腐字才發現。
 */
export async function assertFontsAvailable(): Promise<void> {
  for (const name of [FONT_REGULAR, FONT_BOLD]) {
    const path = join(FONT_DIR, `${name}.otf`);
    try {
      await readFile(path);
    } catch {
      throw new Error(
        `找不到字型 ${path}。\n` +
          `未註冊 CJK 字型時產生的 PDF 會是豆腐字且無錯誤訊息。\n` +
          `下載方式見 pdfme/assets/fonts/README.md`,
      );
    }
  }
}
