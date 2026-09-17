/**
 * 單據模板 API
 *
 * 預覽直接打渲染服務（Node，:3002），不經 Rust API。
 * 原因是渲染服務不碰資料庫、無狀態，而預覽用的是設計中的模板
 * 與範例資料，沒有租戶資料流經。正式產生單據時走 Python Activity
 * → Rust internal API → 渲染服務，那條路徑才有租戶隔離的需求。
 */

import type { TemplateElement } from '@/template/useTemplateDesigner'

const RENDER_BASE = import.meta.env.VITE_PDF_RENDER_BASE ?? '/pdf'

/** 預覽用的範例資料。與設計稿 pdf/code.html 的內容一致。 */
const SAMPLE = {
  company_name: '台製精工股份有限公司',
  company_info: '統一編號：54881029\n台中市西屯區精密機械園區精科路 88 號\nTEL：(04) 2359-8800',
  doc_meta: '報價單號：QT-20250330-009\n報價日期：2025/03/30',
  customer_name:
    '客戶名稱：台灣松下精密機械股份有限公司\n統一編號：11093847\n地　　址：台中市西屯區科雅路 18 號\n聯 絡 人：王景榮 經理',
  payment_terms:
    '有效期限：2025/04/29 (30天)\n付款條件：月結 60 天 (TT 電匯)\n報價幣別：TWD\n交易條件：DAP 工廠交貨含運',
  terms: '備註與交易條款 (Terms & Conditions)：\n1. 本報價單自發出日起 30 日內有效。\n2. 交貨期限：接獲確認訂單後 21 個工作天。',
  items: JSON.stringify([
    ['SP-4402 高硬度淬火模具鋼定位銷\n規格：Ø12mm × L85mm｜公差：±0.005mm', '1,500', 'PCS', '158.00', '237,000'],
    ['PL-8819 航空級輕量化伺服夾爪滑塊\n材質：AL7075-T6 陽極黑', '800', 'PCS', '380.00', '304,000'],
    ['BS-1044 耐磨合金同心軸承套筒\n材質：SUS316L 不鏽鋼', '1,200', 'PCS', '135.00', '162,000'],
    ['', '', '', '小計', '703,000'],
    ['', '', '', '營業稅 5%', '35,150'],
    ['', '', '', '報價總計', '738,150'],
  ]),
}

const FONT_REGULAR = 'NotoSansCJKtc-Regular'
const FONT_BOLD = 'NotoSansCJKtc-Bold'

/**
 * 把設計器的元素轉成 pdfme 模板
 *
 * 設計器的資料結構刻意比 pdfme schema 精簡（例如用 alternateBackground
 * 而非完整的 bodyStyles），讓屬性面板好寫。轉換集中在這裡，
 * 兩種結構的對應只存在一處。
 */
export function toPdfmeTemplate(elements: TemplateElement[]) {
  const cell = (fontName: string, fontSize: number, extra: Record<string, unknown> = {}) => ({
    fontName,
    fontSize,
    alignment: 'left',
    verticalAlignment: 'middle',
    lineHeight: 1.35,
    characterSpacing: 0,
    fontColor: '#1E293B',
    backgroundColor: '',
    borderColor: '#CBD5E1',
    borderWidth: { top: 0.1, right: 0.1, bottom: 0.1, left: 0.1 },
    padding: { top: 2, right: 3, bottom: 2, left: 3 },
    ...extra,
  })

  const schemas = elements.map((el) => {
    if (el.type === 'table') {
      return {
        name: el.name,
        type: 'table',
        position: el.position,
        width: el.width,
        height: el.height,
        showHead: el.showHead !== false,
        repeatHead: el.repeatHead !== false,
        summaryRowCount: el.summaryRowCount ?? 0,
        summaryStyles: { fontName: FONT_BOLD, fontSize: 11 },
        summaryTopBorder: { width: 0.4, color: '#1E293B' },
        head: el.head ?? [],
        headWidthPercentages: el.headWidthPercentages ?? [],
        content: '[]',
        tableStyles: { borderColor: '#CBD5E1', borderWidth: 0.1 },
        headStyles: cell(FONT_BOLD, 8.5, {
          alignment: 'center',
          backgroundColor: '#F1F5F9',
        }),
        bodyStyles: cell(FONT_REGULAR, 9, {
          alternateBackgroundColor: el.alternateBackground !== false ? '#FAFAFA' : '',
        }),
        columnStyles: { alignment: el.columnAlignment ?? {} },
      }
    }

    if (el.type === 'line') {
      return {
        name: el.name,
        type: 'line',
        position: el.position,
        width: el.width,
        height: el.height,
        color: '#1E293B',
      }
    }

    return {
      name: el.name,
      type: el.type,
      position: el.position,
      width: el.width,
      height: el.height,
      content: el.content ?? '',
      fontName: (el.fontSize ?? 9) >= 13 ? FONT_BOLD : FONT_REGULAR,
      fontSize: el.fontSize ?? 9,
      alignment: el.alignment ?? 'left',
      lineHeight: 1.45,
      fontColor: '#1E293B',
    }
  })

  return {
    basePdf: { width: 210, height: 297, padding: [15, 15, 15, 15] },
    schemas: [schemas],
  }
}

/**
 * 產生預覽 PDF
 *
 * 未綁定欄位的元素用自己的 content，綁定的則填入範例資料——
 * 沒有對應範例時留空而非填假字串，讓使用者看得出哪些欄位還沒接上資料。
 */
export async function renderPreview(elements: TemplateElement[]): Promise<Blob> {
  const inputs: Record<string, string> = {}
  for (const el of elements) {
    if (!el.bound) continue
    inputs[el.name] = SAMPLE[el.name as keyof typeof SAMPLE] ?? ''
  }

  const response = await fetch(`${RENDER_BASE}/render`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ template: toPdfmeTemplate(elements), inputs: [inputs] }),
  })

  if (!response.ok) {
    const body: unknown = await response.json().catch(() => null)
    const message =
      body && typeof body === 'object' && 'message' in body
        ? String((body as { message: unknown }).message)
        : `渲染服務回應 ${response.status}`
    throw new Error(message)
  }

  return response.blob()
}
