/**
 * Workflow DSL 圖結構驗證
 *
 * JSON Schema 只能驗單一節點的結構，驗不了跨節點的語意。
 * 本檔實作 docs/系統規劃/03_功能規劃.md 4.2 節的 WF-E001～E008。
 *
 * 此為參考實作（JavaScript）。正式版在 Rust
 * （server/crates/domain/src/workflow/validator.rs，任務 4.1），
 * 兩者必須對同一組 fixture 產生相同的錯誤碼集合。
 */

import fs from 'fs';
import path from 'path';

const RULES = {
  'WF-E001': '必須恰有一個 trigger 節點與至少一個 end 節點',
  'WF-E002': '所有節點須可從 trigger 到達，且可到達某個 end',
  'WF-E003': 'condition 節點須有 when=true 與 when=false 兩條出邊',
  'WF-E004': 'on_reject/on_failure 的 goto 目標須在該節點的上游路徑',
  'WF-E005': 'resolver 的角色或部門須存在於租戶（本檔以白名單模擬）',
  'WF-E006': '表達式須可解析（本檔僅做粗略語法檢查）',
  'WF-E007': 'action 名稱須存在於 Action Registry',
  'WF-E008': 'participant=external 的 resolver 必須是 external_contacts',
};

// 模擬租戶資料與 Registry。正式版查 DB。
const KNOWN_ROLES = new Set([
  'admin', 'designer', 'approver', 'requester', 'viewer',
  'finance_manager', 'sales_director', 'cfo', 'purchase_manager',
]);
const ACTION_REGISTRY = new Set([
  'quotation.publish',
  'odoo.create_sale_order',
  'odoo.create_purchase_order',
]);

/**
 * 建鄰接表
 *
 * 兩種控制流：
 *   1. edges 陣列 — 正常前進路徑
 *   2. on_reject / on_failure 的 goto — 退回路徑
 *
 * 可達性分析必須包含兩者，否則只靠 goto 進入的節點（例如 revise）
 * 會被誤判為不可達。但「上游」判斷只能看 edges，否則 goto 自己會
 * 讓目標變成上游，E004 就失效了。
 */
function buildAdjacency(dsl) {
  const out = new Map();       // 含 goto，供可達性分析
  const inn = new Map();       // 含 goto
  const outFwd = new Map();    // 僅 edges，供上游判斷
  const innFwd = new Map();    // 僅 edges

  for (const n of dsl.nodes) {
    out.set(n.id, []); inn.set(n.id, []);
    outFwd.set(n.id, []); innFwd.set(n.id, []);
  }

  for (const e of dsl.edges) {
    const [from, to, meta] = e;
    if (!out.has(from) || !out.has(to)) continue; // 由 E002 另行報告
    const edge = { to, meta: meta ?? {} };
    out.get(from).push(edge);
    outFwd.get(from).push(edge);
    inn.get(to).push({ from, meta: meta ?? {} });
    innFwd.get(to).push({ from, meta: meta ?? {} });
  }

  // 把 goto 併入可達性用的鄰接表
  for (const n of dsl.nodes) {
    for (const key of ['on_reject', 'on_failure']) {
      const p = n[key];
      if (p?.action !== 'goto' || !out.has(p.node)) continue;
      out.get(n.id).push({ to: p.node, meta: { via: key } });
      inn.get(p.node).push({ from: n.id, meta: { via: key } });
    }
  }

  return { out, inn, outFwd, innFwd };
}

/** 從起點可達集合 */
function reachable(startId, adjacency) {
  const seen = new Set();
  const stack = [startId];
  while (stack.length) {
    const cur = stack.pop();
    if (seen.has(cur)) continue;
    seen.add(cur);
    for (const { to } of adjacency.get(cur) ?? []) stack.push(to);
  }
  return seen;
}

/** 反向可達，用於「可到達 end」與「上游」判斷 */
function reachableReverse(startId, inn) {
  const seen = new Set();
  const stack = [startId];
  while (stack.length) {
    const cur = stack.pop();
    if (seen.has(cur)) continue;
    seen.add(cur);
    for (const { from } of inn.get(cur) ?? []) stack.push(from);
  }
  return seen;
}

/** 收集 resolver 內所有 role 值，含 composite 遞迴 */
function collectRoles(resolver, acc = []) {
  if (!resolver || typeof resolver !== 'object') return acc;
  if (resolver.type === 'role' && resolver.value) acc.push(resolver.value);
  if (resolver.type === 'composite') {
    for (const item of resolver.of ?? []) collectRoles(item.spec, acc);
  }
  return acc;
}

/** 粗略表達式檢查。正式版用 CEL parser */
function looksLikeValidExpression(expr) {
  if (typeof expr !== 'string' || !expr.trim()) return false;
  let depth = 0;
  for (const ch of expr) {
    if (ch === '(') depth++;
    if (ch === ')') depth--;
    if (depth < 0) return false;
  }
  if (depth !== 0) return false;
  // 單等號比較是常見筆誤
  if (/[^=!<>]=[^=]/.test(expr)) return false;
  return true;
}

/** 收集節點與其巢狀結構裡的所有表達式 */
function collectExpressions(node) {
  const out = [];
  if (node.type === 'condition' && node.expression) out.push(node.expression);
  const walk = (r) => {
    if (!r || typeof r !== 'object') return;
    if (r.type === 'expression' && r.cel) out.push(r.cel);
    if (r.type === 'composite') for (const i of r.of ?? []) { if (i.when) out.push(i.when); walk(i.spec); }
  };
  walk(node.resolver);
  walk(node.assignee);
  walk(node.timeout?.to);
  return out;
}

export function validateGraph(dsl) {
  const errors = [];
  const add = (code, nodeId, detail) =>
    errors.push({ code, node_id: nodeId ?? null, message: detail ?? RULES[code] });

  const byId = new Map(dsl.nodes.map((n) => [n.id, n]));
  const { out, inn, outFwd, innFwd } = buildAdjacency(dsl);

  // 邊參照的節點必須存在
  for (const [from, to] of dsl.edges) {
    if (!byId.has(from)) add('WF-E002', from, `邊的起點節點不存在：${from}`);
    if (!byId.has(to)) add('WF-E002', to, `邊的終點節點不存在：${to}`);
  }

  // E001 起訖節點
  const triggers = dsl.nodes.filter((n) => n.type === 'trigger');
  const ends = dsl.nodes.filter((n) => n.type === 'end');
  if (triggers.length !== 1) add('WF-E001', null, `trigger 節點應恰有 1 個，實際 ${triggers.length} 個`);
  if (ends.length < 1) add('WF-E001', null, 'end 節點至少需 1 個');

  if (triggers.length === 1 && ends.length >= 1) {
    const start = triggers[0].id;
    const fwd = reachable(start, out);
    const bwdFromEnds = new Set();
    for (const e of ends) for (const id of reachableReverse(e.id, inn)) bwdFromEnds.add(id);

    // E002 可達性
    for (const n of dsl.nodes) {
      if (!fwd.has(n.id)) add('WF-E002', n.id, `節點無法從 trigger 到達：${n.id}`);
      else if (!bwdFromEnds.has(n.id)) add('WF-E002', n.id, `節點無法到達任何 end：${n.id}`);
    }

    // E004 goto 目標須在上游
    for (const n of dsl.nodes) {
      for (const key of ['on_reject', 'on_failure']) {
        const p = n[key];
        if (p?.action !== 'goto') continue;
        if (!byId.has(p.node)) { add('WF-E004', n.id, `${key} 目標節點不存在：${p.node}`); continue; }
        // 上游判斷只看正向 edges，不含 goto，否則 goto 會讓目標自己變成上游
        const upstream = reachableReverse(n.id, innFwd);
        if (!upstream.has(p.node)) {
          add('WF-E004', n.id, `${key} 目標「${p.node}」不在節點「${n.id}」的上游路徑`);
        }
      }
    }
  }

  // 逐節點檢查
  for (const n of dsl.nodes) {
    const outs = out.get(n.id) ?? [];

    // E003 condition 分支
    if (n.type === 'condition') {
      const hasTrue = outs.some((e) => e.meta.when === true);
      const hasFalse = outs.some((e) => e.meta.when === false);
      if (!hasTrue || !hasFalse) {
        add('WF-E003', n.id, `condition 節點「${n.id}」缺少 ${!hasTrue ? 'when=true' : ''}${!hasTrue && !hasFalse ? ' 與 ' : ''}${!hasFalse ? 'when=false' : ''} 分支`);
      }
    }

    // E005 角色存在
    for (const role of [...collectRoles(n.resolver), ...collectRoles(n.assignee), ...collectRoles(n.timeout?.to)]) {
      if (!KNOWN_ROLES.has(role)) add('WF-E005', n.id, `角色不存在：${role}`);
    }

    // E006 表達式
    for (const expr of collectExpressions(n)) {
      if (!looksLikeValidExpression(expr)) add('WF-E006', n.id, `表達式無法解析：${expr}`);
    }

    // E007 action 註冊
    if (n.type === 'action' && !ACTION_REGISTRY.has(n.action)) {
      add('WF-E007', n.id, `action 不存在於 Registry：${n.action}`);
    }

    // E008 外部參與者的 resolver
    if (n.participant === 'external') {
      const r = n.resolver ?? n.assignee;
      if (r?.type !== 'external_contacts') {
        add('WF-E008', n.id, `participant=external 的 resolver 必須是 external_contacts，實際為 ${r?.type}`);
      }
    }

    // 寫入外部系統的 action 必須有 idempotency_key
    if (n.type === 'action' && /^(odoo|erp)\./.test(n.action ?? '') && !n.idempotency_key) {
      add('WF-E007', n.id, `寫入外部系統的 action「${n.action}」缺少 idempotency_key`);
    }
  }

  return errors;
}

// CLI：僅在直接執行時跑，被 import 時不執行
if (process.argv[1] && path.resolve(process.argv[1]) === path.resolve(import.meta.filename)) {
  const dir = path.resolve(import.meta.dirname, '../fixtures');
  const files = fs.readdirSync(dir).filter((f) => f.endsWith('.json') && f.includes('approval'));

  let fail = 0;
  for (const f of files) {
    const dsl = JSON.parse(fs.readFileSync(path.join(dir, f), 'utf8'));
    const errors = validateGraph(dsl);
    if (errors.length === 0) {
      console.log(`PASS  ${f}`);
    } else {
      fail++;
      console.log(`FAIL  ${f}`);
      for (const e of errors) console.log(`      [${e.code}] ${e.message}`);
    }
  }
  process.exit(fail ? 1 : 0);
}
