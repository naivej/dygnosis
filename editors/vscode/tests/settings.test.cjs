const { compiledModule } = require("./helpers/extension_modules.cjs");
const assert = require("node:assert/strict");
const test = require("node:test");
const Module = require("node:module");
const path = require("node:path");
const globals = { searchPaths: ["user"], formatIndent: 2, "nameDetails.tex": false };
const workspace = { searchPaths: ["workspace"], formatIndent: 6, "outline.sections": ["blocks", "blocks", "invented"] };
const defaults = { searchPaths: [], formatIndent: "tab" };
const folderOverrides = new Map();
const vscode = { workspace: {
  workspaceFolders: [{ uri: { scheme: "file", toString: () => "file:///folder" } }],
  getConfiguration: (_section, scope) => ({
    get: (key, fallback) => folderOverrides.get((scope?.uri ?? scope)?.toString())?.[key] ?? workspace[key] ?? globals[key] ?? defaults[key] ?? fallback,
    inspect: key => ({ globalValue: globals[key], defaultValue: defaults[key], workspaceValue: workspace[key] }),
  }),
} };
const originalLoad = Module._load;
Module._load = function(id, ...args) { return id === "vscode" ? vscode : originalLoad.call(this, id, ...args); };
const { configurationSnapshot, listSetting, booleanSetting } = require(compiledModule("settings"));
const { executablePath, validateMcpBinary } = require(compiledModule("binary"));
Module._load = originalLoad;

test("loose files use user defaults while folders receive complete workspace settings", () => {
  const logged = [];
  const snapshot = configurationSnapshot(message => logged.push(message)).dynare.configuration;
  assert.deepEqual(snapshot.loose.searchPaths, ["user"]);
  assert.equal(snapshot.loose.formatIndent, 2);
  // Guards a client fallback that silently overrides the advertised default.
  const manifest = require("../package.json");
  const workDefault = Object.assign({}, ...manifest.contributes.configuration.map(group => group.properties))["dynare.macroWorkLimit"].default;
  assert.equal(snapshot.loose.macroWorkLimit, workDefault);
  assert.equal(snapshot.folders[0].settings.macroWorkLimit, workDefault);
  assert.equal(snapshot.loose.nameDetails.tex, false);
  assert.deepEqual(snapshot.folders[0].settings.searchPaths, ["workspace"]);
  assert.equal(snapshot.folders[0].settings.formatIndent, 6);
  assert.deepEqual(snapshot.folders[0].settings.outline.sections, ["blocks"]);
  assert.ok(logged.length > 0);
});
test("invalid values fall back and empty presentation lists remain empty", () => {
  globals.enabled = "invalid"; globals.sections = [];
  assert.equal(booleanSetting("enabled", undefined, true, () => {}), true);
  assert.deepEqual(listSetting("sections", undefined, ["one"], ["one"], () => {}), []);
});

test("Outline activity choices resolve by resource and invalid values use Both", () => {
  const logged = [];
  globals["outline.activity"] = "inactive";
  workspace["outline.activity"] = "active";
  try {
    let settings = configurationSnapshot(message => logged.push(message)).dynare.configuration;
    assert.equal(settings.loose.outline.activity, "inactive");
    assert.equal(settings.folders[0].settings.outline.activity, "active");
    workspace["outline.activity"] = "invalid";
    settings = configurationSnapshot(message => logged.push(message)).dynare.configuration;
    assert.equal(settings.folders[0].settings.outline.activity, "both");
    assert.ok(logged.some(message => /Invalid dynare.outline.activity.*expected both, active or inactive; using both/.test(message)));
  } finally { delete globals["outline.activity"]; delete workspace["outline.activity"]; }
});
test("macro work settings reach each root scope and simultaneous tint changes reach the engine", () => {
  const { analysisSettingsChanged } = require(compiledModule("settings"));
  globals.macroWorkLimit = 500000;
  workspace.macroWorkLimit = 2000000;
  try {
    const snapshot = configurationSnapshot(() => {}).dynare.configuration;
    assert.equal(snapshot.loose.macroWorkLimit, 500000);
    assert.equal(snapshot.folders[0].settings.macroWorkLimit, 2000000);
    assert.equal(analysisSettingsChanged({ affectsConfiguration: key => ["dynare.effectiveModel.macroTint", "dynare.macroWorkLimit"].includes(key) }), true);
    assert.equal(analysisSettingsChanged({ affectsConfiguration: key => key === "dynare.effectiveModel.macroTint" }), false);
    globals.macroWorkLimit = "invalid";
    assert.equal(configurationSnapshot(() => {}).dynare.configuration.loose.macroWorkLimit, "invalid", "the server validates and explains the fallback");
  } finally { delete globals.macroWorkLimit; delete workspace.macroWorkLimit; }
});
test("workspace switch uses window precedence while exclusions resolve independently for each folder", () => {
  const originalFolders = vscode.workspace.workspaceFolders;
  vscode.workspace.workspaceFolders = ["file:///a", "file:///b", "remote:/virtual"].map(value => ({ uri: { scheme: value.startsWith("file:") ? "file" : "remote", toString: () => value } }));
  globals.workspaceDiagnostics = false; workspace.workspaceDiagnostics = true;
  globals.workspaceExcludePaths = ["user/**"]; workspace.workspaceExcludePaths = ["workspace/**"];
  folderOverrides.set("file:///a", { workspaceDiagnostics: false, workspaceExcludePaths: ["a/**"] });
  folderOverrides.set("file:///b", { workspaceDiagnostics: false, workspaceExcludePaths: ["b/**"] });
  try {
    let settings = configurationSnapshot(() => {}).dynare.configuration;
    assert.equal(settings.loose.workspaceDiagnostics, true, "workspace window value overrides the user switch");
    assert.deepEqual(settings.loose.workspaceExcludePaths, ["user/**"], "loose defaults retain resource-setting user precedence");
    assert.equal(settings.folders.length, 2);
    assert.deepEqual(settings.folders.map(entry => entry.settings.workspaceExcludePaths), [["a/**"], ["b/**"]]);
    assert.ok(settings.folders.every(entry => !Object.hasOwn(entry.settings, "workspaceDiagnostics")), "folder values cannot override the window switch");
    workspace.workspaceDiagnostics = false;
    assert.equal(configurationSnapshot(() => {}).dynare.configuration.loose.workspaceDiagnostics, false);
    delete workspace.workspaceDiagnostics;
    assert.equal(configurationSnapshot(() => {}).dynare.configuration.loose.workspaceDiagnostics, false, "reset to the user switch");
    delete globals.workspaceDiagnostics;
    assert.equal(configurationSnapshot(() => {}).dynare.configuration.loose.workspaceDiagnostics, true, "reset to default on");
    folderOverrides.delete("file:///a");
    settings = configurationSnapshot(() => {}).dynare.configuration;
    assert.deepEqual(settings.folders[0].settings.workspaceExcludePaths, ["workspace/**"], "folder reset restores inherited exclusions");
    delete workspace.workspaceExcludePaths;
    assert.deepEqual(configurationSnapshot(() => {}).dynare.configuration.folders[0].settings.workspaceExcludePaths, ["user/**"]);
    delete globals.workspaceExcludePaths;
    assert.deepEqual(configurationSnapshot(() => {}).dynare.configuration.folders[0].settings.workspaceExcludePaths, []);
  } finally {
    vscode.workspace.workspaceFolders = originalFolders; folderOverrides.clear();
    delete globals.workspaceDiagnostics; delete workspace.workspaceDiagnostics; delete globals.workspaceExcludePaths; delete workspace.workspaceExcludePaths;
  }
});
test("invalid workspace values fall back or filter entries and explain the exact setting", () => {
  globals.workspaceDiagnostics = "on"; globals.workspaceExcludePaths = ["valid/**", 7, null];
  workspace.workspaceExcludePaths = "invalid";
  try {
    const logged = [], settings = configurationSnapshot(message => logged.push(message)).dynare.configuration;
    assert.equal(settings.loose.workspaceDiagnostics, true);
    assert.deepEqual(settings.loose.workspaceExcludePaths, ["valid/**"]);
    assert.deepEqual(settings.folders[0].settings.workspaceExcludePaths, []);
    assert.ok(logged.some(message => /Invalid dynare.workspaceDiagnostics.*expected true or false; using true/.test(message)));
    assert.ok(logged.some(message => /Invalid dynare.workspaceExcludePaths.*non-string entries ignored/.test(message)));
  } finally { delete globals.workspaceDiagnostics; delete globals.workspaceExcludePaths; delete workspace.workspaceExcludePaths; }
});
test("one absolute executable resolver supports spaces and never selects PATH", () => {
  const extension = path.resolve("directory with spaces");
  const override = path.join(extension, "custom engine.exe");
  assert.deepEqual(executablePath(extension, override), { path: override, override: true });
  assert.equal(executablePath(extension, "").override, false);
  assert.equal(executablePath(extension, "").path, path.join(extension, "bin", process.platform === "win32" ? "dygnosis.exe" : "dygnosis"));
  assert.throws(() => executablePath(extension, "dygnosis"));
  assert.throws(() => executablePath(extension, 123));
});
test("selected engine answers a real MCP initialize probe", { skip: !process.env.DYGNOSIS_TEST_BINARY }, async () => {
  await validateMcpBinary({ path: process.env.DYGNOSIS_TEST_BINARY, override: true, version: "test" }, () => {});
});
