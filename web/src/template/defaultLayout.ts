/**
 * 報價單預設版面
 *
 * 與渲染服務 pdfme/src/template/quotation.ts 的 buildQuotationTemplate()
 * 對應。兩者的 name 必須一致，否則設計器排好的版面送去渲染時
 * 對不到資料，印出來是空白。
 *
 * 之所以在前端也有一份：使用者第一次進設計器時要看到可編輯的版面，
 * 而不是空白頁。後續版本會改為從後端載入已發布的模板。
 */

import type { TemplateElement } from './useTemplateDesigner'

export function defaultQuotationLayout(): TemplateElement[] {
  return [
    {
      name: 'company_name',
      type: 'text',
      position: { x: 15, y: 21 },
      width: 110,
      height: 7,
      fontSize: 13,
      bound: true,
    },
    {
      name: 'company_info',
      type: 'text',
      position: { x: 15, y: 29 },
      width: 110,
      height: 12,
      fontSize: 7.5,
      bound: true,
    },
    {
      name: 'doc_title',
      type: 'text',
      position: { x: 140, y: 21 },
      width: 55,
      height: 10,
      content: '報 價 單',
      fontSize: 20,
      alignment: 'right',
      bound: false,
    },
    {
      name: 'doc_meta',
      type: 'text',
      position: { x: 115, y: 38 },
      width: 80,
      height: 14,
      fontSize: 8.5,
      alignment: 'right',
      bound: true,
    },
    {
      name: 'header_rule',
      type: 'line',
      position: { x: 15, y: 50 },
      width: 180,
      height: 0.4,
      bound: false,
    },
    {
      name: 'customer_name',
      type: 'text',
      position: { x: 15, y: 55 },
      width: 95,
      height: 34,
      fontSize: 8.5,
      bound: true,
    },
    {
      name: 'payment_terms',
      type: 'text',
      position: { x: 112, y: 55 },
      width: 83,
      height: 34,
      fontSize: 8.5,
      bound: true,
    },
    {
      name: 'items',
      type: 'table',
      position: { x: 15, y: 96 },
      width: 180,
      height: 40,
      showHead: true,
      repeatHead: true,
      // 三列合計（小計、稅額、總計），與渲染服務的 buildSummaryRows 一致
      summaryRowCount: 3,
      alternateBackground: true,
      head: ['品名與規格', '數量', '單位', '單價(NT$)', '金額(NT$)'],
      headWidthPercentages: [46, 11, 9, 16, 18],
      columnAlignment: { 0: 'left', 1: 'right', 2: 'center', 3: 'right', 4: 'right' },
      bound: true,
    },
    {
      name: 'terms',
      type: 'text',
      position: { x: 15, y: 190 },
      width: 180,
      height: 22,
      fontSize: 7.5,
      bound: true,
    },
  ]
}
