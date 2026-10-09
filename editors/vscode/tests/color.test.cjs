const assert = require("node:assert/strict");
const test = require("node:test");
const fs = require("node:fs");
const path = require("node:path");
const Module = require("node:module");
const { URL } = require("node:url");

class Disposable {
  constructor(dispose) { this.callback = dispose; }
  dispose() { this.callback?.(); this.callback = undefined; }
}
class Emitter {
  listeners = new Set();
  event = listener => { this.listeners.add(listener); return new Disposable(() => this.listeners.delete(listener)); };
  fire(value) { for (const listener of [...this.listeners]) listener(value); }
}
class Range {
  constructor(line, character, lastLine, lastCharacter) {
    this.start = { line, character }; this.end = { line: lastLine, character: lastCharacter };
  }
}
class ThemeColor { constructor(id) { this.id = id; } }
function uri(value) {
  const parsed = new URL(value);
  return { scheme: parsed.protocol.slice(0, -1), path: parsed.pathname, toString: () => value };
}
function document(value = "file:///project/main.mod", extra = {}) {
  const lines = Array(60).fill(" ".repeat(40));
  return { uri: uri(value), languageId: "dynare", version: 1, isClosed: false,
    lineCount: lines.length, lineAt: index => {
      assert.ok(index >= 0 && index < lines.length); return { text: lines[index] };
    }, ...extra };
}
function editor(doc) {
  return { document: doc, decorations: new Map(), calls: [], setDecorations(type, ranges) {
    assert.equal(type.disposed, false); this.decorations.set(type.id, [...ranges]); this.calls.push({ type, ranges });
  } };
}
function deferred() {
  let resolve, reject;
  const promise = new Promise((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}
const drain = () => new Promise(resolve => setImmediate(resolve));
let timers;
test.beforeEach(context => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  timers = context.mock.timers;
});
const flush = async () => { timers.tick(200); await drain(); };
let host;
const vscode = {
  Disposable, Range, ThemeColor, DecorationRangeBehavior: { ClosedClosed: 3 },
  window: {
    get visibleTextEditors() { return host.editors; },
    get activeTextEditor() { return host.editors[0]; },
    createTextEditorDecorationType: options => {
      const type = { id: options.backgroundColor.id, options, disposed: false, dispose() { this.disposed = true; } };
      host.types.push(type); return type;
    },
    onDidChangeVisibleTextEditors: listener => host.visible.event(listener),
    onDidChangeActiveTextEditor: listener => host.active.event(listener),
  },
  workspace: {
    getConfiguration: (section, scope) => {
      host.scopes.push({ section, scope });
      return { get: (key, fallback) => {
        const local = host.resourceSettings.get(scope.uri.toString()) ?? {};
        return Object.hasOwn(local, key) ? local[key] : Object.hasOwn(host.settings, key) ? host.settings[key] : fallback;
      } };
    },
    onDidChangeTextDocument: listener => host.edited.event(listener),
    onDidOpenTextDocument: listener => host.opened.event(listener),
    onDidCloseTextDocument: listener => host.closed.event(listener),
    onDidChangeConfiguration: listener => host.configured.event(listener),
  },
};
const originalLoad = Module._load;
Module._load = function(id, ...args) {
  if (id === "vscode") return vscode;
  if (id === "./client" && args[0]?.filename.endsWith("color.js")) return {
    isAnalysisDocument: doc => doc.languageId === "dynare" && ["file", "untitled"].includes(doc.uri.scheme),
  };
  return originalLoad.call(this, id, ...args);
};
const { dimensionOverrides, registerColors, tintCategoryDefaults, tintPreferences, tintRanges } = require("../out/color");
Module._load = originalLoad;
const manifest = require("../package.json").contributes;
const colorContributions = { ...manifest, configuration: manifest.configuration.find(group => group.title === "Dygnosis: Appearance") };

const registry = tintCategoryDefaults.map(([category, fallback]) => ({ category, default: fallback }));
function location(value = "file:///project/main.mod", start = 2, end = 5, character = 0, lastCharacter = 4) {
  return { uri: value, range: { start: { line: start, character }, end: { line: end, character: lastCharacter } } };
}
function block(overrides = {}) {
  return { id: "block:1", kind: "block", name: "model", complete: true, native: false,
    category: "model.aggregate", subtype: null, dimension: null, lens_anchor: null, equation_count: 1,
    location: location(), anchor: location(), segments: [location()], origin_frames: [], ...overrides };
}
function facts(doc = document(), overrides = {}) {
  return { schema_version: 1, root_uri: doc.uri.toString(), document_uri: doc.uri.toString(), document_version: doc.version,
    client_instance: 1, revision: "current", complete: true, owner_roots: [doc.uri.toString()],
    statements: [block({ segments: [location(doc.uri.toString())] })], declarations: [], equations: [],
    related_files: [], block_categories: registry, first_model_anchor: null, ...overrides };
}
function setup(doc = document(), extras = []) {
  host = { editors: doc ? [editor(doc), ...extras.map(editor)] : [], settings: {}, resourceSettings: new Map(),
    scopes: [], types: [], visible: new Emitter(), active: new Emitter(), edited: new Emitter(),
    opened: new Emitter(), closed: new Emitter(), configured: new Emitter() };
  const changed = new Emitter();
  const service = { currentInstance: 1, client: {}, logged: [], requests: [], roots: [],
    log: message => service.logged.push(message), onDidChange: changed.event,
    rootForDocument: async current => {
      service.roots.push(current.uri.toString());
      return service.select ? service.select(current) : service.root ?? (/\.(mod|dyn)$/i.test(current.uri.path) ? current.uri : undefined);
    },
    modelInfo: async (root, current) => {
      service.requests.push({ root: root.toString(), document: current.toString() });
      const currentDoc = host.editors.find(item => item.document.uri.toString() === current.toString())?.document ?? doc;
      return service.info?.(root, current, currentDoc) ?? facts(currentDoc, { root_uri: root.toString(), document_uri: current.toString(), client_instance: service.currentInstance });
    },
  };
  const configure = (key, value, resource) => {
    if (resource) {
      const settings = host.resourceSettings.get(resource) ?? {}; settings[key] = value; host.resourceSettings.set(resource, settings);
    } else host.settings[key] = value;
    host.configured.fire({ affectsConfiguration: (section, target) => key.startsWith(section.replace(/^dynare\./, "")) && (!resource || target?.toString() === resource) });
  };
  const registration = registerColors(service);
  return { host, service, changed, configure, registration, doc,
    ranges: (style, index = 0) => host.editors[index]?.decorations.get(`dynare.blockTint.${style}Background`) ?? [] };
}
function pure(info, prefs = {}) {
  const doc = document(info.document_uri);
  return tintRanges(info, doc, { enabled: true, styles: new Map(tintCategoryDefaults), heterogeneousModels: new Map(), ...prefs });
}

test("all 45 schema controls match the engine registry, defaults and native controls", () => {
  const rust = fs.readFileSync(path.resolve(__dirname, "../../../src/model_map.rs"), "utf8");
  const registryText = rust.match(/pub const BLOCK_CATEGORIES:[\s\S]*?= &\[([\s\S]*?)\];/)[1];
  const engine = [...registryText.matchAll(/\("([^"]+)", "(off|subtle|model)"\)/g)].map(match => [match[1], match[2]]);
  const ordered = rows => [...rows].sort(([left], [right]) => left.localeCompare(right));
  assert.equal(engine.length, 45);
  assert.deepEqual(ordered(tintCategoryDefaults), ordered(engine));
  const properties = colorContributions.configuration.properties;
  assert.equal(Object.keys(properties).length, 47);
  for (const [category, fallback] of engine) {
    const entry = properties[`dynare.blockTint.${category}`];
    assert.deepEqual(entry.enum, ["off", "subtle", "model"]); assert.deepEqual(entry.enumItemLabels, ["Off", "Subtle", "Model-strength"]);
    assert.equal(entry.default, fallback); assert.equal(entry.scope, "resource"); assert.equal(entry.type, "string");
  }
  assert.equal(properties["dynare.blockTint.enabled"].default, true);
  assert.equal(properties["dynare.blockTint.enabled"].scope, "resource");
  const overrides = properties["dynare.blockTint.heterogeneousModels"];
  assert.deepEqual(overrides.default, {}); assert.equal(overrides.scope, "resource"); assert.equal(overrides.type, "object");
  assert.deepEqual(overrides.additionalProperties.enum, ["off", "subtle", "model"]);
  assert.match(overrides.markdownDescription, /command:dygnosis.editSettingsJson/); assert.match(overrides.markdownDescription, /households/);
});

test("four independently customizable roles use variable parent and explicit Dynare fallback scopes", () => {
  assert.deepEqual(colorContributions.semanticTokenTypes.map(row => row.id), ["dynareEndogenous", "dynareExogenous", "dynareParameter", "dynareModelLocal"]);
  assert.ok(colorContributions.semanticTokenTypes.every(row => row.superType === "variable"));
  const scopes = colorContributions.semanticTokenScopes[0];
  assert.equal(scopes.language, "dynare");
  assert.equal(new Set(Object.values(scopes.scopes).flat()).size, 4);
  for (const row of colorContributions.semanticTokenTypes) assert.match(scopes.scopes[row.id][0], /^variable\.other\.readwrite\.dynare\./);
  assert.deepEqual(colorContributions.semanticTokenModifiers.map(row => row.id), ["forwardLooking", "predetermined"]);
  assert.equal(colorContributions.configurationDefaults, undefined);
});

test("shared backgrounds use four theme defaults, transparent high contrast, and no token foreground styling", async () => {
  const env = setup(); await flush();
  assert.equal(env.host.types.length, 2);
  for (const type of env.host.types) {
    assert.equal(type.options.isWholeLine, true); assert.equal(type.options.rangeBehavior, 3);
    assert.deepEqual(Object.keys(type.options).sort(), ["backgroundColor", "isWholeLine", "rangeBehavior"]);
    assert.ok(type.options.backgroundColor instanceof ThemeColor);
  }
  for (const color of colorContributions.colors.filter(color => color.id.endsWith("Background"))) {
    assert.deepEqual(Object.keys(color.defaults).sort(), ["dark", "highContrast", "highContrastLight", "light"]);
    assert.equal(color.defaults.highContrast, "#00000000"); assert.equal(color.defaults.highContrastLight, "#00000000");
    assert.match(color.defaults.dark, /^#[0-9A-F]{8}$/); assert.match(color.defaults.light, /^#[0-9A-F]{8}$/);
  }
  const [model, subtle] = colorContributions.colors.filter(color => color.id.startsWith("dynare.blockTint."));
  for (const theme of ["dark", "light"]) assert.ok(parseInt(model.defaults[theme].slice(-2), 16) > parseInt(subtle.defaults[theme].slice(-2), 16));
  assert.equal(env.ranges("model").length, 1); env.registration.dispose();
});

test("runtime uses the response registry rather than a second block-kind classification", async () => {
  const env = setup(); env.service.info = () => facts(env.doc, {
    block_categories: [{ category: "new_engine_category", default: "subtle" }, { category: "model.aggregate", default: "off" }],
    statements: [block(), block({ name: "anything", category: "new_engine_category", segments: [location(undefined, 8, 9)] }), block({ category: "unregistered", segments: [location(undefined, 11, 12)] })],
  }); env.changed.fire(); await flush();
  assert.equal(env.ranges("model").length, 0); assert.equal(env.ranges("subtle").length, 1);
  assert.equal(env.ranges("subtle")[0].start.line, 8); env.registration.dispose();
});

test("every shipped category's default, off, subtle and model choices reach its written segment", () => {
  for (const [category, fallback] of tintCategoryDefaults) {
    const expectedDefault = ["model.aggregate", "model.heterogeneous"].includes(category) ? "model" : "subtle";
    assert.equal(fallback, expectedDefault, `${category} default`);
    const info = facts(undefined, { statements: [block({ category, name: "ignored", subtype: "ignored" })] });
    for (const choice of [fallback, "off", "subtle", "model"]) {
      const result = pure(info, { styles: new Map([[category, choice]]) });
      assert.equal(result.model.length, choice === "model" ? 1 : 0, category);
      assert.equal(result.subtle.length, choice === "subtle" ? 1 : 0, category);
    }
  }
});

test("resource settings apply to displayed includes independently in multiple folders", async () => {
  const a = document("file:///one/part.inc"), b = document("file:///two/main.mod");
  const env = setup(a, [b]); env.service.select = current => current === a ? uri("file:///one/owner.mod") : current.uri;
  env.configure("blockTint.model.aggregate", "subtle", a.uri.toString());
  env.configure("blockTint.model.aggregate", "off", b.uri.toString()); await flush();
  assert.equal(env.ranges("subtle", 0).length, 1); assert.equal(env.ranges("model", 0).length, 0);
  assert.equal(env.ranges("model", 1).length, 0); assert.equal(env.ranges("subtle", 1).length, 0);
  assert.ok(env.service.requests.some(row => row.root === "file:///one/owner.mod" && row.document === a.uri.toString()));
  assert.ok(env.host.scopes.every(row => row.section === "dynare" && row.scope.languageId === "dynare"));
  assert.ok(env.host.scopes.some(row => row.scope.uri === a.uri)); env.registration.dispose();
});

test("invalid enabled/style values fall back and explain their setting in Output", async () => {
  const env = setup(); env.configure("blockTint.enabled", "false"); env.configure("blockTint.model.aggregate", null);
  env.configure("blockTint.initval", "loud"); await flush();
  assert.equal(env.ranges("model").length, 1);
  const prefs = tintPreferences(env.doc, registry, env.service.log);
  assert.equal(prefs.enabled, true); assert.equal(prefs.styles.get("initval"), "subtle");
  assert.ok(env.service.logged.some(row => row.includes("dynare.blockTint.enabled")));
  assert.ok(env.service.logged.some(row => row.includes("dynare.blockTint.model.aggregate")));
  assert.ok(env.service.logged.some(row => row.includes("dynare.blockTint.initval"))); env.registration.dispose();
});

test("dimension override validation keeps valid entries and handles JSON special property names safely", () => {
  const messages = [], log = message => messages.push(message);
  for (const value of [null, false, [], "off", 1]) assert.equal(dimensionOverrides(value, log).size, 0);
  const value = JSON.parse('{"households":"off","firms":"subtle","banks":"model","bad":false,"":"model","__proto__":"model","constructor":"subtle"}');
  const overrides = dimensionOverrides(value, log);
  assert.deepEqual([...overrides], [["households", "off"], ["firms", "subtle"], ["banks", "model"], ["__proto__", "model"], ["constructor", "subtle"]]);
  assert.equal({}.polluted, undefined); assert.ok(messages.some(row => row.includes("'bad'")));
  assert.ok(messages.some(row => row.includes("entry ''")));
});

test("dimension overrides take priority only for registered heterogeneous model blocks", async () => {
  const env = setup(); env.service.info = () => facts(env.doc, { statements: [
    block(), block({ category: "model.heterogeneous", dimension: "households", segments: [location(undefined, 10, 12)] }),
    block({ category: "model.heterogeneous", dimension: "firms", segments: [location(undefined, 15, 17)] }),
    block({ category: "shocks.heterogeneous", dimension: "households", segments: [location(undefined, 20, 21)] }),
  ] });
  env.configure("blockTint.model.heterogeneous", "subtle"); env.configure("blockTint.heterogeneousModels", { households: "off", firms: "model" }); await flush();
  assert.deepEqual(env.ranges("model").map(range => range.start.line), [2, 15]);
  assert.deepEqual(env.ranges("subtle").map(range => range.start.line), [20]);
  assert.equal(pure(facts(undefined, { block_categories: [], statements: [block({ category: "model.heterogeneous", dimension: "firms" })] }),
    { styles: new Map(), heterogeneousModels: new Map([["firms", "model"]]) }).model.length, 0);
  env.registration.dispose();
});

test("split includes stay separate, macro copies deduplicate, source envelopes and anchors are never used", () => {
  const local = location(undefined, 2, 3), later = location(undefined, 10, 11), foreign = location("file:///project/part.inc", 1, 7);
  const statement = block({ location: location(undefined, 0, 40), anchor: location(undefined, 1, 1), segments: [local, foreign, later] });
  const ranges = pure(facts(undefined, { statements: [statement, { ...statement, id: "macro:copy" }, block({ segments: [], location: location(undefined, 30, 40) })] }));
  assert.deepEqual(ranges.model.map(range => [range.start.line, range.end.line]), [[2, 3], [10, 11]]);
  const child = document(foreign.uri);
  assert.deepEqual(tintRanges(facts(child, { statements: [statement] }), child, { enabled: true, styles: new Map(tintCategoryDefaults), heterogeneousModels: new Map() }).model.map(range => [range.start.line, range.end.line]), [[1, 7]]);
});

test("macro copies with conflicting dimension styles leave the shared written site uncolored", () => {
  const statements = ["households", "firms", "firms"].map(dimension => block({ category: "model.heterogeneous", dimension }));
  const ranges = pure(facts(undefined, { statements }), { heterogeneousModels: new Map([["households", "off"], ["firms", "subtle"]]) });
  assert.deepEqual(ranges, { model: [], subtle: [] });
});

test("exclusive end at column zero does not tint the next line; malformed/out-of-file segments are refused", () => {
  const segments = [location(undefined, 2, 5, 3, 0), location(undefined, 9, 9, 0, 0), location(undefined, 10, 61),
    location(undefined, 11, 12, 41), location(undefined, 11, 12, 0, 41), location(undefined, 13, 12),
    { uri: "file:///project/main.mod", range: { start: { line: -1, character: 0 }, end: { line: 2, character: 0 } } }];
  const result = pure(facts(undefined, { statements: [block({ segments })] }));
  assert.equal(result.model.length, 1); assert.deepEqual(result.model[0], new Range(2, 3, 4, 40));
});

test("incomplete expansion keeps only complete file-local recovered blocks", async () => {
  const env = setup(); env.service.info = () => facts(env.doc, { complete: false, statements: [block(),
    block({ complete: false, segments: [location(undefined, 10, 20)] }), block({ native: true, segments: [location(undefined, 30, 40)] })] });
  env.changed.fire(); await flush(); assert.equal(env.ranges("model").length, 1); env.registration.dispose();
});

test("removed and retyped blocks clear prior colors instead of accumulating decorations", async () => {
  const env = setup(); await flush(); assert.equal(env.ranges("model").length, 1);
  env.service.info = () => facts(env.doc, { statements: [block({ category: "initval" })] }); env.changed.fire();
  assert.equal(env.ranges("model").length, 1); await flush(); assert.equal(env.ranges("model").length, 0); assert.equal(env.ranges("subtle").length, 1);
  env.service.info = () => facts(env.doc, { statements: [] }); env.changed.fire(); await flush();
  assert.equal(env.ranges("subtle").length, 0); env.registration.dispose();
});

test("global disabling clears immediately, supersedes pending work and performs no surface requests until reset", async () => {
  const env = setup(); await flush(); const late = deferred(); env.service.modelInfo = () => late.promise;
  env.changed.fire(); await flush(); env.configure("blockTint.enabled", false);
  const roots = env.service.roots.length; env.changed.fire(); env.host.active.fire(); await flush();
  assert.equal(env.service.roots.length, roots); assert.equal(env.ranges("model").length, 0);
  late.resolve(facts(env.doc)); await flush(); assert.equal(env.ranges("model").length, 0);
  env.service.modelInfo = async () => facts(env.doc); delete env.host.settings["blockTint.enabled"];
  env.host.configured.fire({ affectsConfiguration: () => true }); await flush(); assert.equal(env.ranges("model").length, 1);
  env.registration.dispose();
});

test("style and dimension reset restore declared defaults live without restarting the engine", async () => {
  const env = setup(); env.service.info = () => facts(env.doc, { statements: [block({ category: "model.heterogeneous", dimension: "households" })] });
  env.configure("blockTint.model.heterogeneous", "off"); env.configure("blockTint.heterogeneousModels", { households: "subtle" }); await flush();
  assert.equal(env.ranges("subtle").length, 1); const instance = env.service.currentInstance;
  delete env.host.settings["blockTint.model.heterogeneous"]; delete env.host.settings["blockTint.heterogeneousModels"];
  env.host.configured.fire({ affectsConfiguration: () => true }); await flush();
  assert.equal(env.ranges("subtle").length, 0); assert.equal(env.ranges("model").length, 1); assert.equal(env.service.currentInstance, instance);
  env.registration.dispose();
});

test("unsupported previews, other languages, closed documents, and ownerless includes stay clear", async () => {
  for (const doc of [document("dygnosis-effective:/main.mod"), document("vscode-vfs:/main.mod"),
    document(undefined, { languageId: "plaintext" }), document(undefined, { isClosed: true }), document("file:///project/ownerless.inc")]) {
    const env = setup(doc); await flush(); assert.equal(env.ranges("model").length, 0); assert.equal(env.service.requests.length, 0); env.registration.dispose();
  }
});

test("ordinary dyn and untitled models preserve their URI for model facts and decorations", async () => {
  for (const value of ["file:///project/model.DYN", "untitled:/scratch.mod"]) {
    const env = setup(document(value)); await flush();
    assert.deepEqual(env.service.requests[0], { root: value, document: value }); assert.equal(env.ranges("model").length, 1);
    env.registration.dispose();
  }
});

test("stale root replies lose to current roots and old revisions cannot restore earlier decorations", async () => {
  const env = setup(); await flush(); const old = deferred(), next = deferred();
  env.service.modelInfo = () => old.promise; env.changed.fire(); await flush();
  env.service.root = uri("file:///project/next.mod"); env.service.modelInfo = () => next.promise; env.changed.fire(); await flush();
  next.resolve(facts(env.doc, { root_uri: "file:///project/next.mod", revision: "next", statements: [block({ category: "initval" })] })); await flush();
  old.resolve(facts(env.doc)); await flush(); assert.equal(env.ranges("model").length, 0); assert.equal(env.ranges("subtle").length, 1);
  env.registration.dispose();
});

test("version, engine, document identity and visible-editor changes reject late replies", async () => {
  for (const kind of ["version", "instance", "client", "document", "hidden", "closed", "language"]) {
    const env = setup(); await flush(); const late = deferred(); env.service.modelInfo = () => late.promise;
    env.changed.fire(); await flush();
    const original = env.host.editors[0], calls = original.calls.length;
    if (kind === "version") ++env.doc.version;
    if (kind === "instance") ++env.service.currentInstance;
    if (kind === "client") env.service.client = {};
    if (kind === "document") original.document = document("file:///project/new.mod");
    if (kind === "hidden") env.host.editors = [];
    if (kind === "closed") env.doc.isClosed = true;
    if (kind === "language") env.doc.languageId = "plaintext";
    late.resolve(facts(env.doc)); await flush();
    assert.equal(original.calls.length, calls, kind); env.registration.dispose();
  }
});

test("mismatched snapshot root, resource, version or instance is refused", async () => {
  for (const mismatch of [{ root_uri: "file:///other.mod" }, { document_uri: "file:///other.inc" }, { document_version: 0 }, { document_version: null }, { client_instance: 9 }]) {
    const env = setup(); env.service.info = () => facts(env.doc, mismatch); env.changed.fire(); await flush();
    assert.equal(env.ranges("model").length, 0); env.registration.dispose();
  }
});

test("owner changes while fetching or checking the root prevent old include tint", async () => {
  const env = setup(document("file:///project/part.inc")); env.service.root = uri("file:///project/a.mod");
  const late = deferred(); env.service.modelInfo = () => late.promise; env.changed.fire(); await flush();
  env.service.root = uri("file:///project/b.mod"); late.resolve(facts(env.doc, { root_uri: "file:///project/a.mod" })); await flush();
  assert.equal(env.ranges("model").length, 0);
  const rootCheck = deferred(); let calls = 0;
  env.service.rootForDocument = () => ++calls === 1 ? Promise.resolve(uri("file:///project/a.mod")) : rootCheck.promise;
  env.service.modelInfo = async () => facts(env.doc, { root_uri: "file:///project/a.mod" }); env.changed.fire(); await flush();
  env.configure("blockTint.enabled", false); rootCheck.resolve(uri("file:///project/a.mod")); await flush();
  assert.equal(env.ranges("model").length, 0); env.registration.dispose();
});

test("failed and unavailable snapshots clear stale tint and explain actual failures quietly", async () => {
  const env = setup(); await flush();
  env.service.modelInfo = async () => undefined; env.changed.fire(); await flush(); assert.equal(env.ranges("model").length, 0);
  env.service.modelInfo = async () => { throw new Error("Unsupported schema"); }; env.changed.fire(); await flush();
  assert.equal(env.ranges("model").length, 0); assert.ok(env.service.logged.includes("Error: Unsupported schema")); env.registration.dispose();
});

test("edits retain tint; detached editors clear and cannot receive old replies", async () => {
  const env = setup(); await flush(); const first = env.host.editors[0];
  const late = deferred(); env.service.modelInfo = () => late.promise; ++env.doc.version;
  env.host.edited.fire({ document: env.doc, contentChanges: [{ text: "x" }] }); assert.equal(env.ranges("model").length, 1); await flush();
  const replacement = editor(document("file:///project/second.mod")); env.host.editors = [replacement]; env.host.visible.fire(env.host.editors);
  late.resolve(facts(env.doc)); await flush();
  assert.equal(first.decorations.get("dynare.blockTint.modelBackground").length, 0); assert.equal(env.ranges("model").length, 0);
  env.registration.dispose();
});

test("closed and retyped document events clear current decorations", async () => {
  for (const change of ["closed", "retyped"]) {
    const env = setup(); await flush(); assert.equal(env.ranges("model").length, 1);
    if (change === "closed") { env.doc.isClosed = true; env.host.closed.fire(env.doc); }
    else { env.doc.languageId = "plaintext"; env.host.opened.fire(env.doc); }
    assert.equal(env.ranges("model").length, 0); await flush(); assert.equal(env.ranges("model").length, 0);
    env.registration.dispose();
  }
});

test("disposal clears ranges, disposes both types/listeners and refuses pending results", async () => {
  const env = setup(); await flush(); const late = deferred(); env.service.modelInfo = () => late.promise; env.changed.fire(); await flush();
  env.registration.dispose(); late.resolve(facts(env.doc)); await flush();
  assert.equal(env.ranges("model").length, 0); assert.ok(env.host.types.every(type => type.disposed));
  for (const event of [env.changed, env.host.visible, env.host.active, env.host.edited, env.host.opened, env.host.closed, env.host.configured]) assert.equal(event.listeners.size, 0);
  env.registration.dispose();
});


test("typing retains tint through the 200 ms pause and the pending model response", async () => {
  const env = setup(); await flush();
  const painted = env.host.editors[0], calls = painted.calls.length, requests = env.service.requests.length;
  const late = deferred(); env.service.info = () => late.promise;
  ++env.doc.version;
  env.host.edited.fire({ document: env.doc, contentChanges: [{ text: "x" }] });
  env.changed.fire();
  assert.equal(env.ranges("model").length, 1);
  assert.equal(painted.calls.length, calls, "an edit must not clear the editor's tracked ranges");
  timers.tick(199); await drain();
  assert.equal(env.service.requests.length, requests);
  timers.tick(1); await drain();
  assert.equal(env.service.requests.length, requests + 1);
  assert.equal(env.ranges("model").length, 1);
  late.resolve(facts(env.doc, { statements: [block({ segments: [location(undefined, 4, 8)] })] })); await drain();
  assert.deepEqual(env.ranges("model").map(range => [range.start.line, range.end.line]), [[4, 8]]);
  assert.ok(painted.calls.slice(calls).filter(call => call.type.id === "dynare.blockTint.modelBackground").every(call => call.ranges.length === 1));
  env.registration.dispose();
});

test("successive edits coalesce and reject a response that arrives during the next pause", async () => {
  const env = setup(); await flush();
  const old = deferred(), next = deferred(); env.service.info = () => old.promise;
  env.changed.fire(); await flush();
  const calls = env.host.editors[0].calls.length, requests = env.service.requests.length;
  env.service.info = () => next.promise;
  ++env.doc.version; env.host.edited.fire({ document: env.doc, contentChanges: [{ text: "x" }] });
  timers.tick(100);
  ++env.doc.version; env.host.edited.fire({ document: env.doc, contentChanges: [{ text: "y" }] }); env.changed.fire();
  old.resolve(facts(env.doc, { statements: [] })); await drain();
  assert.equal(env.host.editors[0].calls.length, calls);
  timers.tick(199); await drain(); assert.equal(env.service.requests.length, requests);
  timers.tick(1); await drain(); assert.equal(env.service.requests.length, requests + 1);
  assert.equal(env.ranges("model").length, 1);
  next.resolve(facts(env.doc, { statements: [] })); await drain();
  assert.equal(env.ranges("model").length, 0, "a current result removes a deleted block");
  env.registration.dispose();
});

test("focus changes preserve visible tint and empty document changes make no requests", async () => {
  const env = setup(undefined, [document("file:///project/other.mod")]); await flush();
  const editors = [...env.host.editors], offsets = editors.map(item => item.calls.length);
  const requests = env.service.requests.length;
  env.host.edited.fire({ document: env.doc, contentChanges: [] }); await flush();
  assert.equal(env.service.requests.length, requests);
  env.host.active.fire(); env.host.visible.fire(env.host.editors); await flush();
  for (const [index, item] of editors.entries()) {
    assert.equal(env.ranges("model", index).length, 1);
    assert.ok(item.calls.slice(offsets[index]).filter(call => call.type.id === "dynare.blockTint.modelBackground").every(call => call.ranges.length === 1));
  }
  env.registration.dispose();
});

test("an engine change clears retained tint immediately and an owner loss clears after lookup", async () => {
  for (const kind of ["instance", "client"]) {
    const env = setup(); await flush();
    if (kind === "instance") ++env.service.currentInstance; else env.service.client = {};
    env.service.info = () => new Promise(() => {}); env.changed.fire();
    assert.equal(env.ranges("model").length, 0, kind); env.registration.dispose();
  }
  const env = setup(); await flush(); env.service.rootForDocument = async () => undefined;
  env.changed.fire(); await flush(); assert.equal(env.ranges("model").length, 0); env.registration.dispose();
});

test("disposal during the typing pause cancels the scheduled requests", async () => {
  const env = setup(); await flush(); const requests = env.service.requests.length;
  ++env.doc.version; env.host.edited.fire({ document: env.doc, contentChanges: [{ text: "x" }] });
  env.registration.dispose(); await flush();
  assert.equal(env.service.requests.length, requests); assert.equal(env.ranges("model").length, 0);
});
