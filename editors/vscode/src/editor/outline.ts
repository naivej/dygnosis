import * as vscode from "vscode";
import { DygnosisClient, isAnalysisDocument } from "../client/client";
import { OutlineActivity, outlineActivitySetting } from "../client/settings";
import { runCommand } from "../client/refusal";

export function registerOutlineFilter(client: DygnosisClient): vscode.Disposable {
  return vscode.commands.registerCommand("dygnosis.filterOutline", () => runCommand(client, async () => {
    const document = vscode.window.activeTextEditor?.document;
    if (!document || !isAnalysisDocument(document)) {
      await vscode.window.showInformationMessage("Open a Dynare source file before filtering Outline.");
      return;
    }
    const current = outlineActivitySetting(document.uri, client.log);
    const choices: (vscode.QuickPickItem & { activity: OutlineActivity })[] = [
      { label: "Both", activity: "both" },
      { label: "Active", activity: "active" },
      { label: "Inactive", activity: "inactive" },
    ];
    const selected = await vscode.window.showQuickPick(choices.map(choice => ({
      ...choice, description: choice.activity === current ? "Current" : undefined,
    })), { title: "Filter Outline by macro activity", placeHolder: "Show active, inactive, or both" });
    if (!selected || selected.activity === current) return;
    const folder = vscode.workspace.getWorkspaceFolder(document.uri);
    await vscode.workspace.getConfiguration("dynare", document.uri).update("outline.activity", selected.activity,
      folder ? vscode.ConfigurationTarget.WorkspaceFolder : vscode.ConfigurationTarget.Global);
  }));
}
