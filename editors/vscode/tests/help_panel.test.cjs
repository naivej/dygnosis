const { compiledModule } = require("./helpers/extension_modules.cjs");
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
  Disposable, ViewColumn: { Active: -1, Beside: -2 },
  TabInputCustom: class { constructor(viewType) { this.viewType = viewType; } },
  Uri: { joinPath: (root, ...parts) => uri(path.join(root.fsPath, ...parts)), parse: text => ({ toString: () => text }) },
  env: { uriScheme: 'vscode', clipboard: { writeText: text => { host.clipboard = text; return Promise.resolve(); } }, openExternal: target => { host.external.push(target.toString()); return Promise.resolve(true); } },
  workspace: {},
  commands: {
    registerCommand: (name, run) => { host.commands.set(name, run); return new Disposable(() => host.commands.delete(name)); },
    executeCommand: (name, ...args) => { host.executed.push([name, ...args]); return Promise.resolve(); },
  },
  window: {
    get activeTextEditor() { return host.editor; },
    tabGroups: { get activeTabGroup() { return { activeTab: host.tab }; } },
    createWebviewPanel: (viewType, title, column, options) => { const panel = makePanel(); panel.column = column; panel.options = options; host.panels.push(panel); return panel; },
    registerWebviewPanelSerializer: (_name, serializer) => { host.serializer = serializer; return new Disposable(); },
    registerUriHandler: handler => { host.links = handler; return new Disposable(); },
    showInformationMessage: () => { ++host.invitations; return Promise.resolve(undefined); },
  },
};
function makePanel() {
  const panel = { webview: { cspSource: 'local-resource', asWebviewUri: uri => uri,
    onDidReceiveMessage: run => { panel.receive = run; return new Disposable(); },
    postMessage: message => { panel.messages.push(message); return Promise.resolve(true); } },
    messages: [], reveal() { ++host.reveals; }, dispose() { panel.close?.(); }, onDidDispose: run => { panel.close = run; return new Disposable(); } };
  return panel;
}
const load = Module._load;
Module._load = function (name, ...args) { if (name === 'vscode') return vscode; return load.call(this, name, ...args); };
const { registerHelp } = require(compiledModule("help"));
Module._load = load;
function setup() {
  host = { commands: new Map(), executed: [], external: [], panels: [], invitations: 0, reveals: 0 };
  const context = { extensionUri: uri(path.resolve(__dirname, '..')), extension: { id: 'CoconutWater.dygnosis' } };
  const registration = registerHelp(context);
  return { registration, open: argument => host.commands.get('dygnosis.openHelp')(argument), host };
}

test('Help opens and restores without a model, engine or startup invitation', async () => {
  const env = setup(); await new Promise(resolve => setImmediate(resolve));
  assert.equal(env.host.invitations, 0);
  env.open('diagnostics'); const panel = env.host.panels[0];
  assert.equal(panel.column, vscode.ViewColumn.Active);
  assert.equal(panel.options.enableFindWidget, true);
  assert.match(panel.webview.html, /default-src 'none'/); assert.match(panel.webview.html, /Copy topic link/);
  await panel.receive({ type: 'ready' });
  assert.equal(panel.messages[0].type, 'bundle'); assert.equal(panel.messages[1].destination, 'diagnostics');
  env.open('get-started'); assert.equal(env.host.panels.length, 1);
  assert.equal(panel.messages.at(-1).destination, 'get-started');
  assert.deepEqual(env.host.executed, [['setContext', 'dygnosis.walkthroughHelpOpened', true]]);
  panel.dispose(); const restored = makePanel(); await env.host.serializer.deserializeWebviewPanel(restored);
  await restored.receive({ type: 'ready' }); assert.equal(restored.messages[0].type, 'bundle');
  env.registration.dispose(); assert.equal(env.host.commands.size, 0);
});

test('plain Help resumes reading and explicit links navigate in the existing panel', async () => {
  const env = setup();
  env.open('CoconutWater.dygnosis'); const panel = env.host.panels[0];
  await panel.receive({ type: 'ready' });
  assert.equal(panel.messages.at(-1).destination, 'get-started');
  env.open('effective-model');
  assert.equal(panel.messages.at(-1).destination, 'effective-model');
  const messages = panel.messages.length;
  env.open();
  env.open('coconutwater.dygnosis');
  assert.equal(panel.messages.length, messages, 'reveal sends no navigation that could replace the current article or search');
  assert.equal(env.host.reveals, 3);
  assert.equal(env.host.panels.length, 1);
  env.registration.dispose();
});

test('new Help opens beside a model, a macro expansion or a Changes editor', async () => {
  for (const [editor, tab, topic] of [
    [{ document: { languageId: 'dynare', uri: { scheme: 'file' } } }, undefined, 'get-started'],
    [{ document: { languageId: 'dynare', uri: { scheme: 'dygnosis-effective' } } }, undefined, 'effective-model'],
    [undefined, { input: new vscode.TabInputCustom('dygnosis.changes') }, 'get-started'],
  ]) {
    const env = setup();
    env.host.editor = editor;
    env.host.tab = tab;
    env.open();
    const panel = env.host.panels[0];
    assert.equal(panel.column, vscode.ViewColumn.Beside);
    await panel.receive({ type: 'ready' });
    assert.equal(panel.messages.at(-1).destination, topic);
    env.registration.dispose();
  }
});

test('messages dispatch only known native actions and topic permalinks', async () => {
  const env = setup(); env.open(); const panel = env.host.panels[0]; await panel.receive({ type: 'ready' });
  env.host.executed.length = 0;
  for (const target of ['command:workbench.action.quit', 'settings:unknown.key', 'javascript:evil', 'action:delete']) await panel.receive({ type: 'link', target });
  assert.deepEqual(env.host.executed, []); assert.deepEqual(env.host.external, []);
  await panel.receive({ type: 'link', target: 'settings:dynare.searchPaths' });
  assert.deepEqual(env.host.executed.pop(), ['workbench.action.openSettings', '@id:dynare.searchPaths']);
  await panel.receive({ type: 'link', target: 'action:settings' });
  assert.deepEqual(env.host.executed.pop(), ['dygnosis.openSettings']);
  await panel.receive({ type: 'permalink', destination: 'diagnostics' });
  assert.equal(env.host.clipboard, 'vscode://CoconutWater.dygnosis/help?topic=diagnostics');
  env.host.links.handleUri({ path: '/help', query: 'topic=check%3AE020' });
  assert.deepEqual(env.host.executed.pop(), ['dygnosis.openHelp', 'check:E020']);
  env.host.links.handleUri({ path: '/run', query: 'topic=diagnostics' }); assert.deepEqual(env.host.executed, []);
  env.registration.dispose();
});

test('old Support requests, links and saved destinations resolve to the merged topic', async () => {
  const env = setup(); env.open('troubleshoot'); const panel = env.host.panels[0];
  await panel.receive({ type: 'ready', history: [{ destination: 'troubleshoot#report-a-problem' }] });
  assert.equal(panel.messages[0].historyDetails[0].destination, 'about#report-a-problem');
  assert.equal(panel.messages[1].destination, 'about#support');
  env.host.links.handleUri({ path: '/help', query: 'topic=troubleshoot%23report-a-problem' });
  assert.deepEqual(env.host.executed.pop(), ['dygnosis.openHelp', 'about#report-a-problem']);
  env.registration.dispose();
});

test('active engine explanation takes precedence over the same bundled code and is escaped', async () => {
  const env = setup(); env.open({ code: 'E001', markdown: '# Active\n\n<script>evil()</script>', engineVersion: '0.10.0' });
  const panel = env.host.panels[0]; await panel.receive({ type: 'ready' });
  const opened = panel.messages[1]; assert.equal(opened.destination, 'check:E001');
  assert.ok(panel.messages[0].topics.some(topic => topic.id === opened.destination));
  assert.equal(opened.extra.edition, 'Explanation from engine 0.10.0'); assert.doesNotMatch(opened.extra.html, /<script/);
  env.registration.dispose();
});

test('restored explanation history is rendered from validated Markdown, not saved HTML', async () => {
  const env = setup(); env.open(); const panel = env.host.panels[0];
  await panel.receive({ type: 'ready', history: [
    { destination: 'check:E999', explanation: { code: 'E999', markdown: '# Saved\n\n<script>evil()</script>', engineVersion: '0.10.0' }, html: '<script>evil()</script>' },
    { destination: 'check:E020', explanation: { code: 'E999', markdown: 'Wrong code' } },
  ] });
  const restored = panel.messages[0].historyDetails;
  assert.match(restored[0].extra.html, /Saved/); assert.doesNotMatch(restored[0].extra.html, /<script/);
  assert.equal(restored[0].extra.edition, 'Explanation from engine 0.10.0'); assert.equal(restored[1].extra, undefined);
  env.registration.dispose();
});
