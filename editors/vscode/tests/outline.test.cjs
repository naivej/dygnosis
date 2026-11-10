// Exercise the native Outline filter command's choice, scope and cancellation.
const assert = require("node:assert/strict");
const test = require("node:test");
const Module = require("node:module");
const { compiledModule, importedModule } = require("./helpers/extension_modules.cjs");
let host;
const vscode = {
  ConfigurationTarget: { WorkspaceFolder: 3, Global: 1 },
  commands: { registerCommand: (_id, callback) => { host.run = callback; return { dispose() {} }; } },
  window: {
    get activeTextEditor() { return host.editor; },
    showQuickPick: async (items, options) => { host.picker = { items, options }; host.onPick?.(); return items.find(item => item.activity === host.answer); },
    showInformationMessage: async message => host.messages.push(message),
  },
  workspace: {
    getWorkspaceFolder: () => host.folder,
    getConfiguration: (_name, scope) => ({
      get: (_key, fallback) => host.current ?? fallback,
      update: async (...args) => { host.updates.push({ scope, args }); },
    }),
  },
};
const originalLoad = Module._load;
Module._load = function (id, ...args) {
  if (id === "vscode") return vscode;
  if (importedModule(id, args[0]) === "client") return { isAnalysisDocument: document => document.languageId === "dynare" && document.uri.scheme !== "dygnosis-effective" };
  return originalLoad.call(this, id, ...args);
};
const { registerOutlineFilter } = require(compiledModule("outline"));
Module._load = originalLoad;
function setup() {
  const document = name => ({ languageId: "dynare", uri: { scheme: "file", path: "/" + name, toString() { return this.path; } }, version: 1, isClosed: false });
  const first = document("first.mod"), second = document("second.mod");
  host = { editor: { document: first }, first, second, updates: [], messages: [], filters: new Map(), instance: 1, owner: "owner" };
  host.client = { log() {}, ensureStarted: async () => {}, get currentInstance() { return host.instance; }, supportsOutlineActivity: true,
    chosenRootForDocument: doc => doc.uri, rootForDocument: async () => undefined, documentContextIdentity: () => host.owner,
    outlineActivity: root => host.filters.get(root.path) ?? "both",
    setOutlineActivity: async (root, activity) => { host.filters.set(root.path, activity); host.updates.push({ root: root.path, activity }); } };
  registerOutlineFilter(host.client);
}
test("current choice is first and Enter or dismissal changes nothing", async () => {
  setup(); host.filters.set("/first.mod", "inactive"); host.answer = "inactive"; await host.run();
  assert.equal(host.picker.items[0].label, "Inactive"); assert.equal(host.picker.items[0].description, "Current");
  assert.match(host.picker.options.title, /first.mod/); assert.deepEqual(host.updates, []);
  host.answer = undefined; await host.run(); assert.deepEqual(host.updates, []);
});
test("Outline choice follows each model and returns with its current choice", async () => {
  setup(); host.answer = "active"; await host.run();
  host.editor = { document: host.second }; host.answer = "inactive"; await host.run();
  host.editor = { document: host.first }; host.answer = "active"; await host.run();
  assert.equal(host.picker.items[0].label, "Active");
  assert.deepEqual(host.updates, [{ root: "/first.mod", activity: "active" }, { root: "/second.mod", activity: "inactive" }]);
});
test("ambiguous owner and older engine explain why no filter was applied", async () => {
  setup(); host.client.chosenRootForDocument = () => undefined; await host.run();
  assert.match(host.messages[0], /Choose model owner/); assert.deepEqual(host.updates, []);
  setup(); host.client.supportsOutlineActivity = false; await host.run();
  assert.match(host.messages[0], /engine does not support temporary Outline filters/); assert.deepEqual(host.updates, []);
});
test("stale selection cannot apply to another source or owner", async () => {
  for (const change of [() => { host.editor = { document: host.second }; }, () => { ++host.first.version; }, () => { host.owner = "other"; }]) {
    setup(); host.answer = "active"; host.onPick = change; await host.run();
    assert.match(host.messages[0], /source or model changed/); assert.deepEqual(host.updates, []);
  }
});
test("unsupported editors explain the filter outcome", async () => {
  setup(); host.editor.document.languageId = "plaintext"; await host.run();
  assert.match(host.messages[0], /Open a Dynare source file/); assert.deepEqual(host.updates, []);
});
