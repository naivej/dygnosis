const assert = require("node:assert/strict");
const test = require("node:test");
const Module = require("node:module");
const path = require("node:path");
const globals = { searchPaths: ["user"], formatIndent: 2, "nameDetails.tex": false };
const workspace = { searchPaths: ["workspace"], formatIndent: 6, "outline.sections": ["blocks", "blocks", "invented"] };
const defaults = { searchPaths: [], formatIndent: "tab" };
const vscode = { workspace: {
  workspaceFolders: [{ uri: { scheme: "file", toString: () => "file:///folder" } }],
  getConfiguration: (_section, scope) => ({
    get: (key, fallback) => (scope?.uri ? workspace[key] : workspace[key]) ?? globals[key] ?? defaults[key] ?? fallback,
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
