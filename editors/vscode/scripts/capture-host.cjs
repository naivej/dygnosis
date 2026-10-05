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
  const capture = async (name, steps) => {
    try {
      await steps();
      await settle(600);
      const original = path.join(originalsDir, `${name}.png`);
      shot(original);
      manifest.push({ name, original, capturedAt: new Date().toISOString(), status: "ok" });
    } catch (error) {
      manifest.push({ name, status: "failed", error: String(error) });
      failures.push(name);
    }
  };

  const extension = vscode.extensions.getExtension("dygnosis.dygnosis");
  assert.ok(extension, "Dygnosis extension missing");
  const service = await extension.activate();
  const workspace = vscode.workspace.workspaceFolders[0].uri.fsPath;

  const fixtureModel = await fs.readFile(path.join(workspace, "model.mod"), "utf8");
  const fixtureProblems = await fs.readFile(path.join(workspace, "problems.mod"), "utf8");
  const fixtureAfter = await fs.readFile(path.join(workspace, "after.mod"), "utf8");

  await capture("help-panel", async () => {
    await vscode.commands.executeCommand("dygnosis.openHelp", "reference");
    await waitFor(() => vscode.window.tabGroups.all.some(group => group.tabs.some(tab => tab.label === "Dygnosis Help")), "Help panel");
    await settle(800);
  });

  await capture("get-started", async () => {
    await vscode.commands.executeCommand("workbench.action.closeAllEditors");
    await focusExplorer();
  });

  const model = await openModel(service, workspace, fixtureModel);
  await capture("model-overview", async () => {
    await vscode.commands.executeCommand("workbench.action.closeAllEditors");
    await vscode.window.showTextDocument(model.uri, { preview: false, viewColumn: vscode.ViewColumn.One });
    await focusExplorer();
    await showView("dygnosis.model");
  });

  await capture("appearance", async () => {
    await vscode.window.showTextDocument(model.uri, { preview: false, viewColumn: vscode.ViewColumn.One });
    await vscode.commands.executeCommand("workbench.action.closeSidebar");
    await settle(300);
  });

  await capture("edit-assistance", async () => {
    await vscode.window.showTextDocument(model.uri, { preview: false, viewColumn: vscode.ViewColumn.One });
    const editor = vscode.window.activeTextEditor;
    const position = new vscode.Position(2, 8);
    editor.selection = new vscode.Selection(position, position);
    await vscode.commands.executeCommand("editor.action.showHover");
    await settle(500);
  });

  const problems = await openModel(service, workspace, fixtureProblems, "problems.mod");
  await capture("diagnostics", async () => {
    await vscode.window.showTextDocument(problems.uri, { preview: false, viewColumn: vscode.ViewColumn.One });
    await waitFor(() => vscode.languages.getDiagnostics(problems.uri).some(item => item.severity === vscode.DiagnosticSeverity.Error), "Problems diagnostics");
    await vscode.commands.executeCommand("workbench.actions.view.problems");
    await settle(500);
  });

  await capture("navigate-code", async () => {
    await vscode.window.showTextDocument(model.uri, { preview: false, viewColumn: vscode.ViewColumn.One });
    await vscode.commands.executeCommand("workbench.action.closeSidebar");
    await vscode.commands.executeCommand("outline.focus");
    await settle(500);
  });

  await capture("effective-model", async () => {
    await vscode.window.showTextDocument(model.uri, { preview: false, viewColumn: vscode.ViewColumn.One });
    await vscode.commands.executeCommand("dygnosis.showEffectiveModel");
    await waitFor(() => vscode.window.activeTextEditor?.document.uri.scheme === "dygnosis-effective", "effective preview");
    await settle(500);
  });

  await capture("structural-diff", async () => {
    const before = await openModel(service, workspace, fixtureModel, "before-diff.mod");
    await vscode.window.showTextDocument(before.uri, { preview: false, viewColumn: vscode.ViewColumn.One });
    const afterPath = path.join(workspace, "after.mod");
    await writeModel(service, afterPath, fixtureAfter);
    const originalPicker = vscode.window.showOpenDialog;
    vscode.window.showOpenDialog = async () => [vscode.Uri.file(afterPath)];
    try {
      await vscode.commands.executeCommand("dygnosis.diffWith");
    } finally {
      vscode.window.showOpenDialog = originalPicker;
    }
    await waitFor(async () => {
      const tabs = vscode.window.tabGroups.all.flatMap(group => group.tabs);
      return tabs.find(tab => typeof tab.label === "string" && tab.label.includes("Diff"));
    }, "structural diff view", 200);
    await settle(1200);
  });

  await capture("project-checks", async () => {
    await vscode.commands.executeCommand("workbench.action.closeAllEditors");
    await focusExplorer();
    await showView("dygnosis.project");
    await waitFor(async () => {
      const status = await service.execute("dynare/projectStatus");
      return status?.complete;
    }, "project status");
  });

  await capture("agents-mcp", async () => {
    await vscode.commands.executeCommand("workbench.action.closeAllEditors");
    await focusExplorer();
    const commands = ["workbench.mcp.listServer", "workbench.action.openMcpServersView", "mcp.listServers"];
    for (const command of commands) {
      try { await vscode.commands.executeCommand(command); break; } catch { /* try next */ }
    }
    await settle(800);
  });

  await capture("troubleshoot", async () => {
    await vscode.commands.executeCommand("dygnosis.showOutput");
    await settle(500);
  });

  await fs.writeFile(path.join(outputDir, "manifest.json"), JSON.stringify({ vscode: vscode.version, captures: manifest, failures }, null, 2) + "\n");
  if (failures.length) throw new Error(`Capture failed for: ${failures.join(", ")}`);
};
