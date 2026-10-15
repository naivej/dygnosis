// Verify Dynare syntax consumers through the native editor providers.
const assert = require("node:assert/strict");
const path = require("node:path");
const vscode = require("vscode");
const { nativeWorkbench } = require("./native_webview.cjs");

async function checkSyntax(service, workspaceRoot, evidence, helpers) {
  const { waitFor, currentSnapshot, writeObservedInput } = helpers;
  const body = "[name='aux'] x=rho*x(-1);\n[name='wanted'] y=var_expectation(ve)+log(1);\n";
  await writeObservedInput(service, path.join(workspaceRoot, "syntax-body.inc"), body);
  const source = "@#define twice(x) = x*2\nvar y x;\nparameters rho betta;\nrho=.5; betta=.99;\nmodel;\n@#include \"syntax-body.inc\"\nend;\nvar_model(model_name=vm,eqtags=['aux']);\nvar_expectation_model(model_name=ve,auxiliary_model_name=vm,horizon=1,expression=x);\npac_model(model_name=pm,auxiliary_model_name=vm,discount=betta);\n";
  const filename = path.join(workspaceRoot, "syntax.mod");
  await writeObservedInput(service, filename, source);
  const document = await vscode.workspace.openTextDocument(filename);
  const editor = await vscode.window.showTextDocument(document);
  await currentSnapshot(service, document, () => service.modelInfo(document.uri), "syntax owner snapshot", info => info.complete);
  const symbols = await vscode.commands.executeCommand("vscode.executeDocumentSymbolProvider", document.uri);
  const flat = rows => rows.flatMap(row => [row, ...flat(row.children ?? [])]);
  for (const name of ["twice", "vm", "ve", "pm"]) {
    const row = flat(symbols).find(row => row.name === name);
    assert.ok(row, `native Outline includes ${name}`);
    assert.equal(document.getText(row.selectionRange), name, `native Outline selects ${name}`);
  }
  for (const word of ["var_model", "pac_model"]) {
    const hover = await vscode.commands.executeCommand("vscode.executeHoverProvider", document.uri, document.positionAt(source.indexOf(word) + 1));
    assert.ok(hover?.length, `${word} has native syntax help`);
  }
  const child = await vscode.workspace.openTextDocument(path.join(workspaceRoot, "syntax-body.inc"));
  await vscode.window.showTextDocument(child);
  await currentSnapshot(service, child, () => service.modelForDocument(child), "included expression snapshot");
  const completion = await vscode.commands.executeCommand("vscode.executeCompletionItemProvider", child.uri, child.positionAt(body.indexOf("log(") + 4));
  assert.ok(completion.items.some(item => item.label === "diff"), "included model argument offers model operators");
  const signature = await vscode.commands.executeCommand("vscode.executeSignatureHelpProvider", child.uri, child.positionAt(body.indexOf("log(") + 4));
  assert.ok(signature?.signatures.length, "included expression has native signature help");
  const definition = await vscode.commands.executeCommand("vscode.executeDefinitionProvider", child.uri, child.positionAt(body.indexOf("ve") + 1));
  assert.ok(definition?.some(location => (location.targetUri ?? location.uri).toString() === document.uri.toString()), "named expectation use navigates to its declared model");
  const namedUse = source.indexOf("auxiliary_model_name=vm") + "auxiliary_model_name=".length;
  const refs = await vscode.commands.executeCommand("vscode.executeReferenceProvider", document.uri, document.positionAt(namedUse));
  assert.ok(refs?.length >= 3, "named VAR binding includes declaration and both accepted uses");
  const tokens = await vscode.commands.executeCommand("vscode.provideDocumentSemanticTokens", document.uri);
  assert.ok(tokens?.data?.length, "native editor receives semantic names");
  evidence.checks.push("native syntax help, include completion/signature/definition, typed references, semantic names and exact macro/model Outline targets");

  await vscode.window.showTextDocument(document);
  const configuration = vscode.workspace.getConfiguration("editor", document.uri);
  const oldSemantic = configuration.inspect("semanticHighlighting.enabled")?.workspaceFolderValue;
  const workbench = vscode.workspace.getConfiguration("workbench");
  const oldTheme = workbench.inspect("colorTheme")?.workspaceValue;
  let colorEvidence;
  try {
    await configuration.update("semanticHighlighting.enabled", false, vscode.ConfigurationTarget.WorkspaceFolder);
    await workbench.update("colorTheme", "Default Dark+", vscode.ConfigurationTarget.Workspace);
    await waitFor(async () => nativeWorkbench(async send => {
      const result = await send("Runtime.evaluate", { expression: 'Array.from(document.querySelectorAll(".monaco-editor .view-lines .view-line")).filter(line=>line.getClientRects().length).map(line=>({text:line.textContent,parts:Array.from(line.querySelectorAll("span")).filter(span=>!span.children.length).map(span=>({text:span.textContent,color:getComputedStyle(span).color}))}))', returnByValue: true });
      const lines = result.result.value ?? [];
      colorEvidence = ["var_model", "pac_model"].map(word => {
        const line = lines.find(line => line.text.startsWith(word));
        const head = line?.parts.find(part => part.text.includes(word));
        const value = line?.parts.find(part => /\bvm\b/.test(part.text));
        return { word, head: head?.color, value: value?.color };
      });
      return colorEvidence.every(row => row.head && row.value && row.head !== row.value);
    }), "native TextMate distinguishes PAC/VAR command heads from plain values");
    evidence.syntax_colors = colorEvidence;
    evidence.checks.push("native PAC/VAR command heads differ from plain values under Default Dark+ with semantic highlighting disabled");
  } finally {
    evidence.syntax_colors = colorEvidence;
    await configuration.update("semanticHighlighting.enabled", oldSemantic, vscode.ConfigurationTarget.WorkspaceFolder);
    await workbench.update("colorTheme", oldTheme, vscode.ConfigurationTarget.Workspace);
  }

  const recoveryFile = path.join(workspaceRoot, "syntax-recovery.mod");
  const recovery = "var y;\nmodel;\ny=log(1);\nend;\nconditional_forecast_paths;\nvar y;\nperiods 1;\nvalues 1;\nend;\n";
  await writeObservedInput(service, recoveryFile, recovery);
  const recovering = await vscode.workspace.openTextDocument(recoveryFile);
  const recoveryEditor = await vscode.window.showTextDocument(recovering);
  await currentSnapshot(service, recovering, () => service.modelInfo(recovering.uri), "format syntax snapshot", info => info.complete);
  const edits = await vscode.commands.executeCommand("vscode.executeFormatDocumentProvider", recovering.uri, { tabSize: 2, insertSpaces: true });
  assert.ok(edits?.length, "parser-owned blocks offer native formatting");
  const edit = new vscode.WorkspaceEdit(); edit.set(recovering.uri, edits);
  assert.equal(await vscode.workspace.applyEdit(edit), true);
  await currentSnapshot(service, recovering, () => service.modelInfo(recovering.uri), "formatted syntax snapshot", info => info.complete);
  const folded = await vscode.commands.executeCommand("vscode.executeFoldingRangeProvider", recovering.uri);
  assert.ok(folded?.some(range => range.start === 4 && range.end >= 7), "conditional paths have native folding");
  const tail = recovering.getText().lastIndexOf("end;");
  await recoveryEditor.edit(builder => builder.delete(new vscode.Range(recovering.positionAt(tail), recovering.positionAt(tail + 4))));
  await currentSnapshot(service, recovering, () => service.modelInfo(recovering.uri), "unfinished typed block snapshot", info => !info.complete);
  const recovered = await vscode.commands.executeCommand("vscode.executeDocumentSymbolProvider", recovering.uri);
  assert.ok(flat(recovered).some(row => row.name.startsWith("conditional_forecast_paths")), "typing recovery keeps the written block target");
  assert.equal(document.getText(), source, "provider requests leave the source owner unchanged");
  assert.equal(editor.document.uri.toString(), document.uri.toString());
  evidence.checks.push("native parser-owned formatting and folding preserve accepted structure; removing a terminator keeps useful recovered Outline without a complete claim");
}

module.exports = { checkSyntax };
