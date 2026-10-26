const assert = require('node:assert/strict');
const childProcess = require('node:child_process');
const fs = require('node:fs/promises');
const fsSync = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const test = require('node:test');
const common = require('../scripts/common.cjs');
const help = require('../scripts/help.cjs');

async function fixture(t) {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'dygnosis-help-build-'));
  t.after(() => {
    assert.equal(path.dirname(path.resolve(root)), path.resolve(os.tmpdir()));
    assert.ok(path.basename(root).startsWith('dygnosis-help-build-'));
    return fs.rm(root, { recursive: true, force: true });
  });
  const files = {
    'Cargo.toml': '[package]\nname = "dygnosis"\nversion = "0.1.0"\n',
    'Cargo.lock': 'version = 4\n\n[[package]]\nname = "dygnosis"\nversion = "0.1.0"\n',
    'src/engine/diagnostics/explain.rs': '// Diagnostic source\n',
    'src/mcp/mod.rs': '// MCP source\n',
    'editors/vscode/scripts/help.cjs': '// Help exporter\n',
    'editors/vscode/package.json': JSON.stringify({ name: 'dygnosis', version: '0.1.0' }),
    'editors/vscode/package-lock.json': JSON.stringify({ name: 'dygnosis', version: '0.1.0', packages: { '': { name: 'dygnosis', version: '0.1.0' } } }),
    'help/reference.json': JSON.stringify({ version: '0.1.0', tools: [], codes: [], options: [] }, null, 2) + '\n',
  };
  for (const [file, text] of Object.entries(files)) {
    await fs.mkdir(path.dirname(path.join(root, file)), { recursive: true });
    await fs.writeFile(path.join(root, file), text);
  }
  const state = { builds: 0, lsp: path.join(root, 'custom target', 'debug', 'dygnosis.exe'),
    executable: path.join(root, 'custom target', 'debug', 'dygnosis-mcp.exe'), calls: [], tools: [], version: '0.1.0' };
  t.mock.method(childProcess, 'execFileSync', (command, args, options) => {
    assert.equal(command, 'cargo');
    assert.equal(options.cwd, root);
    assert.ok(args.includes('--locked'));
    assert.deepEqual(args.flatMap((value, index) => value === '--bin' ? [args[index + 1]] : []), ['dygnosis', 'dygnosis-mcp']);
    assert.ok(args.includes('--message-format=json'));
    state.builds++;
    return [
      { reason: 'compiler-artifact', target: { name: 'other', kind: ['bin'] }, executable: 'wrong' },
      { reason: 'compiler-artifact', target: { name: 'dygnosis', kind: ['bin'] }, executable: state.lsp },
      { reason: 'compiler-artifact', target: { name: 'dygnosis-mcp', kind: ['bin'] }, executable: state.executable },
    ].map(item => JSON.stringify(item)).join('\n');
  });
  t.mock.method(common, 'rpc', executable => {
    state.calls.push(executable);
    return {
      notify() {}, close() {},
      async request(method, args) {
        if (method === 'initialize') return { serverInfo: { version: state.version } };
        if (method === 'tools/list') return { tools: state.tools };
        assert.equal(method, 'tools/call');
        const data = args.name === 'dynare_diagnostic_info' ? { diagnostics: [] } : { forms: [] };
        return { structuredContent: { schema_version: 3, outcome: { status: 'ok' }, data, selection: null } };
      },
    };
  });
  return { root, state, reference: path.join(root, 'help/reference.json'), stamp: path.join(root, 'editors/vscode/out/help-reference-state.json') };
}

// Guards first-run generation choosing an old bundled engine or guessing Cargo's target folder.
test('Help builds the workspace engine reported by Cargo and reuses a verified local stamp', async t => {
  const { root, state, stamp } = await fixture(t);
  await help.ensureReference({ root });
  assert.equal(state.builds, 1);
  assert.deepEqual(state.calls, [state.executable]);
  assert.equal(JSON.parse(await fs.readFile(stamp, 'utf8')).fingerprint, await help.engineFingerprint(root));
  await help.ensureReference({ root });
  assert.equal(state.builds, 1, 'a current reference must not require Cargo or an engine process');
  assert.equal(state.calls.length, 1);
});

// Guards formatting or unrelated Rust edits causing a tracked fingerprint-only diff.
test('source changes refresh the local stamp without rewriting identical Help content', async t => {
  const { root, state, reference, stamp } = await fixture(t);
  await help.ensureReference({ root });
  const before = await fs.readFile(reference, 'utf8');
  const oldTime = new Date('2000-01-01T00:00:00Z');
  await fs.utimes(reference, oldTime, oldTime);
  await fs.appendFile(path.join(root, 'src/mcp/mod.rs'), '// Formatting changed\n');
  await help.ensureReference({ root });
  assert.equal(state.builds, 2);
  assert.equal(await fs.readFile(reference, 'utf8'), before);
  assert.equal((await fs.stat(reference)).mtimeMs, oldTime.getTime());
  assert.equal(JSON.parse(await fs.readFile(stamp, 'utf8')).fingerprint, await help.engineFingerprint(root));
  assert.equal(Object.hasOwn(JSON.parse(before), 'fingerprint'), false);
});

// Guards a modified/deleted reference being trusted because source text did not change.
test('reference changes and missing references invalidate the local stamp', async t => {
  const { root, state, reference } = await fixture(t);
  await help.ensureReference({ root });
  state.tools = [{ name: 'current_tool' }];
  await fs.writeFile(reference, JSON.stringify({ version: '0.1.0', tools: [{ name: 'stale_tool' }] }));
  await help.ensureReference({ root });
  assert.equal(state.builds, 2);
  assert.deepEqual(JSON.parse(await fs.readFile(reference, 'utf8')).tools, state.tools);
  await fs.unlink(reference);
  await help.ensureReference({ root });
  assert.equal(state.builds, 3);
  assert.deepEqual(JSON.parse(await fs.readFile(reference, 'utf8')).tools, state.tools);
});

// Guards explicit regeneration using a bundled executable, and F5 rebuilding twice.
test('forced generation rebuilds; a caller-supplied matching engine is used directly', async t => {
  const { root, state } = await fixture(t);
  await help.ensureReference({ root });
  await help.ensureReference({ root, force: true });
  assert.equal(state.builds, 2);
  const staged = path.join(root, 'staged engine.exe');
  await help.ensureReference({ root, force: true, executable: staged });
  assert.equal(state.builds, 2);
  assert.equal(state.calls.at(-1), staged);
});

// Guards failed builds/exports marking old Help as fresh or replacing the last valid reference.
test('a failed refresh preserves the reference and its last verified stamp', async t => {
  const { root, state, reference, stamp } = await fixture(t);
  await help.ensureReference({ root });
  const before = await fs.readFile(reference, 'utf8'), oldStamp = await fs.readFile(stamp, 'utf8');
  state.version = '0.0.1';
  await assert.rejects(help.ensureReference({ root, force: true }), /matching engine/);
  assert.equal(await fs.readFile(reference, 'utf8'), before);
  assert.equal(await fs.readFile(stamp, 'utf8'), oldStamp);
  t.mock.method(childProcess, 'execFileSync', () => { throw new Error('compiler failure'); });
  await assert.rejects(help.ensureReference({ root, force: true }), /compiler failure/);
  assert.equal(await fs.readFile(reference, 'utf8'), before);
  assert.equal(await fs.readFile(stamp, 'utf8'), oldStamp);
});

// Guards an interrupted local stamp becoming a permanent manual-repair requirement.
test('an invalid local stamp is rebuilt automatically', async t => {
  const { root, state, stamp } = await fixture(t);
  await help.ensureReference({ root });
  await fs.writeFile(stamp, '{interrupted');
  await help.ensureReference({ root });
  assert.equal(state.builds, 2);
  assert.equal(JSON.parse(await fs.readFile(stamp, 'utf8')).fingerprint, await help.engineFingerprint(root));
});

// Guards source edits during a Cargo build blessing an export from an older input snapshot.
test('a source change during refresh keeps the previous reference and stamp', async t => {
  const { root, reference, stamp, state } = await fixture(t);
  await help.ensureReference({ root });
  const before = await fs.readFile(reference, 'utf8'), oldStamp = await fs.readFile(stamp, 'utf8');
  t.mock.method(childProcess, 'execFileSync', () => {
    fsSync.appendFileSync(path.join(root, 'src/mcp/mod.rs'), '// Edited during build\n');
    return [
      { reason: 'compiler-artifact', target: { name: 'dygnosis', kind: ['bin'] }, executable: state.lsp },
      { reason: 'compiler-artifact', target: { name: 'dygnosis-mcp', kind: ['bin'] }, executable: state.executable },
    ].map(item => JSON.stringify(item)).join('\n');
  });
  await assert.rejects(help.ensureReference({ root, force: true }), /source changed during Help generation/);
  assert.equal(await fs.readFile(reference, 'utf8'), before);
  assert.equal(await fs.readFile(stamp, 'utf8'), oldStamp);
});
