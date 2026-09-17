/**
 * 報價單資料 → 模板 inputs
 *
 * 把業務資料攤平成 pdfme 的 inputs。刻意獨立成一個純函數模組：
 * 呼叫端（Python Activity）傳來的是業務物件，這裡決定它怎麼呈現。
 *
 * 金額格式化集中在這裡而非分散到各處。千分位、小數位數、
 * 負數寫法一旦不一致，同一張單上會出現兩種格式。
 */

import { formatAmountInWords } from './amountInWords.js';

export interface QuotationItem {
  /** 品號 */
  code?: string;
  /** 品名 */
  name: string;
  /** 規格說明，顯示於品名下方 */
  spec?: string;
  quantity: number;
  unit: string;
  unitPrice: number;
  amount: number;
}

export interface QuotationData {
  quotationNo: string;
  quotationDate: string;
  validUntil?: string;

  company: {
    name: string;
    nameEn?: string;
    taxId?: string;
    address?: string;
    tel?: string;
    fax?: string;
  };

  customer: {
    name: string;
    taxId?: string;
    address?: string;
    contact?: string;
    phone?: string;
  };

  paymentTerms?: string;
  currency?: string;
  tradeTerms?: string;

  items: QuotationItem[];

  subtotal: number;
  taxRate?: number;
  tax: number;
  total: number;

  terms?: string[];
}

/** 千分位。小數位數依欄位而定，因此由呼叫端指定。 */
function money(value: number, decimals = 0): string {
  return value.toLocaleString('en-US', {
    minimumFractionDigits: decimals,
    maximumFractionDigits: decimals,
  });
}

/** 品名與規格合併為一格。規格以較小字級顯示需要 plugin 支援，
 *  目前以換行呈現，靠 lineHeight 拉開。 */
function itemLabel(item: QuotationItem): string {
  const head = item.code ? `${item.code} ${item.name}` : item.name;
  return item.spec ? `${head}\n${item.spec}` : head;
}

/**
 * 明細列
 *
 * 回傳的是完整的表格資料（明細 + 合計），由 splitRows() 再拆成兩張表。
 * 合併在一起產生是為了讓「最後三列是合計」這個約定只存在一處。
 */
export function buildRows(data: QuotationData): string[][] {
  const detail = data.items.map((item) => [
    itemLabel(item),
    money(item.quantity),
    item.unit,
    money(item.unitPrice, 2),
    money(item.amount),
  ]);

  return detail;
}

/**
 * 合計列
 *
 * 只有兩欄（標籤、金額），對應合計表的兩欄結構。
 * 與明細分開產生而非拼在一起再拆——欄數不同，拼起來需要補空欄，
 * 而那些空欄是上一版畫出多餘格線的原因。
 *
 * 標籤不加英文對照：單價欄只有 16% 寬（約 29mm），
 * 「小計 (Subtotal)」會被折成兩行，讓合計列比明細列還高。
 */
export function buildSummaryRows(data: QuotationData): string[][] {
  const taxLabel = data.taxRate
    ? `營業稅 ${(data.taxRate * 100).toFixed(0)}%`
    : '營業稅';

  // 欄數必須與明細列一致。少給欄位時官方的 parseSection 會用
  // undefined 建 Cell，在 raw.split() 崩潰，錯誤訊息只有
  // 「Cannot read properties of undefined (reading 'split')」，
  // 看不出是欄數不符。
  const pad = (label: string, amount: string): string[] => [
    '',
    '',
    '',
    label,
    amount,
  ];

  return [
    pad('小計', money(data.subtotal)),
    pad(taxLabel, money(data.tax)),
    pad('報價總計', money(data.total)),
  ];
}

/** 合計列數。與 buildSummaryRows 的長度一致。 */
export const SUMMARY_ROW_COUNT = 3;

export function buildInputs(data: QuotationData): Record<string, string> {
  const currency = data.currency ?? 'TWD';

  const companyInfo = [
    data.company.taxId ? `統一編號：${data.company.taxId}` : '',
    data.company.address ?? '',
    [data.company.tel && `TEL：${data.company.tel}`, data.company.fax && `FAX：${data.company.fax}`]
      .filter(Boolean)
      .join('　'),
  ]
    .filter(Boolean)
    .join('\n');

  const customerBlock = [
    `客戶名稱：${data.customer.name}`,
    data.customer.taxId ? `統一編號：${data.customer.taxId}` : '',
    data.customer.address ? `地　　址：${data.customer.address}` : '',
    data.customer.contact ? `聯 絡 人：${data.customer.contact}` : '',
    data.customer.phone ? `電　　話：${data.customer.phone}` : '',
  ]
    .filter(Boolean)
    .join('\n');

  const termsBlock = [
    data.validUntil ? `有效期限：${data.validUntil}` : '',
    data.paymentTerms ? `付款條件：${data.paymentTerms}` : '',
    `報價幣別：${currency}`,
    data.tradeTerms ? `交易條件：${data.tradeTerms}` : '',
  ]
    .filter(Boolean)
    .join('\n');

  const docMeta = [
    `報價單號：${data.quotationNo}`,
    `報價日期：${data.quotationDate}`,
  ].join('\n');

  const terms = (data.terms ?? []).map((t, i) => `${i + 1}. ${t}`).join('\n');

  return {
    form_identifier: `FORM IDENTIFIER: ${data.quotationNo}`,
    company_name: data.company.name,
    company_info: companyInfo,
    doc_meta: docMeta,
    customer_block: customerBlock,
    terms_block: termsBlock,
    // items 與 items__summary 由呼叫端以 splitRows 填入
    amount_in_words: `報價總計中文大寫：${formatAmountInWords(data.total, currencyName(currency))}`,
    terms: terms ? `備註與交易條款 (Terms & Conditions)：\n${terms}` : '',
  };
}

function currencyName(code: string): string {
  const map: Record<string, string> = {
    TWD: '新台幣',
    USD: '美金',
    JPY: '日圓',
    CNY: '人民幣',
    EUR: '歐元',
  };
  return map[code] ?? code;
}
