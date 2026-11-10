import * as vscode from "vscode";
import { posix } from "node:path";
import { DygnosisClient, isAnalysisDocument } from "../client/client";
import type { OutlineActivity } from "../client/settings";
import { refuse, runCommand } from "../client/refusal";

export function registerOutlineFilter(client: DygnosisClient): vscode.Disposable {
  return vscode.commands.registerCommand("dygnosis.filterOutline", () => runCommand(client, async () => {
    const editor = vscode.window.activeTextEditor;
    const document = editor?.document;
    if (!document || !isAnalysisDocument(document)) {
      await vscode.window.showInformationMessage("Open a Dynare source file before filtering Outline.");
      return;
    }
    const version = document.version;
    await client.ensureStarted();
    if (!client.supportsOutlineActivity) refuse("This engine does not support temporary Outline filters. Use the bundled binary or update Server Path.");
    const instance = client.currentInstance;
    const root = client.chosenRootForDocument(document) ?? await client.rootForDocument(document);
    if (!root) refuse("Run Choose model owner for this include, then filter Outline again.");
    const owner = client.documentContextIdentity(document.uri);
    const stillCurrent = (): boolean => !document.isClosed && document.version === version && vscode.window.activeTextEditor === editor &&
      client.currentInstance === instance && client.documentContextIdentity(document.uri) === owner;
    if (!stillCurrent()) refuse("The source or model changed. Filter Outline again.");
    const current = client.outlineActivity(root, document.uri);
    const choices: (vscode.QuickPickItem & { activity: OutlineActivity })[] = [
      { label: "Both", activity: "both" },
      { label: "Active", activity: "active" },
      { label: "Inactive", activity: "inactive" },
    ];
    const items = choices.map(choice => ({
      ...choice, description: choice.activity === current ? "Current" : undefined,
    }));
    items.sort((left, right) => Number(right.activity === current) - Number(left.activity === current));
    const selected = await vscode.window.showQuickPick(items, {
      title: `Filter Outline for ${posix.basename(root.path)}`, placeHolder: "Show active, inactive, or both",
    });
    if (!selected || selected.activity === current) return;
    if (!stillCurrent()) refuse("The source or model changed. Filter Outline again.");
    await client.setOutlineActivity(root, selected.activity);
  }));
}
