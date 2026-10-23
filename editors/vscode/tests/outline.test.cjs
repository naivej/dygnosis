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
    showQuickPick: async (items, options) => { host.picker = { items, options }; return items.find(item => item.activity === host.answer); },
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
  host = { editor: { document: { languageId: "dynare", uri: { scheme: "file" } } }, folder: {}, current: "both", updates: [], messages: [] };
  registerOutlineFilter({ log() {} });
}
test("filter writes the chosen mode for the current folder and marks the current choice", async () => {
  setup(); host.answer = "inactive";
  await host.run();
  assert.deepEqual(host.picker.items.map(item => item.label), ["Both", "Active", "Inactive"]);
  assert.equal(host.picker.items[0].description, "Current");
  assert.deepEqual(host.updates, [{ scope: host.editor.document.uri, args: ["outline.activity", "inactive", 3] }]);
});
test("loose files use the user setting and cancel or unchanged choices write nothing", async () => {
  setup(); host.folder = undefined; host.answer = "active";
  await host.run(); assert.equal(host.updates[0].args[2], 1);
  for (const answer of [undefined, "both"]) {
    setup(); host.answer = answer; await host.run(); assert.deepEqual(host.updates, []);
  }
});
test("unsupported editors explain the filter outcome without changing settings", async () => {
  setup(); host.editor.document.languageId = "plaintext";
  await host.run(); assert.equal(host.messages.length, 1); assert.deepEqual(host.updates, []);
});
