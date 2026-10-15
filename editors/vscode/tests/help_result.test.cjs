const assert = require('node:assert/strict');
const test = require('node:test');
const fs = require('node:fs/promises');
const os = require('node:os');
const path = require('node:path');
const { resultData, collectPages, referenceData, engineFingerprint } = require('../scripts/help.cjs');

test('Help generation reads canonical data without joining legacy and canonical JSON documents', () => {
  const data = [{ code: 'E001' }];
  const result = {
    content: [
      { type: 'text', text: JSON.stringify(data) },
      { type: 'text', text: JSON.stringify({ schema_version: 3, data }) },
    ],
  };
  assert.deepEqual(resultData(result), data);
  assert.deepEqual(resultData({ structuredContent: { schema_version: 3, data } }), data);
  assert.throws(() => resultData({ structuredContent: { schema_version: 2, data } }), /requires canonical MCP tool contract 3/);
});

test('Help generation rejects a result with no canonical schema-versioned object', () => {
  assert.throws(() => resultData({ content: [{ type: 'text', text: 'legacy Markdown' }] }), /canonical schema_version 3/);
});

function envelope(data, continuation = null, count = 1) {
  return { structuredContent: { schema_version: 3, outcome: { status: 'ok', reasons: [] }, data,
    selection: { continuation, collections: { rows: { returned_count: count } } } } };
}

test('Help follows unchanged catalog selectors until the continuation is complete', async () => {
  const calls = [];
  const pages = await collectPages(async (name, args) => {
    calls.push({ name, args });
    return args.continuation ? envelope({ forms: [{ name: 'irf' }] }) : envelope({ forms: [{ name: 'order' }] }, 'next');
  }, 'dynare_syntax', { kind: 'option', parent: 'stoch_simul', page_size: 1 });
  assert.deepEqual(pages.flatMap(page => page.forms.map(form => form.name)), ['order', 'irf']);
  assert.deepEqual(calls[1], { name: 'dynare_syntax', args: { kind: 'option', parent: 'stoch_simul', page_size: 1, continuation: 'next' } });
  await assert.rejects(collectPages(async () => envelope({ forms: [] }, 'same'), 'dynare_syntax'), /repeated a continuation/);
});

test('Help projects every paged canonical code and option into the existing reader shape', async () => {
  const heads = {
    command: [{ name: 'stoch_simul', aliases: [] }, { name: 'pac_model', aliases: [] }],
    block: [{ name: 'estimated_params', aliases: ['priors'] }, { name: 'priors', aliases: ['estimated_params'] }], declaration: [{ name: 'var', aliases: [] }],
    statement_form: [{ name: 'subsamples', aliases: [] }],
  };
  const options = { stoch_simul: ['order', 'irf', 'nograph'], pac_model: ['discount'], estimated_params: [], priors: [], var: ['long_name'], prior: ['shape'], options: ['init'], subsamples: [] };
  const codes = [{ code: 'E001', title: 'Syntax', kind: 'shared' }, { code: 'W010', title: 'Unassigned parameter', kind: 'shared' }];
  const calls = [];
  const result = await referenceData(async (name, args) => {
    calls.push({ name, args });
    let rows, key;
    if (name === 'dynare_diagnostic_info') {
      key = 'diagnostics';
      rows = args.code ? codes.filter(code => code.code === args.code).map(code => ({ ...code, explanation: `# ${code.code}\nCorrection guidance.` })) : codes;
    } else {
      assert.equal(name, 'dynare_syntax');
      key = 'forms';
      rows = args.parent ? options[args.parent].map(name => ({ name, summary: `${name} rule` })) : args.kind === 'option'
        ? Object.entries(options).flatMap(([parent, names]) => names.map(name => ({ name, parent }))) : heads[args.kind];
      if (!args.parent && args.kind !== 'option') assert.equal(args.context, 'top_level');
    }
    const offset = Number(args.continuation ?? 0);
    const selected = rows.slice(offset, offset + args.page_size);
    return envelope({ [key]: selected }, offset + selected.length < rows.length ? String(offset + selected.length) : null, selected.length);
  }, 1);
  assert.deepEqual(result.codes.map(code => code.code), ['E001', 'W010']);
  assert.ok(result.codes.every(code => code.markdown.includes(code.code)));
  const simulation = result.options.find(entry => entry.command === 'stoch_simul');
  assert.deepEqual(simulation, { command: 'stoch_simul', known: true, n_options: 3,
    options: ['order', 'irf', 'nograph'].map(name => ({ name, description: `${name} rule` })) });
  assert.deepEqual(result.options.find(entry => entry.command === 'priors').options,
    result.options.find(entry => entry.command === 'estimated_params').options);
  assert.equal(result.options.filter(entry => entry.command === 'priors').length, 1);
  for (const command of ['prior', 'options', 'subsamples']) assert.ok(result.options.some(page => page.command === command));
  assert.ok(calls.some(call => call.args.parent === 'stoch_simul' && call.args.continuation === '2'));
});

test('Help fingerprint changes when consumed syntax JSON changes', async () => {
  const base = path.resolve(os.tmpdir());
  const root = await fs.mkdtemp(path.join(base, 'dygnosis-help-fingerprint-'));
  try {
    for (const folder of ['src/engine/diagnostics', 'src/engine/catalog', 'src/mcp']) await fs.mkdir(path.join(root, folder), { recursive: true });
    for (const file of ['src/engine/diagnostics/explain.rs', 'src/mcp/mod.rs', 'Cargo.toml', 'Cargo.lock']) await fs.writeFile(path.join(root, file), 'unchanged\n');
    const syntax = path.join(root, 'src/engine/catalog/syntax_data.json');
    await fs.writeFile(syntax, '{"name":"diff"}\n');
    const before = await engineFingerprint(root);
    await fs.writeFile(syntax, '{"name":"log"}\n');
    assert.notEqual(await engineFingerprint(root), before);
  } finally {
    assert.equal(path.dirname(path.resolve(root)), base);
    await fs.rm(root, { recursive: true, force: true });
  }
});
