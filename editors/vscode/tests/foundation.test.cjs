const { compiledModule } = require("./helpers/extension_modules.cjs");
const assert = require("node:assert/strict");
const test = require("node:test");
const { ClientLifecycle } = require(compiledModule("lifecycle"));
const { parseModelInfo, location } = require(compiledModule("protocol"));

function deferred() { let resolve; const promise = new Promise(done => { resolve = done; }); return { promise, resolve }; }
test("replacement disposes a pending initialize and only activates the newest client", async () => {
  const lifecycle = new ClientLifecycle();
  const pending = deferred();
  let disposed = 0;
  const ready = [];
  const a = { start: () => pending.promise, dispose: () => { ++disposed; pending.resolve(); return Promise.resolve(); } };
  const b = { start: () => Promise.resolve(), dispose: () => Promise.resolve() };
  const first = lifecycle.replace(() => a, () => ready.push("old"));
  await new Promise(resolve => setImmediate(resolve));
  const second = lifecycle.replace(() => b, () => ready.push("new"));
  await Promise.all([first, second]);
  assert.equal(disposed > 0, true);
  assert.deepEqual(ready, ["new"]);
  assert.equal(lifecycle.client, b);
  await lifecycle.shutdown();
});
test("shutdown during startup prevents late callbacks and later replacements", async () => {
  const lifecycle = new ClientLifecycle();
  const pending = deferred();
  let ready = false;
  const start = lifecycle.replace(() => ({ start: () => pending.promise, dispose: () => { pending.resolve(); return Promise.resolve(); } }), () => { ready = true; });
  await new Promise(resolve => setImmediate(resolve));
  await lifecycle.shutdown(); await start;
  await lifecycle.replace(() => { throw new Error("must not create"); }, () => {});
  assert.equal(ready, false);
  assert.equal(lifecycle.client, undefined);
});
function info() {
  return { schema_version: 1, root_uri: "file:///root.mod", document_uri: "file:///root.mod", document_version: 1,
    revision: "one", complete: false, owner_roots: [], statements: [], declarations: [], equations: [], related_files: [], block_categories: [], first_model_anchor: null };
}
test("unknown schemas, wrong ownership and malformed nested locations are rejected", () => {
  assert.equal(parseModelInfo(info(), "file:///root.mod", "file:///root.mod").complete, false);
  assert.throws(() => parseModelInfo({ ...info(), schema_version: 2 }, "file:///root.mod", "file:///root.mod"));
  assert.throws(() => parseModelInfo(info(), "file:///different.mod", "file:///root.mod"));
  assert.equal(location({ uri: "file:///x", range: { start: { line: 1, character: 0 }, end: { line: 0, character: 5 } } }), false);
  assert.throws(() => parseModelInfo({ ...info(), complete: true, n_endogenous: -1 }, "file:///root.mod", "file:///root.mod"));
});
test("preview schemes and user-only executable scope are manifest constraints", () => {
  const manifest = require("../package.json");
  assert.equal(manifest.engines.vscode, "^1.102.0");
  assert.deepEqual(manifest.extensionKind, ["workspace"]);
  assert.equal(manifest.contributes.configuration[0].properties["dynare.serverPath"].scope, "machine");
  assert.equal(manifest.contributes.mcpServerDefinitionProviders[0].id, "dygnosis");
  assert.equal(manifest.activationEvents.includes("onStartupFinished"), true);
});
test("equation navigation manifest replaces old commands and removes the model panel", () => {
  const { contributes } = require("../package.json");
  const commands = contributes.commands.map(row => row.command);
  const replacement = contributes.commands.find(row => row.command === "dygnosis.jumpToEquation");
  assert.equal(replacement.title, "Jump to equation..."); assert.equal(replacement.icon, "$(list-ordered)");
  assert.equal(replacement.enablement, "dygnosis.analysisDocument");
  for (const command of ["dygnosis.goToEquation", "dygnosis.jumpToNamedEquation", "dygnosis.refreshModelView", "dygnosis.openRelatedFile"]) {
    assert.equal(commands.includes(command), false, command);
    assert.equal(Object.values(contributes.menus).flat().some(row => row.command === command), false, command);
  }
  assert.equal(Object.values(contributes.views).flat().some(row => row.id === "dygnosis.model"), false);
  assert.equal(contributes.viewsWelcome.some(row => row.view === "dygnosis.model"), false);
  assert.equal(Object.values(contributes.menus).flat().some(row => /dygnosis\.(model|equationNavigation|includeOwners)\b/.test(row.when ?? "")), false);
  assert.equal(contributes.configuration.some(group => Object.hasOwn(group.properties, "dynare.modelView.sections")), false);
  for (const command of ["dygnosis.jumpToEquation", "dygnosis.chooseModelOwner"]) {
    assert.equal(contributes.menus.commandPalette.some(row => row.command === command && row.when === "dygnosis.analysisDocument"), true, command);
  }
});
