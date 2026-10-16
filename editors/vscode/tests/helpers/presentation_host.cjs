// Check equation lists and Outline presentation in an isolated native VS Code host.
const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const vscode = require("vscode");
const { compiledModule } = require("./extension_modules.cjs");
const { modelStatus } = require(compiledModule("status"));
const { nativeWorkbench } = require("./native_webview.cjs");

async function pickerState() {
  return nativeWorkbench(async send => {
    const response = await send("Runtime.evaluate", {
      expression: '({labels:Array.from(document.querySelectorAll(".quick-input-list .monaco-list-row")).filter(row => row.getClientRects().length > 0).map(row => row.querySelector(".label-name")?.textContent),placeholder:document.querySelector(".quick-input-widget input")?.placeholder,notifications:Array.from(document.querySelectorAll(".notification-list-item-message")).map(row=>row.textContent)})',
      returnByValue: true,
    });
    return response.result.value;
  });
}

async function closePicker(waitFor) {
  await waitFor(async () => nativeWorkbench(async send => {
    const visible = await send("Runtime.evaluate", {
      expression: 'Boolean(document.querySelector(".quick-input-widget")?.getClientRects().length)', returnByValue: true,
    });
    if (!visible.result.value) return true;
    await send("Runtime.evaluate", { expression: 'document.querySelector(".quick-input-widget input")?.focus()' });
    await send("Input.dispatchKeyEvent", { type: "keyDown", key: "Escape", code: "Escape", windowsVirtualKeyCode: 27, nativeVirtualKeyCode: 27 });
    await send("Input.dispatchKeyEvent", { type: "keyUp", key: "Escape", code: "Escape", windowsVirtualKeyCode: 27, nativeVirtualKeyCode: 27 });
    return false;
  }), "native picker closes after Escape");
}

async function checkPresentation(service, workspaceRoot, evidence, helpers) {
  const { waitFor, currentSnapshot, writeObservedInput } = helpers;
  assert.ok(process.env.DYGNOSIS_HOST_CDP_PORT, "Presentation acceptance requires the native equation picker");
  await waitFor(() => service.client && service.supportsModelInfo, "presentation language client startup");
  const open = async (name, text) => {
    const filename = path.join(workspaceRoot, name);
    await writeObservedInput(service, filename, text);
    const document = await vscode.workspace.openTextDocument(filename);
    const editor = await vscode.window.showTextDocument(document);
    const info = await currentSnapshot(service, document, () => service.modelInfo(document.uri), `${name} snapshot`);
    return { document, editor, info };
  };
  const input = await open("presentation.mod", "var y;\nparameters rho;\nrho=.9;\nhelper=1;\nmodel;\ny=rho*y(-1);\nend;\n");
  const symbols = await vscode.commands.executeCommand("vscode.executeDocumentSymbolProvider", input.document.uri);
  assert.deepEqual(symbols.map(symbol => symbol.name), ["var", "parameters", "model"]);
  const parameter = symbols.find(symbol => symbol.name === "parameters").children.find(symbol => symbol.name === "rho");
  assert.ok(parameter, "parameter declarations remain available without assignment rows");
  assert.equal(parameter.selectionRange.start.line, 1);
  assert.equal(symbols.find(symbol => symbol.name === "model").detail, "");
  const commands = await vscode.commands.getCommands(true);
  assert.ok(commands.includes("dygnosis.chooseModelOwner"));
  assert.ok(commands.includes("dygnosis.openChanges"));
  evidence.checks.push("native Outline keeps parameter declarations and model equations without assignment rows; complete model has no detail; owner and Changes actions remain registered");

  const pick = async (environment, command, args, cancel = false) => {
    const labels = environment.info.equations.map(row => `${row.number} · ${row.name || "Unnamed equation"}`);
    const before = environment.editor.selection;
    const pending = vscode.commands.executeCommand(command, ...args);
    let settled = false;
    void pending.then(() => { settled = true; }, () => { settled = true; });
    let observed;
    try {
      await waitFor(async () => {
        observed = await pickerState();
        return JSON.stringify(observed.labels) === JSON.stringify(labels);
      }, "one native equation list with every expected row").catch(error => {
        throw new Error(`${error.message}; expected ${JSON.stringify(labels)}, observed ${JSON.stringify(observed)}`);
      });
      if (cancel) await closePicker(waitFor);
      else await vscode.commands.executeCommand("workbench.action.acceptSelectedQuickOpenItem");
      await waitFor(() => settled, "native equation command completes");
      await pending;
      if (cancel) assert.deepEqual(environment.editor.selection, before);
      else {
        const target = environment.info.equations[0].location;
        assert.equal(vscode.window.activeTextEditor.document.uri.toString(), target.uri);
        assert.deepEqual(vscode.window.activeTextEditor.selection.start, new vscode.Position(target.range.start.line, target.range.start.character));
      }
    } finally {
      await closePicker(waitFor);
    }
  };
  await pick(input, "dygnosis.jumpToEquation", [], true);
  await pick(input, "dygnosis.jumpToEquation", []);
  let lens;
  await waitFor(async () => {
    const lenses = await vscode.commands.executeCommand("vscode.executeCodeLensProvider", input.document.uri);
    lens = lenses?.find(row => row.command?.command === "dygnosis.browseLensEquations");
    return lens?.command.title === "Jump to equation...";
  }, "uniform native equation CodeLens title");
  await pick(input, lens.command.command, lens.command.arguments);
  evidence.checks.push("native command and CodeLens each show one unnamed-equation list, jump to written source, and cancel without moving selection");

  const repeated = await open("repeated-opener.mod", "var y;\n@#for copy in 1:2\nmodel;\ny=@{copy};\nend;\n@#endfor\n");
  assert.equal(repeated.info.equations.length, 2);
  const repeatedLenses = await vscode.commands.executeCommand("vscode.executeCodeLensProvider", repeated.document.uri);
  const repeatedLens = repeatedLenses.find(row => row.command?.command === "dygnosis.browseLensEquations");
  assert.equal(repeatedLens.command.title, "Jump to equation...");
  await pick(repeated, repeatedLens.command.command, repeatedLens.command.arguments);
  evidence.checks.push("repeated macro opener lists both counted copies directly and keeps the written target");
  await checkOwnerContexts(service, workspaceRoot, evidence, helpers);

  // External examples are optional for the general harness; release evidence records their pin.
  if (process.env.DYGNOSIS_PINNED_DYNARE_ROOT) {
    for (const [relative, heterogeneous] of [["examples/macroprocessor/bkk_1992.mod", false], ["examples/heterogeneity/krusell_smith_1998.mod", true]]) {
      const example = await open(path.basename(relative), await fs.readFile(path.join(process.env.DYGNOSIS_PINNED_DYNARE_ROOT, relative), "utf8"));
      assert.ok(example.info.complete && example.info.equations.length > 0, `${relative} has complete counted equations`);
      const outline = await vscode.commands.executeCommand("vscode.executeDocumentSymbolProvider", example.document.uri);
      if (heterogeneous) {
        for (const name of ["model(heterogeneity=households)", "var(heterogeneity=households)", "varexo(heterogeneity=households)"]) assert.ok(outline.some(row => row.name === name), name);
        const status = modelStatus(example.info, ["endogenous", "exogenous", "equations"], path.basename(relative));
        assert.match(status.tooltip, /Heterogeneous model: households/);
        assert.match(status.accessibilityLabel, /Heterogeneous model: households/);
      } else assert.ok(example.info.equations.some(row => row.origin_frames?.length > 0), "real macro equations retain origin frames");
      evidence.checks.push(`pinned official ${relative}: complete counts and native presentation`);
    }
  }
}

// Verify the Help owner walkthrough through native commands and diagnostics.
async function checkOwnerContexts(service, workspaceRoot, evidence, { waitFor, currentSnapshot, writeObservedInput }) {
  const directory = path.join(workspaceRoot, "owner-context");
  await fs.mkdir(directory, { recursive: true });
  const rootPath = path.join(directory, "a.mod"), secondPath = path.join(directory, "b.mod"), helperPath = path.join(directory, "helper.mod");
  const parentText = '@#define supplied = 1\n@#include "helper.mod"\n';
  const helperText = '@#define rejected = supplied + "text"\n';
  await writeObservedInput(service, helperPath, helperText);
  await writeObservedInput(service, rootPath, parentText);
  const root = await vscode.workspace.openTextDocument(rootPath);
  await currentSnapshot(service, root, () => service.modelInfo(root.uri), "owner walkthrough parent snapshot");
  const helper = await vscode.workspace.openTextDocument(helperPath);
  await vscode.window.showTextDocument(helper);
  const diagnosticCode = note => typeof note.code === "object" ? note.code.value : note.code;
  const codes = () => vscode.languages.getDiagnostics(helper.uri).map(diagnosticCode);
  await waitFor(async () => (await service.rootForDocument(helper))?.toString() === root.uri.toString() && codes().includes("E285") && !codes().includes("E063"), "native .mod helper uses its unique parent and keeps the real macro type error");
  assert.equal(service.chosenRootForDocument(helper), undefined, "automatic context is not explicit project priority");
  await vscode.commands.executeCommand("dygnosis.treatAsRoot");
  await waitFor(() => codes().includes("E063") && !codes().includes("E285"), "native standalone command changes diagnostic context");
  const pending = vscode.commands.executeCommand("dygnosis.chooseModelOwner");
  try {
    await waitFor(async () => (await pickerState()).labels.some(label => label?.includes("a.mod")), "native owner dropdown shows the parent path");
    await vscode.commands.executeCommand("workbench.action.acceptSelectedQuickOpenItem");
    await pending;
  } finally { await closePicker(waitFor); }
  await waitFor(() => service.chosenRootForDocument(helper)?.toString() === root.uri.toString() && codes().includes("E285") && !codes().includes("E063"), "native chosen owner reaches server diagnostic routing");
  const before = await currentSnapshot(service, helper, () => service.modelForDocument(helper), "chosen helper snapshot");
  const edit = new vscode.WorkspaceEdit();
  edit.replace(helper.uri, new vscode.Range(0, 0, helper.lineCount, 0), "@#define accepted = supplied + 1\n");
  assert.ok(await vscode.workspace.applyEdit(edit));
  await currentSnapshot(service, helper, () => service.modelForDocument(helper), "unsaved helper updates its selected owner", info => info.revision !== before.revision);
  await waitFor(() => !codes().includes("E285") && !codes().includes("E063"), "unsaved helper clears obsolete owner diagnostics");
  assert.ok(await helper.save());
  await writeObservedInput(service, secondPath, parentText);
  const second = await vscode.workspace.openTextDocument(secondPath);
  await currentSnapshot(service, second, () => service.modelInfo(second.uri), "second parent snapshot");
  await vscode.window.showTextDocument(helper);
  await vscode.commands.executeCommand("workbench.action.closeActiveEditor");
  await waitFor(() => !vscode.window.tabGroups.all.some(group => group.tabs.some(tab => tab.input instanceof vscode.TabInputText && tab.input.uri.toString() === helper.uri.toString())) &&
    service.chosenRootForDocument(helper) === undefined, "last helper editor tab closes and clears its explicit context");
  const reopened = await vscode.workspace.openTextDocument(helperPath);
  await vscode.window.showTextDocument(reopened);
  assert.equal(service.chosenRootForDocument(reopened), undefined, "reopening a retained document must not restore the old explicit owner");
  await waitFor(async () => {
    await service.ownerChoices(reopened);
    return service.knownOwners(reopened.uri).length === 2 && await service.rootForDocument(reopened) === undefined;
  }, "reopened helper with two parents waits for a choice");
  await waitFor(() => vscode.languages.getDiagnostics(reopened.uri).length === 0, "ambiguous native context does not publish standalone helper errors");
  await vscode.commands.executeCommand("dygnosis.treatAsRoot");
  await waitFor(() => vscode.languages.getDiagnostics(reopened.uri).some(note => diagnosticCode(note) === "E063"), "explicit standalone stays available with two parents");
  const unknownPath = path.join(directory, "unknown.mod");
  await writeObservedInput(service, unknownPath, "@#define missing = no_parent_value\n");
  const unknown = await vscode.workspace.openTextDocument(unknownPath);
  await vscode.window.showTextDocument(unknown);
  await waitFor(async () => (await service.rootForDocument(unknown))?.toString() === unknown.uri.toString() && vscode.languages.getDiagnostics(unknown.uri).some(note => diagnosticCode(note) === "E063"), "unknown .mod helper keeps ordinary standalone analysis");
  evidence.checks.push("owner Help walkthrough: unique .mod parent, real E285 without phantom E063, native owner picker/standalone commands, unsaved revision, ambiguous reopen, unknown-owner fallback");
}

module.exports = { checkPresentation };
