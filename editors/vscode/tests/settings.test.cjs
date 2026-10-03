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
const { configurationSnapshot, listSetting, booleanSetting } = require("../out/settings");
const { executablePath, validateMcpBinary } = require("../out/binary");
Module._load = originalLoad;

test("loose files use user defaults while folders receive complete workspace settings", () => {
  const logged = [];
  const snapshot = configurationSnapshot(message => logged.push(message)).dynare.configuration;
  assert.deepEqual(snapshot.loose.searchPaths, ["user"]);
  assert.equal(snapshot.loose.formatIndent, 2);
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
test("project switch uses window precedence while exclusions resolve independently for each folder", () => {
  const originalFolders = vscode.workspace.workspaceFolders;
  vscode.workspace.workspaceFolders = ["file:///a", "file:///b", "remote:/virtual"].map(value => ({ uri: { scheme: value.startsWith("file:") ? "file" : "remote", toString: () => value } }));
  globals.projectDiagnostics = false; workspace.projectDiagnostics = true;
  globals.projectExcludePaths = ["user/**"]; workspace.projectExcludePaths = ["workspace/**"];
  folderOverrides.set("file:///a", { projectDiagnostics: false, projectExcludePaths: ["a/**"] });
  folderOverrides.set("file:///b", { projectDiagnostics: false, projectExcludePaths: ["b/**"] });
  try {
    let settings = configurationSnapshot(() => {}).dynare.configuration;
    assert.equal(settings.loose.projectDiagnostics, true, "workspace window value overrides the user switch");
    assert.deepEqual(settings.loose.projectExcludePaths, ["user/**"], "loose defaults retain resource-setting user precedence");
    assert.equal(settings.folders.length, 2);
    assert.deepEqual(settings.folders.map(entry => entry.settings.projectExcludePaths), [["a/**"], ["b/**"]]);
    assert.ok(settings.folders.every(entry => !Object.hasOwn(entry.settings, "projectDiagnostics")), "folder values cannot override the window switch");
    workspace.projectDiagnostics = false;
    assert.equal(configurationSnapshot(() => {}).dynare.configuration.loose.projectDiagnostics, false);
    delete workspace.projectDiagnostics;
    assert.equal(configurationSnapshot(() => {}).dynare.configuration.loose.projectDiagnostics, false, "reset to the user switch");
    delete globals.projectDiagnostics;
    assert.equal(configurationSnapshot(() => {}).dynare.configuration.loose.projectDiagnostics, true, "reset to default on");
    folderOverrides.delete("file:///a");
    settings = configurationSnapshot(() => {}).dynare.configuration;
    assert.deepEqual(settings.folders[0].settings.projectExcludePaths, ["workspace/**"], "folder reset restores inherited exclusions");
    delete workspace.projectExcludePaths;
    assert.deepEqual(configurationSnapshot(() => {}).dynare.configuration.folders[0].settings.projectExcludePaths, ["user/**"]);
    delete globals.projectExcludePaths;
    assert.deepEqual(configurationSnapshot(() => {}).dynare.configuration.folders[0].settings.projectExcludePaths, []);
  } finally {
    vscode.workspace.workspaceFolders = originalFolders; folderOverrides.clear();
    delete globals.projectDiagnostics; delete workspace.projectDiagnostics; delete globals.projectExcludePaths; delete workspace.projectExcludePaths;
  }
});
test("invalid project values fall back or filter entries and explain the exact setting", () => {
  globals.projectDiagnostics = "on"; globals.projectExcludePaths = ["valid/**", 7, null];
  workspace.projectExcludePaths = "invalid";
  try {
    const logged = [], settings = configurationSnapshot(message => logged.push(message)).dynare.configuration;
    assert.equal(settings.loose.projectDiagnostics, true);
    assert.deepEqual(settings.loose.projectExcludePaths, ["valid/**"]);
    assert.deepEqual(settings.folders[0].settings.projectExcludePaths, []);
    assert.ok(logged.includes("Invalid dynare.projectDiagnostics; using true."));
    assert.ok(logged.includes("Invalid dynare.projectExcludePaths entries were ignored."));
  } finally { delete globals.projectDiagnostics; delete globals.projectExcludePaths; delete workspace.projectExcludePaths; }
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
