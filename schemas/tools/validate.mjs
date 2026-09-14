import Ajv2020 from 'ajv/dist/2020.js';
import addFormats from 'ajv-formats';
import fs from 'fs';

const S = '/Volumes/Mac/workflow/schemas';
const ajv = new Ajv2020({ allErrors: true, strict: false });
addFormats(ajv);

const compiled = {};
const get = (f) => compiled[f] ??= ajv.compile(JSON.parse(fs.readFileSync(`${S}/${f}`, 'utf8')));

const cases = [
  ['workflow-dsl.schema.json', 'fixtures/quotation_approval_v1.json'],
  ['workflow-dsl.schema.json', 'fixtures/purchase_approval_v1.json'],
  ['form.schema.json',         'fixtures/quotation_form_v1.json'],
];

let fail = 0;
for (const [schemaFile, fixtureFile] of cases) {
  const validate = get(schemaFile);
  const data = JSON.parse(fs.readFileSync(`${S}/${fixtureFile}`, 'utf8'));
  const ok = validate(data);
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${fixtureFile}`);
  if (!ok) {
    fail++;
    const seen = new Set();
    for (const e of validate.errors) {
      const k = `${e.instancePath}|${e.message}`;
      if (seen.has(k)) continue;
      seen.add(k);
      if (seen.size > 12) break;
      console.log(`      ${e.instancePath || '/'}  ${e.message}`);
    }
  }
}
process.exit(fail ? 1 : 0);
