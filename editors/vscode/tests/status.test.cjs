const { compiledModule, importedModule } = require("./helpers/extension_modules.cjs");
const assert = require("node:assert/strict");
const { MarkdownString, tooltipText } = require("./helpers/markdown.cjs");
const test = require("node:test");
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
  dispose() { this.listeners.clear(); }
}
function uri(value) {
  const parsed = new URL(value);
  return { scheme: parsed.protocol.slice(0, -1), path: parsed.pathname, fsPath: decodeURIComponent(parsed.pathname), toString: () => value };
}
function document(value = "file:///workspace/main.mod", extra = {}) {
  return { uri: uri(value), languageId: "dynare", version: 1, isClosed: false, ...extra };
}
function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}
const flush = () => new Promise(resolve => setImmediate(resolve));
let host;
const vscode = {
  Disposable, MarkdownString, EventEmitter: Emitter, Uri: { parse: uri },
  StatusBarAlignment: { Left: 1 },
  window: {
    get activeTextEditor() { return host.editor; },
    createStatusBarItem: (id, alignment, priority) => {
      const item = { id, alignment, priority, visible: false, disposed: false, show() { this.visible = true; }, hide() { this.visible = false; }, dispose() { this.disposed = true; } };
      host.items.push(item);
      return item;
    },
    onDidChangeActiveTextEditor: listener => host.active.event(listener),
  },
  workspace: {
    asRelativePath: value => value.path,
    getConfiguration: (section, scope) => {
      host.scopes.push({ section, scope });
      const configuration = host.resourceSettings.get(scope.uri.toString()) ?? {};
      return { get: (key, fallback) => configuration[key] ?? host.settings[key] ?? fallback };
    },
    onDidChangeTextDocument: listener => host.edited.event(listener),
    onDidCloseTextDocument: listener => host.closed.event(listener),
    onDidChangeConfiguration: listener => host.configured.event(listener),
  },
};
const originalLoad = Module._load;
Module._load = function(id, ...args) {
  if (id === "vscode") return vscode;
  if (importedModule(id, args[0]) === "client" && args[0]?.filename.endsWith("status.js")) return {
    isAnalysisDocument: doc => doc.languageId === "dynare" && ["file", "untitled"].includes(doc.uri.scheme),
    isRootUri: value => ["file", "untitled"].includes(value.scheme) && /\.(mod|dyn)$/i.test(value.path),
  };
  return originalLoad.call(this, id, ...args);
};
const { modelStatus, registerStatus } = require(compiledModule("status"));
const { RequestActivity } = require(compiledModule("activity"));
Module._load = originalLoad;

function facts(doc = document(), overrides = {}) {
  return {
    schema_version: 1, root_uri: doc.uri.toString(), document_uri: doc.uri.toString(), document_version: doc.version,
    revision: "current", client_instance: 1, complete: true, owner_roots: [doc.uri.toString()],
    statements: [], declarations: [], equations: [], related_files: [], block_categories: [], first_model_anchor: null,
    n_endogenous: 4, n_exogenous: 2, n_parameters: 1, n_equations: 3,
    endogenous: ["a", "b", "c", "d"], exogenous: ["e", "f"], parameters: ["p"],
    static: ["a"], predetermined: ["b"], forward_looking: ["c"], mixed: ["d"],
    heterogeneity_dimensions: [{ dimension: "households", n_endogenous: 100, n_exogenous: 10, n_parameters: 20, n_equations: 90,
      endogenous: [], exogenous: [], parameters: [], static: ["h"], predetermined: [], forward_looking: [], mixed: [] }],
    ...overrides,
  };
}
function analysis(configuration = {}, extra = {}) {
  return { state: "selected", selection: { configuration_id: "choice", name: null, preview: false, invocation_scope: "explicit_macro_inputs" },
    configuration, selection_generation: 1, include_paths: null, macro_complete: true, model_complete: true, overrides: [], ...extra };
}
function setup(doc = document()) {
  host = { editor: doc ? { document: doc } : undefined, settings: {}, resourceSettings: new Map(), scopes: [], items: [],
    active: new Emitter(), edited: new Emitter(), closed: new Emitter(), configured: new Emitter() };
  const changed = new Emitter();
  const activityChanged = new Emitter();
  const service = {
    client: {}, currentInstance: 1, supportsModelInfo: true, logged: [], requests: [],
    log: message => service.logged.push(message), onDidChange: changed.event,
    onDidChangeActivity: activityChanged.event, modelActivity: () => undefined,
    rootForDocument: async current => service.root ?? current.uri,
    modelInfo: async (root, current) => {
      service.requests.push({ root, document: current });
      return facts(doc, { root_uri: root.toString(), document_uri: current.toString() });
    },
  };
  const configure = (key, value, resource) => {
    if (resource) {
      const settings = host.resourceSettings.get(resource) ?? {};
      settings[key] = value;
      host.resourceSettings.set(resource, settings);
    } else host.settings[key] = value;
    host.configured.fire({ affectsConfiguration: (section, target) => key.startsWith(section.replace(/^dynare\./, "")) && (!resource || target?.toString() === resource) });
  };
  return { host, service, changed, activityChanged, configure, doc };
}

function trackActivity(context, env) {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const activity = new RequestActivity();
  env.service.onDidChangeActivity = activity.onDidChange;
  env.service.modelActivity = (document, root) => activity.forDocument(document, root);
  context.after(() => activity.dispose());
  const middleware = activity.middleware({});
  return { activity, clock: context.mock.timers,
    request: (method, params, pending) => middleware.sendRequest(method, params, undefined, () => pending.promise) };
}

test("total icons and separate count blocks retain every class and dimension in hover and speech", () => {
  // Guards omitting dimension counts from the status bar or losing the separate hover facts.
  const value = modelStatus(facts(), ["endogenous", "exogenous", "equations"]);
  assert.equal(value.text, "Source: $(symbol-variable) 104  $(symbol-field) 12  $(symbol-operator) 93");
  const report = [
    "Aggregate model",
    "4 endogenous (1 static, 1 predetermined, 1 forward-looking, 1 mixed)",
    "2 exogenous", "3 equations", "Heterogeneous model: households",
    "100 endogenous (1 static, 0 predetermined, 0 forward-looking, 0 mixed)",
    "10 exogenous", "90 equations",
  ];
  assert.equal(tooltipText(value.tooltip), `[Current config](command:dygnosis.selectAnalysisConfiguration): Source\n${report.join("\n")}`);
  assert.match(value.tooltip.value, /\*\*Aggregate model\*\*/);
  assert.doesNotMatch(tooltipText(value.tooltip), /main\.mod|before transformation|Click/);
  assert.equal(value.accessibilityLabel, `Dynare model counts. Total: 104 endogenous, 12 exogenous, 93 equations. ${report.join(". ")}. Choose the analysis configuration.`);
  assert.doesNotMatch(value.text, /warning|error/);
});

test("total counts include every dimension and fall back to aggregate when none are present", () => {
  // Guards summing only the first dimension or requiring a dimension for ordinary models.
  const info = facts();
  info.heterogeneity_dimensions.push({ ...info.heterogeneity_dimensions[0], dimension: "firms", n_endogenous: 3, n_exogenous: 1, n_equations: 2 });
  assert.equal(modelStatus(info, ["endogenous", "exogenous", "equations"]).text,
    "Source: $(symbol-variable) 107  $(symbol-field) 13  $(symbol-operator) 95");
  info.heterogeneity_dimensions = [];
  assert.equal(modelStatus(info, ["endogenous", "exogenous", "equations"]).text,
    "Source: $(symbol-variable) 4  $(symbol-field) 2  $(symbol-operator) 3");
});

test("configuration hover shows selected expressions and links to the shared selector", () => {
  // Guards showing only a preset name, or losing the selector when macro choices change.
  const metadata = analysis({ defines: [{ name: "N", expression: "2" }, { name: "M", expression: "N+1" }] });
  const source = modelStatus(facts(document(), { analysis: analysis() }), ["equations"]);
  assert.equal(source.text, "Source: $(symbol-operator) 93");
  assert.match(source.tooltip.value, /^\[Current config\]\(command:dygnosis.selectAnalysisConfiguration\): Source/);
  const configured = modelStatus(facts(document(), { analysis: metadata }), ["equations"]);
  assert.equal(configured.text, "Current config: $(symbol-operator) 93");
  assert.match(tooltipText(configured.tooltip), /N = 2; M = N\+1/);
  assert.deepEqual(configured.tooltip.isTrusted, { enabledCommands: ["dygnosis.selectAnalysisConfiguration"] });
});

test("preview values and stale or incomplete outcomes keep configuration access", () => {
  // Guards unavailable counts hiding the choices or cause needed to reset a preview.
  const target = { file: null, site_id: "definition", source_revision: "source" };
  const operation = { kind: "definition", target, expression: '"[link](command:untrusted)"' };
  const metadata = analysis({ overrides: [operation] }, {
    state: "stale", selection: { configuration_id: "preview", name: "Alternative", preview: true, invocation_scope: "preview" },
    cause: { code: "STALE", message: "The written definition changed. Reset the analysis configuration." },
    overrides: [{ operation, application: "unknown", known_execution_count: 0, execution_count: null, cause: "Its target changed." }],
  });
  for (const complete of [false, true]) {
    const value = modelStatus(facts(document(), { analysis: metadata, complete, n_equations: undefined }), ["equations"]);
    assert.equal(value.text, complete ? "$(info) Current config: unavailable" : "$(info) Current config: source only");
    assert.match(value.tooltip.value, /^\[Current config\]\(command:dygnosis.selectAnalysisConfiguration\)/);
    assert.match(tooltipText(value.tooltip), /Definition override = "\[link\]\(command:untrusted\)"/);
    assert.ok(value.tooltip.value.includes('\\[link\\]\\(command:untrusted\\)'));
    assert.match(tooltipText(value.tooltip), /preview.*stale/);
    assert.match(tooltipText(value.tooltip), /Its target changed/);
    assert.match(tooltipText(value.tooltip), /Reset the analysis configuration/);
  }
});

test("a total outside the safe integer range is unavailable", () => {
  // Guards displaying a rounded total even when each scope has a valid count.
  const info = facts(document(), { n_endogenous: Number.MAX_SAFE_INTEGER });
  assert.equal(modelStatus(info, ["endogenous"]).text, "$(info) Source: unavailable");
});
test("selected count order applies to each dimension and hiding endogenous hides every class", () => {
  // Guards against dimensions ignoring count selection or exposing hidden classes.
  const info = facts(document(), { heterogeneity_dimensions: [
    ...facts().heterogeneity_dimensions,
    { dimension: "firms", n_endogenous: 0, n_exogenous: 0, n_equations: 0 },
  ] });
  const value = modelStatus(info, ["equations", "exogenous"]);
  assert.equal(value.text, "Source: $(symbol-operator) 93  $(symbol-field) 12");
  assert.match(tooltipText(value.tooltip), /Aggregate model\n3 equations\n2 exogenous\nHeterogeneous model: households\n90 equations\n10 exogenous\nHeterogeneous model: firms\n0 equations\n0 exogenous/);
  for (const text of [tooltipText(value.tooltip), value.accessibilityLabel]) {
    assert.doesNotMatch(text, /endogenous|static|predetermined|forward-looking|mixed/);
    assert.ok(text.indexOf("households") < text.indexOf("firms"));
  }
});
test("missing class lists display all four zero classes for aggregate and dimensions", () => {
  // Guards compatibility with count facts that omit class lists.
  const info = facts(document(), { static: undefined, predetermined: undefined, forward_looking: undefined, mixed: undefined,
    heterogeneity_dimensions: [{ dimension: "households", n_endogenous: 0, n_exogenous: 0, n_equations: 0 }] });
  const value = modelStatus(info, ["endogenous"]);
  const classes = "(0 static, 0 predetermined, 0 forward-looking, 0 mixed)";
  for (const text of [tooltipText(value.tooltip), value.accessibilityLabel]) {
    assert.ok(text.includes(`4 endogenous ${classes}`));
    assert.ok(text.includes(`0 endogenous ${classes}`));
    assert.doesNotMatch(text, /exogenous|equations/);
  }
});
test("incomplete expansion withholds even supplied old numbers", () => {
  const value = modelStatus(facts(document(), { complete: false, message: "Model expansion is incomplete" }), ["endogenous", "equations"]);
  assert.equal(value.text, "$(info) Source: incomplete");
  assert.match(tooltipText(value.tooltip), /Counts are unavailable/);
  assert.match(tooltipText(value.tooltip), /Model expansion is incomplete/);
  assert.doesNotMatch(tooltipText(value.tooltip), /Endogenous:|households|Timing:/);
});
test("incomplete reasons appear in the hover without reading diagnostics", () => {
  const value = modelStatus(facts(document(), {
    complete: false,
    message: "Model expansion is incomplete",
    incomplete_reasons: [
      { code: "E063", message: "Unknown variable missing", location: null },
      { code: "I211", message: "Macro expression 'length([1])' could not be evaluated; some model checks were withheld.", location: null },
    ],
  }), ["endogenous", "equations"]);
  assert.equal(value.text, "$(info) Source: incomplete");
  assert.match(tooltipText(value.tooltip), /Unknown variable missing/);
  assert.match(tooltipText(value.tooltip), /Macro expression 'length\(\[1\]\)' could not be evaluated/);
  assert.doesNotMatch(tooltipText(value.tooltip), /Model expansion is incomplete/);
  assert.match(tooltipText(value.tooltip), /Counts are unavailable/);
});
test("older engines without incomplete_reasons keep the generic hover", () => {
  const value = modelStatus(facts(document(), { complete: false, message: "Model expansion is incomplete" }), ["endogenous"]);
  assert.match(tooltipText(value.tooltip), /Model expansion is incomplete/);
  assert.doesNotMatch(tooltipText(value.tooltip), /Unknown variable/);
});
test("ignored diagnostic codes do not remove incomplete hover reasons", () => {
  // Hover reads model-info reasons only; Ignore filters painted diagnostics elsewhere.
  const value = modelStatus(facts(document(), {
    complete: false,
    incomplete_reasons: [{ code: "I211", message: "Macro expression '1 in 1:3' could not be evaluated; some model checks were withheld." }],
  }), ["equations"]);
  assert.match(tooltipText(value.tooltip), /Macro expression '1 in 1:3' could not be evaluated/);
});
test("default status is native, accessible, and selects the shared analysis configuration", async () => {
  const env = setup();
  const registration = registerStatus(env.service);
  await flush();
  const item = env.host.items[0];
  assert.equal(item.id, "dygnosis.modelCounts");
  assert.equal(item.command, "dygnosis.selectAnalysisConfiguration");
  assert.equal(item.name, "Dynare model counts");
  assert.equal(item.visible, true);
  assert.match(item.accessibilityInformation.label, /Total: 104 endogenous/);
  assert.equal(item.backgroundColor, undefined);
  assert.equal(item.color, undefined);
  registration.dispose();
});
test("resource settings choose count order, live disable, empty list, and reset", async () => {
  const env = setup();
  env.configure("statusBar.counts", ["equations", "endogenous"], env.doc.uri.toString());
  const registration = registerStatus(env.service);
  await flush();
  const item = env.host.items[0];
  assert.equal(item.text, "Source: $(symbol-operator) 93  $(symbol-variable) 104");
  assert.ok(env.host.scopes.every(value => value.section === "dynare" && value.scope.languageId === "dynare" && value.scope.uri === env.doc.uri));
  env.configure("statusBar.enabled", false);
  assert.equal(item.visible, false);
  assert.equal(item.text, "");
  env.configure("statusBar.enabled", true);
  await flush();
  assert.equal(item.visible, true);
  env.configure("statusBar.counts", [], env.doc.uri.toString());
  assert.equal(item.visible, false);
  env.host.resourceSettings.delete(env.doc.uri.toString());
  env.configure("statusBar.counts", undefined);
  await flush();
  assert.equal(item.text, "Source: $(symbol-variable) 104  $(symbol-field) 12  $(symbol-operator) 93");
  registration.dispose();
});
test("invalid settings are safely filtered or fall back with an Output explanation", async () => {
  const env = setup();
  env.configure("statusBar.enabled", "yes");
  env.configure("statusBar.counts", ["equations", "equations", "invented", 1]);
  const registration = registerStatus(env.service);
  await flush();
  assert.equal(env.host.items[0].text, "Source: $(symbol-operator) 93");
  assert.ok(env.service.logged.some(message => message.includes("Invalid dynare.statusBar.enabled")));
  assert.ok(env.service.logged.some(message => message.includes("Invalid or repeated entries")));
  registration.dispose();
});
test("unregistered virtual previews, other languages and no editor do not request or show counts", async () => {
  const cases = [
    document("dygnosis-effective:/main.mod"),
    document("file:///workspace/main.mod", { languageId: "plaintext" }),
    undefined,
  ];
  for (const doc of cases) {
    const env = setup(doc);
    if (!doc) env.host.editor = undefined;
    const registration = registerStatus(env.service);
    await flush();
    assert.equal(env.host.items[0].visible, false);
    assert.equal(env.service.requests.length, 0);
    registration.dispose();
  }
});
test("dyn and untitled model roots are supported", async () => {
  for (const value of ["file:///workspace/model.DYN", "untitled:/scratch.mod"]) {
    const env = setup(document(value));
    const registration = registerStatus(env.service);
    await flush();
    assert.equal(env.host.items[0].visible, true);
    assert.match(env.host.items[0].text, /symbol-operator/);
    registration.dispose();
  }
});
test("a mod opened as an include uses its chosen root while preferences use the displayed file", async () => {
  const env = setup(document("file:///other-folder/part.mod"));
  env.service.root = uri("file:///workspace/owner.mod");
  env.configure("statusBar.counts", ["equations"], env.doc.uri.toString());
  const registration = registerStatus(env.service);
  await flush();
  assert.equal(env.service.requests[0].root.toString(), "file:///workspace/owner.mod");
  assert.equal(env.service.requests[0].document.toString(), env.doc.uri.toString());
  assert.equal(env.host.items[0].text, "Source: $(symbol-operator) 93");
  assert.match(tooltipText(env.host.items[0].tooltip), /Aggregate model/);
  assert.doesNotMatch(tooltipText(env.host.items[0].tooltip), /owner\.mod|part\.mod/);
  registration.dispose();
});
test("ambiguous background model counts explain the owner-selection action", async () => {
  const env = setup(document("file:///workspace/helper.mod"));
  env.service.rootForDocument = async () => undefined;
  const registration = registerStatus(env.service); await flush();
  assert.match(env.host.items[0].text, /choose model owner/);
  assert.match(env.host.items[0].tooltip, /Dygnosis: Choose model owner/);
  assert.equal(env.service.requests.length, 0);
  registration.dispose();
});
test("edit invalidation removes numbers immediately and superseded versions cannot restore them", async () => {
  const env = setup();
  const registration = registerStatus(env.service);
  await flush();
  const first = deferred(), second = deferred();
  env.service.modelInfo = () => first.promise;
  ++env.doc.version;
  env.host.edited.fire({ document: env.doc });
  assert.equal(env.host.items[0].text, "$(sync~spin) Dynare: updating");
  assert.equal(env.host.items[0].tooltip, "Refreshing model counts…");
  await flush();
  env.service.modelInfo = () => second.promise;
  ++env.doc.version;
  env.host.edited.fire({ document: env.doc });
  await flush();
  second.resolve(facts(env.doc, { n_equations: 8 }));
  await flush();
  first.resolve(facts(env.doc, { document_version: 2, n_equations: 99 }));
  await flush();
  assert.match(env.host.items[0].text, /symbol-operator\) 98$/);
  assert.doesNotMatch(env.host.items[0].text, /189/);
  registration.dispose();
});
test("root changes and active-editor changes reject old replies", async () => {
  const env = setup();
  const old = deferred(), next = deferred();
  env.service.modelInfo = () => old.promise;
  const registration = registerStatus(env.service);
  await flush();
  env.service.root = uri("file:///workspace/new-owner.mod");
  env.service.modelInfo = () => next.promise;
  env.changed.fire();
  await flush();
  next.resolve(facts(env.doc, { root_uri: env.service.root.toString(), n_endogenous: 7 }));
  await flush();
  old.resolve(facts(env.doc, { n_endogenous: 99 }));
  await flush();
  assert.match(env.host.items[0].text, /symbol-variable\) 107/);
  const late = deferred();
  env.service.modelInfo = () => late.promise;
  env.changed.fire();
  await flush();
  env.host.editor = { document: document("file:///workspace/part.inc", { languageId: "plaintext" }) };
  env.host.active.fire(env.host.editor);
  late.resolve(facts(env.doc));
  await flush();
  assert.equal(env.host.items[0].visible, false);
  registration.dispose();
});
test("settings disabled during a request and disposal prevent late item updates", async () => {
  const env = setup();
  const pending = deferred();
  env.service.modelInfo = () => pending.promise;
  const registration = registerStatus(env.service);
  await flush();
  env.configure("statusBar.enabled", false);
  pending.resolve(facts(env.doc));
  await flush();
  const item = env.host.items[0];
  assert.equal(item.visible, false);
  const afterDispose = deferred();
  env.service.modelInfo = () => afterDispose.promise;
  env.configure("statusBar.enabled", true);
  await flush();
  registration.dispose();
  afterDispose.resolve(facts(env.doc));
  await flush();
  assert.equal(item.visible, false);
  assert.equal(item.disposed, true);
  for (const emitter of [env.changed, env.activityChanged, env.host.active, env.host.edited, env.host.closed, env.host.configured]) assert.equal(emitter.listeners.size, 0);
});
test("client replacement and mismatched root, document or version cannot publish numbers", async () => {
  for (const mismatch of ["client", "root", "document", "version"]) {
    const env = setup();
    const pending = deferred();
    env.service.modelInfo = () => pending.promise;
    const registration = registerStatus(env.service);
    await flush();
    const override = mismatch === "root" ? { root_uri: "file:///wrong.mod" }
      : mismatch === "document" ? { document_uri: "file:///wrong.mod" }
        : mismatch === "version" ? { document_version: 0 } : {};
    if (mismatch === "client") env.service.client = {};
    pending.resolve(facts(env.doc, override));
    await flush();
    assert.doesNotMatch(env.host.items[0].text, /symbol-variable|symbol-operator/);
    registration.dispose();
  }
});
test("unavailable, incomplete and unsupported engines replace previous numbers", async () => {
  const env = setup();
  const registration = registerStatus(env.service);
  await flush();
  env.service.modelInfo = async () => facts(env.doc, { complete: false });
  env.changed.fire();
  await flush();
  assert.equal(env.host.items[0].text, "$(info) Source: incomplete");
  env.service.modelInfo = async () => undefined;
  env.changed.fire();
  await flush();
  assert.equal(env.host.items[0].text, "$(info) Dynare: unavailable");
  env.service.supportsModelInfo = false;
  env.changed.fire();
  await flush();
  assert.match(env.host.items[0].tooltip, /bundled binary/);
  registration.dispose();
});
test("a failed request remains quiet apart from Output and shows unavailable state", async () => {
  const env = setup();
  env.service.modelInfo = async () => { throw new Error("connection closed"); };
  const registration = registerStatus(env.service);
  await flush();
  assert.equal(env.host.items[0].text, "$(info) Dynare: unavailable");
  assert.deepEqual(env.service.logged, ["Error: connection closed"]);
  registration.dispose();
});

test("pending model actions reuse the count item and restore configuration counts without another request", async context => {
  // Guards configuration details leaking into the busy hover, lost action feedback, and a redundant modelInfo refresh on completion.
  const env = setup(), tracked = trackActivity(context, env);
  env.service.modelInfo = async () => {
    env.service.requests.push("modelInfo");
    return facts(env.doc, { analysis: analysis({ defines: [{ name: "N", expression: "2" }] }) });
  };
  const registration = registerStatus(env.service); await flush();
  const item = env.host.items[0], counts = item.text, tooltip = item.tooltip, requests = env.service.requests.length;
  const expansion = deferred(), hover = deferred();
  const first = tracked.request("workspace/executeCommand", { command: "dynare/showEffectiveModel", arguments: [env.doc.uri.toString()] }, expansion);
  const second = tracked.request("textDocument/hover", { textDocument: { uri: env.doc.uri.toString() } }, hover);
  tracked.clock.tick(249); assert.equal(item.text, counts);
  tracked.clock.tick(1); assert.match(item.text, /sync~spin.*Expanding macros/);
  assert.match(tooltipText(item.tooltip), /Loading hover/); assert.equal(env.host.items.length, 1);
  assert.doesNotMatch(tooltipText(item.tooltip), /Current config|N = 2/);
  assert.equal(item.command, "dygnosis.selectAnalysisConfiguration");
  expansion.resolve(); await first; assert.match(item.text, /Loading hover/);
  hover.resolve(); await second;
  assert.equal(item.text, counts); assert.match(item.text, /^Current config:/); assert.equal(item.tooltip, tooltip);
  assert.equal(env.service.requests.length, requests);
  registration.dispose();
});

test("activity before owner resolution keeps the latest incomplete result underneath the action", async context => {
  // Guards a root lookup or completed count refresh hiding unfinished expansion, then restoring stale counts.
  const env = setup(), tracked = trackActivity(context, env), owner = deferred(), model = deferred(), expansion = deferred();
  env.service.rootForDocument = () => owner.promise;
  env.service.modelInfo = () => model.promise;
  const pending = tracked.request("workspace/executeCommand", { command: "dynare/showEffectiveModel", arguments: [{ root_uri: env.doc.uri.toString() }] }, expansion);
  const registration = registerStatus(env.service);
  tracked.clock.tick(250); assert.match(env.host.items[0].text, /Expanding macros/);
  assert.doesNotMatch(tooltipText(env.host.items[0].tooltip), /Current config/);
  owner.resolve(env.doc.uri); await flush();
  model.resolve(facts(env.doc, { complete: false, incomplete_reasons: [{ message: "The selected branch cannot be expanded." }] }));
  await flush(); assert.match(env.host.items[0].text, /Expanding macros/);
  assert.doesNotMatch(tooltipText(env.host.items[0].tooltip), /Current config/);
  expansion.resolve(); await pending;
  assert.equal(env.host.items[0].text, "$(info) Source: incomplete");
  assert.match(tooltipText(env.host.items[0].tooltip), /selected branch cannot be expanded/);
  registration.dispose();
});

test("edits during activity cannot restore old numbers before the new guarded count result", async context => {
  // Guards action completion repainting counts from an older document version.
  const env = setup(), tracked = trackActivity(context, env);
  const registration = registerStatus(env.service); await flush();
  const expansion = deferred(), model = deferred();
  const pending = tracked.request("workspace/executeCommand", { command: "dynare/showEffectiveModel", arguments: [env.doc.uri.toString()] }, expansion);
  tracked.clock.tick(250);
  env.service.modelInfo = () => model.promise;
  ++env.doc.version; env.host.edited.fire({ document: env.doc }); await flush();
  assert.match(env.host.items[0].text, /Expanding macros/);
  expansion.resolve(); await pending;
  assert.equal(env.host.items[0].text, "$(sync~spin) Dynare: updating");
  model.resolve(facts(env.doc, { n_equations: 8, analysis: analysis({ defines: [{ name: "N", expression: "3" }] }) }));
  await flush();
  assert.equal(env.host.items[0].text, "Current config: $(symbol-variable) 104  $(symbol-field) 12  $(symbol-operator) 98");
  registration.dispose();
});

test("other model activity cannot replace the active item's counts after an editor switch", async context => {
  // Guards root A's unfinished action replacing root B's status or a non-Dynare editor's hidden item.
  const env = setup(), tracked = trackActivity(context, env);
  env.service.modelInfo = async (root, current) => facts(env.host.editor.document, { root_uri: root.toString(), document_uri: current.toString() });
  const registration = registerStatus(env.service); await flush();
  const expansion = deferred(), pending = tracked.request("workspace/executeCommand", {
    command: "dynare/showEffectiveModel", arguments: [{ root_uri: env.doc.uri.toString() }],
  }, expansion);
  tracked.clock.tick(250); assert.match(env.host.items[0].text, /Expanding macros/);
  env.host.editor = { document: document("file:///workspace/second.mod") };
  env.host.active.fire(env.host.editor); await flush();
  assert.match(env.host.items[0].text, /^Source:/); assert.doesNotMatch(env.host.items[0].text, /Expanding/);
  env.host.editor = { document: document("file:///workspace/notes.txt", { languageId: "plaintext" }) };
  env.host.active.fire(env.host.editor);
  expansion.resolve(); await pending; assert.equal(env.host.items[0].visible, false);
  registration.dispose();
});

test("empty count selection and disposal also hide pending model actions", async context => {
  // Guards the shared action display bypassing the status-bar setting or reviving a disposed item.
  const env = setup(), tracked = trackActivity(context, env), expansion = deferred();
  const registration = registerStatus(env.service); await flush();
  const pending = tracked.request("workspace/executeCommand", { command: "dynare/showEffectiveModel", arguments: [env.doc.uri.toString()] }, expansion);
  tracked.clock.tick(250); assert.match(env.host.items[0].text, /Expanding macros/);
  env.configure("statusBar.counts", []); assert.equal(env.host.items[0].visible, false);
  env.configure("statusBar.counts", ["equations"]); await flush();
  assert.match(env.host.items[0].text, /Expanding macros/);
  registration.dispose();
  expansion.resolve(); await pending;
  assert.equal(env.host.items[0].visible, false); assert.equal(env.host.items[0].disposed, true);
});
