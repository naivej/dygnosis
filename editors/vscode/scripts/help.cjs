/* global console */
const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const { createHash } = require('node:crypto');
const { productRoot, extensionRoot, rpc } = require('./common.cjs');
const source = path.join(productRoot, 'help');

async function engineFingerprint() {
  const files = ['src/explain.rs', 'src/mcp.rs', 'Cargo.toml', 'Cargo.lock'];
  // Options live in the command catalog. Include all Rust inputs so a changed
  // fact cannot leave an apparently current snapshot behind.
  async function walk(dir) {
    for (const entry of await fs.readdir(dir, { withFileTypes: true })) {
      const file = path.join(dir, entry.name);
      if (entry.isDirectory()) await walk(file);
      else if (entry.name.endsWith('.rs')) files.push(path.relative(productRoot, file).replaceAll('\\', '/'));
    }
  }
  await walk(path.join(productRoot, 'src'));
  const hash = createHash('sha256');
  for (const file of [...new Set(files)].sort()) hash.update(file).update((await fs.readFile(path.join(productRoot, file), 'utf8')).replaceAll('\r\n', '\n'));
  return hash.digest('hex');
}

async function generate(executable) {
  const session = rpc(executable, ['mcp'], false);
  try {
    const init = await session.request('initialize', { protocolVersion: '2024-11-05', capabilities: {}, clientInfo: { name: 'dygnosis-help-build', version: '1' } });
    const manifest = require('../package.json');
    assert.equal(init.serverInfo.version, manifest.version, 'Help must use the matching engine');
    session.notify('notifications/initialized', {});
    const { tools } = await session.request('tools/list', {});
    async function call(name, args = {}) {
      const result = await session.request('tools/call', { name, arguments: args });
      assert.ok(!result.isError, `${name} failed`);
      return result.content.filter(item => item.type === 'text').map(item => item.text).join('\n');
    }
    const codes = JSON.parse(await call('dynare_list_diagnostic_codes'));
    for (const code of codes) code.markdown = await call('dynare_explain', { code: code.code });
    const commands = JSON.parse(await call('dynare_list_options')).commands;
    const options = [];
    for (const command of commands) options.push(JSON.parse(await call('dynare_list_options', { command })));
    await fs.writeFile(path.join(source, 'reference.json'), JSON.stringify({ version: init.serverInfo.version, fingerprint: await engineFingerprint(), tools, codes, options }, null, 2) + '\n');
  } finally { session.close(); }
}

async function build() {
  const manifest = require('../package.json');
  const registry = JSON.parse(await fs.readFile(path.join(source, 'topics.json'), 'utf8'));
  const reference = JSON.parse(await fs.readFile(path.join(source, 'reference.json'), 'utf8'));
  assert.equal(reference.version, manifest.version, 'Regenerate Help from the matching engine');
  assert.equal(reference.fingerprint, await engineFingerprint(), 'Engine source changed. Press F5 so the launch task rebuilds the engine and refreshes Help.');
  const ids = new Set(registry.map(topic => topic.id));
  assert.equal(ids.size, registry.length);
  const settings = Object.assign({}, ...manifest.contributes.configuration.map(group => group.properties));
  const topics = [];
  for (const topic of registry) {
    assert.match(topic.id, /^[a-z][a-z-]+$/);
    const markdown = await fs.readFile(path.join(source, `${topic.id}.md`), 'utf8');
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
    for (const [, file] of topic.markdown.matchAll(/!\[[^\]]*\]\(([^)]+)\)/g)) {
      assert.match(file, /^assets\/[a-zA-Z0-9/_-]+\.(png|jpg|svg)$/);
      assert.ok((await fs.stat(path.join(source, file))).isFile(), `Missing image ${file}`);
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
  const assets = path.join(target, 'assets');
  assert.equal(path.dirname(path.dirname(assets)), extensionRoot);
  await fs.rm(assets, { recursive: true, force: true });
  await fs.cp(path.join(source, 'assets'), assets, {
    recursive: true,
    filter: (src) => path.basename(src) !== 'originals' && !src.split(path.sep).includes('originals'),
  });
  // The Marketplace introduction is generated from the short product README.
  // This file is editors/vscode/README.md. Two levels up reaches product help/.
  // vsce joins the link onto .../editors/vscode; the client then resolves "..".
  // The listing already shows the extension icon, so the README omits the logo.
  const readme = (await fs.readFile(path.join(productRoot, 'README.md'), 'utf8'))
    .replace(/!\[[^\]]*\]\([^)]*logo[^)]*\)\s*/g, '');
  await fs.writeFile(path.join(extensionRoot, 'README.md'), readme
    .replace(/\]\((help\/[^)]+\.md(?:#[^)]*)?|help\/assets\/[^)]+|LICENSE|CHANGELOG\.md)\)/g, '](../../$1)'));
  console.log(`Help: ${topics.length} topics, ${reference.codes.length} checks, ${reference.tools.length} tools, ${Object.keys(settingsTopics).length} native settings`);
}

if (require.main === module) (async () => {
  if (process.argv[2] === '--generate') await generate(path.resolve(process.argv[3] ?? path.join(extensionRoot, 'bin', process.platform === 'win32' ? 'dygnosis.exe' : 'dygnosis')));
  await build();
})().catch(error => { console.error(error); process.exitCode = 1; });
module.exports = { build, generate, engineFingerprint };
