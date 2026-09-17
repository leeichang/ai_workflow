import { writeFile } from 'node:fs/promises';
import { renderQuotation } from './src/render.ts';

const data = {
  quotationNo: 'QT-20250330-009',
  quotationDate: '2025/03/30',
  validUntil: '2025/04/29 (30天)',
  company: {
    name: '台製精工股份有限公司',
    taxId: '54881029',
    address: '台中市西屯區精密機械園區精科路 88 號',
    tel: '(04) 2359-8800',
  },
  customer: {
    name: '台灣松下精密機械股份有限公司',
    taxId: '11093847',
    address: '台中市西屯區科雅路 18 號 2 樓生產中心',
    contact: '王景榮 經理',
    phone: '(04) 2568-1200 #412',
  },
  paymentTerms: '月結 60 天 (TT 電匯)',
  currency: 'TWD',
  tradeTerms: 'DAP 工廠交貨含運',
  items: [
    { code:'SP-4402', name:'高硬度淬火模具鋼定位銷', spec:'規格：Ø12mm × L85mm｜公差：±0.005mm (HRC58-62)', quantity:1500, unit:'PCS', unitPrice:158, amount:237000 },
    { code:'PL-8819', name:'航空級輕量化伺服夾爪滑塊', spec:'材質：AL7075-T6 陽極黑｜耐衝擊硬度研磨', quantity:800, unit:'PCS', unitPrice:380, amount:304000 },
    { code:'BS-1044', name:'耐磨合金同心軸承套筒', spec:'材質：SUS316L 不鏽鋼｜鏡面研磨 Ra 0.2', quantity:1200, unit:'PCS', unitPrice:135, amount:162000 },
  ],
  subtotal: 703000,
  taxRate: 0.05,
  tax: 35150,
  total: 738150,
  terms: [
    '本報價單自發出日起 30 日內有效，逾期需重新核價。',
    '交貨期限：接獲雙方用印確認訂單後 21 個工作天內依指示批次交付。',
    '匯款資訊：兆豐國際商業銀行 (017) 台中分行｜帳號 028-10-88921-5｜戶名：台製精工股份有限公司',
  ],
};

const pdf = await renderQuotation(data);
await writeFile('/tmp/quotation.pdf', pdf);
console.log('OK bytes=', pdf.length);
