// Verify complete F2 Rename and atomic stale refusal in native VS Code application.
const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const { clearTimeout } = require("node:timers");
const vscode = require("vscode");
const { nativeWorkbench } = require("./native_webview.cjs");

async function nativeValue(expression) {
  return nativeWorkbench(async send => {
    const response = await send("Runtime.evaluate", { expression, returnByValue: true });
    assert.equal(response.exceptionDetails, undefined, "native Rename inspection succeeds");
    return response.result?.value;
  });
}

const renameWidget = 'Array.from(document.querySelectorAll(".quick-input-widget")).find(widget => widget.getClientRects().length > 0 && getComputedStyle(widget).visibility !== "hidden" && getComputedStyle(widget).display !== "none" && widget.textContent.includes("Dygnosis: Rename symbol"))';
const renameInput = `(${renameWidget})?.querySelector('input[type="text"]')`;
const visibleRename = `Boolean(${renameWidget})`;

async function settledCommand(result, waitFor, description, dismissNotice = false) {
  let settled = false, value, failure;
  Promise.resolve(result).then(answer => { value = answer; settled = true; }, error => { failure = error; settled = true; });
  await waitFor(async () => {
    if (settled) return true;
    if (dismissNotice) await vscode.commands.executeCommand("notifications.clearAll");
    return settled;
  }, description);
  if (failure) throw failure;
  return value;
}

async function enterRename(document, offset, newName, waitFor, command = false) {
  await waitFor(async () => !(await nativeValue(visibleRename)), "the previous native Rename input is closed");
  const editor = await vscode.window.showTextDocument(document);
  const cursor = document.positionAt(offset);
  editor.selection = new vscode.Selection(cursor, cursor);
  editor.revealRange(new vscode.Range(cursor, cursor));
  await vscode.commands.executeCommand("workbench.action.focusActiveEditorGroup");
  let result;
  if (command) {
    result = vscode.commands.executeCommand("dygnosis.renameSymbol");
    // Handle a rejected command while the bounded input wait is still active.
    Promise.resolve(result).catch(() => {});
  }
  else await nativeWorkbench(async send => {
      await send("Input.dispatchKeyEvent", { type: "keyDown", key: "F2", code: "F2", windowsVirtualKeyCode: 113, nativeVirtualKeyCode: 113 });
      await send("Input.dispatchKeyEvent", { type: "keyUp", key: "F2", code: "F2", windowsVirtualKeyCode: 113, nativeVirtualKeyCode: 113 });
    });
  await waitFor(async () => (await nativeValue(`(() => { const input = ${renameInput}; return Boolean(input && !input.disabled); })()`)),
    `F2 opens an enabled Rename input for ${document.uri.toString()}`);
  let focus;
  await waitFor(async () => nativeWorkbench(async send => {
    const response = await send("Runtime.evaluate", {
      expression: `(() => { const input = ${renameInput}; if (!input || input.disabled) return false; input.focus(); input.select(); return document.activeElement === input; })()`,
      returnByValue: true,
    });
    assert.equal(response.exceptionDetails, undefined);
    focus = response.result?.value;
    return focus === true;
  }), "the native Rename input owns keyboard focus before typing");
  assert.equal(focus, true);
  await nativeWorkbench(async send => {
    await send("Input.insertText", { text: newName });
  });
  await waitFor(async () => (await nativeValue(`(${renameInput})?.value`)) === newName,
    `native Rename input receives ${newName}`);
  await vscode.commands.executeCommand("workbench.action.acceptSelectedQuickOpenItem");
  await waitFor(async () => !(await nativeValue(visibleRename)), "acceptance closes the native Rename input");
  // Wrapping keeps the pending command available to the debugger test, which
  // must inspect the application boundary before the command resolves.
  return { result };
}

async function refusedRename(document, offset, newName, waitFor) {
  const { result } = await enterRename(document, offset, newName, waitFor, true);
  // Refused edits show an information notice. Dismiss it in this isolated
  // host because the command promise can wait for that notice to close.
  assert.equal(await settledCommand(result, waitFor, `Rename refuses collision with ${newName}`, true), false, `Rename must refuse ${newName}`);
}

async function refusedPrepare(document, offset, waitFor, description) {
  await waitFor(async () => !(await nativeValue(visibleRename)), "the previous native Rename input is closed");
  const editor = await vscode.window.showTextDocument(document), cursor = document.positionAt(offset);
  editor.selection = new vscode.Selection(cursor, cursor);
  const before = document.getText();
  const result = vscode.commands.executeCommand("dygnosis.renameSymbol");
  assert.equal(await settledCommand(result, waitFor, `${description} refuses before name input`), false, description);
  assert.equal(await nativeValue(visibleRename), false, `${description} must not open a name input`);
  assert.equal(document.getText(), before, `${description} must preserve the selected source`);
}

async function checkTagRename(open, ready, workspaceRoot, evidence, waitFor, writeObservedInput, service) {
  const source = "var y(long_name='Output y'); model; [name='y', endogenous='y', mcp='y  >  0'] y=1; end;\n";
  const document = await open("rename-semantic-tags.mod", source); await ready(document);
  await enterRename(document, source.indexOf("y("), "output", waitFor);
  const renamed = "var output(long_name='Output y'); model; [name='y', endogenous='output', mcp='output  >  0'] output=1; end;\n";
  await waitFor(() => document.getText() === renamed, "native F2 Rename changes literal semantic tag references and preserves display labels/quotes/spacing");
  await ready(document);
  const insideTag = renamed.indexOf("endogenous='output'") + "endogenous='".length;
  await enterRename(document, insideTag, "result", waitFor);
  const fromTag = "var result(long_name='Output y'); model; [name='y', endogenous='result', mcp='result  >  0'] result=1; end;\n";
  await waitFor(() => document.getText() === fromTag, "native F2 from an endogenous tag reference changes only its bound symbol sites");
  evidence.checks.push("native F2 from a declaration or semantic tag changes endogenous/mcp references and preserves exact quotes, spacing, name and long_name labels");

  const boundSource = "@#define bound = 0\nvar y; model; [name='y', mcp='y > @{bound}'] y=1; end;\n";
  const bound = await open("rename-tag-generated-bound.mod", boundSource); await ready(bound);
  await enterRename(bound, boundSource.indexOf("var y") + "var ".length, "output", waitFor);
  const boundRenamed = "@#define bound = 0\nvar output; model; [name='y', mcp='output > @{bound}'] output=1; end;\n";
  await waitFor(() => bound.getText() === boundRenamed, "native F2 renames a literal mcp identifier and preserves its generated numeric bound, quotes and label");
  evidence.checks.push("native F2 renames the literal mcp identifier while preserving the written bound interpolation, quote delimiters and name label");

  const generatedSource = "@#define target = \"y\"\nvar y; model; [name='y', endogenous='@{target}', mcp='y > 0'] y=1; end;\n";
  const generated = await open("rename-generated-tag.mod", generatedSource); await ready(generated);
  await refusedPrepare(generated, generatedSource.indexOf("var y") + "var ".length, waitFor, "generated semantic tag identifier");
  evidence.checks.push("native Rename refuses a generated semantic identifier before the name input and changes no source");

  const sharedName = "rename-ambiguous-tag.inc", sharedSource = "[name='y', endogenous='y', mcp='y > 0'] y=1;\n";
  await writeObservedInput(service, path.join(workspaceRoot, sharedName), sharedSource);
  const owners = [];
  for (const name of ["a", "b"]) {
    const text = `var y; model;\n@#include "${sharedName}"\nend;\n`;
    const owner = await open(`rename-ambiguous-tag-${name}.mod`, text); await ready(owner);
    owners.push({ document: owner, text });
  }
  await refusedPrepare(owners[0].document, owners[0].text.indexOf("y;"), waitFor, "shared semantic tags with different declaration owners");
  for (const owner of owners) assert.equal(owner.document.getText(), owner.text, "an ambiguous tag binding must preserve every model owner");
  assert.equal(await fs.readFile(path.join(workspaceRoot, sharedName), "utf8"), sharedSource, "an ambiguous tag binding must preserve the saved include");
  evidence.checks.push("native Rename refuses incompatible shared semantic tag owners before input and preserves both roots and their saved include");
}

async function staleNativeApplication(child, mutation, invoke, evidence, waitFor) {
  return nativeWorkbench(async send => {
    let script, breakpoint, paused;
    let acceptPause, rejectPause;
    const pause = new Promise((resolve, reject) => { acceptPause = resolve; rejectPause = reject; });
    // An early timeout must remain handled while the input UI is still open.
    pause.catch(() => {});
    let timer;
    const unlisten = send.onEvent(event => {
      if (event.method === "Debugger.scriptParsed" && /\/workbench\.desktop\.main\.js$/.test(event.params.url)) script = event.params;
      if (event.method === "Debugger.paused") { paused = event.params; acceptPause(paused); }
    });
    try {
      await send("Debugger.enable");
      assert.ok(script, "The supported native host must expose its workbench script");
      const { scriptSource } = await send("Debugger.getScriptSource", { scriptId: script.scriptId });
      // Test-only debugger injection, after the public applyEdit adapter has
      // serialized versions and before native Bulk Text Edits validates them.
      // No native method is patched or copied into the product.
      const marker = vscode.version === "1.102.0" ? "q(){for(const e of this.c.values())for(const t of e)if(typeof t.versionId" : "_validateBeforePrepare(){";
      const at = scriptSource.indexOf(marker);
      assert.ok(at >= 0, `VS Code ${vscode.version} native Bulk Text Edits validation point must be found`);
      const boundary = at + marker.indexOf("{") + 1, prefix = scriptSource.slice(0, boundary);
      const location = { scriptId: script.scriptId, lineNumber: prefix.split("\n").length - 1, columnNumber: boundary - prefix.lastIndexOf("\n") - 1 };
      breakpoint = (await send("Debugger.setBreakpoint", { location })).breakpointId;
      timer = setTimeout(() => rejectPause(new Error("Timed out before native Rename version validation")), 15000);
      // Input acceptance can wait for the workbench thread that this test
      // pauses. Handle the pause before awaiting that command's UI reply.
      const invoked = invoke();
      invoked.catch(rejectPause);
      const stopped = await pause;
      const expression = `(() => {
        const service = Object.values(this).find(value => value && typeof value.getModels === "function" && typeof value.getModel === "function");
        if (!service) throw new Error("Native bulk edit model service was not found");
        const model = service.getModels().find(value => value.uri.toString() === ${JSON.stringify(child.uri.toString())});
        if (!model) throw new Error("The included target must be loaded in the native client");
        const edits = Object.values(this).filter(value => value && typeof value.values === "function")
          .flatMap(value => Array.from(value.values())).flat().filter(value => value && value.textEdit && value.resource);
        const rows = edits.map(value => ({ uri: value.resource.toString(), version: value.versionId }));
        if (new Set(rows.map(value => value.uri)).size < 2 || rows.some(value => !Number.isInteger(value.version)))
          throw new Error("Native Rename DTO must contain versions for every target");
        const before = model.getVersionId();
        if (!rows.some(value => value.uri === model.uri.toString() && value.version === before))
          throw new Error("Native included-target DTO version must match before the mutation");
        model.applyEdits([{ range: { startLineNumber: 1, startColumn: 1, endLineNumber: 1, endColumn: 1 }, text: ${JSON.stringify(mutation)} }]);
        return { rows, target: model.uri.toString(), before, after: model.getVersionId() };
      })()`;
      const inspected = await send("Debugger.evaluateOnCallFrame", { callFrameId: stopped.callFrames[0].callFrameId, expression, returnByValue: true });
      assert.equal(inspected.exceptionDetails, undefined, "native stale injection reaches the real versioned Rename DTO and TextModel");
      const captured = inspected.result?.value;
      assert.ok(captured.after > captured.before, "the native included target version must change after serialization");
      (evidence.rename.stale_applications ??= []).push({ native_versions: captured.rows, changed_target: captured.target, version_before: captured.before, version_after: captured.after, validation_function: stopped.callFrames[0].functionName });
      await send("Debugger.removeBreakpoint", { breakpointId: breakpoint }); breakpoint = undefined;
      await send("Debugger.resume"); paused = undefined;
      const { result } = await invoked;
      assert.equal(await settledCommand(result, waitFor, "the native stale Rename command settles"), false, "native applyEdit must refuse the complete stale Rename operation");
    } finally {
      clearTimeout(timer); unlisten();
      // Always resume and disable the debugger, including an assertion failure.
      try { if (breakpoint) await send("Debugger.removeBreakpoint", { breakpointId: breakpoint }); }
      finally {
        try { if (paused) await send("Debugger.resume"); }
        finally { await send("Debugger.disable"); }
      }
    }
  });
}

async function checkRename(service, workspaceRoot, evidence, helpers) {
  const { waitFor, currentSnapshot, writeObservedInput } = helpers;
  assert.ok(process.env.DYGNOSIS_HOST_CDP_PORT, "Rename acceptance requires the isolated native host CDP port");
  await waitFor(() => service.client && service.supportsModelInfo, "Rename language client startup");
  const open = async (name, text) => {
    const filename = path.join(workspaceRoot, name);
    if (/\.(mod|dyn|inc)$/i.test(name)) await writeObservedInput(service, filename, text);
    else await fs.writeFile(filename, text);
    return vscode.workspace.openTextDocument(filename);
  };
  const ready = async document => {
    await vscode.window.showTextDocument(document);
    if (document.uri.scheme === "untitled" && !/\.(mod|dyn)$/i.test(document.uri.path)) {
      // Model Info requires a .mod/.dyn root. Bare untitled inputs are a
      // Rename contract, so wait for their real Prepare Rename response.
      await waitFor(async () => service.client.sendRequest("textDocument/prepareRename", {
        textDocument: { uri: document.uri.toString() }, position: document.positionAt(document.getText().indexOf("p;")),
      }), `Prepare Rename synchronizes ${document.uri.toString()}`);
      return;
    }
    await currentSnapshot(service, document, () => service.modelInfo(document.uri), `Rename snapshot for ${document.uri.toString()}`);
  };
  const plain = "var y; parameters p; p=.9; model; y=p; end;\n";
  for (const name of ["rename-control.mod", "rename-control.dyn"]) {
    const document = await open(name, plain);
    await ready(document);
    await enterRename(document, plain.indexOf("p;"), "rho", waitFor);
    await waitFor(() => document.getText() === plain.replace(/\bp\b/g, "rho"), `F2 applies complete ${path.extname(name)} Rename`);
    evidence.checks.push(`native F2 Rename applies every declaration and use in ${path.extname(name)}`);
  }
  for (const name of [undefined, "rename-untitled.dyn"]) {
    let document = name
      ? await vscode.workspace.openTextDocument(vscode.Uri.parse(`untitled:${name}`))
      : await vscode.workspace.openTextDocument({ language: "dynare", content: plain });
    if (name) {
      document = await vscode.languages.setTextDocumentLanguage(document, "dynare");
      const insert = new vscode.WorkspaceEdit(); insert.insert(document.uri, new vscode.Position(0, 0), plain);
      assert.equal(await vscode.workspace.applyEdit(insert), true);
    }
    await ready(document);
    // Associated untitled files can adopt the native default CRLF on insert.
    // Rename must preserve the actual input's line endings and every other byte.
    const before = document.getText();
    await enterRename(document, before.indexOf("p;"), "rho", waitFor);
    try { await waitFor(() => document.getText() === before.replace(/\bp\b/g, "rho"), "F2 applies complete untitled Rename"); }
    catch (error) {
      error.message += `; uri=${document.uri.toString()} before=${JSON.stringify(before)} after=${JSON.stringify(document.getText())}`;
      throw error;
    }
    evidence.checks.push(`native F2 Rename applies supported untitled input ${name ?? "without suffix"}`);
  }

  const localSource = "var y; parameters p; model; #helper=p; y=helper; end;\n";
  const local = await open("rename-local.mod", localSource); await ready(local);
  await refusedRename(local, localSource.indexOf("helper"), "p", waitFor);
  await enterRename(local, localSource.indexOf("helper"), "result", waitFor);
  await waitFor(() => local.getText() === localSource.replaceAll("helper", "result"), "native local Rename applies only its bound definition and use");
  evidence.checks.push("native F2 preserves model-local Rename and its ordinary-name collision guard");

  const collision = await open("rename-collision.mod", "var y; parameters p q; model; #helper=p; y=p; end;\n");
  await ready(collision);
  const unchanged = collision.getText();
  for (const name of ["q", "helper"]) await refusedRename(collision, unchanged.indexOf("p q"), name, waitFor);
  assert.equal(collision.getText(), unchanged, "refused ordinary and local collisions change no source");
  evidence.checks.push("native Rename command refuses ordinary and model-local replacement collisions");
  await checkTagRename(open, ready, workspaceRoot, evidence, waitFor, writeObservedInput, service);

  const childName = "rename-equations.txt", childSource = "y=p;\n";
  const guardName = "rename-input.txt", guardSource = "// known input without a renamed use\n";
  const childPath = path.join(workspaceRoot, childName), childUri = vscode.Uri.file(childPath);
  // These fresh arbitrary-name files have no owner/dependency watcher yet.
  // The root's first analysis must acquire their saved source from disk.
  await fs.writeFile(childPath, childSource);
  await fs.writeFile(path.join(workspaceRoot, guardName), guardSource);
  const rootSource = `var y; parameters p; p=.9; model;\n@#include "${childName}"\n@#include "${guardName}"\nend;\n`;
  const root = await open("rename-includes.mod", rootSource);
  const independent = await open("rename-independent.mod", plain); await ready(independent);
  const companion = await open("rename-includes_steadystate.m", "p = 1;\n");
  await ready(root);
  assert.ok(!vscode.workspace.textDocuments.some(document => document.uri.toString() === childUri.toString()), "the arbitrary-name include starts saved and closed");
  let guardInitial;
  const opened = vscode.workspace.onDidOpenTextDocument(document => {
    if (document.uri.fsPath === path.join(workspaceRoot, guardName)) guardInitial = { version: document.version, dirty: document.isDirty };
  });
  try {
    await enterRename(root, rootSource.indexOf("p;"), "rho", waitFor);
    await waitFor(() => root.getText() === rootSource.replaceAll("p;", "rho;").replace("p=.9", "rho=.9")
      && vscode.workspace.textDocuments.find(document => document.uri.toString() === childUri.toString())?.getText() === "y=rho;\n",
    "native F2 applies the root and saved owned include");
  }
  finally { opened.dispose(); }
  const child = vscode.workspace.textDocuments.find(document => document.uri.toString() === childUri.toString());
  assert.ok(child, "Rename loads the saved arbitrary-name target before native application");
  const guard = vscode.workspace.textDocuments.find(document => document.uri.fsPath === path.join(workspaceRoot, guardName));
  assert.ok(guard, "Rename loads the unedited dependency before native application");
  assert.ok(guardInitial, "the native document-open event records the unedited input before Rename applies");
  assert.equal(guard.getText(), guardSource, "the unedited input's no-op version guard preserves its source");
  assert.equal(guard.version, guardInitial.version, "the unedited input's no-op version guard does not advance its document version");
  assert.equal(guard.isDirty, guardInitial.dirty, "the unedited input's no-op version guard does not make it dirty");
  assert.equal(independent.getText(), plain, "the independent root with the same name remains unchanged");
  assert.equal(companion.getText(), "p = 1;\n", "MATLAB companion text remains outside Rename");
  evidence.checks.push("native F2 applies a complete root/saved arbitrary-name include edit; independent root and MATLAB companion stay unchanged");

  // Inspect the actual native versioned DTO; changing one target must prevent
  // every edit, including the unchanged root's declaration and assignment.
  await ready(root);
  const rootBefore = root.getText(), childBefore = child.getText();
  evidence.rename = { input: "native F2 and Dygnosis input box", application: "public workspace.applyEdit", saved_include_loaded: true };
  await staleNativeApplication(child, "// edit after capture\n", () => enterRename(root, root.getText().indexOf("rho;"), "theta", waitFor, true), evidence, waitFor);
  const childChanged = `// edit after capture\n${childBefore}`;
  await waitFor(() => child.getText() === childChanged, "native stale target mutation reaches the document overlay");
  assert.equal(root.getText(), rootBefore, "stale native application changes no root declaration or assignment");
  assert.equal(child.getText(), childChanged, "stale native application preserves the changed include without applying any Rename use");
  evidence.checks.push("changing an included target after native Rename serialization refuses every edit atomically");

  assert.equal(await child.save(), true, "save the test mutation before capturing the next current model");
  await ready(root);
  const guardBefore = guard.getText(), rootBeforeGuard = root.getText(), childBeforeGuard = child.getText();
  await staleNativeApplication(guard, "// changed unedited input\n", () => enterRename(root, root.getText().indexOf("rho;"), "theta", waitFor, true), evidence, waitFor);
  await waitFor(() => guard.getText() === `// changed unedited input\n${guardBefore}`, "native unedited-input mutation reaches its document overlay");
  assert.equal(root.getText(), rootBeforeGuard, "a changed unedited dependency refuses the root's Rename edits");
  assert.equal(child.getText(), childBeforeGuard, "a changed unedited dependency refuses the child use edit");
  evidence.checks.push("unedited-input guards preserve source/version/dirty state on success and atomically refuse Rename after a dependency changes");

  const sharedName = "rename-shared.inc", sharedSource = "parameters shared_p;\n";
  await writeObservedInput(service, path.join(workspaceRoot, sharedName), sharedSource);
  const sharedRoots = [];
  for (const name of ["a", "b"]) {
    const source = `var ${name};\n@#include "${sharedName}"\nmodel; ${name}=shared_p; end;\n`;
    const document = await open(`rename-shared-${name}.mod`, source); await ready(document);
    sharedRoots.push({ document, source });
  }
  const first = sharedRoots[0];
  await enterRename(first.document, first.source.indexOf("shared_p"), "shared_rho", waitFor);
  await waitFor(() => sharedRoots.every(({ document, source }) => document.getText() === source.replace("shared_p", "shared_rho")), "native shared-binding Rename applies both known roots");
  const shared = vscode.workspace.textDocuments.find(document => document.uri.fsPath === path.join(workspaceRoot, sharedName));
  assert.equal(shared?.getText(), "parameters shared_rho;\n", "compatible shared Rename updates its one declaration");
  evidence.checks.push("native F2 Rename preserves a compatible shared include declaration and edits both known model owners");
}

module.exports = { checkRename };
