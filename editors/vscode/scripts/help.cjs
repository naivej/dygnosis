/* global console */
const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const { createHash } = require('node:crypto');
const common = require('./common.cjs');
const { productRoot, extensionRoot } = common;
const { readVersion, syncMetadata, syncVersion } = require('../../../scripts/version.cjs');
const { buildEngine } = require('../../../scripts/build-engine.cjs');
const source = path.join(productRoot, 'help');

/** Marketplace listing and Help About. Both omit the logo: the listing already shows the extension icon, and Help only packs images under help/assets. */
function productIntroduction(markdown) {
  return markdown.replace(/!\[[^\]]*\]\([^)]*logo[^)]*\)\s*/g, '');
}

async function copyHelpAssets(topics, sourceRoot, targetRoot) {
  const images = new Set();
  for (const topic of topics) for (const [, file] of topic.markdown.matchAll(/!\[[^\]]*\]\(([^)]+)\)/g)) {
    assert.match(file, /^assets\/[a-zA-Z0-9/_-]+\.(png|jpg|svg)$/);
    assert.ok((await fs.stat(path.join(sourceRoot, file))).isFile(), `Missing image ${file}`);
    images.add(file);
  }
  const assets = path.resolve(targetRoot, 'assets');
  assert.equal(path.dirname(assets), path.resolve(targetRoot));
  assert.notEqual(assets, path.resolve(sourceRoot, 'assets'));
  await fs.rm(assets, { recursive: true, force: true });
  for (const file of images) {
    const destination = path.join(targetRoot, file);
    await fs.mkdir(path.dirname(destination), { recursive: true });
    await fs.copyFile(path.join(sourceRoot, file), destination);
  }
}

async function engineFingerprint(root = productRoot) {
  const files = ['src/engine/diagnostics/explain.rs', 'src/mcp/mod.rs', 'Cargo.toml', 'Cargo.lock', 'editors/vscode/scripts/help.cjs'];
  // The catalog consumes generated JSON as well as Rust. Include both so a
  // changed syntax fact cannot leave an apparently current snapshot behind.
  async function walk(dir) {
    for (const entry of await fs.readdir(dir, { withFileTypes: true })) {
      const file = path.join(dir, entry.name);
      if (entry.isDirectory()) await walk(file);
      else if (/\.(rs|json)$/.test(entry.name)) files.push(path.relative(root, file).replaceAll('\\', '/'));
    }
  }
  await walk(path.join(root, 'src'));
  const hash = createHash('sha256');
  for (const file of [...new Set(files)].sort()) hash.update(file).update((await fs.readFile(path.join(root, file), 'utf8')).replaceAll('\r\n', '\n'));
  return hash.digest('hex');
}

async function optionalText(file) {
  return fs.readFile(file, 'utf8').catch(error => {
    if (error.code === 'ENOENT') return null;
    throw error;
  });
}

function savedObject(text) {
  try { return text == null ? null : JSON.parse(text); }
  catch (error) { if (error instanceof SyntaxError) return null; throw error; }
}

const referenceHash = text => createHash('sha256').update(text).digest('hex');
const stateFile = root => path.join(root, 'editors/vscode/out/help-reference-state.json');

/** Verify both source and reference bytes; refresh from a workspace build only when needed. */
async function ensureReference({ root = productRoot, force = false, executable } = {}) {
  const fingerprint = await engineFingerprint(root);
  const text = await optionalText(path.join(root, 'help/reference.json'));
  const reference = savedObject(text);
  const state = savedObject(await optionalText(stateFile(root)));
  if (!force && reference?.version === await readVersion(root)
      && state?.fingerprint === fingerprint && state.reference_sha256 === referenceHash(text)) return;
  console.log('Refreshing Help reference.');
  await generate(executable ?? buildEngine(root).mcp, { root, fingerprint });
}

async function collectPages(call, name, args = {}) {
  const query = { page_size: 1000, ...args };
  const pages = [], seen = new Set();
  let continuation;
  do {
    const result = common.canonicalMcpResult(await call(name, { ...query, ...(continuation ? { continuation } : {}) }));
    assert.equal(result.outcome?.status, 'ok', `${name}: ${JSON.stringify(result.outcome)}`);
    pages.push(result.data);
    continuation = result.selection?.continuation;
    if (continuation != null) {
      assert.equal(typeof continuation, 'string', `${name} returned an invalid continuation`);
      assert.ok(continuation && !seen.has(continuation), `${name} repeated a continuation without completing its catalog`);
      assert.ok(Object.values(result.selection.collections ?? {}).some(collection => collection.returned_count > 0), `${name} returned a continuation without a catalog page`);
      seen.add(continuation);
    }
  } while (continuation != null);
  return pages;
}

/** Keep the Help reader's code/options projection while using canonical tools. */
async function referenceData(call, pageSize = 1000) {
  const query = args => ({ page_size: pageSize, ...args });
  const codes = (await collectPages(call, 'dynare_diagnostic_info', query({}))).flatMap(data => data.diagnostics);
  for (const code of codes) {
    const details = (await collectPages(call, 'dynare_diagnostic_info', query({ code: code.code }))).flatMap(data => data.diagnostics);
    assert.equal(details.length, 1, `Missing diagnostic detail for ${code.code}`);
    assert.equal(typeof details[0].explanation, 'string', `Missing explanation for ${code.code}`);
    code.markdown = details[0].explanation;
  }
  const options = [];
  const byCommand = new Map();
  for (const kind of ['command', 'block', 'declaration', 'statement_form']) {
    const heads = (await collectPages(call, 'dynare_syntax', query({ kind, context: 'top_level' }))).flatMap(data => data.forms);
    for (const head of heads) {
      // Query options as records so each page is independent of the containing
      // form's option collection. Open partition keys are not finite options.
      const forms = (await collectPages(call, 'dynare_syntax', query({ kind: 'option', parent: head.name }))).flatMap(data => data.forms);
      const entries = forms.map(form => ({ name: form.name, description: form.summary || form.value_rules?.join('; ') || form.signatures.join('; ') }));
      for (const command of [head.name, ...head.aliases]) {
        const page = { command, known: true, n_options: entries.length, options: entries };
        if (byCommand.has(command)) assert.deepEqual(byCommand.get(command), page, `Conflicting Help options for ${command}`);
        else { byCommand.set(command, page); options.push(page); }
      }
    }
  }
  // Qualified prior/options instructions have option parents without a fixed
  // top-level head. They retain their own Help pages as well.
  const optionForms = (await collectPages(call, 'dynare_syntax', query({ kind: 'option' }))).flatMap(data => data.forms);
  for (const parent of new Set(optionForms.map(form => form.parent).filter(Boolean))) {
    if (byCommand.has(parent)) continue;
    const forms = (await collectPages(call, 'dynare_syntax', query({ kind: 'option', parent }))).flatMap(data => data.forms);
    const entries = forms.map(form => ({ name: form.name, description: form.summary || form.value_rules?.join('; ') || form.signatures.join('; ') }));
    const page = { command: parent, known: true, n_options: entries.length, options: entries };
    byCommand.set(parent, page); options.push(page);
  }
  options.sort((a, b) => a.command < b.command ? -1 : a.command > b.command ? 1 : 0);
  assert.equal(new Set(options.map(entry => entry.command)).size, options.length, 'Syntax heads produced duplicate Help option pages');
  return { codes, options };
}

async function generate(executable, { root = productRoot, fingerprint } = {}) {
  fingerprint ??= await engineFingerprint(root);
  const session = common.rpc(executable, [], false);
  try {
    const init = await session.request('initialize', { protocolVersion: '2025-06-18', capabilities: {}, clientInfo: { name: 'dygnosis-help-build', version: '1' } });
    const manifest = JSON.parse(await fs.readFile(path.join(root, 'editors/vscode/package.json'), 'utf8'));
    const version = await readVersion(root);
    assert.equal(manifest.version, version, 'Synchronize the extension version before generating Help');
    assert.equal(init.serverInfo.version, version, 'Help must use the matching engine');
    session.notify('notifications/initialized', {});
    const { tools } = await session.request('tools/list', {});
    async function call(name, args = {}) {
      const result = await session.request('tools/call', { name, arguments: args });
      assert.ok(!result.isError, `${name} failed`);
      return result;
    }
    const { codes, options } = await referenceData(call);
    assert.equal(await engineFingerprint(root), fingerprint, 'Engine source changed during Help generation. Rerun the command; the previous reference was kept.');
    const referencePath = path.join(root, 'help/reference.json');
    const text = JSON.stringify({ version: init.serverInfo.version, tools, codes, options }, null, 2) + '\n';
    if (await optionalText(referencePath) !== text) await fs.writeFile(referencePath, text);
    await fs.mkdir(path.dirname(stateFile(root)), { recursive: true });
    await fs.writeFile(stateFile(root), JSON.stringify({ fingerprint, reference_sha256: referenceHash(text) }) + '\n');
  } finally { session.close(); }
}

async function build() {
  await syncVersion();
  const manifest = JSON.parse(await fs.readFile(path.join(extensionRoot, 'package.json'), 'utf8'));
  const registry = JSON.parse(await fs.readFile(path.join(source, 'topics.json'), 'utf8'));
  const reference = JSON.parse(await fs.readFile(path.join(source, 'reference.json'), 'utf8'));
  assert.equal(reference.version, manifest.version, 'Regenerate Help from the matching engine');
  const ids = new Set(registry.map(topic => topic.id));
  assert.equal(ids.size, registry.length);
  const introduction = productIntroduction(await fs.readFile(path.join(productRoot, 'README.md'), 'utf8'));
  assert.doesNotMatch(introduction, /\]\((?!https?:\/\/|#)[^)]+\)/, 'Product README links must be absolute http(s) URLs');
  const settings = Object.assign({}, ...manifest.contributes.configuration.map(group => group.properties));
  const topics = [];
  for (const topic of registry) {
    assert.match(topic.id, /^[a-z][a-z-]+$/);
    const markdown = topic.id === 'about' ? introduction : await fs.readFile(path.join(source, `${topic.id}.md`), 'utf8');
    assert.equal([...markdown.matchAll(/^```/gm)].length % 2, 0, `Unclosed code fence in ${topic.id}`);
    for (const [, key] of markdown.matchAll(/\]\(settings:([^)]*)\)/g)) assert.ok(Object.hasOwn(settings, key), `Unknown setting ${key}`);
    topics.push({ ...topic, markdown });
  }
  for (const topic of topics) {
    for (const [, id, anchor] of topic.markdown.matchAll(/\]\((?:help:)?([a-z][a-z-]+)(?:\.md)?(?:#([\w-]+))?\)/g)) {
      const destination = topics.find(page => page.id === id);
      assert.ok(destination, `Unknown topic ${id} in ${topic.id}`);
      if (anchor) {
        const headings = [...destination.markdown.matchAll(/^#+\s+(.+)$/gm)].map(([, heading]) => heading.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, ''));
        assert.ok(headings.includes(anchor), `Unknown heading ${id}#${anchor} in ${topic.id}`);
      }
    }
  }
  const target = path.join(extensionRoot, 'help');
  await fs.mkdir(target, { recursive: true });
  const settingsTopics = JSON.parse(await fs.readFile(path.join(source, 'settings-topics.json'), 'utf8'));
  const commandsTopics = JSON.parse(await fs.readFile(path.join(source, 'commands-topics.json'), 'utf8'));
  assert.deepEqual(Object.keys(commandsTopics).sort(), manifest.contributes.commands.map(command => command.command).sort(), 'Every command needs a feature owner');
  for (const id of Object.values(commandsTopics)) assert.ok(ids.has(id), `Unknown command owner ${id}`);
  assert.deepEqual(Object.keys(settingsTopics).sort(), Object.keys(settings).sort(), 'Every setting needs a feature owner');
  for (const id of Object.values(settingsTopics)) assert.ok(ids.has(id), `Unknown setting owner ${id}`);
  await fs.writeFile(path.join(target, 'bundle.json'), JSON.stringify({ version: manifest.version, topics, reference, commands: manifest.contributes.commands, keybindings: manifest.contributes.keybindings, settingsTopics, commandsTopics }) + '\n');
  await copyHelpAssets(topics, source, target);
  await fs.writeFile(path.join(extensionRoot, 'README.md'), introduction);
  console.log(`Help: ${topics.length} topics, ${reference.codes.length} checks, ${reference.tools.length} tools, ${Object.keys(settingsTopics).length} native settings`);
}

if (require.main === module) (async () => {
  const [mode, executable, ...extra] = process.argv.slice(2);
  assert.ok(mode === undefined || (mode === '--generate' && !extra.length), 'Usage: node scripts/help.cjs [--generate [executable]]');
  if (mode === '--generate') {
    await syncMetadata();
    await ensureReference({ force: true, executable: executable ? path.resolve(executable) : undefined });
  }
  await build();
})().catch(error => { console.error(error); process.exitCode = 1; });
module.exports = { build, generate, ensureReference, engineFingerprint, copyHelpAssets, collectPages, referenceData };
