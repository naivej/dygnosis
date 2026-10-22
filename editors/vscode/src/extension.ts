import { registerWorkspaceStatus } from "./editor/workspace_status";
import { registerChanges } from "./comparison/changes_editor";
import { registerWorkspaceMcp } from "./mcp/workspace_mcp";
// The installed-package harness verifies and stages the actual bundled engine.
export { verifyPackagedBundle, ensureManagedBinary } from "./mcp/workspace_mcp";
import { registerOriginJumps } from "./editor/origin_jumps";
import { registerDiagnosticActions } from "./editor/quickfix";
import { registerLenses } from "./editor/lenses";
import { registerColors } from "./editor/color";
import { registerModelNavigation } from "./editor/model_navigation";
import { registerStatus } from "./editor/status";
import { registerAnalysisConfigurations } from "./editor/analysis_configuration";
import { registerSourceCandidates } from "./editor/source_candidates";
import * as vscode from "vscode";
import { DygnosisClient, isAnalysisDocument } from "./client/client";
import { registerMcp } from "./mcp/mcp";
import { registerEffectivePreview } from "./editor/preview";
import { listSetting } from "./client/settings";
import { registerHelp } from "./help/help";
import { changesViewType, historyScheme } from "./comparison/changes_resource";
import * as path from "node:path";
import { record } from "./client/protocol";
import { runCommand } from "./client/refusal";

let service: DygnosisClient | undefined;
export function activate(context: vscode.ExtensionContext): DygnosisClient {
  context.subscriptions.push(registerHelp(context));
  const client = new DygnosisClient(context);
  service = client;
  context.subscriptions.push(registerWorkspaceMcp(context, client));
  const previews = registerEffectivePreview(client);
  const changes = registerChanges(client, context.extensionUri);
  context.subscriptions.push(client, registerDiagnosticActions(client), registerWorkspaceStatus(client), registerLenses(client), registerStatus(client, previews), registerModelNavigation(client), registerColors(client), registerMcp(context, client.log), previews, registerOriginJumps(client, previews), changes,
    registerAnalysisConfigurations(client, previews, changes.compareAnalysis), registerSourceCandidates(client),
    vscode.commands.registerCommand("dygnosis.restartServer", () => runCommand(client, () => client.restart())),
    vscode.commands.registerCommand("dygnosis.showOutput", () => client.output.show()),
    vscode.commands.registerCommand("dygnosis.openSettings", () => vscode.commands.executeCommand("workbench.action.openSettings", "@ext:CoconutWater.dygnosis")),
    vscode.commands.registerCommand("dygnosis.editSettingsJson", () => vscode.commands.executeCommand("workbench.action.openSettingsJson")),
    vscode.commands.registerCommand("dygnosis.treatAsRoot", () => runCommand(client, async () => {
      const document = vscode.window.activeTextEditor?.document;
      if (document) await client.treatDocumentAsRoot(document);
      else await vscode.window.showInformationMessage("Open a Dynare model file before treating it as a root.");
    })),
  );
  let contextGeneration = 0;
  const updateContext = (): void => {
    const generation = ++contextGeneration;
    const document = vscode.window.activeTextEditor?.document;
    const ordinary = document ? isAnalysisDocument(document) : false;
    const tab = vscode.window.tabGroups.activeTabGroup.activeTab?.input;
    const custom = tab instanceof vscode.TabInputCustom && tab.viewType === changesViewType;
    const gitPath = document?.uri.scheme === "git" ? (() => {
      try {
        if (document.uri.query.length > 32768) return undefined;
        const value: unknown = JSON.parse(document.uri.query);
        return record(value) && typeof value.path === "string" && path.isAbsolute(value.path) && !/[\0\r\n]/.test(value.path) ? value.path : undefined;
      } catch { return undefined; }
    })() : undefined;
    // Availability can lead to explicit source selection. Execution still
    // proves the repository, fixed commit and displayed bytes before capture.
    const historical = !!document && (document.uri.scheme === historyScheme || !ordinary && !document.uri.scheme.startsWith("dygnosis-") &&
      (document.languageId === "dynare" || gitPath !== undefined &&
        (/\.(mod|dyn)$/i.test(gitPath) || client.knownOwners(vscode.Uri.file(gitPath)).length > 0)));
    void vscode.commands.executeCommand("setContext", "dygnosis.changesContext", custom || historical);
    void vscode.commands.executeCommand("setContext", "dygnosis.knownIncludedPaths", client.knownIncludedFiles());
    const actions = listSetting("editorActions", document?.uri, ["toolbar", "contextMenu"], ["toolbar", "contextMenu"], client.log);
    void vscode.commands.executeCommand("setContext", "dygnosis.analysisDocument", ordinary);
    void vscode.commands.executeCommand("setContext", "dygnosis.modelContext", false);
    void vscode.commands.executeCommand("setContext", "dygnosis.toolbarActions", actions.includes("toolbar"));
    void vscode.commands.executeCommand("setContext", "dygnosis.contextActions", actions.includes("contextMenu"));
    if (ordinary && document) void client.rootForDocument(document).then(root => {
      if (generation !== contextGeneration) return;
      const hasOwner = !!root || client.knownOwners(document.uri).length > 0;
      if (!custom) void vscode.commands.executeCommand("setContext", "dygnosis.changesContext", hasOwner);
      const supported = client.client?.initializeResult?.capabilities.executeCommandProvider?.commands.includes("dynare/showEffectiveModel") ?? false;
      void vscode.commands.executeCommand("setContext", "dygnosis.modelContext", hasOwner && supported);
    }).catch(error => client.log(String(error)));
  };
  context.subscriptions.push(vscode.window.onDidChangeActiveTextEditor(updateContext),
    vscode.window.tabGroups.onDidChangeTabs(updateContext), vscode.window.tabGroups.onDidChangeTabGroups(updateContext),
    vscode.workspace.onDidChangeConfiguration(updateContext), client.onDidChange(updateContext), client.onDidUpdateModelInfo(updateContext));
  updateContext();
  if (vscode.workspace.textDocuments.some(isAnalysisDocument)) void client.ensureStarted().catch(error => client.log(String(error)));
  return client;
}
export async function deactivate(): Promise<void> { await service?.shutdown(); service = undefined; }
