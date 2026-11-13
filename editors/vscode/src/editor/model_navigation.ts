import { refuse, runCommand } from "../client/refusal";
import * as vscode from "vscode";
import { DygnosisClient, isAnalysisDocument } from "../client/client";
import type { ModelSnapshot } from "../client/client";
import type { Equation, Location, Range } from "../client/protocol";
import { fileLabel } from "../client/failure";
import { captureSelection, commandEditor, containsSelection } from "../client/editor_target";
import { displayPath, sameSourceUri } from "../client/source_uri";

export interface NavigationContext {
  document: vscode.TextDocument;
  version: number;
  root: vscode.Uri;
  info: ModelSnapshot;
  guard?: () => boolean;
}
interface EquationPick extends vscode.QuickPickItem { equation: Equation }
interface OwnerPick extends vscode.QuickPickItem { root: vscode.Uri; info: ModelSnapshot }
function sourceLabel(location: Location | null): string {
  if (!location) return "No single source location";
  const uri = vscode.Uri.parse(location.uri);
  return `${displayPath(vscode.workspace.asRelativePath(uri))}:${String(location.range.start.line + 1)}`;
}
function scopeLabel(equation: Equation): string {
  return equation.scope === "aggregate" ? "Aggregate" : `Dimension ${equation.dimension ?? "(unnamed)"}`;
}
function equationPicks(context: NavigationContext, selection?: Range): EquationPick[] {
  const { document, info } = context;
  const uri = document.uri.toString();
  const local: Equation[] = [], other: Equation[] = [];
  let block: string | undefined;
  let ambiguousBlock = false;
  for (const equation of info.equations) {
    const inFile = equation.segments.some(segment => segment.uri === uri);
    (inFile ? local : other).push(equation);
    if (selection && inFile && equation.segments.some(segment => segment.uri === uri && containsSelection(segment.range, selection))) {
      if (block !== undefined && block !== equation.block_id) ambiguousBlock = true;
      block = equation.block_id;
    }
  }
  if (block === undefined && selection) {
    const blocks = info.statements.filter(row => row.kind === "block" && row.segments.some(segment =>
      segment.uri === uri && containsSelection(segment.range, selection)));
    if (blocks.length === 1) block = blocks[0].id;
  }
  if (ambiguousBlock) block = undefined;
  const ordered = block === undefined ? local : [...local.filter(row => row.block_id === block), ...local.filter(row => row.block_id !== block)];
  return [...ordered, ...other].map(equation => {
    const origins = equation.origin_frames.map(frame => frame.variable && frame.value !== null
      ? `${frame.variable}=${frame.value}` : ({ override_definition: "Definition override", definition_override: "Definition override", branch: "Branch preview", include: "Include", for: "Macro loop" }[frame.kind] ?? frame.kind.replaceAll("_", " "))).join(", ");
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
/** Reused by equation navigation and macro expansion previews. */
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
  const label = `Equation ${equation.number}${equation.name ? ` [${equation.name}]` : ""} in ${fileLabel(context.root.toString())}`;
  if (!row) { refuse(`${label} is no longer current. Choose the equation again.`); }
  if (!row.location) { refuse(`${label} has no single verified source location: ${row.segments.length > 1 ? `it spans ${row.segments.length} written regions` : row.segments.length === 0 ? "no written region was retained" : "the captured region is not a verified jump target"}. Open macro expansion to inspect its origins.`); }
  const target = row.location;
  await service.openLocation(target, context.root, async loadedDocument => {
    const loadedVersion = loadedDocument.version;
    const afterLoad = await validateNavigationContext(service, context, guard);
    const currentRow = afterLoad?.equations.find(candidate => candidate.id === row.id);
    return !loadedDocument.isClosed && loadedDocument.version === loadedVersion &&
      !!currentRow?.location && sameWrittenLocation(currentRow.location, target);
  });
}
/** Equation navigation and owner selection over the client's shared model facts. */
export function registerModelNavigation(service: DygnosisClient): vscode.Disposable {
  let disposed = false;
  const acquire = async (editor = vscode.window.activeTextEditor): Promise<NavigationContext | undefined> => {
    const document = editor?.document;
    if (disposed) return undefined;
    if (!document || document.isClosed || !isAnalysisDocument(document)) { refuse("Open a Dynare model or include before using equation navigation."); }
    const version = document.version;
    const root = await service.rootForDocument(document, true);
    if (!root) { refuse("Choose a known model owner before using equation navigation."); }
    const info = await service.modelInfo(root, document.uri);
    if (!info) { refuse("Model information is unavailable. Restart the server or open Dygnosis Output for details."); }
    const guard = (): boolean => !disposed && !document.isClosed && vscode.window.activeTextEditor === editor &&
      document.version === version && service.currentInstance === info.client_instance;
    if (!guard() || info.root_uri !== root.toString() ||
        info.document_uri !== document.uri.toString() || info.document_version !== version) { refuse("The document or model owner changed. Request equation navigation again."); }
    if (!info.complete) { refuse(`Equation navigation is unavailable because expansion is incomplete. ${info.incomplete_reasons?.map(reason => reason.message).join(" ") || info.message || "Resolve the reported model problems first."}`); }
    return { document, version, root, info, guard };
  };
  const jumpToEquation = async (): Promise<void> => {
    const editor = vscode.window.activeTextEditor;
    const selection = editor?.selection && !(editor.selections?.length > 1) ? captureSelection(editor, "equation") : undefined;
    const context = await acquire(editor);
    if (!context) return;
    if (!context.info.equations.length) { refuse("This model has no counted equations."); }
    const pick = await vscode.window.showQuickPick(equationPicks(context, selection),
      { placeHolder: "Jump to an equation before transformation", matchOnDescription: true, matchOnDetail: true });
    if (pick) await jumpEquation(service, context, pick.equation, () => !disposed);
  };
  const chooseOwner = async (resource?: unknown): Promise<void> => {
    const editor = commandEditor(resource), document = editor?.document;
    if (disposed) return;
    if (!document || document.isClosed || !isAnalysisDocument(document)) { refuse("Open a Dynare include before choosing its model owner."); }
    const version = document.version;
    const owners = await service.ownerChoices(document);
    // Discovery can start the client. Pin its instance before reading owner facts.
    const instance = service.currentInstance;
    const current = (): boolean => !disposed && !document.isClosed && vscode.window.activeTextEditor === editor &&
      document.version === version && service.currentInstance === instance;
    if (!current()) { if (!disposed) refuse("The document or server changed. Choose its model owner again."); return; }
    const picks: OwnerPick[] = [];
    for (const root of owners) {
      const info = await service.modelInfo(root, document.uri);
      if (!current()) { if (!disposed) refuse("The document or server changed. Choose its model owner again."); return; }
      if (info && info.root_uri === root.toString() && info.document_uri === document.uri.toString() &&
          info.document_version === version && info.client_instance === instance &&
          info.owner_roots.some(owner => sameSourceUri(vscode.Uri.parse(owner), root))) {
        picks.push({ label: displayPath(vscode.workspace.asRelativePath(root)), description: root.fsPath, root, info });
      }
    }
    if (owners.length === 0) { refuse(service.noModelOwnersMessage(document.uri)); }
    if (picks.length === 0) { refuse("The known models no longer include this file in the current analysis. Choose its model owner again."); }
    const pick = await vscode.window.showQuickPick(picks, { placeHolder: "Choose the model that owns this include" });
    if (!pick) return;
    if (!current()) { if (!disposed) refuse("The document or server changed. Choose its model owner again."); return; }
    const fresh = await service.revalidate(pick.root, pick.info.revision, pick.info.client_instance, document.uri);
    if (!fresh || fresh.root_uri !== pick.root.toString() || fresh.document_uri !== document.uri.toString() ||
        fresh.document_version !== version || fresh.client_instance !== pick.info.client_instance ||
        !fresh.owner_roots.some(owner => sameSourceUri(vscode.Uri.parse(owner), pick.root)) ||
        !current()) {
      refuse("The model changed. Choose its owner again.");
    }
    await service.applyOwner(document, pick.root, fresh, current);
    await service.reportDocumentContextOutcome();
  };
  const listeners = [
    vscode.commands.registerCommand("dygnosis.jumpToEquation", () => runCommand(service, jumpToEquation, () => !disposed)),
    vscode.commands.registerCommand("dygnosis.chooseModelOwner", (resource?: unknown) => runCommand(service, () => chooseOwner(resource), () => !disposed)),
  ];
  return new vscode.Disposable(() => {
    disposed = true;
    for (const listener of listeners) listener.dispose();
  });
}
