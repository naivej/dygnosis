import * as vscode from "vscode";
import { DygnosisClient, isAnalysisDocument } from "./client";
import type { ModelSnapshot } from "./client";
import type { Dimension, Equation, Location, ModelInfo, RelatedFile } from "./protocol";
import { listSetting } from "./settings";

export const modelViewSections = ["counts", "timing", "dimensions", "relatedFiles"];
export interface NavigationContext {
  document: vscode.TextDocument;
  version: number;
  root: vscode.Uri;
  info: ModelSnapshot;
  guard?: () => boolean;
}
export interface ModelBlockNavigationContext extends NavigationContext { blockIds: string[] }
interface EquationScope { key: string; label: string; equations: Equation[] }
interface EquationPick extends vscode.QuickPickItem { equation: Equation }
interface OwnerPick extends vscode.QuickPickItem { root: vscode.Uri; info: ModelSnapshot }
interface RelatedTarget { context: NavigationContext; file: RelatedFile }

class ModelItem extends vscode.TreeItem {
  constructor(id: string, label: string, icon: string, readonly children: ModelItem[] = []) {
    super(label, children.length ? vscode.TreeItemCollapsibleState.Collapsed : vscode.TreeItemCollapsibleState.None);
    this.id = id;
    this.iconPath = new vscode.ThemeIcon(icon);
  }
}
class ModelTree implements vscode.TreeDataProvider<ModelItem>, vscode.Disposable {
  private readonly changed = new vscode.EventEmitter<ModelItem | undefined>();
  readonly onDidChangeTreeData = this.changed.event;
  rows: ModelItem[] = [];
  getTreeItem(item: ModelItem): vscode.TreeItem { return item; }
  getChildren(item?: ModelItem): ModelItem[] { return item?.children ?? this.rows; }
  replace(rows: ModelItem[]): void { this.rows = rows; this.changed.fire(undefined); }
  dispose(): void { this.rows = []; this.changed.dispose(); }
}
function factItem(id: string, label: string, value: number, icon: string): ModelItem {
  const item = new ModelItem(id, label, icon);
  item.description = String(value);
  item.accessibilityInformation = { label: `${label}: ${String(value)}` };
  return item;
}
function countItems(id: string, info: Pick<Dimension, "n_endogenous" | "n_exogenous" | "n_parameters" | "n_equations">): ModelItem[] {
  return [
    factItem(`${id}:endogenous`, "Endogenous", info.n_endogenous, "symbol-variable"),
    factItem(`${id}:exogenous`, "Exogenous", info.n_exogenous, "symbol-field"),
    factItem(`${id}:parameters`, "Parameters", info.n_parameters, "symbol-constant"),
    factItem(`${id}:equations`, "Equations", info.n_equations, "list-ordered"),
  ];
}
function timingItems(id: string, info: Pick<ModelInfo, "static" | "predetermined" | "forward_looking" | "mixed">): ModelItem[] {
  return ([
    ["static", "Static", info.static ?? []],
    ["predetermined", "Predetermined", info.predetermined ?? []],
    ["forward_looking", "Forward-looking", info.forward_looking ?? []],
    ["mixed", "Mixed", info.mixed ?? []],
  ] as const).map(([key, label, names]) => {
    const item = new ModelItem(`${id}:${key}`, label, "symbol-variable",
      names.map((name, index) => new ModelItem(`${id}:${key}:${String(index)}`, name, "symbol-variable")));
    item.description = String(names.length);
    item.accessibilityInformation = { label: `${label}: ${String(names.length)} ${names.length === 1 ? "variable" : "variables"}` };
    return item;
  });
}
function sourceLabel(location: Location | null): string {
  if (!location) return "Source unavailable";
  const uri = vscode.Uri.parse(location.uri);
  return `${vscode.workspace.asRelativePath(uri)}:${String(location.range.start.line + 1)}`;
}
function scopeLabel(equation: Equation): string {
  return equation.scope === "aggregate" ? "Aggregate" : `Dimension ${equation.dimension ?? "(unnamed)"}`;
}
function equationScopes(equations: Equation[]): EquationScope[] {
  const scopes = new Map<string, EquationScope>();
  for (const equation of equations) {
    const key = JSON.stringify([equation.scope, equation.dimension]);
    const scope = scopes.get(key) ?? { key, label: scopeLabel(equation), equations: [] };
    scope.equations.push(equation);
    scopes.set(key, scope);
  }
  return [...scopes.values()];
}
function equationPicks(equations: Equation[]): EquationPick[] {
  return equations.map(equation => {
    const origins = equation.origin_frames.map(frame => frame.variable && frame.value !== null
      ? `${frame.variable}=${frame.value}` : frame.kind).join(", ");
    return {
      label: `${String(equation.number)} · ${equation.name || "Unnamed equation"}`,
      description: `${scopeLabel(equation)} · ${sourceLabel(equation.location)}`,
      detail: `${origins ? `${origins} · ` : ""}${equation.text}`,
      equation,
    };
  });
}
function rowsFor(context: NavigationContext, sections: string[]): ModelItem[] {
  const { info } = context;
  return sections.map(section => {
    if (section === "counts") return new ModelItem("counts", "Aggregate counts", "graph", countItems("counts", {
      n_endogenous: info.n_endogenous!, n_exogenous: info.n_exogenous!,
      n_parameters: info.n_parameters!, n_equations: info.n_equations!,
    }));
    if (section === "timing") return new ModelItem("timing", "Aggregate timing", "history", timingItems("timing", info));
    if (section === "dimensions") return new ModelItem("dimensions", "Heterogeneity dimensions", "layers",
      (info.heterogeneity_dimensions ?? []).map((dimension, index) => {
        const id = `dimension:${String(index)}`;
        return new ModelItem(id, dimension.dimension, "layers", [
          new ModelItem(`${id}:counts`, "Counts", "graph", countItems(`${id}:counts`, dimension)),
          new ModelItem(`${id}:timing`, "Timing", "history", timingItems(`${id}:timing`, dimension)),
        ]);
      }));
    return new ModelItem("relatedFiles", "Related files", "files", info.related_files.map((file, index) => {
      const item = new ModelItem(`related:${String(index)}`, file.filename, file.resolved ? "file" : "question");
      item.description = `${file.kind}${file.resolved ? "" : " · unresolved"}`;
      item.tooltip = file.path ?? "This related file could not be resolved.";
      if (file.resolved && file.path) {
        item.resourceUri = vscode.Uri.file(file.path);
        item.command = { command: "dygnosis.openRelatedFile", title: "Open related file", arguments: [{ context, file } satisfies RelatedTarget] };
      }
      return item;
    }));
  });
}
function numberError(value: string): string | undefined {
  return /^[1-9]\d*$/.test(value) && Number.isSafeInteger(Number(value))
    ? undefined : "Enter a positive whole equation number.";
}
/** Native file identity may change URI spelling when an unopened file loads. */
export function sameWrittenLocation(left: Location, right: Location, platform: NodeJS.Platform = process.platform): boolean {
  const leftUri = vscode.Uri.parse(left.uri), rightUri = vscode.Uri.parse(right.uri);
  if (leftUri.scheme !== rightUri.scheme) return false;
  const identity = (uri: vscode.Uri): string => uri.scheme === "file" && platform === "win32"
    ? uri.fsPath.replaceAll("/", "\\").toLowerCase() : uri.toString();
  return identity(leftUri) === identity(rightUri) &&
    left.range.start.line === right.range.start.line && left.range.start.character === right.range.start.character &&
    left.range.end.line === right.range.end.line && left.range.end.character === right.range.end.character;
}

function navigationCurrent(service: DygnosisClient, context: NavigationContext, guard: () => boolean): boolean {
  return guard() && (context.guard?.() ?? true) && !context.document.isClosed &&
    vscode.window.activeTextEditor?.document === context.document && context.document.version === context.version &&
    service.currentInstance === context.info.client_instance;
}
/** Reused by ordinary equation commands and the CodeLens block browser. */
export async function validateNavigationContext(service: DygnosisClient, context: NavigationContext,
  guard: () => boolean = () => true): Promise<ModelSnapshot | undefined> {
  const { document, version, root, info } = context;
  const current = (): boolean => navigationCurrent(service, context, guard);
  if (!current()) return undefined;
  const selectedRoot = await service.rootForDocument(document);
  if (selectedRoot?.toString() !== root.toString() || !current()) return undefined;
  const fresh = await service.revalidate(root, info.revision, info.client_instance, document.uri);
  if (!fresh?.complete || fresh.root_uri !== root.toString() || fresh.document_uri !== document.uri.toString() ||
      fresh.document_version !== version || fresh.client_instance !== info.client_instance || !current()) return undefined;
  const currentRoot = await service.rootForDocument(document);
  return currentRoot?.toString() === root.toString() && current() ? fresh : undefined;
}
async function jumpEquation(service: DygnosisClient, context: NavigationContext, equation: Equation,
  message: (text: string) => void, guard: () => boolean = () => true): Promise<void> {
  const fresh = await validateNavigationContext(service, context, guard);
  const row = fresh?.equations.find(candidate => candidate.id === equation.id);
  if (!row) { message("The model changed. Choose the equation again."); return; }
  if (!row.location) { message("A verified written location is unavailable for this equation."); return; }
  const target = row.location;
  await service.openLocation(target, context.root, async loadedDocument => {
    const loadedVersion = loadedDocument.version;
    const afterLoad = await validateNavigationContext(service, context, guard);
    const currentRow = afterLoad?.equations.find(candidate => candidate.id === row.id);
    return !loadedDocument.isClosed && loadedDocument.version === loadedVersion &&
      !!currentRow?.location && sameWrittenLocation(currentRow.location, target);
  });
}
/** Browse only the explicit root and block occurrences from a validated lens. */
export async function browseModelEquations(service: DygnosisClient, context: ModelBlockNavigationContext): Promise<void> {
  const message = (text: string): void => { void vscode.window.showInformationMessage(text); };
  const info = await validateNavigationContext(service, context);
  if (!info) { message("The model changed. Choose the equation again."); return; }
  const blocks = context.blockIds.map(id => info.statements.find(row => row.id === id));
  if (!blocks.length || blocks.some(row => !row || row.name !== "model" || row.kind !== "block" ||
      !row.complete || row.native || !row.lens_anchor)) return;
  const picks = blocks.map((block, index) => {
    const row = block!;
    const equations = info.equations.filter(equation => equation.block_id === row.id);
    const origins = row.origin_frames.map(frame => frame.variable && frame.value !== null
      ? `${frame.variable}=${frame.value}` : frame.kind).join(", ");
    return { label: row.dimension ? `Dimension ${row.dimension}` : "Aggregate",
      description: `${String(equations.length)} equations · ${sourceLabel(row.lens_anchor)}`,
      detail: `${origins ? `${origins} · ` : ""}Expansion ${index + 1}`, block: row, equations };
  });
  const chosen = picks.length === 1 ? picks[0] : await vscode.window.showQuickPick(picks,
    { placeHolder: "Choose the model block occurrence", matchOnDescription: true, matchOnDetail: true });
  if (!chosen || !await validateNavigationContext(service, context)) return;
  const scopes = equationScopes(chosen.equations);
  if (!scopes.length) { message("This model block has no surviving counted equations."); return; }
  const scope = scopes.length === 1 ? scopes[0] : (await vscode.window.showQuickPick(scopes.map(row => ({ label: row.label, scope: row })),
    { placeHolder: "Choose the equation scope" }))?.scope;
  if (!scope || !await validateNavigationContext(service, context)) return;
  const pick = await vscode.window.showQuickPick(equationPicks(scope.equations),
    { placeHolder: "Browse equations before transformation", matchOnDescription: true, matchOnDetail: true });
  if (pick) await jumpEquation(service, context, pick.equation, message);
}

/** Native presentation and navigation over the client's shared model facts. */
export function registerModelView(service: DygnosisClient): vscode.Disposable {
  const tree = new ModelTree();
  const view = vscode.window.createTreeView("dygnosis.model", { treeDataProvider: tree, showCollapseAll: true });
  let generation = 0;
  let disposed = false;
  let currentContext: NavigationContext | undefined;
  const sectionsFor = (document: vscode.TextDocument): string[] =>
    listSetting("modelView.sections", document.uri, modelViewSections, modelViewSections, service.log);
  const currentDocument = (document: vscode.TextDocument, version: number, instance: number): boolean =>
    !disposed && !document.isClosed && vscode.window.activeTextEditor?.document === document &&
    document.version === version && service.currentInstance === instance;
  const contextKeys = (navigation: boolean, owners: boolean): void => {
    void vscode.commands.executeCommand("setContext", "dygnosis.equationNavigation", navigation);
    void vscode.commands.executeCommand("setContext", "dygnosis.includeOwners", owners);
  };
  const clear = (message?: string): void => {
    currentContext = undefined;
    tree.replace([]);
    view.description = undefined;
    view.message = message;
    contextKeys(false, false);
  };
  const refresh = (): void => {
    const request = ++generation;
    clear();
    const document = vscode.window.activeTextEditor?.document;
    if (disposed || !document || document.isClosed || !isAnalysisDocument(document)) {
      view.message = "Open a Dynare model or an include with a known owner.";
      return;
    }
    const sections = sectionsFor(document);
    if (sections.length === 0 || !view.visible) return;
    const version = document.version;
    const current = (): boolean => !disposed && request === generation && view.visible &&
      vscode.window.activeTextEditor?.document === document && !document.isClosed &&
      document.version === version && sectionsFor(document).join("\n") === sections.join("\n");
    view.message = "Updating model information…";
    void (async () => {
      const root = await service.rootForDocument(document);
      if (!current()) return;
      const owners = service.knownOwners(document.uri);
      contextKeys(false, owners.length > 0);
      if (!root) {
        view.message = owners.length > 0 ? "Choose the model that owns this include." : "No model root is available for this file.";
        return;
      }
      const info = await service.modelInfo(root, document.uri);
      if (!current()) return;
      if (!info || !currentDocument(document, version, info.client_instance) || info.root_uri !== root.toString() ||
          info.document_uri !== document.uri.toString() || info.document_version !== version) {
        clear(service.supportsModelInfo ? "Model information is unavailable. Open Dygnosis Output for details."
          : "Model information is unavailable. Restart the server or use the bundled binary in Settings.");
        return;
      }
      view.description = vscode.workspace.asRelativePath(root);
      contextKeys(info.complete && info.equations.length > 0, service.knownOwners(document.uri).length > 0);
      if (!info.complete) {
        view.message = `${info.message ?? "Model expansion is incomplete."} Counts and equation navigation are unavailable.`;
        return;
      }
      currentContext = { document, version, root, info };
      tree.replace(rowsFor(currentContext, sections));
      view.message = undefined;
    })().catch((error: unknown) => {
      if (current()) { service.log(String(error)); clear("Model information is unavailable. Open Dygnosis Output for details."); }
    });
  };
  const message = (text: string): void => { void vscode.window.showInformationMessage(text); };
  const acquire = async (): Promise<NavigationContext | undefined> => {
    const document = vscode.window.activeTextEditor?.document;
    if (disposed || !document || document.isClosed || !isAnalysisDocument(document)) return undefined;
    const version = document.version;
    const root = await service.rootForDocument(document, true);
    if (!root) { message("Choose a known model owner before using equation navigation."); return undefined; }
    const info = await service.modelInfo(root, document.uri);
    if (!info) { message("Model information is unavailable. Restart the server or open Dygnosis Output for details."); return undefined; }
    if (!currentDocument(document, version, info.client_instance) || info.root_uri !== root.toString() ||
        info.document_uri !== document.uri.toString() || info.document_version !== version) return undefined;
    if (!info.complete) { message("Model expansion is incomplete. Equation navigation is unavailable."); return undefined; }
    return { document, version, root, info };
  };
  const validated = (context: NavigationContext): Promise<ModelSnapshot | undefined> => validateNavigationContext(service, context, () => !disposed);
  const jump = (context: NavigationContext, equation: Equation): Promise<void> => jumpEquation(service, context, equation, message, () => !disposed);
  const goToEquation = async (): Promise<void> => {
    const context = await acquire();
    if (!context) return;
    const scopes = equationScopes(context.info.equations);
    if (scopes.length === 0) { message("This model has no counted equations."); return; }
    const scope = scopes.length === 1 ? scopes[0] : (await vscode.window.showQuickPick(scopes.map(scope => ({ label: scope.label, scope })),
      { placeHolder: "Choose the equation scope" }))?.scope;
    if (!scope) return;
    const input = await vscode.window.showInputBox({
      prompt: `Equation number in ${scope.label} before transformation`,
      placeHolder: "Positive whole equation number", validateInput: numberError,
    });
    if (input === undefined || numberError(input)) return;
    const matches = scope.equations.filter(equation => equation.number === Number(input));
    if (matches.length === 0) { message(`Equation ${input} is unavailable in ${scope.label}.`); return; }
    const equation = matches.length === 1 ? matches[0] : (await vscode.window.showQuickPick(equationPicks(matches),
      { placeHolder: "Choose the equation occurrence", matchOnDescription: true, matchOnDetail: true }))?.equation;
    if (equation) await jump(context, equation);
  };
  const jumpNamedEquation = async (): Promise<void> => {
    const context = await acquire();
    if (!context) return;
    const named = context.info.equations.filter(equation => equation.name.trim().length > 0);
    if (named.length === 0) { message("This model has no named counted equations."); return; }
    const pick = await vscode.window.showQuickPick(equationPicks(named),
      { placeHolder: "Search named equations", matchOnDescription: true, matchOnDetail: true });
    if (pick) await jump(context, pick.equation);
  };
  const chooseOwner = async (): Promise<void> => {
    const document = vscode.window.activeTextEditor?.document;
    if (!document || document.isClosed || !isAnalysisDocument(document)) return;
    const version = document.version;
    const owners = await service.ownerChoices(document);
    const picks: OwnerPick[] = [];
    for (const root of owners) {
      const info = await service.modelInfo(root, document.uri);
      if (info && info.owner_roots.includes(root.toString())) picks.push({ label: vscode.workspace.asRelativePath(root), description: root.fsPath, root, info });
    }
    if (picks.length === 0) { message("No known model owns this include. Open its model root first."); return; }
    const pick = await vscode.window.showQuickPick(picks, { placeHolder: "Choose the model that owns this include" });
    if (!pick || !currentDocument(document, version, pick.info.client_instance)) return;
    const fresh = await service.revalidate(pick.root, pick.info.revision, pick.info.client_instance, document.uri);
    if (!fresh || fresh.document_version !== version || !fresh.owner_roots.includes(pick.root.toString()) ||
        !currentDocument(document, version, pick.info.client_instance)) {
      message("The model changed. Choose its owner again."); return;
    }
    service.selectOwner(document.uri, pick.root);
  };
  const openRelated = async (target: RelatedTarget): Promise<void> => {
    if (!target || target.context !== currentContext) return;
    const fresh = await validated(target.context);
    const file = fresh?.related_files.find(row => row.resolved && row.kind === target.file.kind &&
      row.filename === target.file.filename && row.path === target.file.path);
    if (!file?.path) { message("The model changed or this related file is unavailable. Refresh the model view."); return; }
    const uri = vscode.Uri.file(file.path);
    const opened = await vscode.workspace.openTextDocument(uri);
    if (target.context !== currentContext || !currentDocument(target.context.document, target.context.version, target.context.info.client_instance)) return;
    service.selectOwner(uri, target.context.root);
    await vscode.window.showTextDocument(opened);
  };
  const action = (run: () => Promise<void>): (() => Promise<void>) => async () => {
    try { await run(); } catch (error) { service.log(String(error)); message("This model action is unavailable. Open Dygnosis Output for details."); }
  };
  const listeners = [
    vscode.commands.registerCommand("dygnosis.browseModelEquations", (context: ModelBlockNavigationContext) =>
      action(() => browseModelEquations(service, { ...context, guard: () => !disposed && (context.guard?.() ?? true) }))()),
    vscode.commands.registerCommand("dygnosis.goToEquation", action(goToEquation)),
    vscode.commands.registerCommand("dygnosis.jumpToNamedEquation", action(jumpNamedEquation)),
    vscode.commands.registerCommand("dygnosis.chooseModelOwner", action(chooseOwner)),
    vscode.commands.registerCommand("dygnosis.refreshModelView", () => service.invalidate()),
    vscode.commands.registerCommand("dygnosis.openRelatedFile", (target: RelatedTarget) => action(() => openRelated(target))()),
    vscode.window.onDidChangeActiveTextEditor(refresh),
    vscode.workspace.onDidChangeTextDocument(event => { if (event.document === vscode.window.activeTextEditor?.document) refresh(); }),
    vscode.workspace.onDidCloseTextDocument(document => { if (document === vscode.window.activeTextEditor?.document) refresh(); }),
    vscode.workspace.onDidChangeConfiguration(event => {
      if (event.affectsConfiguration("dynare.modelView", vscode.window.activeTextEditor?.document.uri)) refresh();
    }),
    view.onDidChangeVisibility(refresh),
    service.onDidChange(refresh),
  ];
  refresh();
  return new vscode.Disposable(() => {
    disposed = true;
    ++generation;
    for (const listener of listeners) listener.dispose();
    clear();
    tree.dispose();
    view.dispose();
  });
}
