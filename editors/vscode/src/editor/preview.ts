import { refuse, explainFailure } from "../client/refusal";
import * as vscode from "vscode";
import { DygnosisClient } from "../client/client";
import { NavigationContext, validateNavigationContext } from "./model_navigation";
import { record } from "../client/protocol";

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
/** Older engines omit this array. Messages stay out of the Problems list. */
export function appendMacroMessages(output: vscode.OutputChannel, result: unknown): void {
  if (!record(result) || !Array.isArray(result.macro_messages)) return;
  for (const item of result.macro_messages) {
    if (!record(item) || typeof item.message !== "string") continue;
    const kind = item.kind === "macrovars" ? "echomacrovars" : "echo";
    const line = record(item.range) && record(item.range.start) && typeof item.range.start.line === "number"
      ? item.range.start.line + 1
      : record(item.location) && typeof item.location.line === "number" ? item.location.line : "?";
    output.appendLine(`@#${kind} (line ${line}): ${item.message.replace(/\n$/, "")}`);
  }
}
function previewText(result: unknown): string | undefined {
  const reasons = record(result) && Array.isArray(result.incomplete_reasons)
    ? result.incomplete_reasons.filter(record).flatMap(reason => typeof reason.message === "string" ? [reason.message] : []) : [];
  const explanation = reasons.length ? reasons.join("\n") : record(result) && typeof result.message === "string" ? result.message : "Complete model counts and source jumps are unavailable. Resolve the model problems and refresh.";
  return record(result) && typeof result.effective_text === "string"
    ? (result.status === "incomplete" ? `// INCOMPLETE EXPANSION — this preview is partial.\n// ${explanation.replace(/\r?\n/g, "\n// ")}\n` : "") + result.effective_text : undefined;
}
/** Open, Refresh, and jump proofs must use the same advertised display layout. */
export function effectivePreviewArguments(service: Pick<DygnosisClient, "client">, root: vscode.Uri, legacyUriString = false): unknown[] {
  const experimental: unknown = service.client?.initializeResult?.capabilities.experimental;
  const preview = record(experimental) && record(experimental.dygnosis) && record(experimental.dygnosis.effectivePreview) &&
    experimental.dygnosis.effectivePreview.command === "dynare/showEffectiveModel"
    ? experimental.dygnosis.effectivePreview : undefined;
  if (preview && preview.source_layout === true) {
    return [{ root_uri: root.toString(), layout: "source" }];
  }
  if (preview && preview.readable_layout === true) {
    return [{ root_uri: root.toString(), layout: "readable" }];
  }
  return [legacyUriString ? root.toString() : { root_uri: root.toString() }];
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
    if (disposed) return;
    const expired = (): never => refuse("The source or model changed. Request macro expansion again from the current Dynare document.");
    try {
    if (!document) { refuse("Open a Dynare model or include before expanding macros."); }
    if (expected && expected.document !== document) { expired(); }
    const version = document.version;
      const valid = async (): Promise<boolean> => !disposed && !document.isClosed && document.version === version &&
        vscode.window.activeTextEditor?.document === document && (!expected ||
        !!await validateNavigationContext(service, expected, () => !disposed));
      if (!await valid()) { expired(); }
      const root = expected?.root ?? await service.rootForDocument(document, true);
      if (!root) { refuse("Open a model root or choose a known owner for this include."); }
      await service.ensureStarted();
      const instance = service.currentInstance;
      const validRoot = async (): Promise<boolean> => await valid() && service.currentInstance === instance &&
        (await service.rootForDocument(document))?.toString() === root.toString() && await valid() && service.currentInstance === instance;
      if (!await validRoot()) { expired(); }
      const result = await service.execute("dynare/showEffectiveModel", effectivePreviewArguments(service, root, true));
      if (!await validRoot()) { expired(); }
      if (!record(result) || typeof result.effective_text !== "string") throw new Error("This engine cannot show macro expansion. Update dynare.serverPath or use the bundled binary.");
      appendMacroMessages(service.output, result);
      const uri = vscode.Uri.from({ scheme: "dygnosis-effective", path: `/${++sequence}/${root.path.split("/").at(-1) ?? "model.mod"}` });
      const rendered = previewText(result)!;
      text.set(uri.toString(), rendered);
      const preview = await vscode.workspace.openTextDocument(uri);
      if (!await validRoot()) { text.delete(uri.toString()); expired(); }
      await vscode.languages.setTextDocumentLanguage(preview, "dynare");
      if (!await validRoot()) { text.delete(uri.toString()); expired(); }
      const session: EffectivePreviewSession = { uri, root, document: preview, instance,
        result, text: rendered, generation: 0 };
      sessions.set(uri.toString(), session);
      created.fire(session);
      await vscode.window.showTextDocument(preview, { viewColumn: vscode.ViewColumn.Beside, preview: false });
    } catch (error) { if (!disposed) await explainFailure(service, error); }
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
