const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const vscode = require("vscode");
const { binaryName, mcpBinaryName, model, probeMcp, sha256, writeJson } = require("./common.cjs");
const { probeNativeMcp } = require("./native-mcp-host.cjs");
const samePath = (left, right) => process.platform === "win32"
  ? path.resolve(left).toLowerCase() === path.resolve(right).toLowerCase()
  : path.resolve(left) === path.resolve(right);

async function waitFor(predicate, description) {
  for (let attempt = 0; attempt < 150; ++attempt) {
    const result = await predicate();
    if (result) return result;
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  throw new Error(`Timed out: ${description}`);
}
exports.run = async function run() {
  const source = JSON.parse(process.env.DYGNOSIS_PACKAGE_SOURCE);
  const evidence = { runId: process.env.DYGNOSIS_PACKAGE_RUN_ID, vscode: vscode.version, platform: process.platform, arch: process.arch, target: source.target, version: source.version, commit: source.commit, checks: [], passed: false };
  const checkpoint = async step => {
    evidence.current_step = step;
    const temporary = process.env.DYGNOSIS_PACKAGE_HOST_RESULT + ".tmp";
    await writeJson(temporary, evidence);
    await fs.rename(temporary, process.env.DYGNOSIS_PACKAGE_HOST_RESULT);
  };
  let service, checksPassed = false;
  try {
    await checkpoint("installed extension activation");
    const extension = vscode.extensions.getExtension(`${source.publisher}.dygnosis`);
    assert.ok(extension, "The installed Dygnosis extension is absent");
    assert.equal(extension.packageJSON.version, source.version);
    assert.ok(extension.extensionPath.startsWith(process.env.DYGNOSIS_PACKAGE_EXTENSIONS + path.sep), "Dygnosis must be loaded from the isolated installed extensions directory");
    assert.ok(!extension.extensionPath.includes("extension harness"), "A development extension cannot stand in for the installed VSIX");
    const config = vscode.workspace.getConfiguration("dynare").inspect("serverPath");
    assert.ok(!config?.globalValue && !config?.workspaceValue && !config?.workspaceFolderValue, "The first launch must use the packaged binary");
    const binary = path.join(extension.extensionPath, "bin", binaryName(source.target));
    assert.equal(await sha256(binary), source.binary_sha256);
    assert.equal(source.mcp_binary_name, mcpBinaryName(source.target));
    const mcpBinary = path.join(extension.extensionPath, "bin", source.mcp_binary_name);
    assert.equal((await fs.stat(mcpBinary)).size, source.mcp_binary_size);
    assert.equal(await sha256(mcpBinary), source.mcp_binary_sha256);
    const workspace = vscode.workspace.workspaceFolders[0].uri.fsPath;
    // Guard real executable verification and staging against the installed
    // artifact, independently of generated files left in the source checkout.
    await checkpoint("installed bundle verification and managed MCP staging");
    const { verifyPackagedBundle, ensureManagedBinary } = require(path.join(extension.extensionPath, extension.packageJSON.main));
    const frozen = await verifyPackagedBundle({ extensionPath: extension.extensionPath, extension });
    const managed = await ensureManagedBinary(path.join(workspace, "managed engine with spaces"), frozen);
    assert.deepEqual(await fs.readFile(managed.path), frozen.mcpBytes);
    assert.equal(managed.version, source.version);
    evidence.checks.push("installed LSP/MCP pair provenance, checksum and real version verification; frozen MCP bytes stage and run from a managed path");
    service = await extension.activate();
    await checkpoint("offline Help before opening a model");
    const helpBundle = JSON.parse(await fs.readFile(path.join(extension.extensionPath, "help/bundle.json"), "utf8"));
    assert.equal(helpBundle.version, source.version);
    await vscode.commands.executeCommand("dygnosis.openHelp", "get-started");
    await waitFor(() => vscode.window.tabGroups.all.some(group => group.tabs.some(tab => tab.label === "Dygnosis Help" && tab.input instanceof vscode.TabInputWebview)), "native Help panel before a model");
    await vscode.commands.executeCommand("dygnosis.openHelp", "check:E001");
    assert.equal(vscode.window.tabGroups.all.flatMap(group => group.tabs).filter(tab => tab.label === "Dygnosis Help").length, 1, "Help reuses one panel");
    evidence.checks.push("installed Help before a model, exact-code route and one panel");
    const launchLogs = [];
    const originalAppendLine = service.output.appendLine.bind(service.output);
    service.output.appendLine = message => { launchLogs.push(message); originalAppendLine(message); };
    const workspaceDiagnostics = extension.packageJSON.contributes.configuration.some(group => Object.hasOwn(group.properties, "dynare.workspaceDiagnostics"));
    if (!workspaceDiagnostics) assert.equal(service.client, undefined, "Before workspace diagnostics ships, activation before a model must leave LSP idle");
    else await service.restart(); // Later releases may already analyze the unopened workspace.
    evidence.checks.push("installed extension activation before opening a Dynare document");
    await checkpoint("installed bundled MCP initialize, discovery and model-info call");
    evidence.mcp = await probeMcp(mcpBinary, source.version);
    evidence.checks.push("installed bundled MCP initialize/tools/list/tool call before opening a model");
    await checkpoint("native VS Code MCP discovery and model-info invocation");
    evidence.native_vscode_mcp = await probeNativeMcp(vscode, extension.id);
    assert.ok(launchLogs.some(message => message.startsWith("MCP executable: ") && message.endsWith(` (dygnosis-mcp ${source.version})`) && samePath(message.slice(16, -` (dygnosis-mcp ${source.version})`.length), mcpBinary)), "The native provider must select the packaged MCP companion directly");
    if (!workspaceDiagnostics) assert.equal(service.client, undefined, "Native MCP must work before the first LSP launch");
    evidence.checks.push("native VS Code MCP discovery and direct companion invocation of all published tools before opening a model");
    await checkpoint("bundled LSP counts, symbols and Problems diagnostics");
    const file = path.join(workspace, "installed model.mod");
    await fs.writeFile(file, model);
    const document = await vscode.workspace.openTextDocument(file);
    await vscode.window.showTextDocument(document);
    await waitFor(() => service.client && service.supportsModelInfo, "bundled LSP initialization");
    const snapshot = await waitFor(async () => {
      const result = await service.modelInfo(document.uri);
      return result?.document_version === document.version && result.client_instance === service.currentInstance ? result : undefined;
    }, "current installed model snapshot");
    assert.equal(snapshot.n_endogenous, 1); assert.equal(snapshot.n_equations, 1);
    const symbols = await vscode.commands.executeCommand("vscode.executeDocumentSymbolProvider", document.uri);
    assert.ok(symbols.length > 0);
    assert.ok(launchLogs.some(message => message.startsWith("LSP executable: ") && samePath(message.slice(16), binary)), "The first LSP launch must select the installed bundled path");
    const edit = new vscode.WorkspaceEdit();
    edit.replace(document.uri, new vscode.Range(0, 0, document.lineCount, 0), "var y; model; y=package_unknown_name; end;\n");
    await vscode.workspace.applyEdit(edit);
    await waitFor(() => vscode.languages.getDiagnostics(document.uri).some(diagnostic => diagnostic.severity === vscode.DiagnosticSeverity.Error), "real bundled LSP error reaches VS Code Problems");
    evidence.checks.push("bundled language-client handshake, model counts, native symbols and Problems diagnostics");
    await checkpoint("explicit user override LSP and MCP launch");
    const overrideRoot = path.join(workspace, "explicit override with spaces");
    const override = path.join(overrideRoot, binaryName(source.target));
    const overrideMcp = path.join(overrideRoot, source.mcp_binary_name);
    await fs.mkdir(path.dirname(override), { recursive: true });
    await fs.copyFile(binary, override);
    await fs.copyFile(mcpBinary, overrideMcp);
    if (process.platform !== "win32") {
      for (const executable of [override, overrideMcp]) await fs.chmod(executable, 0o755);
    }
    await vscode.workspace.getConfiguration("dynare").update("serverPath", override, vscode.ConfigurationTarget.Global);
    await service.restart();
    await waitFor(() => service.client && service.supportsModelInfo, "explicit override LSP restart");
    assert.ok(launchLogs.some(message => message.startsWith("LSP executable: ") && samePath(message.slice(16), override)), "The restarted LSP must select the explicit override path");
    const overrideSymbols = await vscode.commands.executeCommand("vscode.executeDocumentSymbolProvider", document.uri);
    assert.ok(overrideSymbols.length > 0, "Override must provide actual language results after restart");
    evidence.overrideMcp = await probeMcp(overrideMcp, source.version);
    evidence.override_native_vscode_mcp = await probeNativeMcp(vscode, extension.id);
    assert.ok(launchLogs.some(message => message.startsWith("MCP executable: ") && message.endsWith(` (dygnosis ${source.version})`) && samePath(message.slice(16, -` (dygnosis ${source.version})`.length), override)), "The global override must route native MCP through the selected main executable");
    evidence.selected_lsp_paths = launchLogs.filter(message => message.startsWith("LSP executable:"));
    evidence.checks.push("explicit user override with spaces launches LSP and MCP");
    await service.shutdown();
    await vscode.commands.executeCommand("dygnosis.openHelp", "troubleshoot");
    assert.equal(service.client, undefined, "Help does not start the stopped engine");
    evidence.checks.push("installed Help remains available after engine shutdown");
    checksPassed = true;
  } catch (error) { evidence.passed = false; evidence.failed_step = evidence.current_step; evidence.error = String(error); throw error; }
  finally {
    await checkpoint("installed extension shutdown");
    await service?.shutdown();
    evidence.passed = checksPassed;
    await checkpoint("finished");
  }
};
