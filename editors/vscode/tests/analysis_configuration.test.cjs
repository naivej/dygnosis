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
const { registerAnalysisConfigurations, chooseAnalysisBaseline, analysisLabel, parseMacroInput, parseEquationSelection } = require(compiledModule("analysis_configuration"));
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
  const registration = registerAnalysisConfigurations(service, previews);
  return { document, proof, service, sessions, registration, run: name => host.commands.get(`dygnosis.${name}`)() };
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
  host.answers.push(items => items.find(item => item.action === "input" && item.index === 0));
  host.inputAnswers.push("N=2");
  await env.run("selectAnalysisConfiguration");
  assert.deepEqual(env.service.calls[0][1].defines, [{ name: "N", expression: "2" }, { name: "M", expression: "N+1" }]);
  assert.equal(host.inputs[0].value, "N=1");
  assert.equal(host.inputs.length, 1);
  env.registration.dispose();
});

test("the first macro input goes straight to one complete assignment prompt", async () => {
  const env = setup();
  host.answers.push(items => items.find(item => item.action === "input" && item.index === -1));
  host.inputAnswers.push('COUNTRY="US"');
  await env.run("selectAnalysisConfiguration");
  assert.equal(host.picks.length, 1, "there is no empty Add macro input picker");
  assert.equal(host.inputs.length, 1, "name and expression use one prompt");
  assert.match(host.inputs[0].prompt, /FLAG=true.*COUNTRY="US"/);
  assert.equal(host.inputs[0].validateInput('COUNTRY="US"'), undefined);
  assert.ok(host.inputs[0].validateInput("COUNTRY="));
  assert.deepEqual(env.service.calls[0][1].defines, [{ name: "COUNTRY", expression: '"US"' }]);
  env.registration.dispose();
});

test("adding, renaming and clearing macro assignments keep the other inputs in order", async () => {
  const env = setup();
  env.proof.analysis.configuration = { defines: [{ name: "N", expression: "1" }, { name: "M", expression: "2" }] };
  for (const [index, answer, expected] of [
    [-1, "FLAG=N==M", [{ name: "N", expression: "1" }, { name: "M", expression: "2" }, { name: "FLAG", expression: "N==M" }]],
    [0, "SIZE=3", [{ name: "SIZE", expression: "3" }, { name: "M", expression: "2" }]],
    [0, "  ", [{ name: "M", expression: "2" }]],
  ]) {
    host.answers.push(items => items.find(item => item.action === "input" && item.index === index));
    host.inputAnswers.push(answer);
    await env.run("selectAnalysisConfiguration");
    assert.deepEqual(env.service.calls.at(-1)[1].defines, expected);
  }
  env.registration.dispose();
});

test("macro assignment parsing preserves Dynare expressions and splits only the assignment equals sign", () => {
  for (const [raw, name, expression] of [
    [" FLAG = N == 2 ", "FLAG", "N == 2"], ['COUNTRY="US"', "COUNTRY", '"US"'],
    ['LABEL="a=b,c"', "LABEL", '"a=b,c"'], ["VALUES=[ i in 1:3 when i > 1 ]", "VALUES", "[ i in 1:3 when i > 1 ]"],
  ]) assert.deepEqual(parseMacroInput(raw), { name, expression });
  for (const raw of ["", "FLAG", "FLAG=", "FLAG=  ", "2FLAG=1", "FLAG==1", "bad-name=1"]) {
    assert.equal(parseMacroInput(raw), undefined, raw);
  }
});

test("equation selection composes with macros and Clear equation selection preserves other choices", async () => {
  const env = setup();
  env.proof.analysis.configuration = { defines: [{ name: "FLAG", expression: "1" }], equation_selection: { mode: "exclude", tag: "role", values: ["aux"] } };
  host.answers.push(items => items.find(item => item.action === "include"));
  host.inputAnswers.push("name=('eq1', 'eq 2')");
  await env.run("selectAnalysisConfiguration");
  assert.equal(host.inputs.length, 1, "tag and values use one prompt");
  assert.equal(host.inputs[0].value, "role='aux'");
  assert.match(host.inputs[0].prompt, /name='policy'/);
  assert.equal(host.inputs[0].validateInput("name='policy'"), undefined);
  assert.ok(host.inputs[0].validateInput('name="policy"'));
  assert.deepEqual(env.service.calls[0][1].equation_selection, { mode: "include", tag: "name", values: ["eq1", "eq 2"] });
  assert.deepEqual(env.service.calls[0][1].defines, [{ name: "FLAG", expression: "1" }]);
  env.proof.analysis.configuration = globalThis.structuredClone(env.service.calls[0][1]);
  host.inputAnswers.push("Selected equations");
  await env.run("saveAnalysisConfiguration");
  assert.deepEqual(host.updates[0].value[0].analysis.equation_selection, { mode: "include", tag: "name", values: ["eq1", "eq 2"] });
  env.proof.analysis.configuration = globalThis.structuredClone(env.service.calls[0][1]);
  env.proof.analysis.configuration.overrides = [{ kind: "definition", target: { file: "/workspace/main.mod", site_id: "kept", source_revision: "source" }, expression: "3" }];
  env.proof.analysis.include_paths = ["/workspace/includes"];
  host.answers.push(items => {
    assert.ok(items.some(item => item.action === "clearEquations"));
    return items.find(item => item.action === "clearEquations");
  });
  await env.run("selectAnalysisConfiguration");
  assert.equal(env.service.calls[1][1].equation_selection, undefined);
  assert.deepEqual(env.service.calls[1][1].defines, [{ name: "FLAG", expression: "1" }]);
  assert.deepEqual(env.service.calls[1][1].overrides, env.proof.analysis.configuration.overrides);
  assert.deepEqual(env.service.calls[1][3], ["/workspace/includes"]);
  delete env.proof.analysis.configuration.equation_selection;
  host.answers.push(items => { assert.ok(!items.some(item => item.action === "clearEquations")); return items.find(item => item.label === "Source"); });
  await env.run("selectAnalysisConfiguration");
  assert.deepEqual(env.service.calls[2][1], {});
  env.registration.dispose();
});

test("tag expressions remove syntax quotes and keep commas, spaces and equals signs inside values", () => {
  assert.deepEqual(parseEquationSelection("name='policy'"), { tag: "name", values: ["policy"] });
  assert.deepEqual(parseEquationSelection(" [ role = ('a tag', 'a tag with a, comma', 'a=b') ] "),
    { tag: "role", values: ["a tag", "a tag with a, comma", "a=b"] });
  assert.deepEqual(parseEquationSelection("role=(policy,resource)"), { tag: "role", values: ["policy", "resource"] });
  for (const raw of ["", "policy", "name=", "name=''", 'name="policy"', "name=('policy',)", "name=(,policy)",
    "name=('policy',,'resource')", "name=('policy','policy')", "name=(policy,'policy')", "name='policy", "name='policy'garbage",
    "[name='policy'", "name='policy']", "name='policy',role='resource'", "name==policy", "name=()", "bad-tag='policy'"]) {
    assert.equal(parseEquationSelection(raw), undefined, raw);
  }
});

test("editing a tag selection preserves quoted commas in both the prompt and the applied values", async () => {
  const env = setup();
  const previous = { mode: "exclude", tag: "role", values: ["a tag with a, comma", "resource"] };
  env.proof.analysis.configuration.equation_selection = previous;
  host.answers.push(items => items.find(item => item.action === "exclude"));
  host.inputAnswers.push("role=('a tag with a, comma', 'resource')");
  await env.run("selectAnalysisConfiguration");
  assert.deepEqual(parseEquationSelection(host.inputs[0].value), { tag: previous.tag, values: previous.values });
  assert.deepEqual(env.service.calls[0][1].equation_selection, previous);
  env.registration.dispose();
});

test("invalid or cancelled expression input keeps the current configuration", async () => {
  for (const action of ["input", "include"]) for (const answer of [undefined, "bad input"]) {
    const env = setup();
    host.answers.push(items => items.find(item => item.action === action));
    host.inputAnswers.push(answer);
    await env.run("selectAnalysisConfiguration");
    assert.equal(env.service.calls.length, 0);
    if (answer !== undefined) assert.match(host.messages.at(-1), /Enter a (macro assignment|tag expression)/);
    env.registration.dispose();
  }
});

test("definition override opens one guarded expression input; skip remains a separate command", async () => {
  const env = setup(), target = { file: "/workspace/main.mod", site_id: "guarded-definition", source_revision: "raw-sha" };
  env.proof.sites = [{ target, kind: "definition", name: "FLAG", expected_text: "@#define FLAG=0", location: { uri: env.document.uri.toString(), range: { start: { line: 1, character: 0 }, end: { line: 1, character: 15 } } }, branches: [] }];
  env.proof.analysis.configuration.overrides = [{ kind: "definition", target, expression: "7" }];
  host.inputAnswers.push("8");
  await env.run("overrideMacroDefinition");
  assert.equal(host.picks.length, 0, "a unique definition opens its expression input without a method picker");
  assert.equal(host.inputs.length, 1); assert.equal(host.inputs[0].value, "7");
  assert.equal(host.inputs[0].validateInput(""), "Enter a Dynare macro expression.");
  assert.deepEqual(env.service.calls[0][1].overrides, [{ kind: "definition", target, expression: "8" }]);
  host.inputAnswers.push("");
  await env.run("overrideMacroDefinition");
  assert.match(host.messages.at(-1), /Use Skip Macro Definition/);
  assert.deepEqual(env.service.calls[0][1].overrides, [{ kind: "definition", target, expression: "8" }], "blank input cannot become a skip operation");
  await env.run("skipMacroDefinition");
  assert.deepEqual(env.service.calls[1][1].overrides, [{ kind: "skip_definition", target }]);
  env.proof.sites = [{ ...env.proof.sites[0], kind: "conditional", branches: [{ branch_id: "else-arm", expected_text: "@#else", location: { uri: env.document.uri.toString(), range: { start: { line: 4, character: 0 }, end: { line: 4, character: 6 } } } }] }];
  host.editor.selection.active.line = 4; host.answers.push(items => items[0]);
  await env.run("previewMacroBranch");
  assert.deepEqual(env.service.calls[2][1].overrides, [{ kind: "branch", target, branch_id: null }]);
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

test("baseline picker always offers written source and saved choices without changing live selection", async () => {
  const env = setup();
  const binding = { operation_index: 0, file: "main.mod", kind: "definition", name: "FLAG", expected_text: "@#define FLAG=0", target_text: "@#define FLAG=0", branch_index: null, branch_text: null };
  host.saved = [{ name: "Alternative", root: "main.mod", analysis: { defines: [{ name: "FLAG", expression: "1" }] }, includePaths: ["models"], bindings: [binding] }];
  host.answers.push(items => { assert.equal(items[0].label, "Written source"); assert.equal(items[0].description, undefined); return items.find(item => item.label === "Alternative"); });
  const token = { isCancellationRequested: false };
  const baseline = await chooseAnalysisBaseline(env.document.uri, token);
  assert.equal(host.picks.length, 1);
  assert.equal(host.picks[0].options.title, "Compare main.mod with");
  assert.equal(host.picks[0].options.placeHolder, "Choose written source or a saved configuration");
  assert.equal(baseline.analysis.name, "Alternative");
  assert.deepEqual(baseline.analysis.defines, [{ name: "FLAG", expression: "1" }]);
  assert.equal(baseline.include_paths[0], path.resolve("/workspace", "models"));
  assert.equal(baseline.preset_bindings[0].file, path.resolve("/workspace", "main.mod"));
  host.saved = [];
  host.answers.push(items => { assert.deepEqual(items.map(item => item.label), ["Written source"]); return items[0]; });
  const written = await chooseAnalysisBaseline(env.document.uri, token);
  assert.deepEqual(written, { analysis: {}, include_paths: null });
  host.answers.push(() => undefined);
  assert.equal(await chooseAnalysisBaseline(env.document.uri, token), undefined, "cancellation leaves the comparison baseline unset");
  assert.equal(env.service.calls.length, 0, "choosing Before does not change the active After configuration");
  assert.equal(analysisLabel({ ...metadata(), state: "stale", selection: { ...metadata().selection, name: "Alternative", preview: true } }), "Alternative (preview) (stale)");
  env.registration.dispose();
});
