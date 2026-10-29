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
    get workspaceFile() { return host.workspaceFile; },
    getWorkspaceFolder: value => value.scheme === "file" ? host.folder : undefined,
    getConfiguration: () => ({ inspect: () => ({ workspaceFolderValue: host.saved, workspaceValue: host.workspaceSaved, globalValue: host.globalSaved }), update: async (key, value, target) => { host.updates.push({ key, value, target }); host.saved = value; } }),
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
function conditionalSite(env, line = 1, opener = "@#if FLAG") {
  const location = line => ({ uri: env.document.uri.toString(), range: { start: { line, character: 0 }, end: { line, character: 20 } } });
  return { target: { file: "/workspace/main.mod", site_id: `conditional-${line}`, source_revision: "raw-sha" }, kind: "conditional", name: null,
    expected_text: opener, location: location(line), branches: [opener, "@#elseif OTHER", "@#else"].map((expected_text, index) => ({
      branch_id: `${line}-arm-${index}`, expected_text, location: location(line + index * 2),
    })) };
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

test("simple saved presets need no source bindings in selection or comparison", async () => {
  // Guards a schema-valid hand-written preset disappearing unless internal binding data is added.
  const env = setup();
  host.saved = [{ name: "Simple", root: "main.mod", analysis: { defines: [{ name: "FLAG", expression: "1" }] } }];
  host.answers.push(items => items.find(item => item.label === "Simple"));
  await env.run("selectAnalysisConfiguration");
  assert.equal(env.service.calls.length, 1);
  assert.deepEqual(env.service.calls[0][1].defines, [{ name: "FLAG", expression: "1" }]);
  assert.deepEqual(env.service.calls[0][4], []);
  host.answers.push(items => items.find(item => item.label === "Simple"));
  const baseline = await chooseAnalysisBaseline(env.document.uri, { isCancellationRequested: false });
  assert.equal(baseline.analysis.name, "Simple");
  assert.deepEqual(baseline.preset_bindings, []);
  assert.equal(env.service.calls.length, 1, "comparison selection does not change the live configuration");
  env.registration.dispose();
});

test("folder presets support single-folder Workspace storage without importing multi-root or User settings", async () => {
  // Guards losing presets when a folder is opened alone, or applying shared presets to another folder.
  const env = setup();
  const single = { name: "Single folder", root: "main.mod", analysis: {} };
  host.saved = undefined; host.workspaceSaved = [single];
  host.globalSaved = [{ ...single, name: "Global" }];
  host.answers.push(items => items.find(item => item.label === "Single folder"));
  await env.run("selectAnalysisConfiguration");
  assert.equal(env.service.calls.at(-1)[1].name, "Single folder");
  assert.ok(host.picks.at(-1).items.some(item => item.label === "User settings" && item.description.includes("Unavailable:")));
  host.inputAnswers.push("Saved locally");
  await env.run("saveAnalysisConfiguration");
  assert.deepEqual(host.saved.map(value => value.name), ["Single folder", "Saved locally"]);
  assert.equal(host.updates[0].target, vscode.ConfigurationTarget.WorkspaceFolder);
  host.workspaceFile = uri("file:///workspace/models.code-workspace");
  host.saved = [{ ...single, name: "Folder" }];
  host.answers.push(items => {
    assert.ok(!items.some(item => ["Global", "Single folder"].includes(item.label)));
    assert.ok(items.some(item => item.label === "Workspace settings" && item.description.includes("multi-root")));
    return items.find(item => item.label === "Folder");
  });
  await env.run("selectAnalysisConfiguration");
  assert.equal(env.service.calls.at(-1)[1].name, "Folder");
  const applied = env.service.calls.length;
  for (const scope of ["User settings", "Workspace settings"]) {
    host.answers.push(items => items.find(item => item.label === scope));
    await env.run("selectAnalysisConfiguration");
    assert.match(host.messages.at(-1), /\.vscode\/settings\.json.*current analysis configuration remains active/);
    assert.equal(env.service.calls.length, applied);
  }
  env.registration.dispose();
});

test("invalid saved presets retain a visible cause and cannot change analysis", async () => {
  // Guards silently hiding invalid configurations or applying guarded overrides without their evidence.
  const override = { kind: "skip_definition", target: { file: "main.mod", site_id: "site", source_revision: "source" } };
  const binding = { operation_index: 0, file: "main.mod", kind: "definition", name: "FLAG", expected_text: "@#define FLAG=0", target_text: "@#define FLAG=0", branch_index: null, branch_text: null };
  for (const [fields, cause] of [
    [{ analysis: { overrides: [override] } }, /source information/],
    [{ analysis: { overrides: [override] }, bindings: [] }, /source information/],
    [{ analysis: { overrides: [override] }, bindings: [{ ...binding, operation_index: 1 }] }, /source information/],
    [{ analysis: { overrides: [override, override] }, bindings: [binding, binding] }, /source information/],
    [{ bindings: [{}] }, /source information is invalid/],
    [{ analysis: null }, /Analysis choices/],
    [{ includePaths: [2] }, /Include paths/],
    [{ name: " " }, /Configuration name/],
    [{ root: null }, /Model root/],
  ]) {
    const env = setup();
    host.saved = [{ name: "Broken", root: "main.mod", analysis: {}, ...fields }];
    host.answers.push(items => {
      const row = items.find(item => item.cause);
      assert.ok(row); assert.match(row.description, cause);
      return row;
    });
    await env.run("selectAnalysisConfiguration");
    assert.equal(env.service.calls.length, 0);
    assert.match(host.messages.at(-1), cause);
    assert.match(host.messages.at(-1), /current analysis configuration remains active/);
    host.answers.push(items => items.find(item => item.cause));
    await assert.rejects(chooseAnalysisBaseline(env.document.uri, { isCancellationRequested: false }), cause);
    env.registration.dispose();
  }
});

test("saving preserves invalid and other-model entries and refuses a non-array setting", async () => {
  // Guards Save discarding entries that need repair or replacing a malformed setting without notice.
  const env = setup();
  const invalid = { name: "Broken", root: "main.mod", analysis: null }, other = { name: "Other", root: "other.mod", analysis: {} };
  host.saved = [invalid, other];
  host.inputAnswers.push("Current"); await env.run("saveAnalysisConfiguration");
  assert.deepEqual(host.saved.slice(0, 2), [invalid, other]);
  host.workspaceFile = uri("file:///workspace/models.code-workspace");
  for (const malformed of [{ malformed: true }, null]) {
    host.saved = malformed; host.messages = [];
    host.answers.push(items => items.find(item => item.cause));
    await env.run("selectAnalysisConfiguration");
    assert.match(host.messages.at(-1), /must be an array/);
    host.inputAnswers.push("Another"); await env.run("saveAnalysisConfiguration");
    assert.equal(host.updates.length, 1);
    assert.match(host.messages.at(-1), /No saved configurations were changed/);
    assert.deepEqual(host.saved, malformed);
  }
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

test("definition commands on a conditional explain the required directive without an empty picker", async () => {
  for (const command of ["overrideMacroDefinition", "skipMacroDefinition"]) {
    const env = setup();
    const location = line => ({ uri: env.document.uri.toString(), range: { start: { line, character: 0 }, end: { line, character: 15 } } });
    env.proof.sites = [
      { target: { file: "/workspace/main.mod", site_id: "definition", source_revision: "raw-sha" }, kind: "definition", name: "FLAG", expected_text: "@#define FLAG=0", location: location(0), branches: [] },
      { target: { file: "/workspace/main.mod", site_id: "conditional", source_revision: "raw-sha" }, kind: "conditional", name: null, expected_text: "@#if FLAG", location: location(1),
        branches: [{ branch_id: "if-arm", expected_text: "@#if FLAG", location: location(1) }] },
    ];
    await env.run(command);
    assert.equal(host.picks.length, 0, "a condition has no selectable definition; do not open a picker that accepts no value");
    assert.equal(host.inputs.length, 0);
    assert.equal(env.service.calls.length, 0, "a rejected action keeps the analysis configuration");
    assert.match(host.messages.at(-1), /@#define/);
    assert.match(host.messages.at(-1), /Dygnosis: Preview macro branch\./);
    env.registration.dispose();
  }
});

test("branch preview without a directive gives a cursor hint or retained source cause", async () => {
  const env = setup();
  await env.run("previewMacroBranch");
  assert.equal(host.picks.length, 0);
  assert.equal(env.service.calls.length, 0);
  assert.match(host.messages.at(-1), /Place the cursor on a macro condition or @#else/);
  env.proof.source_coverage = { complete: false, cause: "Source discovery stopped at its work allowance." };
  await env.run("previewMacroBranch");
  assert.equal(host.messages.at(-1), env.proof.source_coverage.cause);
  env.registration.dispose();
});

test("branch directives preview their own arm without a picker and keep other choices", async () => {
  // Guards choosing a different arm, including an enclosing conditional, from the cursor's directive.
  for (const opener of ["@#if FLAG", "@#ifdef FLAG", "@#ifndef FLAG"]) for (const index of [0, 1, 2]) {
    const env = setup(), site = conditionalSite(env), other = conditionalSite(env, 11, opener);
    env.proof.sites = [site, other];
    env.proof.analysis.configuration = { defines: [{ name: "N", expression: "2" }],
      overrides: [{ kind: "branch", target: site.target, branch_id: null }] };
    host.editor.selection.active = other.branches[index].location.range.start;
    await env.run("previewMacroBranch");
    assert.equal(host.picks.length, 0);
    assert.deepEqual(env.service.calls[0][1].overrides, [env.proof.analysis.configuration.overrides[0],
      { kind: "branch", target: other.target, branch_id: other.branches[index].branch_id }]);
    assert.deepEqual(env.service.calls[0][1].defines, env.proof.analysis.configuration.defines);
    env.registration.dispose();
  }
});

test("branch preview elsewhere keeps the configuration without a branch picker", async () => {
  // Guards restoring the file-wide picker from ordinary code, a branch body or its closing directive.
  const env = setup(), first = conditionalSite(env), second = conditionalSite(env, 11, "@#if SECOND");
  env.proof.sites = [first, second];
  env.proof.analysis.configuration = { overrides: [{ kind: "branch", target: first.target, branch_id: first.branches[2].branch_id }] };
  const accepted = globalThis.structuredClone(env.proof.analysis.configuration);
  for (const line of [0, 2, 6]) {
    host.editor.selection.active.line = line;
    await env.run("previewMacroBranch");
    assert.equal(host.picks.length, 0);
    assert.equal(env.service.calls.length, 0);
    assert.deepEqual(env.proof.analysis.configuration, accepted);
    assert.match(host.messages.at(-1), /Place the cursor on a macro condition or @#else/);
  }
  env.registration.dispose();
});

test("expanded previews direct branch actions to written source without a picker", async () => {
  const env = setup(), site = conditionalSite(env);
  env.proof.sites = [site];
  const expanded = { uri: uri("dygnosis-effective:/1/main.mod"), version: 1, isClosed: false };
  env.sessions.set(expanded.uri.toString(), { root: env.document.uri });
  host.editor = { document: expanded, selection: { active: site.branches[0].location.range.start } };
  await env.run("previewMacroBranch");
  assert.equal(host.picks.length, 0);
  assert.equal(env.service.calls.length, 0);
  assert.match(host.messages.at(-1), /Go to written source.*controlling macro directive/);
  env.registration.dispose();
});

test("a changed source or server cannot apply a direct branch preview", async () => {
  // Guards accepting a directive after its captured source or engine changed.
  for (const changed of ["source", "server"]) {
    const env = setup(); env.proof.sites = [conditionalSite(env)];
    env.service.analysisContext = async () => {
      if (changed === "source") ++env.document.version;
      else ++env.service.currentInstance;
      return env.proof;
    };
    await env.run("previewMacroBranch");
    assert.equal(env.service.calls.length, 0);
    assert.equal(host.picks.length, 0);
    assert.match(host.messages.at(-1), /changed/);
    env.registration.dispose();
  }
});

test("Clear branch previews restores source conditions and preserves every other analysis input", async () => {
  // Guards Clear removing definitions or equation selection, or keeping a forced No branch choice.
  const env = setup(), first = conditionalSite(env), second = conditionalSite(env, 11);
  const definition = { kind: "definition", target: { ...first.target, site_id: "define" }, expression: "3" };
  const skipped = { kind: "skip_definition", target: { ...first.target, site_id: "skip" } };
  env.proof.analysis.configuration = { defines: [{ name: "N", expression: "2" }], equation_selection: { mode: "exclude", tag: "name", values: ["policy"] },
    overrides: [{ kind: "branch", target: first.target, branch_id: first.branches[1].branch_id }, definition,
      { kind: "branch", target: second.target, branch_id: null }, skipped] };
  env.proof.analysis.include_paths = ["/workspace/includes"];
  host.answers.push(items => items.find(item => item.label === "Clear branch previews"));
  await env.run("selectAnalysisConfiguration");
  const choices = env.service.calls[0][1];
  assert.deepEqual(choices.overrides, [definition, skipped]);
  assert.deepEqual(choices.defines, env.proof.analysis.configuration.defines);
  assert.deepEqual(choices.equation_selection, env.proof.analysis.configuration.equation_selection);
  assert.deepEqual(env.service.calls[0][3], env.proof.analysis.include_paths);
  env.proof.analysis.configuration = globalThis.structuredClone(choices);
  host.answers.push(items => { assert.ok(!items.some(item => item.action === "clearBranches")); return undefined; });
  await env.run("selectAnalysisConfiguration");
  assert.equal(env.service.calls.length, 1);
  env.registration.dispose();
});

test("definition override opens one guarded expression input; skip remains a separate command", async () => {
  const env = setup(), target = { file: "/workspace/main.mod", site_id: "guarded-definition", source_revision: "raw-sha" };
  env.proof.sites = [{ target, kind: "definition", name: "FLAG", expected_text: "@#define FLAG=0", location: { uri: env.document.uri.toString(), range: { start: { line: 1, character: 0 }, end: { line: 1, character: 15 } } }, branches: [] }];
  env.proof.analysis.configuration.overrides = [{ kind: "definition", target, expression: "7" }];
  host.inputAnswers.push("8");
  await env.run("overrideMacroDefinition");
  assert.equal(host.picks.length, 0, "a unique definition opens its expression input without a method picker");
  assert.equal(host.inputs.length, 1); assert.equal(host.inputs[0].value, "7");
  assert.equal(host.inputs[0].validateInput(""), "Enter a macro expression.");
  assert.deepEqual(env.service.calls[0][1].overrides, [{ kind: "definition", target, expression: "8" }]);
  host.inputAnswers.push("");
  await env.run("overrideMacroDefinition");
  assert.match(host.messages.at(-1), /Use Skip macro definition/);
  assert.deepEqual(env.service.calls[0][1].overrides, [{ kind: "definition", target, expression: "8" }], "blank input cannot become a skip operation");
  await env.run("skipMacroDefinition");
  assert.deepEqual(env.service.calls[1][1].overrides, [{ kind: "skip_definition", target }]);
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
  host.saved = [{ name: "Alternative", root: "main.mod", analysis: { defines: [{ name: "FLAG", expression: "1" }], overrides: [{ kind: "definition", target: { file: "main.mod", site_id: "definition", source_revision: "source" }, expression: "1" }] }, includePaths: ["models"], bindings: [binding] }];
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
  assert.equal(analysisLabel({ ...metadata(), state: "stale", selection: { ...metadata().selection, name: "Alternative", preview: true } }), "Alternative (stale)");
  env.registration.dispose();
});
