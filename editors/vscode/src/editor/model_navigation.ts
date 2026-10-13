import { refuse, runCommand } from "../client/refusal";
import * as vscode from "vscode";
import { DygnosisClient, isAnalysisDocument } from "../client/client";
import type { ModelSnapshot } from "../client/client";
import type { Equation, Location } from "../client/protocol";

export interface NavigationContext {
  document: vscode.TextDocument;
  version: number;
  root: vscode.Uri;
  info: ModelSnapshot;
  guard?: () => boolean;
}
export interface ModelBlockNavigationContext extends NavigationContext { blockIds: string[] }
interface EquationPick extends vscode.QuickPickItem { equation: Equation }
interface OwnerPick extends vscode.QuickPickItem { root: vscode.Uri; info: ModelSnapshot }
function sourceLabel(location: Location | null): string {
  if (!location) return "Source unavailable";
  const uri = vscode.Uri.parse(location.uri);
  return `${vscode.workspace.asRelativePath(uri)}:${String(location.range.start.line + 1)}`;
}
function scopeLabel(equation: Equation): string {
  return equation.scope === "aggregate" ? "Aggregate" : `Dimension ${equation.dimension ?? "(unnamed)"}`;
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
  guard: () => boolean = () => true): Promise<void> {
  const fresh = await validateNavigationContext(service, context, guard);
  const row = fresh?.equations.find(candidate => candidate.id === equation.id);
  if (!row) { refuse("The model changed. Choose the equation again."); }
  if (!row.location) { refuse("A verified written location is unavailable for this equation."); }
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
export async function browseModelEquations(service: DygnosisClient, context: ModelBlockNavigationContext, active: () => boolean = () => true): Promise<void> {
  await runCommand(service, () => browseEquations(service, context), active);
}
async function browseEquations(service: DygnosisClient, context: ModelBlockNavigationContext): Promise<void> {
  const info = await validateNavigationContext(service, context);
  if (!info) { refuse("The model changed. Choose the equation again."); }
  const blocks = context.blockIds.map(id => info.statements.find(row => row.id === id));
  if (!blocks.length || blocks.some(row => !row || row.name !== "model" || row.kind !== "block" ||
      !row.complete || row.native || !row.lens_anchor)) { refuse("The model block is no longer available. Wait for its CodeLens to refresh."); }
  const blockIds = new Set(context.blockIds);
  const equations = info.equations.filter(equation => blockIds.has(equation.block_id));
  if (!equations.length) { refuse("This model block has no surviving counted equations."); }
  const pick = await vscode.window.showQuickPick(equationPicks(equations),
    { placeHolder: "Jump to an equation before transformation", matchOnDescription: true, matchOnDetail: true });
  if (pick) await jumpEquation(service, context, pick.equation);
}

/** Equation navigation and owner selection over the client's shared model facts. */
export function registerModelNavigation(service: DygnosisClient): vscode.Disposable {
  let disposed = false;
  const currentDocument = (document: vscode.TextDocument, version: number, instance: number): boolean =>
    !disposed && !document.isClosed && vscode.window.activeTextEditor?.document === document &&
    document.version === version && service.currentInstance === instance;
  const acquire = async (): Promise<NavigationContext | undefined> => {
    const document = vscode.window.activeTextEditor?.document;
    if (disposed) return undefined;
    if (!document || document.isClosed || !isAnalysisDocument(document)) { refuse("Open a Dynare model or include before using equation navigation."); }
    const version = document.version;
    const root = await service.rootForDocument(document, true);
    if (!root) { refuse("Choose a known model owner before using equation navigation."); }
    const info = await service.modelInfo(root, document.uri);
    if (!info) { refuse("Model information is unavailable. Restart the server or open Dygnosis Output for details."); }
    if (!currentDocument(document, version, info.client_instance) || info.root_uri !== root.toString() ||
        info.document_uri !== document.uri.toString() || info.document_version !== version) { refuse("The document or model owner changed. Request equation navigation again."); }
    if (!info.complete) { refuse(`Equation navigation is unavailable because expansion is incomplete. ${info.incomplete_reasons?.map(reason => reason.message).join(" ") || info.message || "Resolve the reported model problems first."}`); }
    return { document, version, root, info };
  };
  const jumpToEquation = async (): Promise<void> => {
    const context = await acquire();
    if (!context) return;
    if (!context.info.equations.length) { refuse("This model has no counted equations."); }
    const pick = await vscode.window.showQuickPick(equationPicks(context.info.equations),
      { placeHolder: "Jump to an equation before transformation", matchOnDescription: true, matchOnDetail: true });
    if (pick) await jumpEquation(service, context, pick.equation, () => !disposed);
  };
  const chooseOwner = async (): Promise<void> => {
    const document = vscode.window.activeTextEditor?.document;
    if (disposed) return;
    if (!document || document.isClosed || !isAnalysisDocument(document)) { refuse("Open a Dynare include before choosing its model owner."); }
    const version = document.version;
    const owners = await service.ownerChoices(document);
    // Discovery can start the client. Pin its instance before reading owner facts.
    const instance = service.currentInstance;
    const current = (): boolean => currentDocument(document, version, instance);
    if (!current()) { if (!disposed) refuse("The document or server changed. Choose its model owner again."); return; }
    const picks: OwnerPick[] = [];
    for (const root of owners) {
      const info = await service.modelInfo(root, document.uri);
      if (!current()) { if (!disposed) refuse("The document or server changed. Choose its model owner again."); return; }
      if (info && info.root_uri === root.toString() && info.document_uri === document.uri.toString() &&
          info.document_version === version && info.client_instance === instance && info.owner_roots.includes(root.toString())) picks.push({ label: vscode.workspace.asRelativePath(root), description: root.fsPath, root, info });
    }
    if (picks.length === 0) { refuse("No known model owns this include. Open its model root first."); }
    const pick = await vscode.window.showQuickPick(picks, { placeHolder: "Choose the model that owns this include" });
    if (!pick) return;
    if (!currentDocument(document, version, pick.info.client_instance)) { if (!disposed) refuse("The document or server changed. Choose its model owner again."); return; }
    const fresh = await service.revalidate(pick.root, pick.info.revision, pick.info.client_instance, document.uri);
    if (!fresh || fresh.root_uri !== pick.root.toString() || fresh.document_uri !== document.uri.toString() ||
        fresh.document_version !== version || fresh.client_instance !== pick.info.client_instance || !fresh.owner_roots.includes(pick.root.toString()) ||
        !currentDocument(document, version, pick.info.client_instance)) {
      refuse("The model changed. Choose its owner again.");
    }
    service.selectOwner(document.uri, pick.root);
  };
  const action = (run: () => Promise<void>) => (): Promise<void | undefined> => runCommand(service, run, () => !disposed);
  const listeners = [
    vscode.commands.registerCommand("dygnosis.browseModelEquations", (context: ModelBlockNavigationContext) =>
      browseModelEquations(service, { ...context, guard: () => !disposed && (context.guard?.() ?? true) }, () => !disposed)),
    vscode.commands.registerCommand("dygnosis.jumpToEquation", action(jumpToEquation)),
    vscode.commands.registerCommand("dygnosis.chooseModelOwner", action(chooseOwner)),
  ];
  return new vscode.Disposable(() => {
    disposed = true;
    for (const listener of listeners) listener.dispose();
  });
}
