import { refuse, explainFailure, runCommand } from "../client/refusal";
import * as vscode from "vscode";
import { Buffer } from "node:buffer";
import { DygnosisClient } from "../client/client";
import { NavigationContext, validateNavigationContext } from "./model_navigation";
import { location, position as isPosition, Position, Range, record } from "../client/protocol";
import { sameSourceUri } from "../client/source_uri";
import { analysisLabel, resultAnalysis } from "./analysis_configuration";
import { reasonText } from "../client/failure";
import type { Hover as ProtocolHover } from "vscode-languageserver-protocol";

const openDefinitionCommand = "dygnosis.openPreviewDefinition";
type PreviewHover = ProtocolHover & { definition?: unknown };

export interface EffectivePreviewSession {
  readonly uri: vscode.Uri;
  readonly root: vscode.Uri;
  readonly document: vscode.TextDocument;
  readonly displayVersion: number;
  readonly hoverTextMatches: boolean;
  readonly instance: number;
  result: unknown;
  text: string;
  generation: number;
  outdated?: string;
  readonly navigationProof?: { revision: string; instance: number };
}
export interface EffectivePreviewRegistry extends vscode.Disposable {
  readonly onDidCreate: vscode.Event<EffectivePreviewSession>;
  readonly onDidClose: vscode.Event<EffectivePreviewSession>;
  sessions(): EffectivePreviewSession[];
  get(uri: vscode.Uri): EffectivePreviewSession | undefined;
  markOutdated(session: EffectivePreviewSession, cause: string): void;
  releaseSnapshot(root: vscode.Uri, result: unknown, instance: number): void;
  replace(session: EffectivePreviewSession, result: unknown, current?: () => boolean, navigationProof?: { revision: string; instance: number }): Promise<EffectivePreviewSession | undefined>;
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
    ? result.incomplete_reasons.filter(record).flatMap(reason => typeof reason.message === "string" ? [reasonText({ message: reason.message, location: reason.location })] : []) : [];
  const explanation = reasons.length ? reasons.join("\n") : record(result) && typeof result.message === "string" ? result.message : "Complete model counts and source jumps are unavailable. Resolve the model problems and refresh.";
  return record(result) && typeof result.effective_text === "string"
    ? (result.status === "incomplete" ? `// INCOMPLETE EXPANSION — this preview is partial.\n// ${explanation.replace(/\r?\n/g, "\n// ")}\n` : "") + result.effective_text : undefined;
}
/** VS Code can normalize CRLF to LF without changing LSP line/character positions. */
function hoverTextMatches(displayed: string, rendered: string): boolean {
  return displayed === rendered || displayed.replace(/\r\n/g, "\n") === rendered.replace(/\r\n/g, "\n") &&
    !displayed.replace(/\r\n/g, "").includes("\r") && !rendered.replace(/\r\n/g, "").includes("\r");
}

function previewName(root: vscode.Uri, result: unknown): string {
  const analysis = resultAnalysis(result), label = analysis ? analysisLabel(analysis) : "Source";
  const basename = (root.path.split("/").at(-1) ?? "model.mod").replace(/\.(mod|dyn)$/i, "");
  return `${basename} — ${label.replace(/[/\\:\r\n]/g, " ").slice(0, 100)}.mod`;
}

/** Symbols select the exact display snapshot; source jumps retain their separate revision proof. */
function previewSymbols(result: unknown, text: string): vscode.DocumentSymbol[] {
  if (!record(result) || !Array.isArray(result.document_symbols) || typeof result.effective_text !== "string" || previewText(result) !== text) return [];
  const lines = result.effective_text.split(/\r\n|\r|\n/);
  const lineOffset = text.slice(0, text.length - result.effective_text.length).split("\n").length - 1;
  const before = (left: Position, right: Position): boolean => left.line < right.line || left.line === right.line && left.character <= right.character;
  const contains = (outer: Range, inner: Range): boolean => before(outer.start, inner.start) && before(inner.end, outer.end);
  const range = (value: unknown): Range | undefined => {
    const candidate = { uri: "", range: value };
    if (!location(candidate)) return undefined;
    for (const position of [candidate.range.start, candidate.range.end]) {
      const line = lines[position.line];
      if (line === undefined || position.character > line.length) return undefined;
      const previous = line.charCodeAt(position.character - 1), next = line.charCodeAt(position.character);
      if (previous >= 0xd800 && previous <= 0xdbff && next >= 0xdc00 && next <= 0xdfff) return undefined;
    }
    return candidate.range;
  };
  const convertRange = (range: Range): vscode.Range => new vscode.Range(range.start.line + lineOffset, range.start.character,
    range.end.line + lineOffset, range.end.character);
  const convert = (rows: unknown[], parent?: Range, depth = 0): vscode.DocumentSymbol[] | undefined => {
    if (depth > 128) return undefined;
    const symbols: vscode.DocumentSymbol[] = [];
    for (const row of rows) {
      if (!record(row) || typeof row.name !== "string" || typeof row.kind !== "number" || !Number.isInteger(row.kind) || row.kind < 1 || row.kind > 26 ||
          row.detail !== undefined && typeof row.detail !== "string") return undefined;
      const full = range(row.range), selected = range(row.selectionRange);
      if (!full || !selected || !contains(full, selected) || parent && !contains(parent, full)) return undefined;
      if (row.children !== undefined && !Array.isArray(row.children)) return undefined;
      const children = convert(row.children ?? [], full, depth + 1);
      if (!children) return undefined;
      const symbol = new vscode.DocumentSymbol(row.name, row.detail ?? "", row.kind - 1, convertRange(full), convertRange(selected));
      symbol.children = children; symbols.push(symbol);
    }
    return symbols;
  };
  return convert(result.document_symbols) ?? [];
}
/** Open, Refresh, and jump proofs must use the same advertised display layout. */
export function effectivePreviewArguments(service: Pick<DygnosisClient, "client">, root: vscode.Uri, hover = false): unknown[] {
  const experimental: unknown = service.client?.initializeResult?.capabilities.experimental;
  const preview = record(experimental) && record(experimental.dygnosis) && record(experimental.dygnosis.effectivePreview) &&
    experimental.dygnosis.effectivePreview.command === "dynare/showEffectiveModel"
    ? experimental.dygnosis.effectivePreview : undefined;
  if (preview?.source_layout === true) {
    return [{ root_uri: root.toString(), layout: "source", ...(hover && preview.hover === true ? { hover: true } : {}) }];
  }
  throw new Error("The selected engine does not advertise the source-preserving macro expansion layout. Use the bundled Dygnosis engine or update dynare.serverPath to a current LSP engine.");
}
export function registerEffectivePreview(service: DygnosisClient): EffectivePreviewRegistry {
  const text = new Map<string, string>();
  const sessions = new Map<string, EffectivePreviewSession>();
  const created = new vscode.EventEmitter<EffectivePreviewSession>();
  const closed = new vscode.EventEmitter<EffectivePreviewSession>();
  const contentChanged = new vscode.EventEmitter<vscode.Uri>();
  let sequence = 0;
  let disposed = false;
  const release = (root: vscode.Uri, result: unknown, instance: number): void => {
    const client = service.client;
    if (instance !== service.currentInstance || !client || !record(result) || typeof result.hover_snapshot_id !== "string" || !result.hover_snapshot_id) return;
    void client.sendNotification("dynare/effectivePreviewClosed", { root_uri: root.toString(), snapshot_id: result.hover_snapshot_id }).catch(() => {});
  };
  const provider = vscode.workspace.registerTextDocumentContentProvider("dygnosis-effective", {
    onDidChange: contentChanged.event,
    provideTextDocumentContent(uri) { return text.get(uri.toString()) ?? "// This effective preview is no longer available."; },
  });
  const outline = vscode.languages.registerDocumentSymbolProvider({ language: "dynare", scheme: "dygnosis-effective" }, {
    provideDocumentSymbols(document) {
      const session = sessions.get(document.uri.toString());
      return !disposed && session?.document === document && !session.outdated && !document.isClosed && document.getText() === session.text
        ? previewSymbols(session.result, session.text) : [];
    },
  });
  const hoverGuard = (session: EffectivePreviewSession) => {
    const { document, generation } = session, client = service.client;
    return (): boolean => !disposed && sessions.get(session.uri.toString()) === session && session.generation === generation &&
      !session.outdated && !document.isClosed && document.version === session.displayVersion &&
      session.instance === service.currentInstance && service.client === client;
  };
  const queryHover = (client: NonNullable<DygnosisClient["client"]>, session: EffectivePreviewSession,
    snapshot: string, point: Position, token?: vscode.CancellationToken) =>
    client.sendRequest<PreviewHover | null>("dynare/effectivePreviewHover", {
      root_uri: session.root.toString(), snapshot_id: snapshot, position: point, definition_link: true,
    }, ...(token ? [token] : []));
  const hover = vscode.languages.registerHoverProvider({ language: "dynare", scheme: "dygnosis-effective" }, {
    async provideHover(document, position, token) {
      const session = sessions.get(document.uri.toString());
      const client = service.client;
      if (!session || session.document !== document || !session.hoverTextMatches || session.outdated || document.isClosed || document.version !== session.displayVersion ||
          session.instance !== service.currentInstance || !client || !record(session.result) ||
          typeof session.result.hover_snapshot_id !== "string" || !session.result.hover_snapshot_id ||
          typeof session.result.effective_text !== "string" || token.isCancellationRequested) return undefined;
      const offset = session.text.slice(0, session.text.length - session.result.effective_text.length).split("\n").length - 1;
      if (position.line < offset) return undefined;
      const current = hoverGuard(session), snapshot = session.result.hover_snapshot_id;
      const point = { line: position.line - offset, character: position.character };
      try {
        const result = await queryHover(client, session, snapshot, point, token);
        if (!current() || token.isCancellationRequested || !result) return undefined;
        const converted = client.protocol2CodeConverter.asHover(result);
        if (converted?.range && offset) converted.range = new vscode.Range(converted.range.start.line + offset, converted.range.start.character,
          converted.range.end.line + offset, converted.range.end.character);
        if (converted && location(result.definition)) {
          // The VS Code opener decodes the command query again after parsing the URI.
          const previewKey = Buffer.from(session.uri.toString()).toString("base64url");
          const args = encodeURIComponent(JSON.stringify([previewKey, snapshot, point]));
          const link = new vscode.MarkdownString(`[Definition](command:${openDefinitionCommand}?${args})`);
          link.isTrusted = { enabledCommands: [openDefinitionCommand] };
          converted.contents.push(link);
        }
        return converted;
      } catch { return undefined; }
    },
  });
  const openDefinition = vscode.commands.registerCommand(openDefinitionCommand, (previewKey: unknown, snapshot: unknown, point: unknown) => runCommand(service, async () => {
    const expired = (): never => refuse("The definition link is no longer available. Open or refresh macro expansion, then hover over the name again.");
    const session = typeof previewKey === "string" ? sessions.get(Buffer.from(previewKey, "base64url").toString("utf8")) : undefined;
    const client = service.client;
    if (!session || !client || !isPosition(point) || typeof snapshot !== "string" || !record(session.result) ||
        session.result.hover_snapshot_id !== snapshot || typeof session.result.revision !== "string") return expired();
    const revision = session.result.revision, valid = hoverGuard(session), editor = vscode.window.activeTextEditor;
    const current = (): boolean => valid() && vscode.window.activeTextEditor === editor &&
      vscode.window.visibleTextEditors.some(visible => visible.document === session.document);
    if (!current()) return expired();
    const target = (await queryHover(client, session, snapshot, point))?.definition;
    if (!current() || !record(target) || !location(target)) return expired();
    const targetVersion = target.document_version, targetUri = vscode.Uri.parse(target.uri);
    if (targetVersion !== null && (typeof targetVersion !== "number" || !Number.isSafeInteger(targetVersion))) return expired();
    const range = new vscode.Range(target.range.start.line, target.range.start.character, target.range.end.line, target.range.end.character);
    await service.openLocation(target, session.root, async document => {
      const version = document.version;
      const isCurrent = (): boolean => current() && !document.isClosed && document.version === version &&
        (targetVersion === null ? !document.isDirty : version === targetVersion);
      if (!isCurrent() || !sameSourceUri(document.uri, targetUri) || !document.validateRange(range).isEqual(range)) return expired();
      // Loading a disk-only include can invalidate the original semantic view.
      // Compare the model revision once the editor has loaded it.
      if (targetVersion === null && !(await service.revalidate(session.root, revision, session.instance))?.complete) return expired();
      if (!isCurrent()) return expired();
      return { isCurrent };
    }, { reuseOpen: true });
  }, () => !disposed));
  const command = vscode.commands.registerCommand("dygnosis.showEffectiveModel", async (argument?: unknown) => {
    const document = vscode.window.activeTextEditor?.document;
    // Editor menus pass a URI. Only our explicit snapshot argument supplies
    // an expected input; ordinary palette/menu actions keep their root picker.
    const expected = record(argument) && record(argument.info) && typeof argument.info.revision === "string" &&
      typeof argument.info.client_instance === "number" && typeof argument.version === "number" &&
      record(argument.document) && record(argument.root) ? argument as unknown as NavigationContext : undefined;
    if (disposed) return;
    let selectionWatcher: vscode.Disposable | undefined;
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
      let selectionChanged = false;
      selectionWatcher = service.onDidChangeAnalysis?.(event => { if (event.root === root.toString()) selectionChanged = true; });
      const validRoot = async (): Promise<boolean> => !selectionChanged && await valid() && service.currentInstance === instance &&
        (await service.rootForDocument(document))?.toString() === root.toString() && await valid() && service.currentInstance === instance;
      if (!await validRoot()) { expired(); }
      const result = await service.execute("dynare/showEffectiveModel", effectivePreviewArguments(service, root, true));
      if (!await validRoot()) { release(root, result, instance); expired(); }
      if (record(result) && result.success === false && typeof result.message === "string")
        refuse(`Macro expansion is unavailable: ${result.message}`);
      if (!record(result) || typeof result.effective_text !== "string") throw new Error("This engine cannot show macro expansion. Update dynare.serverPath or use the bundled binary.");
      appendMacroMessages(service.output, result);
      const uri = vscode.Uri.from({ scheme: "dygnosis-effective", path: `/${++sequence}/${previewName(root, result)}` });
      const rendered = previewText(result)!;
      text.set(uri.toString(), rendered);
      const preview = await vscode.workspace.openTextDocument(uri);
      if (!await validRoot()) { text.delete(uri.toString()); release(root, result, instance); expired(); }
      await vscode.languages.setTextDocumentLanguage(preview, "dynare");
      if (!await validRoot()) { text.delete(uri.toString()); release(root, result, instance); expired(); }
      const session: EffectivePreviewSession = { uri, root, document: preview, displayVersion: preview.version,
        hoverTextMatches: hoverTextMatches(preview.getText(), rendered), instance,
        result, text: rendered, generation: 0 };
      sessions.set(uri.toString(), session);
      created.fire(session);
      await vscode.window.showTextDocument(preview, { viewColumn: vscode.ViewColumn.Beside, preview: false });
    } catch (error) { if (!disposed) await explainFailure(service, error); }
    finally { selectionWatcher?.dispose(); }
  });
  const close = vscode.workspace.onDidCloseTextDocument(document => {
    if (document.uri.scheme !== "dygnosis-effective") return;
    const key = document.uri.toString(), session = sessions.get(key);
    if (session) { ++session.generation; sessions.delete(key); release(session.root, session.result, session.instance); closed.fire(session); }
    text.delete(key);
  });
  const disposable = vscode.Disposable.from(provider, outline, hover, openDefinition, command, close, new vscode.Disposable(() => {
    disposed = true;
    for (const session of sessions.values()) { ++session.generation; release(session.root, session.result, session.instance); closed.fire(session); }
    sessions.clear(); text.clear();
  }), created, closed, contentChanged);
  return {
    onDidCreate: created.event, onDidClose: closed.event,
    sessions: () => [...sessions.values()], get: uri => sessions.get(uri.toString()),
    releaseSnapshot: release,
    markOutdated(session, cause) {
      if (disposed || sessions.get(session.uri.toString()) !== session) return;
      if (session.outdated === cause) return;
      session.outdated = cause; ++session.generation;
      text.set(session.uri.toString(), `// OUTDATED EXPANSION — ${cause.replace(/\r?\n/g, " ")}\n// Refresh macro expansion before using this result.\n${session.text}`);
      contentChanged.fire(session.uri);
    },
    async replace(session, result, current = () => true, navigationProof) {
      const rendered = previewText(result);
      const valid = (): boolean => !disposed && sessions.get(session.uri.toString()) === session && !session.document.isClosed && current();
      if (!valid() || rendered === undefined) return undefined;
      const uri = vscode.Uri.from({ scheme: "dygnosis-effective", path: `/${++sequence}/${previewName(session.root, result)}` });
      text.set(uri.toString(), rendered);
      const document = await vscode.workspace.openTextDocument(uri);
      if (!valid()) { text.delete(uri.toString()); return undefined; }
      await vscode.languages.setTextDocumentLanguage(document, "dynare");
      if (!valid()) { text.delete(uri.toString()); return undefined; }
      const next: EffectivePreviewSession = { uri, root: session.root, document, displayVersion: document.version,
        hoverTextMatches: hoverTextMatches(document.getText(), rendered), instance: service.currentInstance, result, text: rendered, generation: 0, navigationProof };
      sessions.set(uri.toString(), next); created.fire(next);
      const visible = vscode.window.visibleTextEditors.find(editor => editor.document === session.document);
      if (visible) await vscode.window.showTextDocument(document, { viewColumn: visible.viewColumn, preview: false, preserveFocus: vscode.window.activeTextEditor !== visible });
      if (!valid()) {
        sessions.delete(uri.toString()); closed.fire(next); text.delete(uri.toString());
        return undefined;
      }
      ++session.generation; sessions.delete(session.uri.toString()); release(session.root, session.result, session.instance); closed.fire(session);
      const tabs = (vscode.window.tabGroups?.all ?? []).flatMap(group => group.tabs).filter(tab => {
        const input = tab.input as { uri?: vscode.Uri };
        return input.uri?.toString() === session.uri.toString();
      });
      if (tabs.length) await vscode.window.tabGroups.close(tabs, true);
      return next;
    },
    dispose: () => { disposable.dispose(); },
  };
}
