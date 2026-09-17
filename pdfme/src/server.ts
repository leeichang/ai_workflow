/**
 * 單據 PDF 渲染服務
 *
 * 單一職責：模板 + 資料 → PDF bytes。
 *
 * **不碰資料庫。** 這是與 CellReport 最關鍵的差異（見 05_報表模組評估.md）：
 * 租戶隔離由呼叫端的 Python Activity 保證，本服務是無狀態純函數。
 * 一旦它開始自己查資料，RLS 就繞過去了。
 *
 * 為何是獨立的 Node 服務而非併入 Python Worker：
 * @pdfme/generator 是 Node 套件，沒有 Python 版本。
 * 選項比較見 06_單據套版設計器選型.md 第 5 節。
 */

import Fastify from 'fastify';
import { assertFontsAvailable } from './fonts.js';
import { renderQuotation, renderTemplate } from './render.js';
import type { QuotationData } from './template/mapper.js';

const PORT = Number(process.env.PDF_RENDER_PORT ?? 3002);
const HOST = process.env.PDF_RENDER_HOST ?? '0.0.0.0';

/** 上限保護。單張單據不該有這麼多明細，超過通常是呼叫端算錯。 */
const MAX_ROWS = 5000;

const app = Fastify({
  logger: { level: process.env.LOG_LEVEL ?? 'info' },
  // 明細多時 payload 會大，但仍需上限避免記憶體耗盡
  bodyLimit: 10 * 1024 * 1024,
});

interface RenderBody {
  /** pdfme 模板。設計器產生，結構不固定，因此不宣告細部型別。 */
  template: unknown;
  inputs: Record<string, unknown>[];
}

app.get('/health', async () => ({ status: 'ok' }));

/**
 * 以內建的報價單模板渲染
 *
 * 呼叫端只需傳業務資料，版面由本服務決定。
 * 設計器改好的模板走 /render，這一支是尚未套版時的預設輸出。
 */
app.post<{ Body: { data: QuotationData } }>('/render/quotation', async (request, reply) => {
  const data = request.body?.data;

  if (!data?.quotationNo || !Array.isArray(data.items)) {
    return reply.code(400).send({
      error: 'BAD_REQUEST',
      message: '需要 data.quotationNo 與 data.items',
    });
  }

  try {
    const pdf = await renderQuotation(data);
    return reply
      .type('application/pdf')
      .header('content-length', String(pdf.length))
      .send(Buffer.from(pdf));
  } catch (error) {
    request.log.error({ err: error }, '報價單渲染失敗');
    return reply.code(422).send({
      error: 'RENDER_FAILED',
      message: error instanceof Error ? error.message : String(error),
    });
  }
});

app.post<{ Body: RenderBody }>('/render', async (request, reply) => {
  const { template, inputs } = request.body ?? {};

  if (!template || !Array.isArray(inputs)) {
    return reply.code(400).send({
      error: 'BAD_REQUEST',
      message: '需要 template 與 inputs',
    });
  }

  const rowCount = countTableRows(inputs);
  if (rowCount > MAX_ROWS) {
    return reply.code(400).send({
      error: 'TOO_MANY_ROWS',
      message: `明細列數 ${rowCount} 超過上限 ${MAX_ROWS}`,
    });
  }

  try {
    const pdf = await renderTemplate(template, inputs);

    return reply
      .type('application/pdf')
      .header('content-length', String(pdf.length))
      .send(Buffer.from(pdf));
  } catch (error) {
    // 模板錯誤與程式錯誤都會走到這裡。回 422 而非 500：
    // 絕大多數是模板或資料的問題，呼叫端重試沒有意義。
    request.log.error({ err: error }, 'PDF 渲染失敗');
    return reply.code(422).send({
      error: 'RENDER_FAILED',
      message: error instanceof Error ? error.message : String(error),
    });
  }
});

/** 粗略計算明細列數，用於上限保護 */
function countTableRows(inputs: Record<string, unknown>[]): number {
  let total = 0;
  for (const input of inputs) {
    for (const value of Object.values(input)) {
      if (typeof value !== 'string') continue;
      try {
        const parsed: unknown = JSON.parse(value);
        if (Array.isArray(parsed)) total += parsed.length;
      } catch {
        // 不是 JSON 就不是表格資料
      }
    }
  }
  return total;
}

async function main(): Promise<void> {
  // 缺字型就別啟動。等到第一張報價單印出豆腐字才發現，
  // 那時單據已經寄給客戶了。
  await assertFontsAvailable();

  await app.listen({ port: PORT, host: HOST });
  app.log.info(`PDF 渲染服務啟動於 http://${HOST}:${PORT}`);
}

// 直接執行時啟動；被匯入（測試）時不啟動
if (process.argv[1] && import.meta.url.endsWith(process.argv[1].split('/').pop() ?? '')) {
  main().catch((error: unknown) => {
    app.log.error({ err: error }, '啟動失敗');
    process.exit(1);
  });
}

export { app };
