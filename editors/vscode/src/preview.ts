import * as vscode from "vscode";
import { DygnosisClient } from "./client";
import { NavigationContext, validateNavigationContext } from "./model_view";
import { record } from "./protocol";

export function registerEffectivePreview(service: DygnosisClient): vscode.Disposable {
  const text = new Map<string, string>();
  let sequence = 0;
  let disposed = false;
  const provider = vscode.workspace.registerTextDocumentContentProvider("dygnosis-effective", {
    provideTextDocumentContent(uri) { return text.get(uri.toString()) ?? "// This effective preview is no longer available."; },
  });
  const command = vscode.commands.registerCommand("dygnosis.showEffectiveModel", async (argument?: unknown) => {
    const document = vscode.window.activeTextEditor?.document;
    // Editor menus pass a URI. Only our explicit snapshot argument supplies
    // an expected input; ordinary palette/menu actions keep their root picker.
    const expected = record(argument) && record(argument.info) && typeof argument.info.revision === "string" &&
      typeof argument.info.client_instance === "number" && typeof argument.version === "number" &&
      record(argument.document) && record(argument.root) ? argument as unknown as NavigationContext : undefined;
    if (disposed || !document || (expected && expected.document !== document)) return;
    try {
      const valid = async (): Promise<boolean> => !disposed && (!expected ||
        !!await validateNavigationContext(service, expected, () => !disposed));
      if (!await valid()) return;
      const root = expected?.root ?? await service.rootForDocument(document, true);
      if (!root) { await vscode.window.showInformationMessage("Open a model root or choose a known owner for this include."); return; }
      const result = await service.execute("dynare/showEffectiveModel", [root.toString()]);
      if (!await valid()) return;
      if (!record(result) || typeof result.effective_text !== "string") throw new Error("This engine cannot show the effective model. Update dynare.serverPath or use the bundle.");
      const uri = vscode.Uri.from({ scheme: "dygnosis-effective", path: `/${++sequence}/${root.path.split("/").at(-1) ?? "model.mod"}` });
      text.set(uri.toString(), (result.status === "incomplete" ? "// INCOMPLETE EXPANSION — this preview is partial.\n" : "") + result.effective_text);
      const preview = await vscode.workspace.openTextDocument(uri);
      if (!await valid()) { text.delete(uri.toString()); return; }
      await vscode.languages.setTextDocumentLanguage(preview, "dynare");
      if (!await valid()) { text.delete(uri.toString()); return; }
      await vscode.window.showTextDocument(preview, { viewColumn: vscode.ViewColumn.Beside, preview: false });
    } catch (error) { await service.failure(String(error)); }
  });
  const close = vscode.workspace.onDidCloseTextDocument(document => {
    if (document.uri.scheme === "dygnosis-effective") text.delete(document.uri.toString());
  });
  return vscode.Disposable.from(provider, command, close, new vscode.Disposable(() => { disposed = true; text.clear(); }));
}
