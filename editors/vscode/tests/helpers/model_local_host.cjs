// Verify macro and model-local editing through the native VS Code providers.
const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const vscode = require("vscode");

async function acceptPrefix(document, editor, prefix, label, waitFor, offset) {
  const source = document.getText();
  const cursor = document.positionAt(offset ?? source.indexOf(prefix) + prefix.length);
  editor.selection = new vscode.Selection(cursor, cursor);
  await waitFor(async () => {
    const result = await vscode.commands.executeCommand("vscode.executeCompletionItemProvider", document.uri, cursor);
    return result?.items.some(item => item.label === label);
  }, `${label} prefix completion`);
  await vscode.commands.executeCommand("editor.action.triggerSuggest");
  await waitFor(async () => {
    await vscode.commands.executeCommand("acceptSelectedSuggestion");
    return document.getText() !== source;
  }, `native acceptance of ${label}`);
  return document.getText();
}

async function checkModelLocalEditing(service, workspaceRoot, evidence, helpers) {
  const { waitFor, currentSnapshot, writeObservedInput } = helpers;
  const macroPath = path.join(workspaceRoot, "macro-prefix.mod");
  await writeObservedInput(service, macroPath, "@#def");
  const macro = await vscode.workspace.openTextDocument(macroPath);
  const macroEditor = await vscode.window.showTextDocument(macro);
  assert.equal(await acceptPrefix(macro, macroEditor, "@#def", "define", waitFor), "@#define");

  const filename = path.join(workspaceRoot, "model-local.mod");
  const source = "var y; parameters p; p=.9; model; #zz_helper=p*y(-1); y=zz_hel; end;";
  await writeObservedInput(service, filename, source);
  const document = await vscode.workspace.openTextDocument(filename);
  const editor = await vscode.window.showTextDocument(document);
  const completed = await acceptPrefix(document, editor, "y=zz_hel", "zz_helper", waitFor);
  assert.equal(completed, source.replace("y=zz_hel;", "y=zz_helper;"));
  const info = await currentSnapshot(service, document, () => service.modelInfo(document.uri), "local completion input revision", result => result.model_locals?.definitions.length === 1);
  assert.equal(info.n_equations, 1);
  assert.equal(info.model_locals.definitions[0].name, "zz_helper");
  const position = document.positionAt(completed.indexOf("y=zz_helper") + 3);
  const hovers = await vscode.commands.executeCommand("vscode.executeHoverProvider", document.uri, position);
  assert.ok(hovers.some(hover => hover.contents.some(content => content.value?.includes("Model-local variable"))));
  const definitions = await vscode.commands.executeCommand("vscode.executeDefinitionProvider", document.uri, position);
  assert.equal(definitions.length, 1);
  const definition = definitions[0].targetSelectionRange ?? definitions[0].range;
  assert.equal(document.getText(definition), "zz_helper");
  const renamed = await vscode.commands.executeCommand("vscode.executeDocumentRenameProvider", document.uri, position, "zz_result");
  assert.ok(renamed instanceof vscode.WorkspaceEdit);
  assert.equal(renamed.entries().reduce((sum, [, edits]) => sum + edits.length, 0), 2);
  assert.equal(await vscode.workspace.applyEdit(renamed), true);
  assert.equal(document.getText(), completed.replaceAll("zz_helper", "zz_result"));
  evidence.checks.push("native macro/local prefix acceptance, helper hover, definition provider and versioned rename");

  const productRoot = path.resolve(__dirname, "../../../..");
  const examples = [
    ["Gali_2008/Gali_2008_chapter_3.mod", "Gali model-local helper"],
    ["Basu_Bundick_2017/Basu_Bundick_2017.mod", "Basu-Bundick model-local helper"],
  ];
  for (const [relative, description] of examples) {
    const original = path.join(productRoot, ".agents/skills/use-dynare/references/examples-code", relative);
    const originalText = await fs.readFile(original, "utf8");
    // The archived Gali example uses the retired resid(1) command form.
    const text = originalText.replace("resid(1);", "resid;");
    if (text !== originalText) evidence.checks.push(`${description}: temporary copy updates legacy resid(1) to resid for Dynare 7.2`);
    const copyPath = path.join(workspaceRoot, `local-${path.basename(relative)}`);
    await writeObservedInput(service, copyPath, text);
    const copy = await vscode.workspace.openTextDocument(copyPath);
    const copyEditor = await vscode.window.showTextDocument(copy);
    const snapshot = await currentSnapshot(service, copy, () => service.modelInfo(copy.uri), `${description} snapshot`, result => result.model_locals?.definitions.length > 0);
    const helper = snapshot.model_locals.definitions.find(row => row.origin?.uri === copy.uri.toString());
    assert.ok(helper, `${description} has a written target`);
    const helperPosition = new vscode.Position(helper.origin.range.start.line, helper.origin.range.start.character + 1);
    // Origin covers the # row; locate its identifier through its written line.
    const line = copy.lineAt(helperPosition.line).text;
    const column = line.indexOf(helper.name, line.indexOf("#") + 1);
    assert.ok(column >= 0);
    const site = new vscode.Position(helperPosition.line, column);
    const help = await vscode.commands.executeCommand("vscode.executeHoverProvider", copy.uri, site);
    assert.ok(help.some(hover => hover.contents.some(content => content.value?.includes("Model-local variable"))), description);
    const edits = await vscode.commands.executeCommand("vscode.executeDocumentRenameProvider", copy.uri, site, "zz_verified_helper");
    assert.ok(edits instanceof vscode.WorkspaceEdit, `${description} rename`);
    assert.ok(edits.entries().flatMap(([, rows]) => rows).length >= 2);
    const references = await vscode.commands.executeCommand("vscode.executeReferenceProvider", copy.uri, site);
    const use = references.find(reference => reference.uri.toString() === copy.uri.toString() && !reference.range.contains(site));
    assert.ok(use, `${description} has a bound use`);
    const useOffset = copy.offsetAt(use.range.start);
    const prefix = helper.name.slice(0, -1) || helper.name.toLowerCase();
    const prefixEdit = new vscode.WorkspaceEdit();
    prefixEdit.replace(copy.uri, use.range, prefix);
    await vscode.workspace.applyEdit(prefixEdit);
    assert.equal(await acceptPrefix(copy, copyEditor, prefix, helper.name, waitFor, useOffset + prefix.length), text);
    copyEditor.selection = new vscode.Selection(use.range.start, use.range.start);
    await vscode.commands.executeCommand("editor.action.revealDefinition");
    await waitFor(() => vscode.window.activeTextEditor?.document.uri.toString() === copy.uri.toString() && vscode.window.activeTextEditor.selection.active.line === site.line, `${description} native definition jump`);
    evidence.checks.push(`${description}: temporary copy, prefix acceptance, hover, native definition jump and scoped rename provider`);
  }
}

module.exports = { checkModelLocalEditing };
