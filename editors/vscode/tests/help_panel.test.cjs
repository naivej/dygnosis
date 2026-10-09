const assert = require('node:assert/strict');
const test = require('node:test');
const path = require('node:path');
const Module = require('node:module');
class Disposable {
  constructor(callback = () => {}) { this.callback = callback; }
  dispose() { this.callback(); }
  static from(...items) { return new Disposable(() => items.forEach(item => item.dispose())); }
}
const uri = fsPath => ({ fsPath, toString: () => fsPath });
let host;
const vscode = {
  Disposable, ViewColumn: { Active: 1 },
  Uri: { joinPath: (root, ...parts) => uri(path.join(root.fsPath, ...parts)), parse: text => ({ toString: () => text }) },
  env: { uriScheme: 'vscode', clipboard: { writeText: text => { host.clipboard = text; return Promise.resolve(); } }, openExternal: target => { host.external.push(target.toString()); return Promise.resolve(true); } },
  workspace: {},
  commands: {
    registerCommand: (name, run) => { host.commands.set(name, run); return new Disposable(() => host.commands.delete(name)); },
    executeCommand: (name, ...args) => { host.executed.push([name, ...args]); return Promise.resolve(); },
  },
  window: {
    createWebviewPanel: (viewType, title, column, options) => { const panel = makePanel(); panel.options = options; host.panels.push(panel); return panel; },
    registerWebviewPanelSerializer: (_name, serializer) => { host.serializer = serializer; return new Disposable(); },
    registerUriHandler: handler => { host.links = handler; return new Disposable(); },
    showInformationMessage: () => { ++host.invitations; return Promise.resolve(undefined); },
  },
};
function makePanel() {
  const panel = { webview: { cspSource: 'local-resource', asWebviewUri: uri => uri,
    onDidReceiveMessage: run => { panel.receive = run; return new Disposable(); },
    postMessage: message => { panel.messages.push(message); return Promise.resolve(true); } },
    messages: [], reveal() {}, dispose() { panel.close?.(); }, onDidDispose: run => { panel.close = run; return new Disposable(); } };
  return panel;
}
const load = Module._load;
Module._load = function (name, ...args) { if (name === 'vscode') return vscode; return load.call(this, name, ...args); };
const { registerHelp } = require('../out/help');
Module._load = load;
function setup(shown = false) {
  host = { commands: new Map(), executed: [], external: [], panels: [], invitations: 0 };
  const state = new Map(shown ? [['helpInvitationShown', true]] : []);
  const context = { extensionUri: uri(path.resolve(__dirname, '..')), extension: { id: 'CoconutWater.dygnosis' },
    globalState: { get: key => state.get(key), update: (key, value) => { state.set(key, value); return Promise.resolve(); } } };
  const registration = registerHelp(context);
  return { registration, state, open: argument => host.commands.get('dygnosis.openHelp')(argument), host };
}

test('Help starts with no engine, persists invitation dismissal and restores a panel', async () => {
  const env = setup(); await new Promise(resolve => setImmediate(resolve));
  assert.equal(env.host.invitations, 1); assert.equal(env.state.get('helpInvitationShown'), true);
  env.open('diagnostics'); const panel = env.host.panels[0];
  assert.equal(panel.options.enableFindWidget, true);
  assert.match(panel.webview.html, /default-src 'none'/); assert.match(panel.webview.html, /Copy topic link/);
  await panel.receive({ type: 'ready' });
  assert.equal(panel.messages[0].type, 'bundle'); assert.equal(panel.messages[1].destination, 'diagnostics');
  env.open('get-started'); assert.equal(env.host.panels.length, 1);
  assert.equal(panel.messages.at(-1).destination, 'get-started');
  assert.deepEqual(env.host.executed, []);
  panel.dispose(); const restored = makePanel(); await env.host.serializer.deserializeWebviewPanel(restored);
  await restored.receive({ type: 'ready' }); assert.equal(restored.messages[0].type, 'bundle');
  env.registration.dispose(); assert.equal(env.host.commands.size, 0);
  const again = setup(true); await new Promise(resolve => setImmediate(resolve)); assert.equal(again.host.invitations, 0); again.registration.dispose();
});

test('messages dispatch only known native actions and topic permalinks', async () => {
  const env = setup(true); env.open(); const panel = env.host.panels[0]; await panel.receive({ type: 'ready' });
  for (const target of ['command:workbench.action.quit', 'settings:unknown.key', 'javascript:evil', 'action:delete']) await panel.receive({ type: 'link', target });
  assert.deepEqual(env.host.executed, []); assert.deepEqual(env.host.external, []);
  await panel.receive({ type: 'link', target: 'settings:dynare.searchPaths' });
  assert.deepEqual(env.host.executed.pop(), ['workbench.action.openSettings', '@id:dynare.searchPaths']);
  await panel.receive({ type: 'permalink', destination: 'diagnostics' });
  assert.equal(env.host.clipboard, 'vscode://CoconutWater.dygnosis/help?topic=diagnostics');
  env.host.links.handleUri({ path: '/help', query: 'topic=check%3AE020' });
  assert.deepEqual(env.host.executed.pop(), ['dygnosis.openHelp', 'check:E020']);
  env.host.links.handleUri({ path: '/run', query: 'topic=diagnostics' }); assert.deepEqual(env.host.executed, []);
  env.registration.dispose();
});

test('active engine explanation is escaped and labelled beside the bundled edition', async () => {
  const env = setup(true); env.open({ code: 'E999', markdown: '# Active\n\n<script>evil()</script>', engineVersion: '0.10.0' });
  const panel = env.host.panels[0]; await panel.receive({ type: 'ready' });
  const opened = panel.messages[1]; assert.equal(opened.destination, 'check:E999');
  assert.equal(opened.extra.edition, `Active engine 0.10.0; Help ${require('../package.json').version}`); assert.doesNotMatch(opened.extra.html, /<script/);
  env.registration.dispose();
});

test('restored explanation history is rendered from validated Markdown, not saved HTML', async () => {
  const env = setup(true); env.open(); const panel = env.host.panels[0];
  await panel.receive({ type: 'ready', history: [
    { destination: 'check:E999', explanation: { code: 'E999', markdown: '# Saved\n\n<script>evil()</script>', engineVersion: '0.10.0' }, html: '<script>evil()</script>' },
    { destination: 'check:E020', explanation: { code: 'E999', markdown: 'Wrong code' } },
  ] });
  const restored = panel.messages[0].historyExtras;
  assert.match(restored[0].html, /Saved/); assert.doesNotMatch(restored[0].html, /<script/);
  assert.match(restored[0].edition, /Active engine 0.10.0/); assert.equal(restored[1], undefined);
  env.registration.dispose();
});
