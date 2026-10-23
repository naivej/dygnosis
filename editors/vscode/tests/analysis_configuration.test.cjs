const { URL } = require("node:url");
// Exercise native command choices and stored evidence without a second interpreter.
const assert = require("node:assert/strict");
const test = require("node:test");
const Module = require("node:module");
const path = require("node:path");
const { compiledModule } = require("./helpers/extension_modules.cjs");

let host;
class Disposable { constructor(callback) { this.callback = callback; } dispose() { this.callback?.(); } }
function uri(value) {
  const parsed = new URL(value);
  return { scheme: parsed.protocol.slice(0, -1), path: decodeURIComponent(parsed.pathname), fsPath: decodeURIComponent(parsed.pathname), toString: () => value };
}
const vscode = {
  Disposable, ConfigurationTarget: { WorkspaceFolder: 3 },
  Uri: { parse: uri, file: file => uri(`file://${file.startsWith("/") ? "" : "/"}${file.replaceAll("\\", "/")}`) },
  commands: { registerCommand: (id, callback) => { host.commands.set(id, callback); return new Disposable(() => host.commands.delete(id)); } },
  window: {
    get activeTextEditor() { return host.editor; },
    showQuickPick: async (items, options) => { host.picks.push({ items, options }); const answer = host.answers.shift(); return typeof answer === "function" ? answer(items) : answer; },
    showInputBox: async options => { host.inputs.push(options); return host.inputAnswers.shift(); },
    showInformationMessage: async message => { host.messages.push(message); },
    showWarningMessage: async (...args) => { host.warnings.push(args); return host.warningAnswer; },
  },
  workspace: {
    getWorkspaceFolder: value => value.scheme === "file" ? host.folder : undefined,
    getConfiguration: () => ({ inspect: () => ({ workspaceFolderValue: host.saved }), update: async (key, value, target) => { host.updates.push({ key, value, target }); host.saved = value; } }),
  },
};
const originalLoad = Module._load;
Module._load = function (id, ...args) { return id === "vscode" ? vscode : originalLoad.call(this, id, ...args); };
const { registerAnalysisConfigurations, analysisLabel } = require(compiledModule("analysis_configuration"));
Module._load = originalLoad;

function metadata(configuration = {}) {
  return { state: "selected", selection: { configuration_id: "source", name: null, preview: false, invocation_scope: "explicit_macro_inputs" }, configuration,
    selection_generation: 0, include_paths: null, macro_complete: true, model_complete: true, overrides: [], cause: null };
}
function setup() {
  const document = { uri: uri("file:///workspace/main.mod"), version: 1, isClosed: false, languageId: "dynare" };
  host = { commands: new Map(), editor: { document, selection: { active: { line: 1, character: 0 } } }, folder: { uri: uri("file:///workspace"), name: "workspace", index: 0 },
    saved: [], answers: [], inputAnswers: [], warningAnswer: undefined, warnings: [], updates: [], picks: [], inputs: [], messages: [] };
  const proof = { schema_version: 1, root_uri: document.uri.toString(), document_uri: document.uri.toString(), document_version: 1, revision: "guarded-source",
    analysis: metadata(), sites: [], source_coverage: { complete: true } };
  const service = { currentInstance: 1, supportsAnalysisConfiguration: true, log() {}, ensureStarted: async () => {},
    rootForDocument: async () => document.uri, analysisContext: async () => proof, calls: [],
    setAnalysisConfiguration: async (...args) => { service.calls.push(args); return proof; } };
  const sessions = new Map(), previews = { get: value => sessions.get(value.toString()) };
  const comparisons = [];
  const registration = registerAnalysisConfigurations(service, previews, async (...args) => { comparisons.push(args); });
  return { document, proof, service, sessions, comparisons, registration, run: name => host.commands.get(`dygnosis.${name}`)() };
}

test("source and expanded selectors use one root and never apply saved startup choices", async () => {
  const env = setup();
  host.saved = [{ name: "Alternative", root: "main.mod", analysis: { defines: [{ name: "FLAG", expression: "1" }] }, bindings: [] }];
  assert.equal(env.service.calls.length, 0, "registration starts in Source rather than applying a saved preset");
  host.answers.push(items => items.find(item => item.label === "Alternative"));
  await env.run("selectAnalysisConfiguration");
  assert.equal(env.service.calls[0][0].toString(), env.document.uri.toString());
  assert.equal(env.service.calls[0][1].name, "Alternative");
  const expanded = { uri: uri("dygnosis-effective:/1/main.mod"), version: 1, isClosed: false };
  env.sessions.set(expanded.uri.toString(), { root: env.document.uri }); host.editor = { document: expanded };
  host.answers.push(items => {
    const reset = items.find(item => item.label === "Source");
    assert.equal(reset.description, "Reset analysis configuration");
    return reset;
  });
  await env.run("selectAnalysisConfiguration");
  assert.equal(env.service.calls[1][0].toString(), env.document.uri.toString());
  assert.deepEqual(env.service.calls[1][1], {});
  env.registration.dispose();
});

test("editing an earlier macro input preserves the order needed by dependent expressions", async () => {
  const env = setup();
  env.proof.analysis.configuration = { defines: [{ name: "N", expression: "1" }, { name: "M", expression: "N+1" }] };
  host.answers.push(items => items.find(item => item.action === "inputs"), items => items.find(item => item.label === "N"));
  host.inputAnswers.push("2");
  await env.run("selectAnalysisConfiguration");
  assert.deepEqual(env.service.calls[0][1].defines, [{ name: "N", expression: "2" }, { name: "M", expression: "N+1" }]);
  env.registration.dispose();
});

test("definition and branch commands retain exact server targets and support Skip and No branch", async () => {
  const env = setup(), target = { file: "/workspace/main.mod", site_id: "guarded-definition", source_revision: "raw-sha" };
  env.proof.sites = [{ target, kind: "definition", name: "FLAG", expected_text: "@#define FLAG=0", location: { uri: env.document.uri.toString(), range: { start: { line: 1, character: 0 }, end: { line: 1, character: 15 } } }, branches: [] }];
  host.answers.push("Skip this definition");
  await env.run("overrideMacroDefinition");
  assert.deepEqual(env.service.calls[0][1].overrides, [{ kind: "skip_definition", target }]);
  env.proof.sites = [{ ...env.proof.sites[0], kind: "conditional", branches: [{ branch_id: "else-arm", expected_text: "@#else", location: { uri: env.document.uri.toString(), range: { start: { line: 4, character: 0 }, end: { line: 4, character: 6 } } } }] }];
  host.editor.selection.active.line = 4; host.answers.push(items => items[0]);
  await env.run("previewMacroBranch");
  assert.deepEqual(env.service.calls[1][1].overrides, [{ kind: "branch", target, branch_id: null }]);
  env.registration.dispose();
});

test("Save is explicit folder storage and preserves exact binding evidence", async () => {
  const env = setup();
  env.proof.preset_bindings = [{ operation_index: 0, file: "/workspace/main.mod", kind: "definition", name: "FLAG", expected_text: "@#define FLAG=0", target_text: "@#define FLAG=0", branch_index: null, branch_text: null }];
  env.proof.analysis.configuration = { overrides: [{ kind: "definition", target: { file: "/workspace/main.mod", site_id: "old", source_revision: "old-revision" }, expression: "1" }] };
  host.inputAnswers.push("Saved policy");
  await env.run("saveAnalysisConfiguration");
  assert.equal(host.updates.length, 1); assert.equal(host.updates[0].target, vscode.ConfigurationTarget.WorkspaceFolder);
  const saved = host.updates[0].value[0];
  assert.equal(saved.root, "main.mod"); assert.equal(saved.bindings[0].file, "main.mod");
  assert.equal(saved.bindings[0].target_text, "@#define FLAG=0");
  assert.equal(saved.analysis.overrides[0].target.source_revision, "old-revision");
  assert.equal(env.service.calls.length, 0, "Save does not change the live or startup selection");
  env.registration.dispose();
});

test("late picker stops after a document change and untitled Save has an explained refusal", async () => {
  const env = setup();
  host.answers.push(items => { ++env.document.version; return items[0]; });
  await env.run("selectAnalysisConfiguration");
  assert.equal(env.service.calls.length, 0); assert.match(host.messages.at(-1), /changed/);
  env.document.uri = uri("untitled:Untitled-1.mod"); host.folder = undefined;
  await env.run("saveAnalysisConfiguration");
  assert.match(host.messages.at(-1), /Untitled models can keep transient choices/);
  assert.equal(host.updates.length, 0);
  env.registration.dispose();
});

test("comparison passes two explicit frozen sides without changing live selection", async () => {
  const env = setup();
  host.saved = [{ name: "Alternative", root: "main.mod", analysis: { defines: [{ name: "FLAG", expression: "1" }] }, includePaths: ["models"], bindings: [] }];
  host.answers.push(items => items[0], items => items.find(item => item.label === "Alternative"));
  await env.run("compareAnalysisConfigurations");
  assert.equal(env.service.calls.length, 0); assert.equal(env.comparisons.length, 1);
  assert.deepEqual(env.comparisons[0][1], { analysis: {}, include_paths: null });
  assert.equal(env.comparisons[0][2].analysis.name, "Alternative");
  assert.equal(env.comparisons[0][2].include_paths[0], path.resolve("/workspace", "models"));
  assert.equal(analysisLabel({ ...metadata(), state: "stale", selection: { ...metadata().selection, name: "Alternative", preview: true } }), "Alternative (preview) (stale)");
  env.registration.dispose();
});
