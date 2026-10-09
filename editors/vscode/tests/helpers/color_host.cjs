// Verify block-tint retention with native editor events and the matching engine.
const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const Module = require("node:module");
const vscode = require("vscode");

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
        if (native === editor) writes.push({ style: styles.get(type.key), ranges: [...ranges] });
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
      styles.set(type.key, options.backgroundColor.id); return type;
    },
  } };
  const colorPath = path.resolve(__dirname, "../../out/color.js");
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
  const modelWrites = () => writes.filter(write => write.style === "dynare.blockTint.modelBackground");
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
  } finally {
    held = false; release(); registration.dispose();
    await config.update("blockTint.enabled", enabled, vscode.ConfigurationTarget.Workspace);
  }
}

module.exports = { checkBlockTintEditing };
