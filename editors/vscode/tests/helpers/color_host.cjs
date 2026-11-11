const { compiledModule } = require("./extension_modules.cjs");
// Verify block-tint retention with native editor events and the matching engine.
const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const Module = require("node:module");
const vscode = require("vscode");
const { nativeWorkbench } = require("./native_webview.cjs");

async function checkBlockTintEditing(service, workspaceRoot, evidence, helpers) {
  const { waitFor, writeObservedInput } = helpers;
  const filename = path.join(workspaceRoot, "block-tint.mod");
  await writeObservedInput(service, filename, "var y;\nparameters p;\np=.9;\nmodel;\ny=p*y(-1);\nend;\n");
  const document = await vscode.workspace.openTextDocument(filename);
  const editor = await vscode.window.showTextDocument(document);
  const writes = [], styles = new Map(), editors = new WeakMap();
  const wrap = native => {
    if (!editors.has(native)) editors.set(native, {
      get document() { return native.document; },
      setDecorations(type, ranges) {
        if (native === editor) writes.push({ ...styles.get(type.key), ranges: [...ranges] });
        native.setDecorations(type, ranges);
      },
    });
    return editors.get(native);
  };
  // Observe writes at the native API boundary. The compiled production module
  // still owns scheduling and guards; the server and document events are real.
  const api = { Disposable: vscode.Disposable, Range: vscode.Range,
    ThemeColor: vscode.ThemeColor, DecorationRangeBehavior: vscode.DecorationRangeBehavior,
    workspace: vscode.workspace, window: {
    onDidChangeVisibleTextEditors: vscode.window.onDidChangeVisibleTextEditors,
    onDidChangeActiveTextEditor: vscode.window.onDidChangeActiveTextEditor,
    get visibleTextEditors() { return vscode.window.visibleTextEditors.map(wrap); },
    createTextEditorDecorationType(options) {
      const type = vscode.window.createTextEditorDecorationType(options);
      styles.set(type.key, { style: options.backgroundColor?.id, whole: options.isWholeLine });
      return type;
    },
  } };
  const colorPath = compiledModule("color");
  const loaded = new Module(colorPath, module);
  loaded.filename = colorPath; loaded.paths = Module._nodeModulePaths(path.dirname(colorPath));
  loaded.require = name => name === "vscode" ? api : Module.prototype.require.call(loaded, name);
  loaded._compile(await fs.readFile(colorPath, "utf8"), colorPath);
  let held = false, heldRequests = 0, release;
  const gate = new Promise(resolve => { release = resolve; });
  const registration = loaded.exports.registerColors({
    get client() { return service.client; },
    get currentInstance() { return service.currentInstance; },
    log: service.log, onDidChange: service.onDidChange,
    rootForDocument: (...args) => service.rootForDocument(...args),
    modelInfo: async (...args) => {
      if (held) { ++heldRequests; await gate; }
      return service.modelInfo(...args);
    },
  });
  const modelWrites = () => writes.filter(write => write.whole && write.style === "dynare.blockTint.modelBackground");
  const latest = () => modelWrites().at(-1);
  const config = vscode.workspace.getConfiguration("dynare", document.uri);
  const enabled = config.inspect("blockTint.enabled")?.workspaceValue;
  try {
    await waitFor(() => latest()?.ranges.length === 1, "native block tint appears");
    held = true;
    const before = modelWrites().length;
    assert.equal(await editor.edit(edit => edit.insert(new vscode.Position(4, 0), "\n")), true);
    assert.equal(document.isDirty, true);
    assert.equal(modelWrites().length, before, "unsaved edits keep native decorations");
    await waitFor(() => heldRequests > 0, "tint request after typing pause");
    assert.equal(modelWrites().length, before, "pending model requests keep native decorations");
    held = false; release();
    await waitFor(() => modelWrites().length > before && latest().ranges[0]?.end.line === 6,
      "current native tint follows the inserted line");
    assert.ok(modelWrites().slice(before).every(write => write.ranges.length === 1), "no blank tint update during typing or response wait");
    await config.update("blockTint.enabled", false, vscode.ConfigurationTarget.Workspace);
    assert.equal(latest().ranges.length, 0, "native disabling clears immediately");
    await config.update("blockTint.enabled", enabled, vscode.ConfigurationTarget.Workspace);
    await waitFor(() => latest()?.ranges.length === 1, "native tint returns after settings reset");
    const start = document.positionAt(document.getText().indexOf("model;"));
    const end = document.positionAt(document.getText().length);
    assert.equal(await editor.edit(edit => edit.delete(new vscode.Range(start, end))), true);
    await waitFor(() => latest()?.ranges.length === 0, "current result removes a deleted native block");
    evidence.checks.push("native block tint survives unsaved edits and a held response, follows inserted lines, clears on disable and current block removal");
    await checkTintAndFormatting(editor, config, writes, evidence, waitFor);
  } finally {
    held = false; release(); registration.dispose();
    await config.update("blockTint.enabled", enabled, vscode.ConfigurationTarget.Workspace);
  }
}

async function checkTintAndFormatting(editor, config, writes, evidence, waitFor) {
  const document = editor.document;
  const target = vscode.ConfigurationTarget.Workspace;
  const source = "var y; disp(1);\nverbatim;\ndisp(2);\nend;\nmodel;\ny=0;\nend;\n";
  const replace = text => editor.edit(edit => edit.replace(new vscode.Range(document.positionAt(0), document.positionAt(document.getText().length)), text));
  const workbench = vscode.workspace.getConfiguration("workbench");
  const window = vscode.workspace.getConfiguration("window");
  const user = vscode.ConfigurationTarget.Global;
  const previousTheme = workbench.inspect("colorTheme")?.globalValue;
  const previousColors = workbench.inspect("colorCustomizations")?.globalValue;
  const autoDetect = window.inspect("autoDetectColorScheme")?.globalValue;
  const last = (style, whole) => writes.findLast(write => write.style === style && write.whole === whole)?.ranges ?? [];
  let rendered = [];
  const visibleTint = (rgb, alpha) => nativeWorkbench(async send => {
    const result = await send("Runtime.evaluate", { returnByValue: true, expression:
      `[...new Set(Array.from(document.querySelectorAll('.monaco-editor .cdr'), node => getComputedStyle(node).backgroundColor))]` });
    rendered = result.result.value ?? [];
    return rendered.some(color => {
      const channels = color.match(/[\d.]+/g)?.map(Number);
      return channels?.slice(0, 3).join(",") === rgb && Math.abs((channels[3] ?? 1) * 255 - alpha) <= 1.5;
    });
  });
  try {
    assert.equal(await replace(source), true);
    await config.update("blockTint.enabled", false, target);
    await config.update("MATLAB/OctaveTint", "Standard", target);
    await waitFor(() => last("dynare.blockTint.modelBackground", false).length === 2, "native-only tint for a mixed line and verbatim body");
    assert.deepEqual(last("dynare.blockTint.modelBackground", false).map(range => document.getText(range).trim()), ["disp(1);", "disp(2);"]);
    assert.deepEqual(last("dynare.blockTint.modelBackground", true), []);
    await config.update("blockTint.enabled", true, target);
    await config.update("tintPalette", "amber", target);
    await waitFor(() => last("dynare.tintPalette.amber.standardBackground", true).length > 0 && last("dynare.tintPalette.amber.standardBackground", false).length === 2,
      "palette repaints Dynare blocks and native spans");
    if (process.env.DYGNOSIS_HOST_CDP_PORT) {
      await window.update("autoDetectColorScheme", false, user);
      for (const [theme, kind, rgb, standard, subtle] of [["Dark Modern", vscode.ColorThemeKind.Dark, "229,173,86", 26, 8], ["Light Modern", vscode.ColorThemeKind.Light, "148,98,0", 18, 6]]) {
        await workbench.update("colorTheme", theme, user);
        await waitFor(() => vscode.window.activeColorTheme.kind === kind, `${theme} applies`);
        await waitFor(async () => await visibleTint(rgb, standard) && await visibleTint(rgb, subtle), `${theme} renders both Amber tint strengths`)
          .catch(error => { error.message += `. Decoration colors ${JSON.stringify(rendered)}`; throw error; });
      }
      await workbench.update("colorTheme", "Default High Contrast", user);
      await waitFor(() => vscode.window.activeColorTheme.kind === vscode.ColorThemeKind.HighContrast, "high contrast theme applies");
      assert.equal(await visibleTint("148,98,0", 18), false);
      await workbench.update("colorTheme", "Dark Modern", user);
      await workbench.update("colorCustomizations", { "dynare.blockTint.modelBackground": "#13579B" }, user);
      await config.update("tintPalette", "theme", target);
      await waitFor(() => visibleTint("19,87,155", 255), "Theme Default renders a custom tint color");
    }
    editor.options = { insertSpaces: true, tabSize: 3, indentSize: 3 };
    await vscode.commands.executeCommand("editor.action.formatDocument");
    assert.equal(document.lineAt(5).text, "   y = 0;");
    assert.equal(document.lineAt(0).text, "var y; disp(1);");
    editor.options = { insertSpaces: false, tabSize: 4 };
    editor.selection = new vscode.Selection(5, 0, 6, 0);
    await vscode.commands.executeCommand("editor.action.formatSelection");
    assert.equal(document.lineAt(5).text, "\ty = 0;");
    evidence.checks.push("MATLAB/Octave tint shades exact mixed and verbatim ranges independently, palette colors update in editor, and both formatting actions use editor indentation");
  } finally {
    await workbench.update("colorTheme", previousTheme, user);
    await workbench.update("colorCustomizations", previousColors, user);
    await window.update("autoDetectColorScheme", autoDetect, user);
    await config.update("MATLAB/OctaveTint", undefined, target);
    await config.update("tintPalette", undefined, target);
  }
}

module.exports = { checkBlockTintEditing };
