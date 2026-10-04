const assert = require("node:assert/strict");
const test = require("node:test");
const Module = require("node:module");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const { URL } = require("node:url");
const { parseDiff, normalizeChoices, diffSections, changeKinds } = require("../out/diff_view");
class Disposable {
  constructor(callback) { this.callback = callback; }
  dispose() { this.callback?.(); this.callback = undefined; }
  static from(...items) { return new Disposable(() => items.forEach(item => item.dispose())); }
}
class Emitter {
  listeners = new Set();
  event = listener => { this.listeners.add(listener); return new Disposable(() => this.listeners.delete(listener)); };
  fire(value) { for (const listener of [...this.listeners]) listener(value); }
}
class CancellationTokenSource {
  token = { isCancellationRequested: false };
  cancel() { this.token.isCancellationRequested = true; }
  dispose() { this.disposed = true; }
}
const before = "file:///before/main.mod", after = "file:///after/variant.mod";
function uri(value) { const parsed = new URL(value); return { scheme: parsed.protocol.slice(0, -1), path: parsed.pathname, fsPath: decodeURIComponent(parsed.pathname), toString: () => value }; }
function location(value = before, line = 3) { return { uri: value, range: { start: { line, character: 2 }, end: { line, character: 12 } } }; }
function target(value, dimension = null) { return { occurrence_id: value.includes("before") ? "old:1" : "new:1", domain: dimension ? "heterogeneous" : "aggregate", dimension, written_locations: [location(value)] }; }
function fixture(options = {}) {
  const result = { added_endogenous: ["n"], removed_endogenous: ["k"], common_endogenous: ["c"], added_exogenous: [], removed_exogenous: [], common_exogenous: [],
    added_parameters: [], removed_parameters: [], common_parameters: ["beta"],
    symbols_changed: [{ name: "c", before: { kind: "endogenous", long_name: "Consumption", tex_name: "c" }, after: { kind: "endogenous", long_name: "Household consumption", tex_name: "c_h" } }],
    changed_parameter_values: [{ name: "beta", old_raw: "0.99", old_value: .99, new_raw: "0.98", new_value: .98 }],
    added_equations: [{ index: 1, text: "n = u", name: null, tags: {}, domain: "aggregate", dimension: null }],
    removed_equations: [], changed_equations: [{ index_old: 2, index_new: 2, text_old: "c = y", text_new: "c = 2*y", name_old: "goods", name_new: "goods", tags_old: { name: "goods" }, tags_new: { name: "goods" }, domain: "aggregate", dimension: null }],
    unmatched_same_name: [{ name: "repeat", dimension: null, removed: [{ index: 3, text: "a = 1", name: "repeat", tags: {}, domain: "aggregate", dimension: null }], added: [{ index: 3, text: "b = 2", name: "repeat", tags: {}, domain: "aggregate", dimension: null }] }],
    heterogeneous_equations: [{ dimension: "households", added: [], removed: [], changed: [{ index_old: 1, index_new: 1, text_old: "c_h = w", text_new: "c_h = w+t", name_old: null, name_new: null, tags_old: {}, tags_new: {}, domain: "heterogeneous", dimension: "households" }], unmatched_same_name: [] }],
    shock_setup_changes: [{ form: "stochastic_shock", role: "variance", target: "u", change: "changed", before: { block: "shocks", values: ["1"], overwrite: false }, after: { block: "shocks", values: ["2"], overwrite: false } },
      { form: "shock_group", role: "membership", target: "supply", change: "added", after: { block: "shock_groups", references: ["u"], overwrite: false } }],
    navigation: { schema_version: 1, before: { root_uri: before, revision: "old", complete: true }, after: { root_uri: after, revision: "new", complete: true }, rows: [] }, markdown: "ignored engine Markdown" };
  const nav = (id, kind, old, next, dimension = null) => result.navigation.rows.push({ id, kind, before: old ? target(before, dimension) : null, after: next ? target(after, dimension) : null });
  nav("/added_endogenous/0", "symbol", false, true); nav("/removed_endogenous/0", "symbol", true, false); nav("/common_endogenous/0", "symbol", true, true); nav("/common_parameters/0", "symbol", true, true);
  nav("/symbols_changed/0", "symbol", true, true); nav("/changed_parameter_values/0", "parameter", true, true); nav("/added_equations/0", "equation", false, true); nav("/changed_equations/0", "equation", true, true);
  nav("/unmatched_same_name/0/removed/0", "equation", true, false); nav("/unmatched_same_name/0/added/0", "equation", false, true);
  nav("/heterogeneous_equations/0/changed/0", "equation", true, true, "households"); nav("/shock_setup_changes/0", "shock", true, true); nav("/shock_setup_changes/1", "shock", false, true);
  return Object.assign(result, options);
}
function noChanges() {
  const result = fixture();
  for (const key of Object.keys(result)) if (Array.isArray(result[key])) result[key] = [];
  result.navigation.rows = []; return result;
}
const defaults = { layout: "auto", expansion: "changes", sections: [...diffSections], changeKinds: [...changeKinds] };
const flush = async () => { for (let i = 0; i < 5; i++) await new Promise(resolve => setImmediate(resolve)); };
function deferred() { let resolve; const promise = new Promise(done => { resolve = done; }); return { promise, resolve }; }

test("every legacy comparison section keeps exact pointer, values, direction and engine pairing", () => {
  const parsed = parseDiff(fixture(), before, after);
  assert.equal(parsed.rows.length, 11); assert.equal(parsed.complete, true);
  assert.deepEqual([...new Set(parsed.rows.map(row => row.section))], [...diffSections]);
  assert.equal(parsed.rows.find(row => row.id === "/added_endogenous/0").before, null);
  assert.equal(parsed.rows.find(row => row.id === "/removed_endogenous/0").after, null);
  const unpaired = parsed.rows.filter(row => row.kind === "unpaired");
  assert.equal(unpaired.length, 2); assert.equal(unpaired[0].after, null); assert.equal(unpaired[1].before, null);
  assert.match(parsed.rows.find(row => row.section === "parameters").before, /0.99/);
  assert.equal(parsed.rows.some(row => row.id.includes("common_")), false);
});
test("symbol scope moves keep both independent dimensions and missing navigation stays unavailable", () => {
  const result = fixture(), source = result.navigation.rows.find(row => row.id === "/symbols_changed/0");
  source.before.dimension = "firms"; source.after.dimension = "households";
  result.navigation.rows.find(row => row.id === "/added_equations/0").after = null;
  const rows = parseDiff(result, before, after).rows;
  assert.deepEqual(rows.find(row => row.id === source.id).scopes, ["firms", "households"]);
  assert.equal(rows.find(row => row.id === "/added_equations/0").navigation.after, null);
});
test("unmapped shock rows retain supplied navigation and independent legacy dimensions", () => {
  for (const legacy of [false, true]) {
    const result = fixture(), nav = result.navigation.rows.find(row => row.id === "/shock_setup_changes/0");
    nav.before = null; nav.after = null;
    if (legacy) {
      result.shock_setup_changes[0].before.heterogeneity = "households";
      result.shock_setup_changes[0].after.heterogeneity = "firms";
    } else nav.dimension = "households";
    const row = parseDiff(result, before, after).rows.find(row => row.id === nav.id);
    assert.deepEqual(row.scopes, legacy ? ["households", "firms"] : ["households"]);
    assert.equal(row.sideScopes.before, "households"); assert.equal(row.sideScopes.after, legacy ? "firms" : "households");
    assert.equal(row.navigation.before, null); assert.equal(row.navigation.after, null);
  }
});
test("malformed, wrong-root, duplicate pointers and unsafe source schemes fail visibly", () => {
  const cases = [value => delete value.navigation, value => value.navigation.schema_version = 2,
    value => value.navigation.before.root_uri = after, value => value.navigation.rows.push(value.navigation.rows[0]),
    value => value.navigation.rows.shift(), value => value.navigation.rows[0].after.written_locations[0].uri = "command:bad",
    value => value.changed_parameter_values[0].old_value = {}, value => value.changed_equations[0].text_new = {}];
  for (const mutate of cases) { const value = fixture(); mutate(value); assert.throws(() => parseDiff(value, before, after), /unsupported comparison/); }
});
test("incomplete expansion withholds rows and no-change responses remain complete", () => {
  assert.equal(parseDiff(noChanges(), before, after).rows.length, 0);
  assert.equal(parseDiff({ status: "incomplete", message: "partial" }, before, after).complete, false);
  const result = fixture(); result.navigation.after.complete = false;
  assert.deepEqual(parseDiff(result, before, after).rows, []);
});
test("control state normalizes lists and preserves empty sections and per-view choices", () => {
  const choices = normalizeChoices({ layout: "stacked", expansion: "none", sections: [], changeKinds: ["changed", "changed", "bad"], search: "<img>", scope: "firms", expanded: { x: true, y: 3 } }, defaults);
  assert.deepEqual(choices.sections, []); assert.deepEqual(choices.changeKinds, ["changed"]); assert.deepEqual(choices.expanded, { x: true });
  assert.equal(choices.layout, "stacked"); assert.equal(choices.scope, "firms"); assert.equal(choices.search, "<img>");
});

let host;
const vscode = {
  Disposable, CancellationTokenSource, ViewColumn: { Beside: 2 },
  Uri: { parse: uri, file: filename => uri(`file:///${filename.replaceAll("\\", "/").replace(/^\//, "")}`), joinPath: (base, ...parts) => uri(`${base.toString()}/${parts.join("/")}`) },
  commands: { registerCommand: (id, callback) => { host.commands.set(id, callback); return new Disposable(() => host.commands.delete(id)); } },
  workspace: {
    asRelativePath: current => current.path,
    getConfiguration: (_section, scope) => {
      host.scopes.push(scope); const settings = host.settings.get(scope.uri.toString()) ?? {};
      return { get: (key, fallback) => settings[key] ?? fallback };
    },
    onDidChangeConfiguration: listener => host.settingsChanged.event(listener),
  },
  window: {
    get activeTextEditor() { return host.editor; },
    showOpenDialog: async options => { host.dialogs.push(options); return host.select ? host.select() : [uri(after)]; },
    showQuickPick: async (items, options) => { host.picks.push({ items, options }); return host.pick ? host.pick(items) : items[0]; },
    createWebviewPanel: (type, title, column, options) => {
      const received = new Emitter(), closed = new Emitter(), visibility = new Emitter();
      const panel = { type, title, column, options, visible: true, received, closed, visibility, messages: [],
        webview: { cspSource: "vscode-webview:", asWebviewUri: current => uri(`vscode-webview://assets${current.path}`),
          postMessage: async message => { panel.messages.push(message); }, onDidReceiveMessage: received.event },
        onDidDispose: closed.event, onDidChangeViewState: visibility.event, reveal() { this.visible = true; this.visibility.fire(); }, dispose() { this.closed.fire(); } };
      host.panels.push(panel); return panel;
    },
  },
};
const originalLoad = Module._load;
Module._load = function(id, ...args) { if (id === "vscode") return vscode; if (id === "./client" && args[0]?.filename.endsWith("diff.js")) return {}; return originalLoad.call(this, id, ...args); };
const { registerDiff, diffPreferences } = require("../out/diff"); Module._load = originalLoad;
function setup() {
  host = { commands: new Map(), dialogs: [], picks: [], panels: [], scopes: [], settings: new Map(), settingsChanged: new Emitter(),
    editor: { document: { uri: uri(before), version: 1, isClosed: false } } };
  const changed = new Emitter(), document = host.editor.document;
  const service = { currentInstance: 1, client: { initializeResult: { capabilities: { experimental: { dygnosis: { compareModels: { navigation_schema_version: 1 } } } } } },
    result: fixture(), requests: [], validations: [], compares: [], jumps: [], failures: [], logged: [], revision: { [before]: "old", [after]: "new" },
    onDidChange: changed.event, log: message => service.logged.push(message), ensureStarted: async () => {},
    rootForDocument: async () => service.root ?? uri(before),
    modelInfo: async (root, current, fresh) => { service.requests.push({ root: root.toString(), document: current.toString(), fresh }); return service.info ? service.info(root) : { revision: service.revision[root.toString()], complete: true, client_instance: service.currentInstance }; },
    execute: async (command, args, token) => { service.compares.push({ command, args, token }); return service.compare ? service.compare() : service.result; },
    revalidate: async (root, revision, instance) => { service.validations.push({ root: root.toString(), revision, instance }); return service.validate ? service.validate(root, revision, instance) : revision === service.revision[root.toString()] && instance === service.currentInstance ? { revision, complete: true, client_instance: instance } : undefined; },
    openLocation: async (target, root, guard) => {
      if (service.load) await service.load();
      if (guard && !await guard({ uri: uri(target.uri), version: 1 })) return;
      service.jumps.push({ target, root: root.toString() });
    },
    failure: async message => { service.failures.push(message); },
  };
  const registration = registerDiff(service);
  const open = () => host.commands.get("dygnosis.diffWith")();
  const panel = () => host.panels.at(-1);
  const last = () => panel().messages.at(-1);
  const source = (rowId = "/changed_equations/0", side = "before", extra = {}) => panel().received.fire({ type: "openSource", token: last().token, rowId, side, ...extra });
  return { host, service, changed, registration, open, panel, last, source, document };
}
test("native dialog labels Before/After and both roots receive fresh model context", async () => {
  const env = setup(); await env.open();
  assert.match(env.host.dialogs[0].title, /After.*Before/); assert.equal(env.host.dialogs[0].canSelectMany, false);
  assert.equal(env.last().before, before); assert.equal(env.last().after, after); assert.equal(env.last().status, "ready");
  assert.deepEqual(env.service.requests, [{ root: before, document: before, fresh: true }, { root: after, document: after, fresh: true }]);
  assert.deepEqual(env.service.compares[0].args, [before, after]);
  assert.equal(env.panel().options.enableCommandUris, false); assert.equal(env.panel().options.retainContextWhenHidden, false);
  assert.match(env.panel().webview.html, /default-src 'none'/); assert.match(env.panel().webview.html, /script-src 'nonce-/);
  assert.equal(env.panel().webview.html.includes("c = 2*y"), false); env.registration.dispose();
});
test("selecting the same model deduplicates root queries and shows a current no-change comparison", async () => {
  const env = setup(); env.host.select = () => [uri(before)]; env.service.result = noChanges();
  env.service.result.navigation.after = { root_uri: before, revision: "old", complete: true };
  await env.open(); assert.equal(env.last().status, "ready"); assert.equal(env.last().rows.length, 0); assert.match(env.last().message, /No structural changes/);
  assert.equal(env.service.requests.length, 1); assert.equal(env.service.validations.length, 1);
  assert.deepEqual(env.service.compares[0].args, [before, before]); assert.equal(env.service.failures.length, 0); env.registration.dispose();
});
test("Before preferences use resource scope, normalize settings and honor empty sections", () => {
  const env = setup(); env.host.settings.set(before, { "diff.layout": "bad", "diff.sections": [], "diff.defaultChangeKinds": ["removed", "removed", "bad"] });
  const result = diffPreferences(uri(before), env.service.log);
  assert.equal(result.layout, "auto"); assert.deepEqual(result.sections, []); assert.deepEqual(result.changeKinds, ["removed"]);
  assert.ok(env.host.scopes.every(scope => scope.uri.toString() === before && scope.languageId === "dynare")); env.registration.dispose();
});
test("dialog cancellation and input/owner changes do not create a view", async () => {
  for (const change of [() => undefined, env => { env.document.version++; return [uri(after)]; }, env => { env.service.root = uri("file:///other.mod"); return [uri(after)]; }]) {
    const env = setup(); env.host.select = () => change(env); await env.open(); assert.equal(env.host.panels.length, 0); env.registration.dispose();
  }
});
test("exact row/side source requests validate both revisions and targets twice", async () => {
  const env = setup(); await env.open(); env.source(); await flush();
  assert.equal(env.service.jumps.length, 1); assert.equal(env.service.jumps[0].root, before);
  assert.deepEqual(env.service.validations.slice(-4).map(row => row.root), [before, after, before, after]);
  assert.equal(env.service.compares.length, 3); env.source("/added_equations/0", "after"); await flush();
  assert.equal(env.service.jumps[1].root, after); env.registration.dispose();
});
test("multiple contributing files use a native picker and preserve engine targets", async () => {
  const env = setup(); const row = env.service.result.navigation.rows.find(row => row.id === "/changed_equations/0");
  row.before.written_locations.push(location("file:///before/equations.inc", 7)); env.host.pick = items => items[1];
  await env.open(); env.source(); await flush(); assert.equal(env.host.picks.length, 1);
  assert.equal(env.service.jumps[0].target.uri, "file:///before/equations.inc"); env.registration.dispose();
});
test("invalid messages, nonexistent side and unavailable written targets cannot navigate", async () => {
  const env = setup(); env.service.result.navigation.rows.find(row => row.id === "/changed_equations/0").before = null;
  await env.open();
  env.source("/changed_equations/0", "before"); env.source("/added_equations/0", "before"); env.source("/not/a/row", "after");
  env.source("/added_equations/0", "after", { token: -1 }); env.source("/added_equations/0", "command:bad");
  env.panel().received.fire({ type: "executeCommand", command: "bad" }); await flush(); assert.equal(env.service.jumps.length, 0); env.registration.dispose();
});
test("overlays/dependencies/settings/restarts mark stale and refuse old source actions", async () => {
  const env = setup(); await env.open(); const token = env.last().token; env.changed.fire();
  assert.equal(env.last().status, "stale"); assert.equal(env.last().rows.length, 11); env.source(); await flush(); assert.equal(env.service.jumps.length, 0);
  env.panel().received.fire({ type: "refresh" }); await flush(); assert.equal(env.last().status, "ready");
  env.panel().received.fire({ type: "openSource", token, rowId: "/changed_equations/0", side: "before" }); await flush(); assert.equal(env.service.jumps.length, 0);
  env.service.currentInstance++; env.changed.fire(); assert.equal(env.last().status, "stale"); env.registration.dispose();
});
test("either revision or current exact row target changing refuses navigation", async () => {
  for (const mutate of [env => env.service.revision[after] = "newer", env => env.service.result.navigation.rows.find(row => row.id === "/changed_equations/0").before.occurrence_id = "different",
    env => env.service.result.navigation.rows.find(row => row.id === "/changed_equations/0").before.written_locations[0].range.start.line++]) {
    const env = setup(); await env.open(); mutate(env); env.source(); await flush(); assert.equal(env.service.jumps.length, 0); assert.equal(env.last().status, "stale"); env.registration.dispose();
  }
});
test("changes during source document loading are checked before native editor reveal", async () => {
  const env = setup(); await env.open(); const load = deferred(); env.service.load = () => load.promise;
  env.source(); await flush(); env.service.revision[after] = "newer"; load.resolve(); await flush();
  assert.equal(env.service.jumps.length, 0); assert.equal(env.last().status, "stale"); env.registration.dispose();
});
test("an old source validation cannot mark a newer ready comparison stale", async () => {
  const env = setup(); await env.open(); const pending = deferred(); env.service.validate = () => pending.promise;
  env.source(); await flush(); env.service.validate = undefined; env.service.result = noChanges();
  env.panel().received.fire({ type: "refresh" }); await flush(); const token = env.last().token;
  assert.equal(env.last().status, "ready"); pending.resolve(undefined); await flush();
  assert.equal(env.last().status, "ready"); assert.equal(env.last().token, token); assert.equal(env.last().rows.length, 0); assert.equal(env.service.jumps.length, 0); env.registration.dispose();
});
test("an old source loader or loading error cannot demote a newer ready comparison", async () => {
  for (const fails of [false, true]) {
    const env = setup(); await env.open(); const pending = deferred();
    env.service.load = async () => { await pending.promise; if (fails) throw new Error("old loading failure"); };
    env.source(); await flush(); env.service.result = noChanges(); env.panel().received.fire({ type: "refresh" }); await flush(); const token = env.last().token;
    assert.equal(env.last().status, "ready"); pending.resolve(); await flush();
    assert.equal(env.last().status, "ready"); assert.equal(env.last().token, token); assert.equal(env.last().rows.length, 0); assert.equal(env.service.jumps.length, 0); env.registration.dispose();
  }
});
test("superseded refresh is cancelled and its late result cannot replace a fresh comparison", async () => {
  const env = setup(); await env.open(); const old = deferred(); let calls = 0;
  env.service.compare = () => ++calls === 1 ? old.promise : noChanges();
  env.panel().received.fire({ type: "refresh" }); await flush(); const token = env.service.compares.at(-1).token;
  env.panel().received.fire({ type: "refresh" }); await flush(); assert.equal(token.isCancellationRequested, true);
  assert.equal(env.last().status, "ready"); assert.equal(env.last().rows.length, 0); old.resolve(fixture()); await flush();
  assert.equal(env.last().rows.length, 0); env.registration.dispose();
});
test("per-view choices survive refresh, hidden-tab ready and closing/reopening", async () => {
  const env = setup(); await env.open(); const choices = { ...env.last().choices, layout: "stacked", expansion: "none", search: "goods", scope: "aggregate", expanded: { "aggregateEquations:Aggregate equations": false }, changeKinds: ["changed"] };
  env.panel().received.fire({ type: "choices", key: env.last().key, choices }); env.panel().received.fire({ type: "refresh" }); await flush();
  assert.deepEqual(env.last().choices, choices); env.panel().visible = false; env.panel().visible = true; env.panel().received.fire({ type: "ready", key: env.last().key, choices });
  assert.deepEqual(env.last().choices, choices); env.panel().dispose(); await env.open(); assert.deepEqual(env.last().choices, choices); env.registration.dispose();
});
test("hidden-tab restoration retains newer resource sections, including an empty list", async () => {
  for (const sections of [[], ["parameters"]]) {
    const env = setup(); await env.open(); const saved = { ...env.last().choices, sections: ["symbols"], search: "beta", layout: "stacked" };
    env.panel().received.fire({ type: "choices", key: env.last().key, choices: saved }); env.panel().visible = false;
    env.host.settings.set(before, { "diff.sections": sections }); env.host.settingsChanged.fire({ affectsConfiguration: () => true });
    env.panel().visible = true; env.panel().received.fire({ type: "ready", key: env.last().key, choices: saved });
    assert.deepEqual(env.last().choices.sections, sections); assert.equal(env.last().choices.layout, "stacked"); assert.equal(env.last().choices.search, "beta");
    env.panel().dispose(); await env.open(); assert.deepEqual(env.last().choices.sections, sections); env.registration.dispose();
  }
});
test("no-change, incomplete, older-engine and failure states clear authoritative rows", async () => {
  const env = setup(); await env.open(); env.service.result = noChanges(); env.panel().received.fire({ type: "refresh" }); await flush();
  assert.equal(env.last().status, "ready"); assert.match(env.last().message, /No structural changes/);
  env.service.result = { status: "incomplete" }; env.panel().received.fire({ type: "refresh" }); await flush(); assert.equal(env.last().status, "incomplete"); assert.deepEqual(env.last().rows, []);
  env.service.result = { error: "Inputs changed", code: "INPUT_CHANGED" }; env.panel().received.fire({ type: "refresh" }); await flush(); assert.equal(env.last().status, "failure"); assert.deepEqual(env.last().rows, []);
  env.service.client.initializeResult.capabilities.experimental = {}; env.panel().received.fire({ type: "refresh" }); await flush(); assert.match(env.last().message, /bundled binary/); assert.equal(env.last().status, "failure"); env.registration.dispose();
});
test("older navigation capabilities and unsupported payload schemas offer native recovery actions", async () => {
  for (const capabilities of [false, true]) {
    const env = setup();
    if (capabilities) env.service.result.navigation.schema_version = 2;
    else env.service.client.initializeResult.capabilities.experimental = {};
    await env.open(); assert.equal(env.last().status, "failure"); assert.equal(env.last().rows.length, 0);
    assert.deepEqual(env.service.failures, [env.last().message]); assert.match(env.service.failures[0], /bundled binary/); env.registration.dispose();
  }
});
test("disposal cancels pending work and removes subscriptions", async () => {
  const env = setup(); await env.open(); const pending = deferred(); env.service.compare = () => pending.promise;
  env.panel().received.fire({ type: "refresh" }); await flush(); const token = env.service.compares.at(-1).token, count = env.panel().messages.length;
  env.registration.dispose(); assert.equal(token.isCancellationRequested, true); pending.resolve(fixture()); await flush();
  assert.equal(env.panel().messages.length, count); assert.equal(env.panel().received.listeners.size, 0); assert.equal(env.changed.listeners.size, 0);
});

class Element {
  constructor(tag) { this.tag = tag; this.children = []; this.listeners = {}; this.attributes = {}; this._text = ""; }
  set textContent(value) { this._text = value; this.children = []; }
  get textContent() { return this._text + this.children.map(child => child.textContent).join(""); }
  append(...children) { this.children.push(...children); }
  replaceChildren(...children) { this.children = children; this._text = ""; }
  addEventListener(name, listener) { this.listeners[name] = listener; }
  setAttribute(name, value) { this.attributes[name] = value; }
  fire(name) { this.listeners[name]?.(); }
}
function descendants(element) { return [element, ...element.children.flatMap(descendants)]; }
function webview() {
  const elements = Object.fromEntries(["models", "status", "search", "scope", "layout", "expansion", "kinds", "sections", "counts", "results", "refresh", "help"].map(id => [id, new Element(id)]));
  const events = {}, posted = [], states = [];
  const sandbox = { document: { getElementById: id => elements[id], createElement: tag => new Element(tag) }, window: { addEventListener: (name, callback) => events[name] = callback },
    acquireVsCodeApi: () => ({ getState: () => undefined, setState: value => states.push(value), postMessage: message => posted.push(message) }) };
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, "../media/diff_view.js"), "utf8"), sandbox);
  const render = (extra = {}) => events.message({ data: { type: "render", key: "view", token: 4, before, after, rows: parseDiff(fixture(), before, after).rows,
    status: "ready", message: "Current comparison", choices: normalizeChoices({}, defaults), ...extra } });
  return { elements, posted, states, render };
}
test("webview uses text nodes, labels source sides and disables missing/stale locations", () => {
  const env = webview(); const rows = parseDiff(fixture(), before, after).rows; rows[0].label = "<img src=x onerror=bad()>";
  env.render({ rows }); assert.match(env.elements.results.textContent, /<img src=x onerror=bad\(\)>/);
  assert.equal(descendants(env.elements.results).some(element => element.tag === "img"), false);
  const buttons = descendants(env.elements.results).filter(element => element.tag === "button");
  assert.equal(buttons.length, 22); assert.equal(buttons[0].disabled, true); assert.equal(buttons[1].disabled, false);
  buttons[1].fire("click"); assert.equal(env.posted.at(-1).side, "after"); assert.equal(env.posted.at(-1).rowId, "/added_endogenous/0");
  env.render({ status: "stale", message: "Out of date" }); assert.ok(descendants(env.elements.results).filter(element => element.tag === "button").every(button => button.disabled));
});
test("webview text/kind/scope filters, counts, layout and expansion preserve state", () => {
  const env = webview(); env.render(); assert.match(env.elements.counts.textContent, /11 of 11 rows shown/);
  env.elements.search.value = "goods"; env.elements.search.fire("input"); assert.match(env.elements.counts.textContent, /1 of 11 rows shown/);
  env.elements.layout.value = "stacked"; env.elements.layout.fire("change"); assert.equal(env.elements.results.className, "results layout-stacked");
  env.elements.expansion.value = "none"; env.elements.expansion.fire("change"); assert.equal(env.elements.results.children[0].open, false);
  env.elements.search.value = ""; env.elements.search.fire("input"); env.elements.scope.value = "households"; env.elements.scope.fire("change"); assert.match(env.elements.counts.textContent, /1 of 11 rows shown/);
  assert.equal(env.states.at(-1).choices.scope, "households");
  const changed = descendants(env.elements.kinds).filter(element => element.tag === "input")[2]; changed.checked = false; changed.fire("change"); assert.match(env.elements.results.textContent, /No rows match/);
});
test("webview scope filters and side labels retain unmapped heterogeneous shock rows", () => {
  const result = fixture(), nav = result.navigation.rows.find(row => row.id === "/shock_setup_changes/0");
  nav.before = null; nav.after = null; nav.dimension = "households";
  const rows = parseDiff(result, before, after).rows.filter(row => row.id === nav.id), env = webview(); env.render({ rows });
  assert.match(env.elements.results.textContent, /Dimension: households/);
  env.elements.scope.value = "households"; env.elements.scope.fire("change"); assert.match(env.elements.counts.textContent, /1 of 1 rows shown/);
  env.elements.scope.value = "aggregate"; env.elements.scope.fire("change"); assert.match(env.elements.counts.textContent, /0 of 1 rows shown/);
  assert.ok(descendants(env.elements.results).filter(element => element.tag === "button").every(button => button.disabled));
});
