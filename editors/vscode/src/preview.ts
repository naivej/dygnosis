import * as vscode from "vscode";
import { DygnosisClient } from "./client";
import { NavigationContext, validateNavigationContext } from "./model_view";
import { record } from "./protocol";

export interface EffectivePreviewSession {
  readonly uri: vscode.Uri;
  readonly root: vscode.Uri;
  readonly document: vscode.TextDocument;
  readonly instance: number;
  result: unknown;
  text: string;
  generation: number;
}
export interface EffectivePreviewRegistry extends vscode.Disposable {
  readonly onDidCreate: vscode.Event<EffectivePreviewSession>;
  readonly onDidClose: vscode.Event<EffectivePreviewSession>;
  sessions(): EffectivePreviewSession[];
  get(uri: vscode.Uri): EffectivePreviewSession | undefined;
  replace(session: EffectivePreviewSession, result: unknown): boolean;
}
function previewText(result: unknown): string | undefined {
  return record(result) && typeof result.effective_text === "string"
    ? (result.status === "incomplete" ? "// INCOMPLETE EXPANSION — this preview is partial.\n" : "") + result.effective_text : undefined;
}
export function registerEffectivePreview(service: DygnosisClient): EffectivePreviewRegistry {
  const text = new Map<string, string>();
  const sessions = new Map<string, EffectivePreviewSession>();
  const created = new vscode.EventEmitter<EffectivePreviewSession>();
  const closed = new vscode.EventEmitter<EffectivePreviewSession>();
  const contentChanged = new vscode.EventEmitter<vscode.Uri>();
  let sequence = 0;
  let disposed = false;
  const provider = vscode.workspace.registerTextDocumentContentProvider("dygnosis-effective", {
    onDidChange: contentChanged.event,
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
    const version = document.version;
    try {
      const valid = async (): Promise<boolean> => !disposed && !document.isClosed && document.version === version &&
        vscode.window.activeTextEditor?.document === document && (!expected ||
        !!await validateNavigationContext(service, expected, () => !disposed));
      if (!await valid()) return;
      const root = expected?.root ?? await service.rootForDocument(document, true);
      if (!root) { await vscode.window.showInformationMessage("Open a model root or choose a known owner for this include."); return; }
      await service.ensureStarted();
      const instance = service.currentInstance;
      const validRoot = async (): Promise<boolean> => await valid() && service.currentInstance === instance &&
        (await service.rootForDocument(document))?.toString() === root.toString() && await valid() && service.currentInstance === instance;
      if (!await validRoot()) return;
      const result = await service.execute("dynare/showEffectiveModel", [root.toString()]);
      if (!await validRoot()) return;
      if (!record(result) || typeof result.effective_text !== "string") throw new Error("This engine cannot show the effective model. Update dynare.serverPath or use the bundle.");
      const uri = vscode.Uri.from({ scheme: "dygnosis-effective", path: `/${++sequence}/${root.path.split("/").at(-1) ?? "model.mod"}` });
      const rendered = previewText(result)!;
      text.set(uri.toString(), rendered);
      const preview = await vscode.workspace.openTextDocument(uri);
      if (!await validRoot()) { text.delete(uri.toString()); return; }
      await vscode.languages.setTextDocumentLanguage(preview, "dynare");
      if (!await validRoot()) { text.delete(uri.toString()); return; }
      const session: EffectivePreviewSession = { uri, root, document: preview, instance,
        result, text: rendered, generation: 0 };
      sessions.set(uri.toString(), session);
      created.fire(session);
      await vscode.window.showTextDocument(preview, { viewColumn: vscode.ViewColumn.Beside, preview: false });
    } catch (error) { await service.failure(String(error)); }
  });
  const close = vscode.workspace.onDidCloseTextDocument(document => {
    if (document.uri.scheme !== "dygnosis-effective") return;
    const key = document.uri.toString(), session = sessions.get(key);
    if (session) { ++session.generation; sessions.delete(key); closed.fire(session); }
    text.delete(key);
  });
  const disposable = vscode.Disposable.from(provider, command, close, new vscode.Disposable(() => {
    disposed = true;
    for (const session of sessions.values()) { ++session.generation; closed.fire(session); }
    sessions.clear(); text.clear();
  }), created, closed, contentChanged);
  return {
    onDidCreate: created.event, onDidClose: closed.event,
    sessions: () => [...sessions.values()], get: uri => sessions.get(uri.toString()),
    replace(session, result) {
      const rendered = previewText(result);
      if (disposed || sessions.get(session.uri.toString()) !== session || rendered === undefined) return false;
      session.result = result; session.text = rendered; ++session.generation;
      text.set(session.uri.toString(), rendered); contentChanged.fire(session.uri);
      return true;
    },
    dispose: () => { disposable.dispose(); },
  };
}
