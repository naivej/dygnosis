import * as vscode from "vscode";
import { documentSelector, DygnosisClient, isAnalysisDocument, ModelSnapshot } from "./client";
import { sameWrittenLocation } from "./model_view";
import type { Declaration, Location, ModelInfo, Statement } from "./protocol";
import { booleanSetting, Log } from "./settings";

/** Exact manifest additions for Ext-launch integration. */
export const lensContributions = { configuration: { title: "Dygnosis: Actions", properties: {
  "dynare.codeLens.modelEquations": { type: "boolean", default: true, scope: "resource",
    description: "Browse surviving equations at each safely mapped model opener. Numbers are before transformation. Also respects editor.codeLens." },
  "dynare.codeLens.declarationReferences": { type: "boolean", default: false, scope: "resource",
    description: "Offer native Find All References at declaration lines. Several names open a symbol picker; no usage count is shown." },
  "dynare.codeLens.effectiveModel": { type: "boolean", default: false, scope: "resource",
    description: "Offer Show effective model at the first safely mapped model opener. Also respects editor.codeLens." },
} } };

export interface LensPreferences { modelEquations: boolean; declarationReferences: boolean; effectiveModel: boolean }
interface ModelGroup { anchor: Location; statements: Statement[]; count: number | null }
interface DeclarationGroup { anchor: Location; declarations: Declaration[] }
interface LensContext {
  document: vscode.TextDocument; version: number; root: vscode.Uri; info: ModelSnapshot;
  generation: number; preferences: string;
}
interface LensTarget { context: LensContext; kind: keyof LensPreferences; anchor: Location; ids: string[] }

export function lensPreferences(document: vscode.TextDocument, log: Log): LensPreferences {
  const native = vscode.workspace.getConfiguration("editor", { uri: document.uri, languageId: "dynare" }).get<unknown>("codeLens", true);
  if (typeof native !== "boolean") log("Invalid editor.codeLens; using true.");
  const visible = native !== false;
  return {
    modelEquations: visible && booleanSetting("codeLens.modelEquations", document.uri, true, log),
    declarationReferences: visible && booleanSetting("codeLens.declarationReferences", document.uri, false, log),
    effectiveModel: visible && booleanSetting("codeLens.effectiveModel", document.uri, false, log),
  };
}
function fingerprint(preferences: LensPreferences): string { return JSON.stringify(preferences); }
function enabled(preferences: LensPreferences): boolean { return Object.values(preferences).some(Boolean); }
function safeAnchor(anchor: Location | null, document: vscode.TextDocument): anchor is Location {
  if (!anchor || !sameWrittenLocation(anchor, { ...anchor, uri: document.uri.toString() })) return false;
  const { start, end } = anchor.range;
  return start.line === end.line && start.line < document.lineCount &&
    start.character < end.character && end.character <= document.lineAt(start.line).text.length;
}
function anchorKey(anchor: Location): string { return JSON.stringify(anchor.range); }
function asRange(anchor: Location): vscode.Range {
  const { start, end } = anchor.range;
  return new vscode.Range(start.line, start.character, end.line, end.character);
}
/** One browser per written opener, retaining all execution identities. */
function mappedModelGroups(info: ModelInfo, document: vscode.TextDocument): ModelGroup[] {
  if (!info.complete) return [];
  const groups = new Map<string, ModelGroup>();
  for (const statement of info.statements) {
    if (statement.name !== "model" || statement.kind !== "block" || !statement.complete || statement.native ||
        !safeAnchor(statement.lens_anchor, document)) continue;
    const key = anchorKey(statement.lens_anchor);
    const group = groups.get(key) ?? { anchor: statement.lens_anchor, statements: [], count: null };
    group.statements.push(statement);
    groups.set(key, group);
  }
  return [...groups.values()];
}
export function modelLensGroups(info: ModelInfo, document: vscode.TextDocument): ModelGroup[] {
  const counts = new Map<string, { count: number; mapped: boolean }>();
  for (const row of info.equations) {
    const count = counts.get(row.block_id) ?? { count: 0, mapped: true };
    ++count.count; count.mapped &&= row.location !== null;
    counts.set(row.block_id, count);
  }
  return mappedModelGroups(info, document).filter(group => {
    // A safe opener alone does not prove every equation's written location.
    const safe = group.statements.every(statement => {
      const rows = counts.get(statement.id) ?? { count: 0, mapped: true };
      return statement.equation_count !== null && statement.equation_count === rows.count && rows.mapped;
    });
    if (safe && group.statements.length === 1) group.count = group.statements[0].equation_count;
    return safe;
  });
}
/** One reference action per written line; macro copies keep symbol choices. */
export function declarationLensGroups(info: ModelInfo, document: vscode.TextDocument): DeclarationGroup[] {
  if (!info.complete) return [];
  const groups = new Map<number, DeclarationGroup>();
  const declarationsByStatement = new Map<string, Declaration[]>();
  for (const declaration of info.declarations) {
    if (!safeAnchor(declaration.location, document)) continue;
    const rows = declarationsByStatement.get(declaration.statement_id) ?? [];
    rows.push(declaration); declarationsByStatement.set(declaration.statement_id, rows);
  }
  for (const statement of info.statements) {
    if (statement.kind !== "declaration" || !statement.complete || statement.native || !safeAnchor(statement.lens_anchor, document)) continue;
    const declarations = declarationsByStatement.get(statement.id) ?? [];
    if (!declarations.length) continue;
    const line = statement.lens_anchor.range.start.line;
    const group = groups.get(line) ?? { anchor: statement.lens_anchor, declarations: [] };
    for (const declaration of declarations) {
      if (!group.declarations.some(row => row.name === declaration.name && row.location && declaration.location &&
          sameWrittenLocation(row.location, declaration.location))) group.declarations.push(declaration);
    }
    groups.set(line, group);
  }
  return [...groups.values()];
}

/** Native CodeLens adapter; all analysis comes from the client's shared cache. */
export function registerLenses(service: DygnosisClient): vscode.Disposable {
  const changed = new vscode.EventEmitter<void>();
  let generation = 0, disposed = false;
  let accepted = new WeakSet<LensTarget>();
  const requests = new WeakMap<vscode.TextDocument, number>();
  const visible = (document: vscode.TextDocument): boolean =>
    vscode.window.visibleTextEditors.some(editor => editor.document === document);
  const contextCurrent = (context: LensContext, active = false, pendingLens = true): boolean => !disposed && (!pendingLens || context.generation === generation) &&
    !context.document.isClosed && isAnalysisDocument(context.document) && visible(context.document) &&
    (!active || vscode.window.activeTextEditor?.document === context.document) && context.document.version === context.version &&
    context.info.client_instance === service.currentInstance && fingerprint(lensPreferences(context.document, service.log)) === context.preferences;
  const validSnapshot = (context: LensContext, info: ModelSnapshot | undefined): info is ModelSnapshot =>
    !!info?.complete && info.root_uri === context.root.toString() && info.document_uri === context.document.uri.toString() &&
    info.document_version === context.version && info.client_instance === context.info.client_instance;
  const freshContext = async (target: LensTarget, ongoing = false): Promise<ModelSnapshot | undefined> => {
    const { context } = target;
    const current = (): boolean => contextCurrent(context, true, !ongoing);
    if ((!ongoing && !accepted.has(target)) || !current() || !lensPreferences(context.document, service.log)[target.kind]) return undefined;
    const root = await service.rootForDocument(context.document);
    if (root?.toString() !== context.root.toString() || !current()) return undefined;
    const info = await service.revalidate(context.root, context.info.revision, context.info.client_instance, context.document.uri);
    if (!validSnapshot(context, info) || !current()) return undefined;
    const selected = await service.rootForDocument(context.document);
    return selected?.toString() === context.root.toString() && current() ? info : undefined;
  };
  const refresh = (): void => { ++generation; accepted = new WeakSet(); changed.fire(); };
  const provider: vscode.CodeLensProvider = {
    onDidChangeCodeLenses: changed.event,
    async provideCodeLenses(document, token) {
      const request = (requests.get(document) ?? 0) + 1;
      requests.set(document, request);
      if (disposed || token.isCancellationRequested || document.isClosed || !isAnalysisDocument(document) || !visible(document)) return [];
      const preferences = lensPreferences(document, service.log);
      if (!enabled(preferences)) return [];
      const version = document.version, currentGeneration = generation, signature = fingerprint(preferences);
      const current = (): boolean => !disposed && !token.isCancellationRequested && generation === currentGeneration &&
        requests.get(document) === request && visible(document) && !document.isClosed && isAnalysisDocument(document) &&
        document.version === version && fingerprint(lensPreferences(document, service.log)) === signature;
      try {
        const root = await service.rootForDocument(document);
        if (!root || !current()) return [];
        const info = await service.modelInfo(root, document.uri);
        if (!info) return [];
        const context: LensContext = { document, version, root, info, generation: currentGeneration, preferences: signature };
        if (!current() || !validSnapshot(context, info) || info.client_instance !== service.currentInstance) return [];
        const selectedRoot = await service.rootForDocument(document);
        if (selectedRoot?.toString() !== root.toString() || !current()) return [];
        const result: vscode.CodeLens[] = [];
        const add = (kind: keyof LensPreferences, anchor: Location, ids: string[], title: string, command: string): void => {
          const target: LensTarget = { context, kind, anchor, ids };
          accepted.add(target);
          result.push(new vscode.CodeLens(asRange(anchor), { title, command, arguments: [target] }));
        };
        const models = modelLensGroups(info, document);
        if (preferences.modelEquations) for (const group of models) add("modelEquations", group.anchor,
          group.statements.map(row => row.id), group.count !== null ? `Browse ${String(group.count)} equations`
            : `Browse equations (${String(group.statements.length)} occurrences)`, "dygnosis.browseLensEquations");
        if (preferences.declarationReferences) for (const group of declarationLensGroups(info, document))
          add("declarationReferences", group.anchor, group.declarations.map(row => row.id), "Find references", "dygnosis.findLensReferences");
        if (preferences.effectiveModel && safeAnchor(info.first_model_anchor, document)) {
          const first = mappedModelGroups(info, document).find(group => sameWrittenLocation(group.anchor, info.first_model_anchor!));
          if (first) add("effectiveModel", first.anchor, first.statements.map(row => row.id), "Show effective model", "dygnosis.showLensEffectiveModel");
        }
        return current() ? result : [];
      } catch (error) { if (current()) service.log(String(error)); return []; }
    },
  };
  const unavailable = (): void => { void vscode.window.showInformationMessage("The model changed. Wait for the CodeLens to refresh and try again."); };
  const action = (run: (target: LensTarget, info: ModelSnapshot) => Promise<void>) => async (target: LensTarget): Promise<void> => {
    if (!target || !accepted.has(target)) return;
    try {
      const info = await freshContext(target);
      if (!info) { unavailable(); return; }
      await run(target, info);
    } catch (error) { service.log(String(error)); void vscode.window.showInformationMessage("This model action is unavailable. Open Dygnosis Output for details."); }
  };
  const listeners = [
    vscode.languages.registerCodeLensProvider(documentSelector, provider),
    vscode.commands.registerCommand("dygnosis.browseLensEquations", action(async (target, info) => {
      const group = modelLensGroups(info, target.context.document).find(row => sameWrittenLocation(row.anchor, target.anchor));
      if (!group || group.statements.map(row => row.id).join("\n") !== target.ids.join("\n")) { unavailable(); return; }
      await vscode.commands.executeCommand("dygnosis.browseModelEquations", {
        document: target.context.document, version: target.context.version, root: target.context.root, info,
        // Opening an ordinary source overlay can refresh UI without changing
        // its input revision. The navigation helper checks that revision again.
        blockIds: target.ids, guard: () => contextCurrent(target.context, true, false),
      });
    })),
    vscode.commands.registerCommand("dygnosis.findLensReferences", action(async (target, info) => {
      const group = declarationLensGroups(info, target.context.document).find(row => row.anchor.range.start.line === target.anchor.range.start.line);
      if (!group || group.declarations.map(row => row.id).join("\n") !== target.ids.join("\n")) { unavailable(); return; }
      const declaration = group.declarations.length === 1 ? group.declarations[0] : (await vscode.window.showQuickPick(
        group.declarations.map(row => ({ label: row.name, description: `${row.final_kind ?? row.written_kind}${row.dimension ? ` · ${row.dimension}` : ""}`,
          detail: row.long_name ?? undefined, declaration: row })), { placeHolder: "Choose the symbol for Find All References", matchOnDescription: true }))?.declaration;
      if (!declaration) return;
      const fresh = await freshContext(target, true);
      const row = fresh?.declarations.find(candidate => candidate.id === declaration.id);
      if (!row?.location || !declaration.location || !sameWrittenLocation(row.location, declaration.location) ||
          !safeAnchor(row.location, target.context.document)) { unavailable(); return; }
      const editor = vscode.window.activeTextEditor;
      if (!editor || editor.document !== target.context.document || !contextCurrent(target.context, true, false)) return;
      editor.selection = new vscode.Selection(row.location.range.start.line, row.location.range.start.character,
        row.location.range.start.line, row.location.range.start.character);
      await vscode.commands.executeCommand("references-view.findReferences");
    })),
    vscode.commands.registerCommand("dygnosis.showLensEffectiveModel", action(async (target, info) => {
      if (!info.first_model_anchor || !sameWrittenLocation(info.first_model_anchor, target.anchor)) { unavailable(); return; }
      await vscode.commands.executeCommand("dygnosis.showEffectiveModel", {
        document: target.context.document, version: target.context.version, root: target.context.root, info,
        guard: () => contextCurrent(target.context, true, false),
      });
    })),
    vscode.window.onDidChangeVisibleTextEditors(refresh),
    vscode.window.onDidChangeActiveTextEditor(refresh),
    vscode.workspace.onDidChangeTextDocument(event => { if (visible(event.document)) refresh(); }),
    vscode.workspace.onDidOpenTextDocument(document => { if (visible(document)) refresh(); }),
    vscode.workspace.onDidCloseTextDocument(refresh),
    vscode.workspace.onDidChangeConfiguration(event => {
      if (event.affectsConfiguration("dynare.codeLens") || event.affectsConfiguration("editor.codeLens")) refresh();
    }),
    service.onDidChange(refresh),
  ];
  return new vscode.Disposable(() => {
    disposed = true; refresh();
    for (const listener of listeners) listener.dispose();
    changed.dispose();
  });
}
