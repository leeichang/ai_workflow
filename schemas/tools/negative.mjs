import Ajv2020 from 'ajv/dist/2020.js';
import addFormats from 'ajv-formats';
import fs from 'fs';

const S = '/Volumes/Mac/workflow/schemas';
const ajv = new Ajv2020({ allErrors: true, strict: false });
addFormats(ajv);
const dsl  = ajv.compile(JSON.parse(fs.readFileSync(`${S}/workflow-dsl.schema.json`, 'utf8')));
const form = ajv.compile(JSON.parse(fs.readFileSync(`${S}/form.schema.json`, 'utf8')));

const base = JSON.parse(fs.readFileSync(`${S}/fixtures/quotation_approval_v1.json`, 'utf8'));
const baseForm = JSON.parse(fs.readFileSync(`${S}/fixtures/quotation_form_v1.json`, 'utf8'));
const clone = (o) => JSON.parse(JSON.stringify(o));

const cases = [
  ['DSL 缺 nodes', dsl, (() => { const d = clone(base); delete d.nodes; return d; })()],
  ['DSL 含 Temporal 詞彙', dsl, (() => { const d = clone(base); d.nodes[1].signal_name = 'x'; return d; })()],
  ['DSL node id 大寫', dsl, (() => { const d = clone(base); d.nodes[0].id = 'Start'; return d; })()],
  ['DSL 未知 node 型別', dsl, (() => { const d = clone(base); d.nodes[0].type = 'loop'; return d; })()],
  ['DSL approval 缺 resolver', dsl, (() => { const d = clone(base); delete d.nodes[1].resolver; return d; })()],
  ['DSL resolver 型別錯', dsl, (() => { const d = clone(base); d.nodes[1].resolver = { type: 'ai' }; return d; })()],
  ['DSL join completion 非法', dsl, (() => { const d = clone(base); d.nodes[5].join.completion = 'MOST'; return d; })()],
  ['DSL duration 格式錯', dsl, (() => { const d = clone(base); d.nodes[1].timeout.after = '2 days'; return d; })()],
  ['DSL action 名稱無點號', dsl, (() => { const d = clone(base); d.nodes[4].action = 'publish'; return d; })()],
  ['DSL edge 只有一個節點', dsl, (() => { const d = clone(base); d.edges[0] = ['start']; return d; })()],
  ['DSL version 為 0', dsl, (() => { const d = clone(base); d.version = 0; return d; })()],
  ['Form 缺 data', form, (() => { const f = clone(baseForm); delete f.fields[0].data; return f; })()],
  ['Form component 非法', form, (() => { const f = clone(baseForm); f.fields[0].ui.component = 'richtext'; return f; })()],
  ['Form data type 非法', form, (() => { const f = clone(baseForm); f.fields[0].data.type = 'money'; return f; })()],
  ['Form 欄寬超過 24', form, (() => { const f = clone(baseForm); f.fields[0].ui.width = 30; return f; })()],
  ['Form 多餘屬性', form, (() => { const f = clone(baseForm); f.fields[0].ui.color = 'red'; return f; })()],
];

let bad = 0;
for (const [name, validate, data] of cases) {
  const ok = validate(data);
  if (ok) { bad++; console.log(`LEAK  ${name}  <- 應被拒卻通過`); }
  else console.log(`OK    ${name}`);
}
console.log(bad ? `\n${bad} 個漏網` : '\n全部正確攔截');
process.exit(bad ? 1 : 0);
