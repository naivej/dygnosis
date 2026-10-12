// Verify Dynare folding through the native VS Code provider and rendered editor.
const assert = require("node:assert/strict");
const { Buffer } = require("node:buffer");
const fs = require("node:fs/promises");
const path = require("node:path");
const vscode = require("vscode");
const { nativeWorkbench } = require("./native_webview.cjs");

const fixture = [
  "var", "y", "c", ";", "",
  "parameters", "betta", "rho", ";", "",
  "model;", "@#for country in [\"home\", \"foreign\"]", "@#if country == \"home\"",
  "y = c;", "y = c;", "@#elseif country == \"foreign\"", "c = y;", "c = y;",
  "@#else", "y = y;", "c = c;", "@#endif", "@#endfor", "end;", "",
].join("\n");

function normalize(ranges) {
  return (ranges ?? []).map(({ start, end, startLine, endLine, kind }) => ({
    start: start ?? startLine, end: end ?? endLine, ...(kind ? { kind } : {}),
  })).sort((left, right) => left.start - right.start || left.end - right.end);
}

async function checkFolding(service, workspaceRoot, evidence, { waitFor, writeObservedInput }) {
  const filename = path.join(workspaceRoot, "folding-acceptance.mod");
  await writeObservedInput(service, filename, fixture);
  const document = await vscode.workspace.openTextDocument(filename);
  await vscode.window.showTextDocument(document);
  let editor;
  await waitFor(() => service.client && service.client.initializeResult, "folding language client startup");

  const requested = service.lastInitializeParams.capabilities.textDocument.foldingRange;
  const offered = service.client.initializeResult.capabilities.foldingRangeProvider;
  assert.ok(requested, "the native language client must negotiate textDocument/foldingRange");
  assert.ok(offered, "the engine must advertise a folding range provider");
  const feature = service.client.getFeature("textDocument/foldingRange");
  assert.ok(feature, "the native language client must register its folding range feature");

  await waitFor(async () => {
    const result = await vscode.commands.executeCommand("vscode.executeFoldingRangeProvider", document.uri);
    return result?.length > 0;
  }, "native VS Code folding provider result");
  const ranges = normalize(await vscode.commands.executeCommand("vscode.executeFoldingRangeProvider", document.uri));
  evidence.folding = { fixture: path.basename(filename), requested, offered, feature: feature.constructor.name, ranges };

  const expected = [
    { start: 0, end: 3 }, { start: 5, end: 8 },
    { start: 10, end: 23 }, { start: 11, end: 22 },
    { start: 12, end: 21 }, { start: 13, end: 14 },
    { start: 15, end: 17 }, { start: 18, end: 20 },
  ];
  const actualBounds = ranges.map(({ start, end }) => ({ start, end }));
  assert.deepEqual(actualBounds, expected, "native provider must return exact unindented declaration, block, and branch fold boundaries");

  const realModel = "C:/Users/Shifu/Github/dynare/examples/macroprocessor/bkk_1992.mod";
  try {
    const source = await fs.readFile(realModel, "utf8");
    const modelDocument = await vscode.workspace.openTextDocument(vscode.Uri.file(realModel));
    await vscode.window.showTextDocument(modelDocument);
    let realRanges;
    await waitFor(async () => {
      realRanges = await vscode.commands.executeCommand("vscode.executeFoldingRangeProvider", modelDocument.uri);
      return realRanges?.length > 0;
    }, "native folding provider result for pinned bkk_1992.mod");
    const relevantLines = source.split(/\r?\n/).filter(line => /^\s*@#(?:for|if|ifdef|ifndef|elseif|else|endif|endfor)\b/.test(line)).length;
    evidence.folding.real_model = { path: realModel, lines: source.split(/\r?\n/).length, macro_directives: relevantLines, ranges: normalize(realRanges) };
    assert.ok(realRanges.some(range => range.start < range.end), "the real macro model must expose at least one foldable source construct");
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
    evidence.folding.real_model = { checked: false, reason: "Pinned bkk_1992.mod is not available at the configured path." };
  }

  editor = await vscode.window.showTextDocument(document);
  evidence.checks.push("native VS Code folding provider negotiates folding and returns exact written source ranges");
  await checkNativeCommands(editor, evidence, waitFor);
  return { document, editor, ranges };
}

async function checkNativeCommands(editor, evidence, waitFor) {
  assert.ok(process.env.DYGNOSIS_HOST_CDP_PORT, "folding acceptance requires the isolated native host CDP port");
  const document = editor.document;
  const configuration = vscode.workspace.getConfiguration("editor", document.uri);
  const previousStrategy = configuration.inspect("foldingStrategy")?.workspaceValue;
  const lineNumbers = async () => nativeWorkbench(async send => {
    const response = await send("Runtime.evaluate", {
      expression: 'JSON.stringify(Array.from(document.querySelectorAll(".monaco-editor.focused .margin-view-overlays .line-numbers")).map(line => Number(line.textContent.trim())).filter(Number.isFinite))',
      returnByValue: true,
    });
    return JSON.parse(response.result.value ?? "[]");
  });
  const saveScreen = async name => nativeWorkbench(async send => {
    const screenshot = await send("Page.captureScreenshot", { format: "png", captureBeyondViewport: false });
    const filename = path.join(path.dirname(process.env.DYGNOSIS_HOST_RESULT), `folding-${vscode.version}-${name}.png`);
    await fs.writeFile(filename, Buffer.from(screenshot.data, "base64"));
    return filename;
  });
  const assertLines = async (expectedVisible, expectedHidden, description) => {
    let observed;
    try {
      await waitFor(async () => {
        observed = await lineNumbers();
        const visible = new Set(observed.map(number => number - 1));
        return expectedVisible.every(line => visible.has(line)) && expectedHidden.every(line => !visible.has(line));
      }, description);
    } catch (error) {
      error.message += `; rendered source lines: ${(observed ?? []).map(number => number - 1).join(", ")}`;
      throw error;
    }
    const visible = new Set(observed.map(number => number - 1));
    for (const line of expectedVisible) assert.ok(visible.has(line), `${description}: source line ${line} is rendered`);
    for (const line of expectedHidden) assert.ok(!visible.has(line), `${description}: source line ${line} is folded`);
    return observed;
  };
  const revealTop = () => editor.revealRange(new vscode.Range(0, 0, 8, 0), vscode.TextEditorRevealType.AtTop);
  const cursorAt = line => { editor.selection = new vscode.Selection(line, 0, line, 0); };

  try {
    await configuration.update("foldingStrategy", "auto", vscode.ConfigurationTarget.Workspace);
    await revealTop();
    await assertLines([0, 1, 2, 3, 4], [], "auto strategy starts with declaration source visible");
    cursorAt(0);
    await vscode.commands.executeCommand("editor.fold");
    const foldedLines = await assertLines([0, 4], [1, 2, 3], "Fold hides the unindented var declaration body");
    const foldScreenshot = await saveScreen("auto-fold");

    cursorAt(0);
    await vscode.commands.executeCommand("editor.unfold");
    const unfoldedLines = await assertLines([0, 1, 2, 3, 4], [], "Unfold restores the declaration body");

    editor.revealRange(new vscode.Range(12, 0, 16, 0), vscode.TextEditorRevealType.AtTop);
    cursorAt(13);
    await vscode.commands.executeCommand("editor.fold");
    const firstBranchLines = await assertLines([13, 15], [14], "first branch fold keeps the @#elseif directive visible");
    cursorAt(13);
    await vscode.commands.executeCommand("editor.unfold");
    await assertLines([13, 14, 15], [], "Unfold restores the first macro branch");

    editor.revealRange(new vscode.Range(18, 0, 21, 0), vscode.TextEditorRevealType.AtTop);
    cursorAt(18);
    await vscode.commands.executeCommand("editor.fold");
    const elseBranchLines = await assertLines([18, 21], [19, 20], "@#else branch fold keeps @#endif visible");
    cursorAt(18);
    await vscode.commands.executeCommand("editor.unfold");
    await assertLines([18, 19, 20, 21], [], "Unfold restores the else branch");

    editor.revealRange(new vscode.Range(10, 0, 24, 0), vscode.TextEditorRevealType.AtTop);
    cursorAt(10);
    await vscode.commands.executeCommand("editor.fold");
    const modelLines = await assertLines([10, 24], [11, 22, 23], "model fold hides its full unindented macro body through end;");
    cursorAt(10);
    await vscode.commands.executeCommand("editor.unfold");
    await assertLines([10, 11, 22, 23, 24], [], "Unfold restores the full model source");

    await revealTop();
    await vscode.commands.executeCommand("editor.foldAll");
    const allFoldedLines = await assertLines([0, 4], [1, 2, 3], "Fold All folds the declaration body with auto strategy");
    await vscode.commands.executeCommand("editor.unfoldAll");
    const allUnfoldedLines = await assertLines([0, 1, 2, 3, 4], [], "Unfold All restores the declaration body");

    await configuration.update("foldingStrategy", "indentation", vscode.ConfigurationTarget.Workspace);
    await revealTop();
    await vscode.commands.executeCommand("editor.foldAll");
    const indentationLines = await assertLines([0, 1, 2, 3, 4], [], "indentation strategy does not fold unindented declaration lines");
    await vscode.commands.executeCommand("editor.unfoldAll");

    evidence.folding.native_editor = {
      strategy: "auto",
      fold_visible_line_numbers: foldedLines,
      unfold_visible_line_numbers: unfoldedLines,
      first_branch_visible_line_numbers: firstBranchLines,
      else_branch_visible_line_numbers: elseBranchLines,
      model_visible_line_numbers: modelLines,
      fold_all_visible_line_numbers: allFoldedLines,
      unfold_all_visible_line_numbers: allUnfoldedLines,
      indentation_visible_line_numbers: indentationLines,
      screenshot: foldScreenshot,
    };
    evidence.checks.push("native Fold/Unfold render declarations, macro branches and model blocks; Fold All/Unfold All and indentation strategy preserve expected source visibility");
  } finally {
    await configuration.update("foldingStrategy", previousStrategy, vscode.ConfigurationTarget.Workspace);
  }
}

module.exports = { checkFolding };
