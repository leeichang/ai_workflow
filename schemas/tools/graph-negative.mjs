/**
 * 圖結構驗證的負向測試
 *
 * 每個案例刻意破壞一條規則，確認驗證器回報預期的錯誤碼。
 * 正式版 Rust validator 必須對同一組案例產生相同結果。
 */

import fs from 'fs';
import path from 'path';
import { validateGraph } from './graph-check.mjs';

const base = JSON.parse(
  fs.readFileSync(path.resolve(import.meta.dirname, '../fixtures/quotation_approval_v1.json'), 'utf8'),
);
const clone = () => JSON.parse(JSON.stringify(base));
const nodeOf = (d, id) => d.nodes.find((n) => n.id === id);

const cases = [
  ['WF-E001', '刪掉 end 節點', () => {
    const d = clone();
    d.nodes = d.nodes.filter((n) => n.type !== 'end');
    d.edges = d.edges.filter((e) => !e.includes('end'));
    return d;
  }],
  ['WF-E001', '兩個 trigger', () => {
    const d = clone();
    d.nodes.push({ id: 'start2', type: 'trigger' });
    d.edges.push(['start2', 'manager_approval']);
    return d;
  }],
  ['WF-E002', '孤立節點', () => {
    const d = clone();
    d.nodes.push({ id: 'orphan', type: 'human_task', participant: 'internal', assignee: { type: 'initiator' } });
    return d;
  }],
  ['WF-E002', '邊指向不存在的節點', () => {
    const d = clone();
    d.edges.push(['manager_approval', 'ghost']);
    return d;
  }],
  ['WF-E003', 'condition 只有一條分支', () => {
    const d = clone();
    d.edges = d.edges.filter((e) => !(e[0] === 'discount_gate' && e[2]?.when === false));
    return d;
  }],
  ['WF-E004', 'goto 指向下游節點', () => {
    const d = clone();
    nodeOf(d, 'manager_approval').on_reject = { action: 'goto', node: 'create_order' };
    return d;
  }],
  ['WF-E004', 'goto 指向不存在的節點', () => {
    const d = clone();
    nodeOf(d, 'manager_approval').on_reject = { action: 'goto', node: 'nowhere' };
    return d;
  }],
  ['WF-E005', '角色不存在', () => {
    const d = clone();
    nodeOf(d, 'finance_approval').resolver = { type: 'role', value: 'chief_wizard' };
    return d;
  }],
  ['WF-E006', '表達式括號不對稱', () => {
    const d = clone();
    nodeOf(d, 'discount_gate').expression = '(quotation.discount_rate > 0.15';
    return d;
  }],
  ['WF-E006', '表達式用單等號', () => {
    const d = clone();
    nodeOf(d, 'discount_gate').expression = 'quotation.status = "draft"';
    return d;
  }],
  ['WF-E007', 'action 未註冊', () => {
    const d = clone();
    nodeOf(d, 'create_order').action = 'odoo.launch_rocket';
    return d;
  }],
  ['WF-E007', 'ERP 寫入缺 idempotency_key', () => {
    const d = clone();
    delete nodeOf(d, 'create_order').idempotency_key;
    return d;
  }],
  ['WF-E008', '外部參與者用角色 resolver', () => {
    const d = clone();
    nodeOf(d, 'customer_review').resolver = { type: 'role', value: 'approver' };
    return d;
  }],
];

let bad = 0;
for (const [expectedCode, name, build] of cases) {
  const errors = validateGraph(build());
  const codes = new Set(errors.map((e) => e.code));
  if (codes.has(expectedCode)) {
    console.log(`OK    ${expectedCode}  ${name}`);
  } else {
    bad++;
    console.log(`MISS  ${expectedCode}  ${name}`);
    console.log(`      實際回報：${[...codes].join(', ') || '（無錯誤）'}`);
  }
}

// 反向確認：未被破壞的 fixture 不得有錯
const clean = validateGraph(clone());
if (clean.length > 0) {
  bad++;
  console.log(`\nFALSE POSITIVE：乾淨的 fixture 被誤判`);
  for (const e of clean) console.log(`      [${e.code}] ${e.message}`);
}

console.log(bad ? `\n${bad} 個案例不符預期` : '\n全部符合預期');
process.exit(bad ? 1 : 0);
