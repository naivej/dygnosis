const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const { execFile } = require("node:child_process");
const { promisify } = require("node:util");
const { createHash } = require("node:crypto");
const { Buffer } = require("node:buffer");
const { clearTimeout } = require("node:timers");
const vscode = require("vscode");
const { probeNativeMcp } = require("../scripts/native-mcp-host.cjs");
const { effectivePreviewArguments } = require("../out/preview");
const { createGitSources } = require("../out/git_source_host");
const { captureComparison } = require("../out/snapshot_compare");
const { historicalUri } = require("../out/history_sources");
const { resourceName, resourceQuery, changesViewType } = require("../out/changes_resource");

async function historyArtifacts() {
  const extensionRoot = path.resolve(__dirname, "..");
  const compiled = [];
  async function collect(directory) {
    for (const entry of await fs.readdir(directory, { withFileTypes: true })) {
      const filename = path.join(directory, entry.name);
      if (entry.isDirectory()) await collect(filename);
      else if (entry.name.endsWith(".js")) compiled.push(filename);
    }
  }
  await collect(path.join(extensionRoot, "out"));
  compiled.push(...["package.json", "media/diff_view.js", "media/diff_view.css"].map(filename => path.join(extensionRoot, filename)));
  const extensionHash = createHash("sha256");
  for (const filename of compiled.sort()) {
    extensionHash.update(path.relative(extensionRoot, filename).split(path.sep).join("/"));
    extensionHash.update("\0"); extensionHash.update(await fs.readFile(filename)); extensionHash.update("\0");
  }
  return { engine_sha256: createHash("sha256").update(await fs.readFile(process.env.DYGNOSIS_TEST_BINARY)).digest("hex"),
    compiled_extension_sha256: extensionHash.digest("hex"), compiled_files: compiled.length };
}

async function hostDevtools(callback) {
  const port = process.env.DYGNOSIS_HOST_CDP_PORT;
  if (!port) return undefined;
  const targets = await (await globalThis.fetch(`http://127.0.0.1:${port}/json/list`)).json();
  const target = targets.find(item => item.type === "page" && item.url.includes("workbench"));
  assert.ok(target?.webSocketDebuggerUrl, "The isolated Code host must expose its workbench CDP target");
  const socket = new globalThis.WebSocket(target.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => { socket.addEventListener("open", resolve, { once: true }); socket.addEventListener("error", reject, { once: true }); });
  let next = 0;
  const pending = new Map();
  socket.addEventListener("message", event => {
    const value = JSON.parse(event.data), entry = pending.get(value.id);
    if (!entry) return;
    pending.delete(value.id); clearTimeout(entry.timer);
    if (value.error) entry.reject(new Error(JSON.stringify(value.error))); else entry.resolve(value.result);
  });
  const send = (method, params = {}) => new Promise((resolve, reject) => {
    const id = ++next, timer = setTimeout(() => { pending.delete(id); reject(new Error(`CDP timed out: ${method}`)); }, 10000);
    pending.set(id, { resolve, reject, timer }); socket.send(JSON.stringify({ id, method, params }));
  });
  try { return await callback(send); } finally { socket.close(); }
}

async function historyScreenshot(name, evidence) {
  if (!process.env.DYGNOSIS_HOST_CDP_PORT) { evidence.history_screenshot = "CDP was not enabled for this host run"; return; }
  const filename = path.join(path.dirname(process.env.DYGNOSIS_HOST_RESULT), `history-${vscode.version}-${name}.png`);
  await hostDevtools(async send => {
    const screenshot = await send("Page.captureScreenshot", { format: "png", captureBeyondViewport: false });
    await fs.writeFile(filename, Buffer.from(screenshot.data, "base64"));
  });
  (evidence.history_screenshots ??= []).push(filename);
}

async function historyThemeChecks(evidence) {
  if (!process.env.DYGNOSIS_HOST_CDP_PORT || vscode.version !== "1.141.0") return;
  const appearance = vscode.workspace.getConfiguration("workbench"), previous = appearance.get("colorTheme");
  evidence.history_theme_checks = [];
  try {
    for (const [theme, bodyClass, name] of [["Default Light Modern", "vs", "changes-light"], ["Default High Contrast", "hc-black", "changes-high-contrast"]]) {
      await appearance.update("colorTheme", theme, vscode.ConfigurationTarget.Workspace);
      await waitFor(async () => hostDevtools(async send => {
        const response = await send("Runtime.evaluate", { expression: `document.body.classList.contains(${JSON.stringify(bodyClass)}) || document.querySelector(".monaco-workbench")?.classList.contains(${JSON.stringify(bodyClass)})`, returnByValue: true });
        return response.result.value;
      }), `native workbench applies ${theme}`);
      await new Promise(resolve => setTimeout(resolve, 800));
      await historyScreenshot(name, evidence); evidence.history_theme_checks.push(theme);
    }
  } finally {
    await appearance.update("colorTheme", previous, vscode.ConfigurationTarget.Workspace);
    await new Promise(resolve => setTimeout(resolve, 800));
  }
}

async function historyPaletteCheck(evidence, description) {
  if (!process.env.DYGNOSIS_HOST_CDP_PORT) return;
  await vscode.commands.executeCommand("workbench.action.quickOpen", ">Dygnosis: Open changes");
  try {
    await waitFor(async () => hostDevtools(async send => {
      const response = await send("Runtime.evaluate", { expression: 'Array.from(document.querySelectorAll(".quick-input-list .monaco-list-row")).some(row => row.getClientRects().length > 0 && row.textContent.includes("Dygnosis: Open changes") && row.getAttribute("aria-disabled") !== "true" && !row.classList.contains("disabled"))', returnByValue: true });
      return response.result.value;
    }), description);
    evidence.history_palette_enabled = true;
  } finally { await vscode.commands.executeCommand("workbench.action.closeQuickOpen"); }
}

async function historyKeyboardCheck(evidence) {
  if (!process.env.DYGNOSIS_HOST_CDP_PORT || vscode.version !== "1.141.0") return;
  let context;
  await hostDevtools(async send => {
    const tree = await send("Page.getFrameTree"), frames = [];
    const collect = node => { for (const child of node.childFrames ?? []) { frames.push(child.frame); collect(child); } };
    collect(tree.frameTree);
    for (const frame of frames.reverse()) {
      try {
        const world = await send("Page.createIsolatedWorld", { frameId: frame.id, worldName: "dygnosis-history-keyboard-evidence" });
        const response = await send("Runtime.evaluate", { contextId: world.executionContextId, expression: 'Boolean(document.getElementById("changeComparison"))', returnByValue: true });
        if (response.result.value) { context = world.executionContextId; break; }
      } catch { /* Cross-process webview frames can require their own CDP target. */ }
    }
    if (!context) { evidence.history_keyboard = { checked: false, barrier: "The workbench CDP target did not expose the rendered webview control frame." }; return; }
    await send("Runtime.evaluate", { contextId: context, expression: 'document.getElementById("changeComparison").focus()' });
    await send("Input.dispatchKeyEvent", { type: "keyDown", key: "Tab", code: "Tab", windowsVirtualKeyCode: 9, nativeVirtualKeyCode: 9 });
    await send("Input.dispatchKeyEvent", { type: "keyUp", key: "Tab", code: "Tab", windowsVirtualKeyCode: 9, nativeVirtualKeyCode: 9 });
    const response = await send("Runtime.evaluate", { contextId: context, expression: 'JSON.stringify({id:document.activeElement.id,outline:getComputedStyle(document.activeElement).outlineStyle,width:getComputedStyle(document.activeElement).outlineWidth})', returnByValue: true });
    const focused = JSON.parse(response.result.value);
    assert.equal(focused.id, "swap", "native Tab moves between the visible Changes controls");
    assert.notEqual(focused.outline, "none"); assert.notEqual(focused.width, "0px");
    evidence.history_keyboard = { checked: true, from: "Change comparison…", to: "Swap sides", focus_outline: focused };
  });
  if (context) await historyScreenshot("changes-keyboard", evidence);
}

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
function diagnosticCode(diagnostic) {
  return typeof diagnostic.code === "object" ? diagnostic.code.value : diagnostic.code;
}
async function metadataCompletion(service, filename, source, prefix, placeholder, replacement) {
  await writeObservedInput(service, filename, source);
  const document = await vscode.workspace.openTextDocument(filename);
  const editor = await vscode.window.showTextDocument(document);
  await currentSnapshot(service, document, () => service.modelInfo(document.uri), "metadata completion snapshot");
  const cursor = document.positionAt(source.indexOf(prefix) + prefix.length);
  editor.selection = new vscode.Selection(cursor, cursor);
  let completion;
  await waitFor(async () => {
    const result = await vscode.commands.executeCommand("vscode.executeCompletionItemProvider", document.uri, cursor);
    completion = result?.items.find(item => item.insertText instanceof vscode.SnippetString &&
      item.insertText.value.includes(`\${1:${placeholder}}`));
    return !!completion;
  }, `metadata completion with editable ${placeholder}`);
  assert.equal(document.getText(), source, "requesting completion must not write metadata");
  await vscode.commands.executeCommand("editor.action.triggerSuggest");
  // Suggestion calculation finishes after Trigger Suggest returns. Acceptance
  // does nothing until the native widget has the real provider result.
  await waitFor(async () => {
    await vscode.commands.executeCommand("acceptSelectedSuggestion");
    return document.getText() !== source;
  }, `native acceptance of ${placeholder} completion`);
  const completed = source.slice(0, document.offsetAt(cursor)) + placeholder + source.slice(document.offsetAt(cursor));
  assert.equal(document.getText(), completed, "completion must preserve the existing quotes and metadata key");
  assert.equal(document.getText(editor.selection), placeholder, "the whole metadata value must be selected");
  assert.deepEqual(editor.selection.start, cursor);
  assert.deepEqual(editor.selection.end, cursor.translate(0, placeholder.length));
  await vscode.commands.executeCommand("default:type", { text: replacement });
  assert.equal(document.getText(), completed.replace(`${prefix}${placeholder}`, `${prefix}${replacement}`),
    "typing must replace the whole selected metadata value");
  await vscode.commands.executeCommand("leaveSnippet");
}
async function metadataAction(document, code, title) {
  let action;
  await waitFor(async () => {
    const diagnostic = vscode.languages.getDiagnostics(document.uri).find(item => diagnosticCode(item) === code);
    if (!diagnostic) return false;
    const actions = await vscode.commands.executeCommand("vscode.executeCodeActionProvider", document.uri,
      diagnostic.range, vscode.CodeActionKind.QuickFix.value, 100);
    action = actions?.find(item => item.title === title);
    return !!action;
  }, `real ${title} code action`);
  assert.equal(action.edit, undefined, `${title} keeps its edit behind the revision guard`);
  assert.equal(action.command?.command, "dygnosis.applyDiagnosticEdit", `${title} must use the guarded native action`);
  return action;
}
async function applyUndoReapplyMetadata(service, document, code, title, accept) {
  const original = document.getText();
  const first = await metadataAction(document, code, title);
  assert.equal(document.getText(), original, "requesting a code action must not write metadata");
  assert.equal(await vscode.commands.executeCommand(first.command.command, ...first.command.arguments), true);
  const changed = document.getText();
  assert.notEqual(changed, original); accept(changed);
  await currentSnapshot(service, document, () => service.modelInfo(document.uri), `${title} applied snapshot`);
  await waitFor(() => !vscode.languages.getDiagnostics(document.uri).some(item => diagnosticCode(item) === code), `${title} clears its note`);
  const wholeDocument = new vscode.Range(document.positionAt(0), document.positionAt(changed.length));
  const repeated = await vscode.commands.executeCommand("vscode.executeCodeActionProvider", document.uri,
    wholeDocument, vscode.CodeActionKind.QuickFix.value, 100);
  assert.ok(!repeated?.some(item => item.title.startsWith(title)), `${title} must not repeat after metadata exists`);
  await vscode.commands.executeCommand("undo");
  await waitFor(() => document.getText() === original, `native undo of ${title}`);
  await currentSnapshot(service, document, () => service.modelInfo(document.uri), `${title} undo snapshot`);
  const reapplied = await metadataAction(document, code, title);
  assert.equal(await vscode.commands.executeCommand(reapplied.command.command, ...reapplied.command.arguments), true);
  assert.equal(document.getText(), changed, `${title} must produce the same edit after undo`);
  await currentSnapshot(service, document, () => service.modelInfo(document.uri), `${title} reapplied snapshot`);
}
async function checkNativeMetadata(service, workspaceRoot, evidence) {
  await metadataCompletion(service, path.join(workspaceRoot, "metadata-equation-completion.mod"),
    "var y(long_name='y');\nmodel;\n[name=''] y=1;\nend;\n", "[name='", "eq1", "host_equation");
  await metadataCompletion(service, path.join(workspaceRoot, "metadata-long-name-completion.mod"),
    "var y (long_name='');\nmodel;\n[name='given'] y=1;\nend;\n", "long_name='", "y", "Host output");
  evidence.checks.push("native equation-tag and long-name completion acceptance selects and replaces the whole value");
  const filename = path.join(workspaceRoot, "metadata-actions.mod");
  await writeObservedInput(service, filename, "var y z;\nmodel;\ny=z;\nz=1;\nend;\n");
  const document = await vscode.workspace.openTextDocument(filename);
  await vscode.window.showTextDocument(document);
  await currentSnapshot(service, document, () => service.modelInfo(document.uri), "metadata actions snapshot");
  await applyUndoReapplyMetadata(service, document, "I208", "Add equation tags", text => {
    assert.equal((text.match(/\[name='eq[12]'\]/g) || []).length, 2);
    assert.match(text, /\[name='eq1'\]\s+y=z;/);
    assert.match(text, /\[name='eq2'\]\s+z=1;/);
  });
  await applyUndoReapplyMetadata(service, document, "I209", "Add long names", text => {
    assert.match(text, /var y\s*\(long_name='y'\) z\s*\(long_name='z'\);/);
    assert.equal((text.match(/\[name='eq[12]'\]/g) || []).length, 2);
  });
  evidence.checks.push("real equation-tag and long-name actions apply, undo, reapply, and do not repeat");
}

async function checkMacroPreview(service, workspaceRoot, evidence) {
  const names = ["home_1", "home_2", "foreign_1", "foreign_2"];
  for (const name of names) await writeObservedInput(service, path.join(workspaceRoot, `${name}.inc`), `${name}=0;\n@#echo "${name}"\n`);
  const filename = path.join(workspaceRoot, "macro-preview.mod");
  const source = '@#define countries=["home","foreign"]\n' +
    '@#define names=[c+"_"+(string)i for (c,i) in countries*(1:2)]\n' +
    '@#if 1 in 1:3\n@#echo "range"\n@#endif\n' +
    '@#for n in names\nvar @{n};\n@#endfor\nmodel;\n' +
    '@#for n in names\n@#include n+".inc"\n@#endfor\nend;\n';
  await writeObservedInput(service, filename, source);
  const document = await vscode.workspace.openTextDocument(filename);
  await vscode.window.showTextDocument(document);
  const info = await currentSnapshot(service, document, () => service.modelInfo(document.uri), "macro preview counts", value => value.n_equations === 4);
  assert.equal(info.n_endogenous, 4);
  await vscode.commands.executeCommand("dygnosis.showEffectiveModel");
  const preview = vscode.window.activeTextEditor;
  const expanded = await service.execute("dynare/showEffectiveModel", effectivePreviewArguments(service, document.uri));
  assert.equal(expanded.complete, true);
  assert.equal(expanded.navigation.filter(row => row.kind === "equation").length, 4);
  assert.deepEqual(expanded.macro_messages.map(message => message.message), ["range", ...names]);
  assert.equal(preview.document.getText(), expanded.effective_text);
  for (const name of names) assert.ok(expanded.effective_text.includes(`${name}=0;`));
  assert.ok(expanded.macro_ranges.length > 0);
  const row = expanded.navigation.find(item => item.kind === "equation" && item.written_locations.some(location => location.uri.endsWith("home_1.inc")));
  assert.ok(row);
  const point = new vscode.Position(row.effective_range.start.line, row.effective_range.start.character + 1);
  preview.selection = new vscode.Selection(point, point);
  await vscode.commands.executeCommand("dygnosis.goToWrittenSource");
  assert.ok(vscode.window.activeTextEditor.document.uri.fsPath.endsWith("home_1.inc"));
  const edit = new vscode.WorkspaceEdit();
  edit.insert(document.uri, document.positionAt(document.getText().length), '@#echo missing_macro\n@#echo "late"\n');
  assert.equal(await vscode.workspace.applyEdit(edit), true);
  const failedInfo = await currentSnapshot(service, document, () => service.modelInfo(document.uri), "failing macro status", value => value.status === "incomplete");
  assert.equal(Object.hasOwn(failedInfo, "n_equations"), false);
  await vscode.window.showTextDocument(preview.document);
  await vscode.commands.executeCommand("dygnosis.refreshEffectiveModel");
  const failed = await service.execute("dynare/showEffectiveModel", effectivePreviewArguments(service, document.uri));
  assert.equal(failed.complete, false);
  assert.equal(failed.status, "incomplete");
  assert.deepEqual(failed.navigation, []);
  assert.deepEqual(failed.macro_ranges, []);
  assert.deepEqual(failed.macro_messages.map(message => message.message), ["range", ...names]);
  await waitFor(() => vscode.window.activeTextEditor.document.getText().startsWith("// INCOMPLETE EXPANSION"), "Refresh publishes the incomplete virtual document");
  assert.ok(vscode.window.activeTextEditor.document.getText().endsWith(failed.effective_text));
  await waitFor(() => vscode.languages.getDiagnostics(document.uri).some(diagnostic => diagnosticCode(diagnostic) === "E063"), "failing macro Problems");
  evidence.checks.push("range, Cartesian and comprehension names, expression includes, exact preview text, macro ranges, written jump, edit/Refresh, debug order and fatal status");
}
async function checkParseFollowups(service, workspaceRoot, evidence) {
  const cases = [
    ["empty-steady-state", "var y; model; y=0; end;\nsteady_state_model; end;\n", "E001",
      "syntax error, unexpected END"],
    ["constructor-denominator", "var y; varexo e unused; parameters p; model; y=e+0/(p^0-1); end;\n", "E278",
      "Division by zero when forming (0)/(0); denominator simplified to 0 (possibly after substituting a variable set to 0)."],
    ["decimal-denominator", "var y; varexo e; model; y=e+1/0.0; end;\n", null, null],
    ["verbatim-bare-end", "var y; varexo e; model; y=e; end;\nverbatim;\nend\nend;\nshocks; var e=1; end;\n", null, null],
  ];
  for (const [name, source, code, sentence] of cases) {
    const filename = path.join(workspaceRoot, `${name}.mod`);
    await writeObservedInput(service, filename, source);
    const document = await vscode.workspace.openTextDocument(filename);
    await vscode.window.showTextDocument(document);
    await currentSnapshot(service, document, () => service.modelInfo(document.uri), `${name} snapshot`);
    if (code) {
      await waitFor(() => vscode.languages.getDiagnostics(document.uri).some(row => diagnosticCode(row) === code), `${name} Problems`);
      const rows = vscode.languages.getDiagnostics(document.uri);
      const refusal = rows.find(row => diagnosticCode(row) === code);
      assert.equal(refusal.message, sentence);
      assert.equal(refusal.severity, vscode.DiagnosticSeverity.Error);
      assert.ok(rows.every(row => !["E021", "W022", "W042"].includes(diagnosticCode(row))), `${name} must stop shared Check reports`);
    } else {
      const info = await service.modelInfo(document.uri);
      assert.equal(info.n_equations, 1);
      assert.ok(vscode.languages.getDiagnostics(document.uri).every(row => row.severity !== vscode.DiagnosticSeverity.Error), `${name} must remain accepted`);
    }
  }
  evidence.checks.push("slice 22 empty steady-state, constructor refusal and phase, decimal quiet, verbatim bare end in native Problems");
}

async function checkShockPathConstructors(service, workspaceRoot, evidence) {
  const head = "var y; varexo e u; parameters p q; model; y=e+u+p+q; end;\n";
  const cases = [
    ["E278", "(q+p)/0", "(q+p)/0.0", "Division by zero when forming (q+p)/(0); denominator simplified to 0 (possibly after substituting a variable set to 0)."],
    ["E276", "log(0)", "log(p)", "log(0) not defined!"],
    ["E277", "log10(0)", "log10(p)", "log10(0) not defined!"],
    ["E405", "self.e(-1)", "0*self.e(-1)", "shock_paths: a lag of 1 is not allowed at period 1"],
    ["E420", "self.e", "0*self.e", "in the definition of 'e' in a 'shock_paths' block, the use of 'self.e' without a lag is not allowed, since it is a circular reference"],
  ];
  for (const [code, fire, quiet, sentence] of cases) {
    const filename = path.join(workspaceRoot, `path-${code}.mod`);
    const source = value => `${head}shock_paths; var e; periods 1; /*😀*/ values ${value}; end;\n`;
    await writeObservedInput(service, filename, source(fire));
    const document = await vscode.workspace.openTextDocument(filename);
    await vscode.window.showTextDocument(document);
    await currentSnapshot(service, document, () => service.modelInfo(document.uri), `${code} path snapshot`);
    await waitFor(() => vscode.languages.getDiagnostics(document.uri).some(row => diagnosticCode(row) === code), `${code} path Problems`);
    const rows = vscode.languages.getDiagnostics(document.uri).filter(row => diagnosticCode(row) === code);
    assert.equal(rows.length, 1);
    assert.equal(rows[0].message, sentence);
    assert.equal(rows[0].severity, vscode.DiagnosticSeverity.Error);
    assert.equal(document.getText(rows[0].range), fire);
    const edit = new vscode.WorkspaceEdit();
    edit.replace(document.uri, new vscode.Range(document.positionAt(0), document.positionAt(document.getText().length)), source(quiet));
    assert.equal(await vscode.workspace.applyEdit(edit), true);
    assert.equal(document.isDirty, true, "quiet control must use unsaved text");
    await currentSnapshot(service, document, () => service.modelInfo(document.uri), `${code} quiet path snapshot`);
    await waitFor(() => vscode.languages.getDiagnostics(document.uri).every(row => row.severity !== vscode.DiagnosticSeverity.Error), `${code} path clears on unsaved edit`);
  }
  evidence.checks.push("slice 23 path E276/E277/E278/E405/E420 sentences and Unicode ranges in native Problems; unsaved quiet controls clear Errors");
}

async function checkGitHistory(service, workspaceRoot, evidence) {
  const repositoryPath = path.join(workspaceRoot, "history-fixture"), rootPath = path.join(repositoryPath, "root.mod");
  const bodyPath = path.join(repositoryPath, "body.data"), rootUri = vscode.Uri.file(rootPath);
  await fs.mkdir(repositoryPath, { recursive: true });
  const rootText = "var y;\nmodel;\n@#include \"body.data\"\nend;\n";
  const bodyText = value => `/* 🧭 */ [name='output'] y=${value};\n`;
  const sources = await createGitSources(), gitExtension = vscode.extensions.getExtension("vscode.git");
  const api = gitExtension.exports.getAPI(1), runGit = promisify(execFile);
  const git = async (...args) => (await runGit(api.git.path, ["-C", repositoryPath, ...args], { windowsHide: true })).stdout.trim();
  await git("init", "-q");
  await git("config", "user.email", "host-test@example.invalid");
  await git("config", "user.name", "Dygnosis host test");
  await git("config", "core.autocrlf", "false");
  await fs.writeFile(rootPath, rootText); await fs.writeFile(bodyPath, bodyText(1));
  await git("add", "."); await git("commit", "-qm", "Initial model and include");
  const beforeCommit = await git("rev-parse", "HEAD");
  await fs.writeFile(bodyPath, bodyText(2)); await git("add", "body.data"); await git("commit", "-qm", "Change only the include");
  const afterCommit = await git("rev-parse", "HEAD");
  await fs.writeFile(bodyPath, bodyText(3));
  const repository = await sources.repositoryFor(rootUri), before = await sources.resolve(repository, beforeCommit), after = await sources.resolve(repository, afterCommit);
  assert.equal(before.hash, beforeCommit); assert.equal(after.hash, afterCommit);
  assert.equal((await sources.previous(repository)).revision.hash, afterCommit);
  assert.equal((await sources.previous(repository, afterCommit)).revision.hash, beforeCommit);
  const selector = commit => ({ kind: "git", repository_uri: repository.rootUri.toString(), commit, root_file: "root.mod", requested_ref: commit });
  const fixedResource = { schema_version: 1, before: selector(beforeCommit), after: selector(afterCommit), anchor: selector(afterCommit), context_uri: rootUri.toString() };
  const historical = {
    retain(_holder, input, texts) { return new Map(Object.keys(texts).map(file_key => [file_key, historicalUri({ repository_uri: input.repository_uri, commit: input.commit, file_key })])); },
    release() {},
  };
  const token = new vscode.CancellationTokenSource(), transport = [], execute = service.execute.bind(service);
  service.execute = async (name, ...args) => {
    const result = await execute(name, ...args);
    if (name === "dynare/compareModelSnapshots") transport.push(result);
    return result;
  };
  try {
    const fixed = await captureComparison(service, async () => sources, historical, fixedResource, token.token);
    const fixedWire = transport.findLast(result => result?.state === "result");
    assert.ok(transport.some(result => result?.state === "needs_sources"), "Rust must request the active include body");
    assert.deepEqual(Object.keys(fixed.texts.before).sort(), ["body.data", "root.mod"]);
    assert.equal(fixed.texts.before["body.data"], bodyText(1)); assert.equal(fixed.texts.after["body.data"], bodyText(2));
    assert.equal(fixed.snapshot.rows.length, 1); assert.equal(fixed.snapshot.rows[0].kind, "changed");
    assert.equal(fixed.snapshot.rows[0].navigation.before.written_locations[0].range.start.character, 9);
    assert.equal(fixedWire.inputs.before.commit, beforeCommit); assert.equal(fixedWire.inputs.after.commit, afterCommit);
    evidence.checks.push("installed Git API resolves HEAD and historical first parent; Rust requests include bodies; fixed commits ignore current saved include bytes and map UTF-16 locations");

    assert.equal(typeof api.toGitUri, "function", "The real Git API provides native source document URIs");
    const nativeGitUri = api.toGitUri(rootUri, afterCommit), nativeGitDocument = await vscode.workspace.openTextDocument(nativeGitUri);
    assert.equal(nativeGitDocument.getText(), rootText);
    const provenance = await sources.provenance(nativeGitDocument);
    assert.equal(provenance.commit.hash, afterCommit); assert.equal(provenance.file_key, "root.mod");
    evidence.checks.push("built-in Git document URI, provider text and fixed-commit provenance work in the installed host");

    const root = await vscode.workspace.openTextDocument(rootUri), body = await vscode.workspace.openTextDocument(bodyPath);
    assert.equal(body.languageId, "plaintext", "the arbitrary-extension include starts outside native analysis");
    await vscode.window.showTextDocument(root);
    await currentSnapshot(service, root, () => service.modelInfo(root.uri), "history Working root snapshot");
    const nativeGitBody = await vscode.workspace.openTextDocument(api.toGitUri(body.uri, afterCommit));
    assert.equal(nativeGitBody.languageId, "plaintext", "a native Git document keeps the arbitrary include extension");
    assert.ok(service.knownOwners(body.uri).some(owner => owner.toString() === rootUri.toString()));
    await vscode.commands.executeCommand("workbench.action.joinAllGroups");
    await vscode.window.showTextDocument(nativeGitBody, { preview: false });
    if (process.env.DYGNOSIS_HOST_CDP_PORT) {
      await waitFor(async () => hostDevtools(async send => {
        const response = await send("Runtime.evaluate", { expression: 'Array.from(document.querySelectorAll(".editor-actions .action-label")).some(action => action.getClientRects().length > 0 && ((action.getAttribute("title") || "") + (action.getAttribute("aria-label") || "")).includes("Open changes") && action.getAttribute("aria-disabled") !== "true" && !action.classList.contains("disabled"))', returnByValue: true });
        return response.result.value;
      }), "native title offers Open changes on a known plaintext Git include");
      await historyPaletteCheck(evidence, "native Command Palette offers Open changes on a known plaintext Git include");
      evidence.history_git_plaintext_context = { title_enabled: true, palette_enabled: true };
      evidence.checks.push("native title and Command Palette offer enabled Open changes for a known plaintext Git .data include");
    }
    const workingResource = { ...fixedResource, before: selector(afterCommit), after: { kind: "working", root_uri: rootUri.toString() }, anchor: { kind: "working", root_uri: rootUri.toString() } };
    await captureComparison(service, async () => sources, historical, workingResource, token.token);
    const savedWire = transport.findLast(result => result?.state === "result");
    const serverId = `${vscode.extensions.getExtension("CoconutWater.dygnosis").id.toLowerCase()}/Dygnosis`;
    let mcpTool;
    await waitFor(async () => {
      await vscode.commands.executeCommand("workbench.mcp.startServer", serverId);
      mcpTool = vscode.lm.tools.find(tool => tool.name.endsWith("_dynare_compare_models")); return !!mcpTool;
    }, "native repository comparison MCP tool discovery");
    const repositoryInput = { repository_path: repositoryPath, before: { kind: "git", root_file: "root.mod", ref: afterCommit }, after: { kind: "working", root_file: "root.mod" } };
    const invoke = async input => {
      const result = await vscode.lm.invokeTool(mcpTool.name, { input }, token.token);
      return JSON.parse(result.content.filter(item => item instanceof vscode.LanguageModelTextPart).map(item => item.value).join("\n"));
    };
    const mcpSaved = await invoke(repositoryInput);
    assert.deepEqual(mcpSaved.changed_equations, savedWire.diff.changed_equations);
    assert.equal(mcpSaved.inputs.after.source_policy, "saved_files");
    const mcpFixed = await invoke({ ...repositoryInput, before: { kind: "git", root_file: "root.mod", ref: beforeCommit }, after: { kind: "git", root_file: "root.mod", ref: afterCommit } });
    assert.deepEqual(mcpFixed.changed_equations, fixedWire.diff.changed_equations);
    const edit = new vscode.WorkspaceEdit(); edit.replace(body.uri, new vscode.Range(body.positionAt(0), body.positionAt(body.getText().length)), bodyText(4));
    assert.equal(await vscode.workspace.applyEdit(edit), true); assert.equal(body.isDirty, true);
    assert.equal(body.languageId, "plaintext", "the unsaved edit must precede the language change");
    assert.ok(service.knownOwners(body.uri).some(owner => owner.toString() === rootUri.toString()), "the root snapshot proves the include owner");
    const unrelated = await vscode.workspace.openTextDocument(path.join(workspaceRoot, "model.mod"));
    await vscode.window.showTextDocument(unrelated);
    await historyPaletteCheck(evidence, "native Command Palette offers Open changes with an unrelated editor active");
    const selected = [], selectOwner = service.selectOwner.bind(service);
    service.selectOwner = (document, owner) => {
      if (document.toString() === body.uri.toString()) selected.push({ root: owner.toString(), language: vscode.workspace.textDocuments.find(open => open.uri.toString() === document.toString())?.languageId });
      return selectOwner(document, owner);
    };
    const openingFromScm = vscode.commands.executeCommand("dygnosis.openChanges", { resourceUri: body.uri });
    try {
      await waitFor(() => selected.length && vscode.workspace.textDocuments.some(open => open.uri.toString() === body.uri.toString() && open.languageId === "dynare"), "explicit SCM include selects its proven owner before changing language");
      assert.deepEqual(selected[0], { root: rootUri.toString(), language: "plaintext" });
      assert.equal(vscode.window.activeTextEditor.document.uri.toString(), unrelated.uri.toString(), "the explicit SCM argument must not use the unrelated active editor");
      await new Promise(resolve => setTimeout(resolve, 800));
      await vscode.commands.executeCommand("workbench.action.acceptSelectedQuickOpenItem"); await openingFromScm;
    } finally { service.selectOwner = selectOwner; }
    const adoptedBody = vscode.workspace.textDocuments.find(open => open.uri.toString() === body.uri.toString() && !open.isClosed);
    assert.equal(adoptedBody.languageId, "dynare"); assert.equal(adoptedBody.getText(), bodyText(4)); assert.equal(adoptedBody.isDirty, true);
    await waitFor(async () => (await service.rootForDocument(adoptedBody))?.toString() === rootUri.toString(), "proven include owner survives language-change cache invalidation");
    let scmWire;
    await waitFor(() => {
      scmWire = transport.findLast(result => result?.state === "result");
      return scmWire?.inputs.after.kind === "working" && scmWire.inputs.after.root_uri === rootUri.toString() && scmWire.diff.changed_equations[0]?.text_new?.includes("4");
    }, "native SCM Open changes captures the unsaved arbitrary-extension include");
    assert.deepEqual((await invoke(repositoryInput)).changed_equations, mcpSaved.changed_equations);
    evidence.checks.push("explicit SCM URI on an unsaved plaintext .data include uses its proven owner with an unrelated editor active, selects owner before Dynare language change, retains context after cache invalidation, and keeps editor/MCP source policies distinct");
    await currentSnapshot(service, root, () => service.modelInfo(root.uri), "history unsaved include snapshot");
    const unsaved = await captureComparison(service, async () => sources, historical, workingResource, token.token);
    assert.ok(unsaved.snapshot.rows[0].after.includes("4"));
    assert.deepEqual((await invoke(repositoryInput)).changed_equations, mcpSaved.changed_equations, "independent MCP still reads saved include bytes");
    await vscode.commands.executeCommand("workbench.mcp.stopServer", serverId);
    evidence.checks.push("native VS Code MCP discovery and repository calls agree with editor snapshots for saved and fixed inputs; an unsaved include affects the editor while MCP retains saved bytes");

    const comparisonUri = resource => vscode.Uri.from({ scheme: "dygnosis-changes", path: `/${resourceName(resource)}`, query: resourceQuery(resource) });
    const fixedUri = comparisonUri(fixedResource), tabs = uri => vscode.window.tabGroups.all.flatMap(group => group.tabs).filter(tab => tab.input instanceof vscode.TabInputCustom && tab.input.viewType === changesViewType && tab.input.uri.toString() === uri.toString());
    await vscode.commands.executeCommand("workbench.action.joinAllGroups");
    if ((await vscode.commands.getCommands(true)).includes("workbench.action.closeAuxiliaryBar")) await vscode.commands.executeCommand("workbench.action.closeAuxiliaryBar");
    await vscode.commands.executeCommand("vscode.openWith", fixedUri, changesViewType, { viewColumn: vscode.ViewColumn.One, preview: false });
    await waitFor(() => tabs(fixedUri).length === 1 && transport.findLast(result => result?.state === "result")?.inputs.after.commit === afterCommit, "read-only Changes custom editor resolves its real capture");
    assert.equal(tabs(fixedUri)[0].isDirty, false);
    await new Promise(resolve => setTimeout(resolve, 1000));
    await historyScreenshot("changes", evidence);
    await historyThemeChecks(evidence);
    await historyKeyboardCheck(evidence);
    await vscode.commands.executeCommand("vscode.openWith", fixedUri, changesViewType, { viewColumn: vscode.ViewColumn.One, preview: false });
    assert.equal(tabs(fixedUri).length, 1, "the same ordered comparison reuses its tab");
    await vscode.commands.executeCommand("vscode.openWith", fixedUri, changesViewType, { viewColumn: vscode.ViewColumn.Two, preview: false });
    await waitFor(() => tabs(fixedUri).length === 2, "comparison opens in two editor groups");
    await vscode.window.tabGroups.close(tabs(fixedUri)[0]);
    assert.equal(tabs(fixedUri).length, 1, "closing one split retains the other");
    const swappedUri = comparisonUri({ ...fixedResource, before: fixedResource.after, after: fixedResource.before });
    await vscode.commands.executeCommand("vscode.openWith", swappedUri, changesViewType, { viewColumn: vscode.ViewColumn.One, preview: false });
    await waitFor(() => tabs(swappedUri).length === 1, "swapped ordered comparison owns another resource");
    evidence.checks.push("installed read-only custom editor: stable resource, same-pair reuse, separate ordered pair, two splits, close-one lifetime and no dirty state");

    const oldUri = historicalUri({ repository_uri: repository.rootUri.toString(), commit: beforeCommit, file_key: "body.data" });
    const newUri = historicalUri({ repository_uri: repository.rootUri.toString(), commit: afterCommit, file_key: "body.data" });
    const oldSource = await vscode.languages.setTextDocumentLanguage(await vscode.workspace.openTextDocument(oldUri), "dynare");
    const newSource = await vscode.languages.setTextDocumentLanguage(await vscode.workspace.openTextDocument(newUri), "dynare");
    assert.equal(oldSource.getText(), bodyText(1)); assert.equal(newSource.getText(), bodyText(2));
    assert.notEqual(oldSource.uri.toString(), newSource.uri.toString());
    await vscode.window.showTextDocument(newSource, { preview: false });
    await vscode.commands.executeCommand("default:type", { text: "EDIT_REFUSAL_TEST" });
    assert.equal(newSource.getText(), bodyText(2)); assert.equal(newSource.isDirty, false);
    await vscode.window.tabGroups.close([...tabs(fixedUri), ...tabs(swappedUri)]);
    assert.equal((await vscode.workspace.openTextDocument(oldUri)).getText(), bodyText(1));
    assert.equal((await vscode.workspace.openTextDocument(newUri)).getText(), bodyText(2));
    const rootBeforeUri = historicalUri({ repository_uri: repository.rootUri.toString(), commit: beforeCommit, file_key: "root.mod" });
    const rootAfterUri = historicalUri({ repository_uri: repository.rootUri.toString(), commit: afterCommit, file_key: "root.mod" });
    await vscode.commands.executeCommand("vscode.diff", rootBeforeUri, rootAfterUri, "Include-only history: unchanged written root", { preview: false });
    await waitFor(() => vscode.window.tabGroups.all.flatMap(group => group.tabs).some(tab => tab.input instanceof vscode.TabInputTextDiff && tab.input.original.toString() === rootBeforeUri.toString() && tab.input.modified.toString() === rootAfterUri.toString()), "native root text diff uses the fixed historical documents");
    assert.equal((await vscode.workspace.openTextDocument(rootBeforeUri)).getText(), (await vscode.workspace.openTextDocument(rootAfterUri)).getText());
    evidence.checks.push("two read-only historical versions at one path retain exact bytes after comparison closes; native root diff uses those identities and has no include-only root hunk");

    await vscode.window.showTextDocument(newSource, { preview: false });
    const opening = vscode.commands.executeCommand("dygnosis.openChanges");
    await new Promise(resolve => setTimeout(resolve, 800));
    if (process.env.DYGNOSIS_HOST_CDP_PORT) await hostDevtools(async send => {
      const response = await send("Runtime.evaluate", { expression: 'JSON.stringify(Array.from(document.querySelectorAll(".quick-input-list .label-name")).filter(row => row.getClientRects().length > 0).map(row => row.textContent))', returnByValue: true });
      assert.deepEqual(JSON.parse(response.result.value), ["With previous revision", "With revision…", "With branch or tag…", "With .mod file…"]);
    });
    await historyScreenshot("picker", evidence);
    await vscode.commands.executeCommand("workbench.action.acceptSelectedQuickOpenItem"); await opening;
    let opened;
    await waitFor(() => {
      const active = vscode.window.tabGroups.activeTabGroup.activeTab?.input;
      if (!(active instanceof vscode.TabInputCustom) || active.viewType !== changesViewType) return false;
      opened = JSON.parse(active.uri.query);
      return opened.before.commit === beforeCommit && opened.after.commit === afterCommit && opened.after.root_file === "root.mod";
    }, "Open changes from retained historical include anchors its historical owner and first parent");
    assert.equal(opened.anchor.commit, afterCommit);
    evidence.checks.push("native Open changes on a retained historical include uses its model owner at that commit and historical first parent");
    evidence.history = { beforeCommit, afterCommit, repository_root: repository.rootUri.toString(), git_api: 1, snapshot_schema: fixedWire.inputs.schema_version,
      navigation_schema: fixedWire.navigation.schema_version, source_requests: transport.filter(value => value?.state === "needs_sources").length,
      fixed_changes: fixed.snapshot.rows.length, native_mcp_agreement: true, historical_tabs_read_only: true };
    await vscode.window.tabGroups.close(vscode.window.tabGroups.all.flatMap(group => group.tabs).filter(tab => tab.input instanceof vscode.TabInputCustom && tab.input.viewType === changesViewType));
  } finally {
    service.execute = execute; token.cancel(); token.dispose(); sources.clear();
  }
}

exports.run = async function run() {
  const resultFile = process.env.DYGNOSIS_HOST_RESULT;
  const evidence = { vscode: vscode.version, runId: process.env.DYGNOSIS_HOST_RUN_ID, checks: [] };
  try {
    assert.ok(process.env.DYGNOSIS_TEST_BINARY, "Set DYGNOSIS_TEST_BINARY to the matching built engine.");
    evidence.artifacts_before = await historyArtifacts();
    const extension = vscode.extensions.getExtension("CoconutWater.dygnosis");
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
    await checkParseFollowups(service, vscode.workspace.workspaceFolders[0].uri.fsPath, evidence);
    await checkShockPathConstructors(service, vscode.workspace.workspaceFolders[0].uri.fsPath, evidence);
    await require("./helpers/color_host.cjs").checkBlockTintEditing(service, vscode.workspace.workspaceFolders[0].uri.fsPath, evidence, { waitFor, writeObservedInput });
    await require("./helpers/model_local_host.cjs").checkModelLocalEditing(service, vscode.workspace.workspaceFolders[0].uri.fsPath, evidence, { waitFor, currentSnapshot, writeObservedInput });
    await vscode.window.showTextDocument(document);
    let equationLens;
    await waitFor(async () => {
      const lenses = await vscode.commands.executeCommand("vscode.executeCodeLensProvider", document.uri);
      equationLens = lenses?.find(lens => lens.command?.command === "dygnosis.browseLensEquations");
      return equationLens?.command?.title === "Jump to equation";
    }, "equation CodeLens shows the jump title");
    const equationJump = vscode.commands.executeCommand(equationLens.command.command, ...equationLens.command.arguments);
    await new Promise(resolve => setTimeout(resolve, 500));
    await vscode.commands.executeCommand("workbench.action.acceptSelectedQuickOpenItem");
    await equationJump;
    assert.equal(vscode.window.activeTextEditor.document.uri.toString(), document.uri.toString());
    const writtenEquation = info.equations[0].location;
    assert.deepEqual(vscode.window.activeTextEditor.selection.start, new vscode.Position(writtenEquation.range.start.line, writtenEquation.range.start.character));
    evidence.checks.push("native equation CodeLens picker jumps to the exact written equation");
    await vscode.commands.executeCommand("dygnosis.showEffectiveModel");
    assert.equal(vscode.window.activeTextEditor.document.uri.scheme, "dygnosis-effective");
    assert.equal(vscode.window.activeTextEditor.document.languageId, "dynare");
    evidence.checks.push("read-only colored effective preview excluded from analysis");
    const previewEditor = vscode.window.activeTextEditor;
    const previewResult = await service.execute("dynare/showEffectiveModel", effectivePreviewArguments(service, document.uri));
    assert.equal(previewEditor.document.getText(), previewResult.effective_text);
    const previewRow = previewResult.navigation.find(row => row.kind === "equation");
    assert.ok(previewRow?.written_locations.length);
    const previewPoint = new vscode.Position(previewRow.effective_range.start.line, previewRow.effective_range.start.character + 1);
    previewEditor.selection = new vscode.Selection(previewPoint, previewPoint);
    await waitFor(async () => {
      if (vscode.window.activeTextEditor.document.uri.scheme === "dygnosis-effective") await vscode.commands.executeCommand("dygnosis.goToWrittenSource");
      return vscode.window.activeTextEditor.document.uri.toString() === document.uri.toString();
    }, "effective preview jumps to its verified written equation");
    assert.equal(vscode.window.activeTextEditor.selection.start.line, previewRow.written_locations[0].range.start.line);
    const writtenCharacter = previewRow.written_locations[0].range.start.character + (previewResult.source_navigation ? 1 : 0);
    assert.equal(vscode.window.activeTextEditor.selection.start.character, writtenCharacter, "source preview maps the clicked character; fallback uses the equation range");
    evidence.checks.push("native preview source action maps the exact clicked written character");
    const previewRootPath = path.join(vscode.workspace.workspaceFolders[0].uri.fsPath, "preview-include.mod");
    const previewBodyPath = path.join(vscode.workspace.workspaceFolders[0].uri.fsPath, "preview-body.inc");
    await writeObservedInput(service, previewBodyPath, "y=3;\n");
    await writeObservedInput(service, previewRootPath, "var y;\nmodel;\n@#include \"preview-body.inc\"\nend;\n");
    const previewRoot = await vscode.workspace.openTextDocument(previewRootPath);
    await vscode.window.showTextDocument(previewRoot);
    await currentSnapshot(service, previewRoot, () => service.modelInfo(previewRoot.uri), "preview include root snapshot");
    await vscode.commands.executeCommand("dygnosis.showEffectiveModel");
    const includePreview = vscode.window.activeTextEditor;
    const includeExpansion = await service.execute("dynare/showEffectiveModel", effectivePreviewArguments(service, previewRoot.uri));
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
    await checkNativeMetadata(service, vscode.workspace.workspaceFolders[0].uri.fsPath, evidence);
    await checkMacroPreview(service, vscode.workspace.workspaceFolders[0].uri.fsPath, evidence);
    await checkGitHistory(service, vscode.workspace.workspaceFolders[0].uri.fsPath, evidence);
    await service.restart(); assert.ok(service.client);
    await service.shutdown(); assert.equal(service.client, undefined);
    evidence.checks.push("restart/shutdown");
    evidence.artifacts_after = await historyArtifacts();
    assert.deepEqual(evidence.artifacts_after, evidence.artifacts_before, "the engine and compiled extension must remain unchanged throughout the host gate");
    evidence.passed = true;
  } catch (error) { evidence.passed = false; evidence.error = String(error.stack || error); throw error; }
  finally { if (resultFile) await fs.writeFile(resultFile, JSON.stringify(evidence, null, 2)); }
};
