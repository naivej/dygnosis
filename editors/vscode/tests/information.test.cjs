// Guards retained action context, useful automatic labels and bounded startup evidence.
const assert = require("node:assert/strict");
const test = require("node:test");
const { EventEmitter } = require("node:events");
const { Buffer } = require("node:buffer");
const Module = require("node:module");
const { compiledModule } = require("./helpers/extension_modules.cjs");
const { responseFailure, recoveryActions, reasonText } = require(compiledModule("failure"));
const { configurationSummary } = require(compiledModule("analysis_presentation"));

test("command failures retain the server code and target for cause-specific recovery", () => {
  const response = { code: "ANALYSIS_TARGET_CHANGED", error: "The selected directive changed.", target: { site_id: "if-2" } };
  const failure = responseFailure(response, "Macro expansion", "file:///models/main.mod");
  assert.equal(failure.code, response.code); assert.equal(failure.details, response);
  assert.match(failure.message, /Macro expansion for main\.mod:.*directive changed/);
  assert.equal(recoveryActions(failure)[0].command, "dygnosis.selectAnalysisConfiguration");
  assert.deepEqual(recoveryActions(new Error("connection closed")), []);
});

test("cleared automatic configurations become Source while user preset names remain", () => {
  assert.equal(configurationSummary({ name: "Analysis preview (modified)", overrides: [] }), "Source");
  assert.equal(configurationSummary({ name: "Policy" }), "Policy");
  assert.equal(configurationSummary({ overrides: [{ kind: "branch" }, { kind: "branch" }] }), "2 branch previews");
  assert.equal(configurationSummary({}, []), "0 include paths");
});

test("incomplete causes use their own mapped file and one-based written line", () => {
  assert.equal(reasonText({ message: "Could not open params.inc.", location: { uri: "file:///models/helper.inc", range: { start: { line: 16 } } } }), "helper.inc:17 — Could not open params.inc.");
  assert.equal(reasonText({ message: "Expansion stopped.", location: null }), "Expansion stopped.");
});

test("early MCP exit retains the selected executable, exit code and bounded stderr", async () => {
  const original = Module._load;
  Module._load = function (id, ...args) {
    if (id === "vscode") return {};
    if (id === "node:child_process") return {
      execFile: (_file, _args, _options, done) => done(null, "dygnosis 0.11.35"),
      spawn: () => {
        const child = new EventEmitter(); child.stdout = new EventEmitter(); child.stderr = new EventEmitter(); child.kill = () => {};
        child.stdin = { write: () => { void Promise.resolve().then(() => { child.stderr.emit("data", Buffer.alloc(32000, "x")); child.emit("exit", 7, null); }); } };
        return child;
      },
    };
    return original.call(this, id, ...args);
  };
  let validateMcpBinary;
  try { delete require.cache[require.resolve(compiledModule("binary"))]; ({ validateMcpBinary } = require(compiledModule("binary"))); }
  finally { Module._load = original; }
  const logs = [];
  await assert.rejects(validateMcpBinary({ path: "C:/tools/custom.exe", version: "test", override: true }, message => logs.push(message)), /custom\.exe:.*code 7/);
  const stderr = logs.find(line => line.startsWith("MCP stderr")); assert.ok(stderr); assert.ok(stderr.length < 16600);
});
