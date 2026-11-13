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

test("inactive source ranges accept old engines and reject malformed activity coordinates", () => {
  // Guards invalid activity data dimming unrelated source; the field is additive.
  const valid = { start: { line: 2, character: 1 }, end: { line: 4, character: 0 } };
  const read = inactive_ranges => parseModelInfo({ ...info(), inactive_ranges }, "file:///root.mod", "file:///root.mod");
  assert.equal(parseModelInfo(info(), "file:///root.mod", "file:///root.mod").inactive_ranges, undefined);
  assert.deepEqual(read([valid]).inactive_ranges, [valid]);
  for (const value of [null, {}, [null], [{ start: { line: -1, character: 0 }, end: valid.end }],
    [{ start: valid.end, end: valid.start }]]) assert.throws(() => read(value));
});
test("analysis receipts accept old engines without names and reject malformed optional names", () => {
  const analysis = { state: "selected", selection: { configuration_id: "c", name: null, preview: false, invocation_scope: "source" },
    configuration: { overrides: [{ kind: "definition", target: { file: null, site_id: "s", source_revision: "r" }, expression: "1" }] },
    selection_generation: 0, include_paths: null, macro_complete: true, model_complete: true,
    overrides: [{ operation: { kind: "definition", target: { file: null, site_id: "s", source_revision: "r" }, expression: "1" }, application: "applied", known_execution_count: 1, execution_count: 1, cause: null }] };
  assert.equal(parseModelInfo({ ...info(), analysis }, "file:///root.mod", "file:///root.mod").analysis.overrides[0].name, undefined);
  for (const name of ["x", null]) {
    const named = { ...analysis, overrides: [{ ...analysis.overrides[0], name }] };
    assert.equal(parseModelInfo({ ...info(), analysis: named }, "file:///root.mod", "file:///root.mod").analysis.overrides[0].name, name);
  }
  assert.throws(() => parseModelInfo({ ...info(), analysis: { ...analysis, overrides: [{ ...analysis.overrides[0], name: 1 }] } }, "file:///root.mod", "file:///root.mod"));
  assert.deepEqual(parseModelInfo({ ...info(), analysis: { ...analysis, overrides: [], override_names: ["x"] } }, "file:///root.mod", "file:///root.mod").analysis.override_names, ["x"]);
  for (const override_names of [[], [1], ["x", "y"], null]) {
    assert.throws(() => parseModelInfo({ ...info(), analysis: { ...analysis, overrides: [], override_names } }, "file:///root.mod", "file:///root.mod"));
  }
  // Guards an invalid label array attaching a branch's text to another override.
  for (const override_branch_labels of [[null], ["@#if FLAG → @#else"]]) {
    assert.deepEqual(parseModelInfo({ ...info(), analysis: { ...analysis, override_branch_labels } }, "file:///root.mod", "file:///root.mod").analysis.override_branch_labels, override_branch_labels);
  }
  for (const override_branch_labels of [[], [1], ["first", "second"], null]) {
    assert.throws(() => parseModelInfo({ ...info(), analysis: { ...analysis, override_branch_labels } }, "file:///root.mod", "file:///root.mod"));
  }
});
test("preview schemes and user-only executable scope are manifest constraints", () => {
  const manifest = require("../package.json");
  assert.equal(manifest.engines.vscode, "^1.102.0");
  assert.deepEqual(manifest.extensionKind, ["workspace"]);
  const settings = Object.assign({}, ...manifest.contributes.configuration.map(group => group.properties));
  assert.equal(settings["dynare.serverPath"].scope, "machine");
  assert.equal(manifest.contributes.mcpServerDefinitionProviders[0].id, "dygnosis");
  assert.equal(manifest.activationEvents.includes("onStartupFinished"), true);
  // Guards Settings accepting simple presets without bindings and rejecting overrides without evidence.
  const schema = settings["dynare.analysisConfigurations"];
  const validate = new (require("ajv"))({ logger: false }).compile(schema);
  const simple = { name: "Alternative", root: "main.mod", analysis: { defines: [{ name: "FLAG", expression: "1" }] } };
  assert.equal(validate([simple]), true);
  assert.equal(validate([{ ...simple, analysis: { defines: [{ name: "FLAG", expression: 1 }] } }]), false);
  assert.equal(validate([{ ...simple, analysis: { overrides: [{ kind: "skip_definition" }] }, bindings: [] }]), false);
  const guarded = { ...simple, analysis: { overrides: [{ kind: "skip_definition", target: { file: "main.mod", site_id: "site", source_revision: "source" } }] } };
  assert.equal(validate([guarded]), false);
  assert.equal(validate([{ ...guarded, bindings: [] }]), false);
  assert.equal(validate([{ ...guarded, bindings: [{}] }]), false);
  assert.equal(validate([{ ...guarded, bindings: [{ operation_index: 0, file: "main.mod", kind: "definition", name: "FLAG", expected_text: "@#define FLAG=0", target_text: "@#define FLAG=0", branch_index: null, branch_text: null }] }]), true);
});
test("equation navigation manifest replaces old commands and removes the model panel", () => {
  const { contributes } = require("../package.json");
  const commands = contributes.commands.map(row => row.command);
  const replacement = contributes.commands.find(row => row.command === "dygnosis.jumpToEquation");
  assert.equal(replacement.title, "Jump to equation..."); assert.equal(replacement.icon, "$(list-ordered)");
  assert.equal(replacement.enablement, "dygnosis.analysisDocument");
  for (const command of ["dygnosis.goToEquation", "dygnosis.jumpToNamedEquation", "dygnosis.refreshModelView", "dygnosis.openRelatedFile", "dygnosis.explainDiagnostic"]) {
    assert.equal(commands.includes(command), false, command);
    assert.equal(Object.values(contributes.menus).flat().some(row => row.command === command), false, command);
  }
  assert.equal(Object.values(contributes.views).flat().some(row => row.id === "dygnosis.model"), false);
  assert.equal(contributes.viewsWelcome.some(row => row.view === "dygnosis.model"), false);
  assert.equal(Object.values(contributes.menus).flat().some(row => /dygnosis\.(model|equationNavigation|includeOwners)\b/.test(row.when ?? "")), false);
  assert.equal(contributes.configuration.some(group => Object.hasOwn(group.properties, "dynare.modelView.sections")), false);
  assert.equal(contributes.configuration.some(group => Object.hasOwn(group.properties, "dynare.diagnosticActions.explain")), false);
});

test("the palette exposes user actions in their context and hides aliases and target-only callbacks", () => {
  // Guards actions disappearing behind local controls or appearing without their editor/workspace context.
  const { contributes } = require("../package.json");
  const groups = [
    [undefined, ["openHelp", "restartServer", "showOutput", "openSettings", "workspaceStatus", "showDiagnostic", "showAllDiagnostics"]],
    ["false", ["editSettingsJson", "ignoreWorkspaceFolder"]],
    ["dygnosis.analysisDocument", ["jumpToEquation", "chooseModelOwner", "filterOutline", "overrideMacroDefinition", "skipMacroDefinition", "previewMacroBranch", "goToSourceCandidate", "filterAnalysisByEquation"]],
    ["dygnosis.analysisDocument && !editorReadonly", ["renameSymbol"]],
    ["dygnosis.modelContext", ["showEffectiveModel"]],
    ["dygnosis.analysisDocument && resourceExtname =~ /\\.(mod|dyn)$/i", ["treatAsRoot"]],
    ["resourceScheme == dygnosis-effective && dygnosis.effectivePreview && dygnosis.previewWrittenSource", ["goToWrittenSource"]],
    ["resourceScheme == dygnosis-effective && dygnosis.effectivePreview", ["refreshEffectiveModel"]],
    ["dygnosis.changesContext", ["openChanges", "diffWith"]],
    ["activeCustomEditorId == dygnosis.changes", ["refreshComparison", "swapComparisonSides", "comparisonTextDiff", "findChanges"]],
    ["dygnosis.analysisDocument || dygnosis.effectivePreview", ["selectAnalysisConfiguration", "resetAnalysisConfiguration"]],
    ["(dygnosis.analysisDocument || dygnosis.effectivePreview) && workspaceFolderCount > 0", ["saveAnalysisConfiguration"]],
    ["workspaceFolderCount > 0", ["showNotFullyCheckedModels", "showAllWorkspaceFiles", "recheckWorkspace", "cancelWorkspace", "configureWorkspaceExclusions"]],
    ["workspaceFolderCount > 0 && isWorkspaceTrusted", ["setupWorkspaceMcp"]],
  ];
  const expected = new Map(groups.flatMap(([when, commands]) => commands.map(command => [`dygnosis.${command}`, when])));
  const palette = new Map(contributes.menus.commandPalette.map(row => [row.command, row.when]));
  const editorActions = new Set(contributes.menus["editor/context"].filter(row => row.group.startsWith("0_navigation_dygnosis@")).map(row => row.command));
  assert.equal(palette.size, contributes.menus.commandPalette.length, "each command has one palette visibility rule");
  assert.deepEqual(new Set(contributes.commands.map(row => row.command)), new Set(expected.keys()), "each public action has a deliberate palette classification");
  for (const command of contributes.commands) {
    assert.equal(command.category, editorActions.has(command.command) ? undefined : "Dygnosis", command.command);
    assert.equal(palette.get(command.command), expected.get(command.command), command.command);
  }
  for (const command of palette.keys()) assert.ok(expected.has(command), command);
});

test("editor context commands form one ordered prefixed block with file and preview gates", () => {
  // Guards the native menu contract and duplicate labels in the Command Palette.
  const { contributes } = require("../package.json");
  const commands = contributes.commands;
  assert.equal(contributes.submenus, undefined);
  assert.equal(contributes.menus["dygnosis.editorContext"], undefined);
  const sourceMenu = "dygnosis.analysisDocument && dygnosis.contextActions";
  const [rename, ...context] = contributes.menus["editor/context"];
  assert.deepEqual(rename, { command: "dygnosis.renameSymbol", when: "dygnosis.analysisDocument && !editorReadonly", group: "1_modification@1" });
  assert.equal(commands.find(command => command.command === rename.command)?.title, "Rename symbol");
  const expected = [
    ["goToSourceCandidate", sourceMenu],
    ["goToWrittenSource", "resourceScheme == dygnosis-effective && dygnosis.previewContextActions && dygnosis.previewHasOrigins"],
    ["overrideMacroDefinition", `${sourceMenu} && dygnosis.hasMacroDefinition`],
    ["skipMacroDefinition", `${sourceMenu} && dygnosis.hasMacroDefinition`],
    ["previewMacroBranch", `${sourceMenu} && dygnosis.hasMacroBranch`],
    ["filterAnalysisByEquation", `${sourceMenu} && dygnosis.hasNamedEquation`],
    ["chooseModelOwner", `${sourceMenu} && dygnosis.hasModelOwnerChoice`],
    ["treatAsRoot", `${sourceMenu} && dygnosis.canTreatAsRoot`],
  ];
  assert.deepEqual(context, expected.map(([command, when], index) => ({ command: `dygnosis.${command}`, when, group: `0_navigation_dygnosis@${index + 1}` })));
  for (const row of context) assert.match(commands.find(command => command.command === row.command)?.title, /^Dygnosis: \S/);

  const candidate = commands.find(row => row.command === "dygnosis.goToSourceCandidate");
  assert.equal(candidate?.title, "Dygnosis: Find possible declarations…");
  assert.ok(contributes.menus["editor/title"].some(row => row.command === "dygnosis.showEffectiveModel"));
  assert.ok(contributes.menus["editor/title"].some(row => row.command === "dygnosis.openChanges"));

  const skip = commands.find(row => row.command === "dygnosis.skipMacroDefinition");
  assert.deepEqual({ title: skip?.title, category: skip?.category, enablement: skip?.enablement }, {
    title: "Dygnosis: Skip macro definition for analysis", category: undefined, enablement: "dygnosis.analysisDocument",
  });
  for (const command of ["dygnosis.filterOutline", "dygnosis.showNotFullyCheckedModels", "dygnosis.showAllWorkspaceFiles"]) {
    assert.ok(contributes.menus["view/title"].some(row => row.command === command), command);
  }
  assert.equal(commands.some(row => row.command === "dygnosis.compareAnalysisConfigurations"), false);
  assert.equal(Object.values(contributes.menus).flat().some(row => row.command === "dygnosis.compareAnalysisConfigurations"), false);

  const bindings = contributes.keybindings;
  assert.deepEqual(bindings.filter(row => ["dygnosis.renameSymbol", "dygnosis.goToWrittenSource", "dygnosis.refreshEffectiveModel"].includes(row.command)), [
    { command: "dygnosis.renameSymbol", key: "f2", when: "dygnosis.analysisDocument && editorTextFocus && !editorReadonly" },
    { command: "dygnosis.goToWrittenSource", key: "ctrl+alt+g", mac: "cmd+alt+g", when: "resourceScheme == dygnosis-effective && dygnosis.effectivePreview && editorTextFocus && dygnosis.previewWrittenSource" },
    { command: "dygnosis.refreshEffectiveModel", key: "ctrl+alt+r", mac: "cmd+alt+r", when: "resourceScheme == dygnosis-effective && dygnosis.effectivePreview && editorTextFocus" },
  ]);
  assert.equal(contributes.menus["view/item/context"].some(row => row.command === "dygnosis.ignoreWorkspaceFolder"), true);
  assert.equal(contributes.menus["scm/resourceState/context"].some(row => row.command === "dygnosis.openChanges"), true);
  // Guards Help disappearing from the palette/extension menu or returning as a generic view button.
  assert.ok(commands.some(row => row.command === "dygnosis.openHelp" && row.enablement === undefined));
  assert.deepEqual(contributes.menus["extension/context"], [{ command: "dygnosis.openHelp", when: "extension == coconutwater.dygnosis" }]);
  for (const menu of ["editor/title", "editor/context", "view/title"]) {
    assert.ok(!contributes.menus[menu].some(row => row.command === "dygnosis.openHelp"), menu);
  }
});
