const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const vscode = require("vscode");

async function waitFor(predicate, description) {
  for (let i = 0; i < 100; ++i) {
    if (await predicate()) return;
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  throw new Error(`Timed out: ${description}`);
}
async function currentSnapshot(service, document, query, description, accept = () => true) {
  let info;
  await waitFor(async () => {
    info = await query();
    return info && info.document_uri === document.uri.toString() && info.document_version === document.version &&
      info.client_instance === service.currentInstance && accept(info);
  }, description);
  return info;
}
exports.run = async function run() {
  const resultFile = process.env.DYGNOSIS_HOST_RESULT;
  const evidence = { vscode: vscode.version, runId: process.env.DYGNOSIS_HOST_RUN_ID, checks: [] };
  try {
    assert.ok(process.env.DYGNOSIS_TEST_BINARY, "Set DYGNOSIS_TEST_BINARY to the matching built engine.");
    await vscode.workspace.getConfiguration("dynare").update("serverPath", process.env.DYGNOSIS_TEST_BINARY, vscode.ConfigurationTarget.Global);
    const extension = vscode.extensions.getExtension("dygnosis.dygnosis");
    assert.ok(extension);
    const service = await extension.activate();
    assert.equal(service.client, undefined, "startup registration must not eagerly launch LSP");
    evidence.checks.push("activate before model/LSP launch");
    const filename = path.join(vscode.workspace.workspaceFolders[0].uri.fsPath, "model.mod");
    await fs.writeFile(filename, "var y; parameters p; p=1/2; model; y=p; end;\n");
    const document = await vscode.workspace.openTextDocument(filename);
    await vscode.window.showTextDocument(document);
    await waitFor(() => service.client && service.supportsModelInfo, "language client startup");
    const capabilities = service.lastInitializeParams.capabilities;
    for (const role of ["dynareEndogenous", "dynareExogenous", "dynareParameter", "dynareModelLocal"])
      assert.ok(capabilities.textDocument.semanticTokens.tokenTypes.includes(role));
    for (const modifier of ["forwardLooking", "predetermined"])
      assert.ok(capabilities.textDocument.semanticTokens.tokenModifiers.includes(modifier));
    assert.equal(capabilities.experimental.dygnosis.modelInfoChanged, true);
    const legend = service.client.initializeResult.capabilities.semanticTokensProvider.legend;
    for (const role of ["dynareEndogenous", "dynareExogenous", "dynareParameter", "dynareModelLocal"]) assert.ok(legend.tokenTypes.includes(role));
    for (const modifier of ["forwardLooking", "predetermined"]) assert.ok(legend.tokenModifiers.includes(modifier));
    evidence.checks.push("actual language-client initialize roles/modifiers and custom advertisement");
    const info = await currentSnapshot(service, document, () => service.modelInfo(document.uri), "current model snapshot after startup/settings/watch invalidations");
    assert.equal(info.n_endogenous, 1); assert.equal(info.n_equations, 1);
    const symbols = await vscode.commands.executeCommand("vscode.executeDocumentSymbolProvider", document.uri);
    assert.ok(symbols.length > 0);
    evidence.checks.push("real engine counts/native symbols");
    await vscode.commands.executeCommand("dygnosis.showEffectiveModel");
    assert.equal(vscode.window.activeTextEditor.document.uri.scheme, "dygnosis-effective");
    assert.equal(vscode.window.activeTextEditor.document.languageId, "dynare");
    evidence.checks.push("read-only colored effective preview excluded from analysis");
    const includingPath = path.join(vscode.workspace.workspaceFolders[0].uri.fsPath, "including.mod");
    const fragmentPath = path.join(vscode.workspace.workspaceFolders[0].uri.fsPath, "fragment.mod");
    await fs.writeFile(fragmentPath, "y=1;\n");
    await fs.writeFile(includingPath, "var y;\nmodel;\n@#include \"fragment.mod\"\nend;\n");
    const including = await vscode.workspace.openTextDocument(includingPath);
    const editor = await vscode.window.showTextDocument(including);
    await currentSnapshot(service, including, () => service.modelInfo(including.uri), "including model snapshot after file creation");
    let links;
    await waitFor(async () => {
      links = await vscode.commands.executeCommand("vscode.executeLinkProvider", including.uri);
      return links?.some(link => link.target?.scheme === "command");
    }, "native include link registration");
    const link = links.find(item => item.target?.scheme === "command");
    const linkPosition = link.range.start.translate(0, 10);
    editor.selection = new vscode.Selection(linkPosition, linkPosition);
    // The hidden editor creates and computes its link detector asynchronously.
    // Drive the native action only while its source editor remains current.
    await waitFor(async () => {
      if (vscode.window.activeTextEditor?.document.uri.fsPath === fragmentPath) return true;
      assert.equal(vscode.window.activeTextEditor, editor, "link source editor changed");
      await vscode.commands.executeCommand("editor.action.openLink");
      return vscode.window.activeTextEditor?.document.uri.fsPath === fragmentPath;
    }, "native link opens fragment through owner command");
    const fragment = vscode.window.activeTextEditor.document;
    await waitFor(async () => (await service.rootForDocument(fragment))?.toString() === including.uri.toString(), "fragment retains its including owner after document-open invalidation");
    const firstInfo = await currentSnapshot(service, fragment, () => service.modelForDocument(fragment), "current fragment model snapshot");
    assert.equal(firstInfo.root_uri, including.uri.toString());
    let outlineRegistrations = 0;
    const feature = service.client.getFeature("textDocument/documentSymbol");
    const register = feature.register.bind(feature);
    feature.register = value => { ++outlineRegistrations; return register(value); };
    const edit = new vscode.WorkspaceEdit();
    edit.replace(fragment.uri, new vscode.Range(0, 0, fragment.lineCount, 0), "y=2;\n");
    await vscode.workspace.applyEdit(edit);
    await waitFor(() => outlineRegistrations > 0, "include edit refreshes native Outline provider");
    const changedInfo = await currentSnapshot(service, fragment, () => service.modelForDocument(fragment), "current model snapshot after unsaved include edit", snapshot => snapshot.revision !== firstInfo.revision);
    assert.notEqual(changedInfo.revision, firstInfo.revision);
    feature.register = register;
    evidence.checks.push("native include link retains mod owner; unsaved include refreshes Outline/model revision");
    await service.restart(); assert.ok(service.client);
    await service.shutdown(); assert.equal(service.client, undefined);
    evidence.checks.push("restart/shutdown");
    evidence.passed = true;
  } catch (error) { evidence.passed = false; evidence.error = String(error); throw error; }
  finally { if (resultFile) await fs.writeFile(resultFile, JSON.stringify(evidence, null, 2)); }
};
