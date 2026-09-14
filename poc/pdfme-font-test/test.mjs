import { generate } from '@pdfme/generator';
import { text, table } from '@pdfme/schemas';
import fs from 'fs';

const F = '/Volumes/Mac/workflow/pdfme/assets/fonts';
const font = {
  tc:      { data: fs.readFileSync(`${F}/NotoSansCJKtc-Regular.otf`), fallback: true },
  'tc-bd': { data: fs.readFileSync(`${F}/NotoSansCJKtc-Bold.otf`) },
};

const template = {
  basePdf: { width: 210, height: 297, padding: [15, 15, 15, 15] },
  schemas: [[
    { name: 'title', type: 'text', position: { x: 15, y: 15 },
      width: 100, height: 10, fontSize: 16, fontName: 'tc-bd' },
    { name: 'lines', type: 'table', position: { x: 15, y: 35 },
      width: 180, height: 40, showHead: true,
      head: ['品名', '數量', '單價', '金額'],
      headWidthPercentages: [50, 15, 17, 18],
      repeatHead: true,
      tableStyles: { borderColor: '#000', borderWidth: 0.3 },
      headStyles: { fontName: 'tc-bd', fontSize: 10, alignment: 'center',
        verticalAlignment: 'middle', lineHeight: 1, characterSpacing: 0,
        fontColor: '#000', backgroundColor: '#eee', borderColor: '#000',
        borderWidth: { top: 0.3, right: 0.3, bottom: 0.3, left: 0.3 },
        padding: { top: 2, right: 2, bottom: 2, left: 2 } },
      bodyStyles: { fontName: 'tc', fontSize: 9, alignment: 'left',
        verticalAlignment: 'middle', lineHeight: 1, characterSpacing: 0,
        fontColor: '#000', backgroundColor: '', alternateBackgroundColor: '#f7f7f7',
        borderColor: '#000', borderWidth: { top: 0.2, right: 0.2, bottom: 0.2, left: 0.2 },
        padding: { top: 2, right: 2, bottom: 2, left: 2 } },
      columnStyles: { alignment: { 0: 'left', 1: 'right', 2: 'right', 3: 'right' } },
    },
  ]],
};

const rows = [];
for (let i = 1; i <= 45; i++) rows.push([`測試品項 ${i} 號鋁擠型`, `${i}`, '1,200', `${(i*1200).toLocaleString()}`]);
rows.push(['', '', '小計', '1,242,000']);
rows.push(['', '', '稅額', '62,100']);
rows.push(['', '', '總計', '1,304,100']);

const pdf = await generate({
  template,
  inputs: [{ title: '報價單 QT-2026-0001', lines: JSON.stringify(rows) }],
  plugins: { text, table },
  options: { font },
});
fs.writeFileSync('/tmp/fonttest/out.pdf', pdf);
console.log('OK bytes=' + pdf.length);
