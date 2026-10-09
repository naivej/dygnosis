const assert = require("node:assert/strict");
const { execFileSync } = require("node:child_process");
const fs = require("node:fs/promises");
const path = require("node:path");
const vscode = require("vscode");

async function waitFor(predicate, description, attempts = 150) {
  for (let i = 0; i < attempts; ++i) {
    const result = await predicate();
    if (result) return result;
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  throw new Error(`Timed out: ${description}`);
}

function clickAt(x, y) {
  execFileSync("xdotool", ["mousemove", "--sync", String(x), String(y), "click", "1"], { stdio: "ignore" });
}

function shot(output, monitor = 1) {
  execFileSync("python3", ["-c", `
import mss
with mss.mss() as sct:
    sct.shot(mon=${monitor}, output=${JSON.stringify(output)})
`], { stdio: "inherit" });
}

async function settle(ms = 400) {
  await new Promise(resolve => setTimeout(resolve, ms));
}

async function writeModel(service, filename, text) {
  const identity = value => process.platform === "win32" ? value.fsPath.toLowerCase() : value.fsPath;
  const expected = identity(vscode.Uri.file(filename));
  let observed = false;
  const subscription = service.onDidInvalidate(event => {
    if (event.reason === "file" && event.uri && identity(vscode.Uri.parse(event.uri)) === expected) observed = true;
  });
  try {
    await fs.writeFile(filename, text);
    await waitFor(() => observed, `native file observation for ${path.basename(filename)}`);
  } finally {
    subscription.dispose();
  }
}

async function openModel(service, workspace, text, name = "model.mod") {
  const filename = path.join(workspace, name);
  await writeModel(service, filename, text);
  const document = await vscode.workspace.openTextDocument(filename);
  await vscode.window.showTextDocument(document, { preview: false, viewColumn: vscode.ViewColumn.One });
  await waitFor(() => service.client && service.supportsModelInfo, "language client startup");
  await waitFor(async () => {
    const info = await service.modelInfo(document.uri);
    return info?.document_version === document.version && info.client_instance === service.currentInstance;
  }, "current model snapshot");
  return document;
}

async function runCommand(command, ...args) {
  try {
    return await vscode.commands.executeCommand(command, ...args);
  } catch (error) {
    return { failed: command, error: String(error) };
  }
}

async function quiet() {
  await runCommand("notifications.clearAll");
  await runCommand("notifications.hideToasts");
  await runCommand("notifications.hideList");
  await runCommand("workbench.action.closeAuxiliaryBar");
  await settle(150);
}

async function focusExplorer() {
  await vscode.commands.executeCommand("workbench.view.explorer");
  await settle(300);
}

async function showView(id) {
  await vscode.commands.executeCommand(`${id}.focus`);
  await settle(400);
}

exports.run = async function run() {
  const outputDir = process.env.DYGNOSIS_CAPTURE_OUTPUT;
  const originalsDir = process.env.DYGNOSIS_CAPTURE_ORIGINALS;
  assert.ok(outputDir, "Set DYGNOSIS_CAPTURE_OUTPUT");
  assert.ok(originalsDir, "Set DYGNOSIS_CAPTURE_ORIGINALS");
  await fs.mkdir(outputDir, { recursive: true });
  await fs.mkdir(originalsDir, { recursive: true });
  const manifest = [];
  const failures = [];
  const notes = {};
  const only = new Set((process.env.DYGNOSIS_CAPTURE_ONLY ?? "").split(",").map(item => item.trim()).filter(Boolean));
  const capture = async (name, steps, keepFocus = false) => {
    if (only.size && !only.has(name)) return;
    try {
      const detail = await steps();
      if (!keepFocus) await quiet();
      await settle(keepFocus ? 200 : 350);
      const original = path.join(originalsDir, `${name}.png`);
      shot(original);
      notes[name] = detail ?? {};
      manifest.push({ name, original, capturedAt: new Date().toISOString(), status: "ok", detail: notes[name] });
    } catch (error) {
      manifest.push({ name, status: "failed", error: String(error) });
      failures.push(name);
    }
  };

  const extension = vscode.extensions.getExtension("CoconutWater.dygnosis");
  assert.ok(extension, "Dygnosis extension missing");
  const service = await extension.activate();
  await settle(800);
  await quiet();
  const workspace = vscode.workspace.workspaceFolders[0].uri.fsPath;

  const fixtureModel = await fs.readFile(path.join(workspace, "model.mod"), "utf8");
  const fixtureProblems = await fs.readFile(path.join(workspace, "problems.mod"), "utf8");
  const fixtureAfter = await fs.readFile(path.join(workspace, "after.mod"), "utf8");

  await capture("help-panel", async () => {
    await runCommand("workbench.action.closeSidebar");
    await runCommand("workbench.action.closePanel");
    await vscode.commands.executeCommand("dygnosis.openHelp", "reference");
    await waitFor(() => vscode.window.tabGroups.all.some(group => group.tabs.some(tab => tab.label === "Dygnosis Help")), "Help panel");
    await settle(700);
  });

  await capture("get-started", async () => {
    await vscode.commands.executeCommand("workbench.action.closeAllEditors");
    await runCommand("workbench.action.closePanel");
    await focusExplorer();
    await showView("dygnosis.model");
    await settle(300);
    clickAt(18, 748);
    await settle(200);
    clickAt(18, 778);
    await settle(200);
    await showView("dygnosis.project");
    for (let i = 0; i < 5; ++i) await runCommand("list.expand");
  });

  const model = await openModel(service, workspace, fixtureModel);
  await quiet();
  await capture("model-overview", async () => {
    await vscode.commands.executeCommand("workbench.action.closeAllEditors");
    await runCommand("workbench.action.closePanel");
    await vscode.window.showTextDocument(model, { preview: false, viewColumn: vscode.ViewColumn.One });
    await focusExplorer();
    await showView("dygnosis.model");
    for (let i = 0; i < 6; ++i) await runCommand("list.expand");
    const editor = await vscode.window.showTextDocument(model, { preview: false, viewColumn: vscode.ViewColumn.One });
    editor.revealRange(new vscode.Range(5, 0, 11, 0), vscode.TextEditorRevealType.InCenter);
    await settle(600);
  });

  await capture("appearance", async () => {
    await vscode.window.showTextDocument(model, { preview: false, viewColumn: vscode.ViewColumn.One });
    await runCommand("workbench.action.closeSidebar");
    await runCommand("workbench.action.closePanel");
    const editor = vscode.window.activeTextEditor;
    editor.revealRange(new vscode.Range(0, 0, 12, 0), vscode.TextEditorRevealType.AtTop);
  });

  await capture("edit-hover", async () => {
    await vscode.window.showTextDocument(model, { preview: false, viewColumn: vscode.ViewColumn.One });
    await runCommand("workbench.action.closeSidebar");
    await runCommand("workbench.action.closePanel");
    await quiet();
    const editor = vscode.window.activeTextEditor;
    const nameOffset = editor.document.getText().indexOf(", c ");
    const position = editor.document.positionAt(nameOffset + 2);
    editor.selection = new vscode.Selection(position, position);
    editor.revealRange(new vscode.Range(0, 0, 10, 0), vscode.TextEditorRevealType.AtTop);
    await vscode.commands.executeCommand("editor.action.showHover");
    await settle(700);
  }, true);

  await capture("edit-suggest", async () => {
    await vscode.window.showTextDocument(model, { preview: false, viewColumn: vscode.ViewColumn.One });
    await runCommand("workbench.action.closeSidebar");
    await runCommand("workbench.action.closePanel");
    await quiet();
    const editor = vscode.window.activeTextEditor;
    const token = editor.document.getText().indexOf("+ e");
    const position = editor.document.positionAt(token + 2);
    editor.selection = new vscode.Selection(position, position);
    editor.revealRange(new vscode.Range(0, 0, 8, 0), vscode.TextEditorRevealType.AtTop);
    await vscode.commands.executeCommand("editor.action.triggerSuggest");
    await settle(700);
  }, true);

  const problems = await openModel(service, workspace, fixtureProblems, "problems.mod");
  await capture("diagnostics", async () => {
    await vscode.window.showTextDocument(problems, { preview: false, viewColumn: vscode.ViewColumn.One });
    await runCommand("workbench.action.closeSidebar");
    await waitFor(() => vscode.languages.getDiagnostics(problems.uri).some(item => item.severity === vscode.DiagnosticSeverity.Error), "Problems diagnostics");
    await vscode.commands.executeCommand("workbench.actions.view.problems");
    vscode.window.activeTextEditor?.revealRange(new vscode.Range(0, 0, 8, 0), vscode.TextEditorRevealType.AtTop);
    await settle(400);
  });

  await capture("navigate-code", async () => {
    await vscode.window.showTextDocument(model, { preview: false, viewColumn: vscode.ViewColumn.One });
    await runCommand("workbench.action.closePanel");
    await vscode.commands.executeCommand("outline.focus");
    for (let i = 0; i < 8; ++i) await runCommand("list.expand");
    await settle(400);
  });

  await capture("effective-model", async () => {
    await vscode.window.showTextDocument(model, { preview: false, viewColumn: vscode.ViewColumn.One });
    await runCommand("workbench.action.closeSidebar");
    await runCommand("workbench.action.closePanel");
    await vscode.commands.executeCommand("dygnosis.showEffectiveModel");
    await waitFor(() => vscode.window.visibleTextEditors.some(editor => editor.document.uri.scheme === "dygnosis-effective"), "effective preview");
    await settle(600);
    return { preview: vscode.window.visibleTextEditors.map(editor => editor.document.uri.toString()) };
  });

  await capture("structural-diff", async () => {
    const before = await openModel(service, workspace, fixtureModel, "before-diff.mod");
    await vscode.window.showTextDocument(before, { preview: false, viewColumn: vscode.ViewColumn.One });
    const afterPath = path.join(workspace, "after.mod");
    await writeModel(service, afterPath, fixtureAfter);
    const originalPicker = vscode.window.showOpenDialog;
    vscode.window.showOpenDialog = async () => [vscode.Uri.file(afterPath)];
    try {
      await vscode.commands.executeCommand("dygnosis.diffWith");
    } finally {
      vscode.window.showOpenDialog = originalPicker;
    }
    await waitFor(async () => vscode.window.tabGroups.all.flatMap(group => group.tabs).some(tab => typeof tab.label === "string" && tab.label.includes("Diff")), "structural diff view", 200);
    await settle(800);
    for (const group of vscode.window.tabGroups.all) {
      for (const tab of group.tabs) {
        if (typeof tab.label === "string" && !tab.label.includes("Diff")) await vscode.window.tabGroups.close(tab);
      }
    }
    await runCommand("workbench.action.closeSidebar");
    await runCommand("workbench.action.closePanel");
    await settle(400);
  });

  await capture("project-checks", async () => {
    await vscode.commands.executeCommand("workbench.action.closeAllEditors");
    await runCommand("workbench.action.closePanel");
    await focusExplorer();
    await waitFor(async () => (await service.execute("dynare/projectStatus"))?.complete, "project status");
    await settle(300);
    clickAt(18, 720);
    await settle(150);
    clickAt(18, 750);
    await settle(150);
    clickAt(18, 790);
    await settle(200);
    await showView("dygnosis.project");
    for (let i = 0; i < 6; ++i) await runCommand("list.expand");
    await settle(300);
  });

  await capture("agents-mcp", async () => {
    await vscode.commands.executeCommand("workbench.action.closeAllEditors");
    await runCommand("workbench.action.closePanel");
    const opened = await runCommand("workbench.mcp.showInstalledServers");
    await settle(900);
    return { opened: opened ?? "workbench.mcp.showInstalledServers" };
  });

  await capture("agents-mcp-picker", async () => {
    await runCommand("workbench.action.closeQuickOpen");
    await runCommand("workbench.action.closePanel");
    await quiet();
    const pending = vscode.commands.executeCommand("workbench.mcp.listServer");
    await settle(900);
    return { pending: typeof pending };
  }, true);

  await capture("troubleshoot", async () => {
    await runCommand("workbench.action.closeQuickOpen");
    await runCommand("workbench.action.closeSidebar");
    await vscode.commands.executeCommand("dygnosis.showOutput");
    await runCommand("workbench.action.toggleMaximizedPanel");
    await settle(500);
  });

  await fs.writeFile(path.join(outputDir, "manifest.json"), JSON.stringify({ vscode: vscode.version, captures: manifest, failures }, null, 2) + "\n");
  if (failures.length) throw new Error(`Capture failed for: ${failures.join(", ")}`);
};
