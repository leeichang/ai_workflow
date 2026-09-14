import { chromium } from 'playwright';
const DIR = '/Volumes/Mac/workflow/docs/測試報告/202609/screenshots/temporal-poc';
const shots = [
  ['01-workflow-list', 'http://localhost:8080/namespaces/default/workflows'],
  ['02-restart-test-summary', 'http://localhost:8080/namespaces/default/workflows/wf-QT-b32e3c72'],
];
const b = await chromium.launch();
const p = await b.newPage({ viewport: { width: 1440, height: 900 } });
for (const [name, url] of shots) {
  await p.goto(url, { waitUntil: 'networkidle' });
  await p.waitForTimeout(2500);
  await p.screenshot({ path: `${DIR}/${name}.png`, fullPage: true });
  console.log('saved', name);
}
await b.close();
