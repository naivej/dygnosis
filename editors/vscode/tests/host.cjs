const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const vscode = require("vscode");
const { probeNativeMcp } = require("../scripts/native-mcp-host.cjs");

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
async function writeObservedInput(service, filename, text) {
  const identity = value => process.platform === "win32" ? value.fsPath.toLowerCase() : value.fsPath;
  const expected = identity(vscode.Uri.file(filename));
  let observed = false;
  const subscription = service.onDidInvalidate(event => {
    if (event.reason === "file" && event.uri && identity(vscode.Uri.parse(event.uri)) === expected) observed = true;
  });
  try {
    await fs.writeFile(filename, text);
    // Folder-only startup makes these writes faster than native watcher delivery.
    // Observe the new input before constructing a preview from it.
    await waitFor(() => observed, `native file observation for ${path.basename(filename)}`);
  } finally { subscription.dispose(); }
}
exports.run = async function run() {
  const resultFile = process.env.DYGNOSIS_HOST_RESULT;
  const evidence = { vscode: vscode.version, runId: process.env.DYGNOSIS_HOST_RUN_ID, checks: [] };
  try {
    assert.ok(process.env.DYGNOSIS_TEST_BINARY, "Set DYGNOSIS_TEST_BINARY to the matching built engine.");
    const extension = vscode.extensions.getExtension("dygnosis.dygnosis");
    assert.ok(extension);
    const service = await extension.activate();
    const projectFeature = extension.packageJSON.contributes.configuration.some(group => Object.hasOwn(group.properties, "dynare.projectDiagnostics"));
    if (projectFeature) {
      await waitFor(() => service.client && service.supportsModelInfo, "folder-only project LSP startup");
      let projectStatus;
      await waitFor(async () => { projectStatus = await service.execute("dynare/projectStatus"); return projectStatus?.complete; }, "folder-only discovery completes");
      assert.equal(projectStatus.enabled, true); assert.equal(projectStatus.roots.length, 0);
      evidence.checks.push("folder-only project activation and empty discovery without opening a model");
    } else {
      assert.equal(service.client, undefined, "startup registration must not eagerly launch LSP");
      evidence.checks.push("activate before model/LSP launch");
    }
    evidence.native_vscode_mcp = await probeNativeMcp(vscode, extension.id);
    if (!projectFeature) assert.equal(service.client, undefined, "native MCP must work before any model/LSP opens");
    evidence.checks.push("native MCP discovers every tool and invokes model info before a model opens");
    if (projectFeature) {
      const projectOnly = path.join(vscode.workspace.workspaceFolders[0].uri.fsPath, "project-only.mod");
      await writeObservedInput(service, projectOnly, "var z; model; z=project_unknown; end;\n");
      let projectStatus;
      await waitFor(async () => { projectStatus = await service.execute("dynare/projectStatus"); return projectStatus?.complete && projectStatus.roots.some(root => root.root_uri.endsWith("project-only.mod") && root.state === "checked" && root.errors > 0); }, "unopened model receives project diagnostics");
      const projectUri = vscode.Uri.file(projectOnly);
      await waitFor(() => vscode.languages.getDiagnostics(projectUri).some(item => item.severity === vscode.DiagnosticSeverity.Error), "unopened project Error reaches Problems");
      assert.ok(!vscode.workspace.textDocuments.some(item => item.uri.toString() === projectUri.toString()));
      await vscode.commands.executeCommand("dygnosis.cancelProject");
      await waitFor(async () => (await service.execute("dynare/projectStatus"))?.cancelled === true, "native Cancel stops this pass");
      await vscode.commands.executeCommand("dygnosis.recheckProject");
      await waitFor(async () => { const result = await service.execute("dynare/projectStatus"); return result?.complete && !result.cancelled; }, "native Recheck resumes discovery");
      await vscode.workspace.getConfiguration("dynare").update("projectDiagnostics", false, vscode.ConfigurationTarget.Workspace);
      await waitFor(async () => (await service.execute("dynare/projectStatus"))?.enabled === false, "project setting off reaches engine");
      await waitFor(() => vscode.languages.getDiagnostics(projectUri).length === 0, "off clears unopened project contribution");
      await vscode.workspace.getConfiguration("dynare").update("projectDiagnostics", true, vscode.ConfigurationTarget.Workspace);
      await waitFor(async () => { const result = await service.execute("dynare/projectStatus"); return result?.enabled && result.complete; }, "project setting on resumes checking");
      evidence.checks.push("unopened project Problems, native Cancel/Recheck, scoped off/on and contribution clearing");
    }
    const filename = path.join(vscode.workspace.workspaceFolders[0].uri.fsPath, "model.mod");
    await writeObservedInput(service, filename, "var y; parameters p; p=1/2; model; y=p; end;\n");
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
    const previewEditor = vscode.window.activeTextEditor;
    const previewResult = await service.execute("dynare/showEffectiveModel", [document.uri.toString()]);
    const previewRow = previewResult.navigation.find(row => row.kind === "equation");
    assert.ok(previewRow?.written_locations.length);
    const previewPoint = new vscode.Position(previewRow.effective_range.start.line, previewRow.effective_range.start.character + 1);
    previewEditor.selection = new vscode.Selection(previewPoint, previewPoint);
    await waitFor(async () => {
      if (vscode.window.activeTextEditor.document.uri.scheme === "dygnosis-effective") await vscode.commands.executeCommand("dygnosis.goToWrittenSource");
      return vscode.window.activeTextEditor.document.uri.toString() === document.uri.toString();
    }, "effective preview jumps to its verified written equation");
    assert.equal(vscode.window.activeTextEditor.selection.start.line, previewRow.written_locations[0].range.start.line);
    assert.equal(vscode.window.activeTextEditor.selection.start.character, previewRow.written_locations[0].range.start.character);
    evidence.checks.push("native preview source action uses the engine's exact written range");
    const previewRootPath = path.join(vscode.workspace.workspaceFolders[0].uri.fsPath, "preview-include.mod");
    const previewBodyPath = path.join(vscode.workspace.workspaceFolders[0].uri.fsPath, "preview-body.inc");
    await writeObservedInput(service, previewBodyPath, "y=3;\n");
    await writeObservedInput(service, previewRootPath, "var y;\nmodel;\n@#include \"preview-body.inc\"\nend;\n");
    const previewRoot = await vscode.workspace.openTextDocument(previewRootPath);
    await vscode.window.showTextDocument(previewRoot);
    await currentSnapshot(service, previewRoot, () => service.modelInfo(previewRoot.uri), "preview include root snapshot");
    await vscode.commands.executeCommand("dygnosis.showEffectiveModel");
    const includePreview = vscode.window.activeTextEditor;
    const includeExpansion = await service.execute("dynare/showEffectiveModel", [previewRoot.uri.toString()]);
    const includedRow = includeExpansion.navigation.find(row => row.written_locations.some(target => target.uri.endsWith("preview-body.inc")));
    assert.ok(includedRow); assert.equal(includedRow.written_locations[0].document_version, null);
    const includedPoint = new vscode.Position(includedRow.effective_range.start.line, includedRow.effective_range.start.character + 1);
    includePreview.selection = new vscode.Selection(includedPoint, includedPoint);
    await waitFor(async () => {
      if (vscode.window.activeTextEditor.document.uri.scheme === "dygnosis-effective") await vscode.commands.executeCommand("dygnosis.goToWrittenSource");
      return vscode.window.activeTextEditor.document.uri.fsPath.toLowerCase() === previewBodyPath.toLowerCase();
    }, "preview source action opens an unchanged unopened include");
    evidence.checks.push("native preview source action opens an unopened include and retains its explicit root");
    const includingPath = path.join(vscode.workspace.workspaceFolders[0].uri.fsPath, "including.mod");
    const fragmentPath = path.join(vscode.workspace.workspaceFolders[0].uri.fsPath, "fragment.mod");
    await writeObservedInput(service, fragmentPath, "y=1;\n");
    await writeObservedInput(service, includingPath, "var y;\nmodel;\n@#include \"fragment.mod\"\nend;\n");
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
