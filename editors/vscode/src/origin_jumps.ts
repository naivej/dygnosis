import * as vscode from "vscode";
import type { DygnosisClient, InputInvalidation, NavigationGuard } from "./client";
import { sameWrittenLocation } from "./model_view";
import type { EffectivePreviewRegistry, EffectivePreviewSession } from "./preview";
import { location, record } from "./protocol";
import type { Location, Position, Range } from "./protocol";
import { listSetting } from "./settings";

/** The input-only event and native placement option are integrated with 0.11.2. */
export interface OriginJumpClient extends Pick<DygnosisClient, "client" | "currentInstance" | "log" | "failure" | "revalidate" | "execute"> {
  readonly onDidInvalidate: vscode.Event<InputInvalidation>;
  openLocation(location: Location, root?: vscode.Uri, guard?: NavigationGuard, options?: { viewColumn?: vscode.ViewColumn }): Promise<void>;
}
export interface PreviewLocation extends Location { document_version: number | null }
export interface PreviewFrame {
  kind: string; variable: string | null; value: string | null;
  directive_locations: PreviewLocation[]; body_locations: PreviewLocation[];
}
export interface PreviewRow {
  id: string; statement_id: string; effective_range: Range; written_locations: PreviewLocation[];
  macro_frames: PreviewFrame[]; kind: "equation" | "local" | "static"; active: boolean;
  number: number | null; scope: "aggregate" | "heterogeneous"; dimension: string | null;
}
export interface PreviewNavigation {
  effective_text: string; navigation_schema_version: 1; root_uri: string; revision: string;
  document_version: number | null; complete: boolean; navigation: PreviewRow[]; dependency_candidates: string[];
}
const previewWhen = "resourceScheme == dygnosis-effective && dygnosis.effectivePreview";
/** Add these entries to contributes; no ordinary model menu is changed. */
export const originJumpContributions = {
  commands: [
    { command: "dygnosis.goToWrittenSource", title: "Go to written source", category: "Dygnosis", icon: "$(go-to-file)", enablement: "dygnosis.previewWrittenSource" },
    { command: "dygnosis.showMacroOrigins", title: "Show macro origins", category: "Dygnosis", icon: "$(list-tree)", enablement: "dygnosis.previewMacroOrigins" },
    { command: "dygnosis.refreshEffectiveModel", title: "Refresh effective model", category: "Dygnosis", icon: "$(refresh)", enablement: "dygnosis.effectivePreview" },
  ],
  menus: {
    commandPalette: ["dygnosis.goToWrittenSource", "dygnosis.showMacroOrigins", "dygnosis.refreshEffectiveModel"].map(command => ({ command, when: previewWhen })),
    "editor/title": ["dygnosis.goToWrittenSource", "dygnosis.showMacroOrigins", "dygnosis.refreshEffectiveModel"].map((command, index) =>
      ({ command, when: `${previewWhen} && dygnosis.previewToolbarActions`, group: `navigation@${String(index + 1)}` })),
    "editor/context": ["dygnosis.goToWrittenSource", "dygnosis.showMacroOrigins", "dygnosis.refreshEffectiveModel"].map((command, index) =>
      ({ command, when: `${previewWhen} && dygnosis.previewContextActions`, group: `navigation@${String(index + 1)}` })),
  },
  keybindings: [
    { command: "dygnosis.goToWrittenSource", key: "ctrl+alt+g", mac: "cmd+alt+g", when: `${previewWhen} && editorTextFocus && dygnosis.previewWrittenSource` },
    { command: "dygnosis.showMacroOrigins", key: "ctrl+alt+m", mac: "cmd+alt+m", when: `${previewWhen} && editorTextFocus && dygnosis.previewMacroOrigins` },
    { command: "dygnosis.refreshEffectiveModel", key: "ctrl+alt+r", mac: "cmd+alt+r", when: `${previewWhen} && editorTextFocus` },
  ],
};
function natural(value: unknown): value is number { return typeof value === "number" && Number.isSafeInteger(value) && value >= 0; }
function version(value: unknown): boolean { return value === null || natural(value); }
function nullableString(value: unknown): boolean { return value === null || typeof value === "string"; }
function nativeUri(value: unknown): value is string {
  if (typeof value !== "string") return false;
  try { const uri = vscode.Uri.parse(value); return ["file", "untitled"].includes(uri.scheme) && !uri.query && !uri.fragment; }
  catch { return false; }
}
function source(value: unknown): value is PreviewLocation { return location(value) && nativeUri(value.uri) && record(value) && version(value.document_version); }
function sources(value: unknown): value is PreviewLocation[] { return Array.isArray(value) && value.every(source); }
function before(left: Position, right: Position): boolean { return left.line < right.line || (left.line === right.line && left.character < right.character); }
function boundedRange(value: unknown, lines: string[]): value is Range {
  if (!record(value) || !location({ uri: "", range: value })) return false;
  const range = value as unknown as Range;
  const bounded = (position: Position): boolean => {
    const line = lines[position.line];
    if (line === undefined || position.character > line.length) return false;
    // A UTF-16 coordinate cannot bisect one Unicode character.
    const previous = line.charCodeAt(position.character - 1), next = line.charCodeAt(position.character);
    return !(previous >= 0xd800 && previous <= 0xdbff && next >= 0xdc00 && next <= 0xdfff);
  };
  return before(range.start, range.end) && bounded(range.start) && bounded(range.end);
}
function sameUri(left: string, right: string): boolean {
  const range = { start: { line: 0, character: 0 }, end: { line: 0, character: 0 } };
  return sameWrittenLocation({ uri: left, range }, { uri: right, range });
}
/** Malformed supported data is a feature failure; old engines keep their text preview. */
export function parsePreviewNavigation(value: unknown, root: vscode.Uri): PreviewNavigation {
  const bad = (): never => { throw new Error("Unsupported or invalid effective-model navigation. Update dynare.serverPath or use the bundled binary."); };
  if (!record(value) || value.navigation_schema_version !== 1 || !nativeUri(value.root_uri) || !sameUri(value.root_uri, root.toString()) ||
      typeof value.effective_text !== "string" || typeof value.revision !== "string" || !value.revision || !version(value.document_version) ||
      typeof value.complete !== "boolean" || !Array.isArray(value.dependency_candidates) || !value.dependency_candidates.every(nativeUri) ||
      !Array.isArray(value.navigation)) return bad();
  const lines = value.effective_text.split(/\r\n|\r|\n/), ids = new Set<string>();
  for (const row of value.navigation) {
    if (!record(row) || typeof row.id !== "string" || !row.id || ids.has(row.id) || typeof row.statement_id !== "string" || !row.statement_id ||
        !boundedRange(row.effective_range, lines) || !sources(row.written_locations) || !["equation", "local", "static"].includes(String(row.kind)) ||
        typeof row.active !== "boolean" || !(row.number === null || (natural(row.number) && row.number > 0)) ||
        !["aggregate", "heterogeneous"].includes(String(row.scope)) || !nullableString(row.dimension) || !Array.isArray(row.macro_frames) ||
        !row.macro_frames.every(frame => record(frame) && typeof frame.kind === "string" && !!frame.kind && nullableString(frame.variable) &&
          nullableString(frame.value) && sources(frame.directive_locations) && sources(frame.body_locations))) return bad();
    ids.add(row.id);
  }
  if (!value.complete && value.navigation.length) return bad();
  return value as unknown as PreviewNavigation;
}
/** A selection crossing several mapped rows has no unambiguous source action. */
export function previewRowAt(rows: PreviewRow[], selection: Range): PreviewRow | undefined {
  const empty = !before(selection.start, selection.end);
  const matches = rows.filter(row => empty
    ? !before(selection.start, row.effective_range.start) && before(selection.start, row.effective_range.end)
    : before(selection.start, row.effective_range.end) && before(row.effective_range.start, selection.end));
  return matches.length === 1 ? matches[0] : undefined;
}
function supportsNavigation(service: OriginJumpClient): boolean {
  const experimental: unknown = service.client?.initializeResult?.capabilities.experimental;
  return record(experimental) && record(experimental.dygnosis) && record(experimental.dygnosis.effectivePreview) &&
    experimental.dygnosis.effectivePreview.command === "dynare/showEffectiveModel" &&
    experimental.dygnosis.effectivePreview.navigation_schema_version === 1 && experimental.dygnosis.effectivePreview.dependency_candidates === true;
}
function loadedVersion(uri: string): number | null {
  return vscode.workspace.textDocuments.find(document => sameUri(document.uri.toString(), uri))?.version ?? null;
}
function targetFitsDocument(target: PreviewLocation, document: vscode.TextDocument): boolean {
  return [target.range.start, target.range.end].every(position => {
    if (position.line >= document.lineCount) return false;
    const line = document.lineAt(position.line).text;
    if (position.character > line.length) return false;
    const previous = line.charCodeAt(position.character - 1), next = line.charCodeAt(position.character);
    return !(previous >= 0xd800 && previous <= 0xdbff && next >= 0xdc00 && next <= 0xdfff);
  });
}
function sourceLabel(target: PreviewLocation): string { return `${vscode.workspace.asRelativePath(vscode.Uri.parse(target.uri))}:${String(target.range.start.line + 1)}`; }
function rowLabel(row: PreviewRow): string {
  return `${row.scope === "aggregate" ? "Aggregate" : `Dimension ${row.dimension ?? "(unnamed)"}`} · ${row.number === null ? row.kind : `Equation ${String(row.number)}`} · Occurrence ${row.id}`;
}
interface SourcePick extends vscode.QuickPickItem { target: PreviewLocation; frameIndex?: number; site?: "directive" | "body" }
export function writtenSourcePicks(row: PreviewRow): SourcePick[] {
  return row.written_locations.map(target => ({ label: sourceLabel(target), description: rowLabel(row), target }));
}
export function macroOriginPicks(row: PreviewRow): SourcePick[] {
  return row.macro_frames.flatMap((frame, frameIndex) => (["directive", "body"] as const).flatMap(site =>
    frame[site === "directive" ? "directive_locations" : "body_locations"].map(target => ({
      label: `${frame.kind}${frame.variable !== null && frame.value !== null ? ` · ${frame.variable}=${frame.value}` : ""} · ${site}`,
      description: sourceLabel(target), detail: `${rowLabel(row)} · Frame ${String(frameIndex + 1)}`, target, frameIndex, site,
    }))));
}
interface State {
  session: EffectivePreviewSession; payload?: PreviewNavigation; instance: number; stale: boolean; closed: boolean;
  epoch: number; operation: number; cancellation?: vscode.CancellationTokenSource; subscriptions: vscode.Disposable[]; watchers: vscode.Disposable[];
  fileEpoch: number; loadingOperation?: number; pendingFile: boolean;
}
export function registerOriginJumps(service: OriginJumpClient, previews: EffectivePreviewRegistry): vscode.Disposable {
  const states = new Map<EffectivePreviewSession, State>();
  let disposed = false;
  const activeState = (): State | undefined => {
    const document = vscode.window.activeTextEditor?.document;
    const session = document && previews.get(document.uri);
    return session && session.document === document ? states.get(session) : undefined;
  };
  const usable = (state: State): boolean => !disposed && !state.closed && !state.stale && state.payload?.complete === true &&
    state.instance === service.currentInstance && supportsNavigation(service) && !state.session.document.isClosed &&
    state.session.document.getText() === state.session.text;
  const updateContext = (): void => {
    const state = activeState(), editor = vscode.window.activeTextEditor;
    const actions = listSetting("editorActions", state?.session.root, ["toolbar", "contextMenu"], ["toolbar", "contextMenu"], service.log);
    const row = state && editor && usable(state) ? previewRowAt(state.payload!.navigation, editor.selection) : undefined;
    for (const [key, value] of Object.entries({ effectivePreview: !!state, previewWrittenSource: !!row?.written_locations.length,
      previewMacroOrigins: !!row && macroOriginPicks(row).length > 0, previewToolbarActions: actions.includes("toolbar"), previewContextActions: actions.includes("contextMenu") }))
      void vscode.commands.executeCommand("setContext", `dygnosis.${key}`, value);
  };
  const cancel = (state: State): void => {
    ++state.operation; state.cancellation?.cancel(); state.cancellation?.dispose(); state.cancellation = undefined;
    state.loadingOperation = undefined; state.pendingFile = false;
  };
  const stale = (state: State): void => { state.stale = true; ++state.epoch; cancel(state); updateContext(); };
  const observeFile = (state: State, target: string): void => {
    if (!(state.payload?.dependency_candidates ?? [state.session.root.toString()]).some(candidate => sameUri(candidate, target))) return;
    ++state.fileEpoch;
    if (!disposed && !state.closed && state.loadingOperation === state.operation) {
      // A filesystem notification may describe the unchanged disk file just
      // loaded. Withhold actions and make that loader prove the same snapshot.
      state.pendingFile = true; state.stale = true; updateContext();
    } else stale(state);
  };
  const watch = (state: State): void => {
    for (const watcher of state.watchers) watcher.dispose();
    state.watchers = [];
    for (const candidate of new Set(state.payload?.dependency_candidates ?? [])) {
      const uri = vscode.Uri.parse(candidate);
      if (uri.scheme !== "file") continue;
      const separator = Math.max(uri.fsPath.lastIndexOf("/"), uri.fsPath.lastIndexOf("\\"));
      const watcher = vscode.workspace.createFileSystemWatcher(new vscode.RelativePattern(vscode.Uri.file(uri.fsPath.slice(0, separator)), "*"));
      const changed = (target: vscode.Uri): void => { if (sameUri(target.toString(), candidate)) observeFile(state, candidate); };
      state.watchers.push(watcher, watcher.onDidCreate(changed), watcher.onDidChange(changed), watcher.onDidDelete(changed));
    }
  };
  const rootVersionMatches = (state: State, payload: PreviewNavigation): boolean => loadedVersion(state.session.root.toString()) === payload.document_version;
  const attach = (session: EffectivePreviewSession): void => {
    const state: State = { session, instance: session.instance, stale: true, closed: false, epoch: 0, operation: 0, fileEpoch: 0, pendingFile: false,
      subscriptions: [], watchers: [] };
    states.set(session, state);
    state.subscriptions.push(service.onDidInvalidate(event => {
      if (event.root && !sameUri(event.root, session.root.toString())) return;
      if (event.reason === "file" && event.uri) observeFile(state, event.uri); else stale(state);
    }),
      vscode.workspace.onDidChangeTextDocument(event => {
        if (event.contentChanges.length && (state.payload?.dependency_candidates ?? [session.root.toString()]).some(uri => sameUri(uri, event.document.uri.toString()))) stale(state);
        else if (event.document === session.document) updateContext();
      }),
      vscode.workspace.onDidCloseTextDocument(document => {
        if ((state.payload?.dependency_candidates ?? [session.root.toString()]).some(uri => sameUri(uri, document.uri.toString()))) stale(state);
      }),
      vscode.workspace.onDidChangeConfiguration(event => { if (event.affectsConfiguration("dynare", session.root)) stale(state); }));
    if (supportsNavigation(service)) {
      try {
        state.payload = parsePreviewNavigation(session.result, session.root); watch(state);
        const request = begin(state);
        void service.revalidate(session.root, state.payload.revision, state.instance, session.root, request.token).then(info => {
          if (!disposed && !state.closed && state.operation === request.operation && state.epoch === request.epoch && !request.token.isCancellationRequested &&
              state.instance === service.currentInstance && info?.complete &&
              state.payload?.complete && rootVersionMatches(state, state.payload)) { state.stale = false; updateContext(); }
        }).catch(error => service.log(String(error)));
      } catch (error) { void service.failure(String(error)); }
    }
    updateContext();
  };
  const detach = (session: EffectivePreviewSession): void => {
    const state = states.get(session);
    if (!state) return;
    state.closed = true; cancel(state);
    for (const subscription of [...state.subscriptions, ...state.watchers]) subscription.dispose();
    states.delete(session); updateContext();
  };
  const begin = (state: State): { operation: number; epoch: number; token: vscode.CancellationToken } => {
    cancel(state); state.cancellation = new vscode.CancellationTokenSource();
    return { operation: state.operation, epoch: state.epoch, token: state.cancellation.token };
  };
  const unavailable = (): void => { void vscode.window.showInformationMessage("This preview has changed or its source is unavailable. Refresh effective model and choose the row again."); };
  const refresh = async (): Promise<void> => {
    const state = activeState();
    if (!state || disposed) return;
    state.stale = true; const request = begin(state); updateContext();
    const instance = service.currentInstance, generation = state.session.generation;
    const current = (): boolean => !disposed && !state.closed && state.operation === request.operation && state.epoch === request.epoch &&
      state.session.generation === generation && !request.token.isCancellationRequested && service.currentInstance === instance;
    try {
      const result = await service.execute("dynare/showEffectiveModel", [{ root_uri: state.session.root.toString() }], request.token);
      if (!current()) return;
      let payload: PreviewNavigation | undefined;
      if (supportsNavigation(service)) {
        payload = parsePreviewNavigation(result, state.session.root);
        if (!rootVersionMatches(state, payload)) return;
        const info = await service.revalidate(state.session.root, payload.revision, instance, state.session.root, request.token);
        if (!info || !current()) return;
      }
      if (!record(result) || typeof result.effective_text !== "string") throw new Error("This engine cannot refresh the effective model. Update dynare.serverPath or use the bundled binary.");
      if (!current()) return;
      state.payload = payload; state.instance = instance; state.stale = !payload?.complete;
      if (!previews.replace(state.session, result)) return;
      watch(state); updateContext();
    } catch (error) { if (current()) await service.failure(String(error)); }
  };
  const navigate = async (macro: boolean): Promise<void> => {
    const state = activeState(), editor = vscode.window.activeTextEditor;
    if (!state || !editor || !usable(state)) return;
    const row = previewRowAt(state.payload!.navigation, editor.selection);
    if (!row) return;
    const picks = macro ? macroOriginPicks(row) : writtenSourcePicks(row);
    if (!picks.length) return;
    const request = begin(state), generation = state.session.generation, documentVersion = editor.document.version, instance = state.instance;
    const selection = JSON.stringify(editor.selection), payload = state.payload!;
    const ownsOperation = (): boolean => !disposed && !state.closed && state.operation === request.operation && state.epoch === request.epoch &&
      state.session.generation === generation && state.instance === instance && service.currentInstance === instance && !request.token.isCancellationRequested;
    const loadProofCurrent = (): boolean => state.loadingOperation === request.operation && state.pendingFile && state.payload?.complete === true &&
      supportsNavigation(service) && !state.session.document.isClosed && state.session.document.getText() === state.session.text;
    const current = (): boolean => ownsOperation() && (usable(state) || loadProofCurrent()) && vscode.window.activeTextEditor === editor &&
      editor.document.version === documentVersion && JSON.stringify(editor.selection) === selection;
    interface Proof { row: PreviewRow; fileEpoch: number; root: string; revision: string }
    const validated = async (pick?: SourcePick, loadedDocument?: vscode.TextDocument): Promise<Proof | undefined> => {
      for (let attempt = 0; attempt < 4; ++attempt) {
        if (!current()) return undefined;
        const fileEpoch = state.fileEpoch;
        const result = await service.execute("dynare/showEffectiveModel", [{ root_uri: state.session.root.toString() }], request.token);
        if (!current()) return undefined;
        const fresh = parsePreviewNavigation(result, state.session.root);
        if (!fresh.complete || fresh.revision !== payload.revision || fresh.effective_text !== payload.effective_text || !rootVersionMatches(state, fresh)) return undefined;
        if (fresh.document_version !== payload.document_version && !(loadedDocument && payload.document_version === null &&
            sameUri(loadedDocument.uri.toString(), state.session.root.toString()) && fresh.document_version === loadedDocument.version)) return undefined;
        const info = await service.revalidate(state.session.root, payload.revision, state.instance, state.session.root, request.token);
        if (!current()) return undefined;
        if (fileEpoch !== state.fileEpoch && loadedDocument) continue;
        if (!info?.complete) return undefined;
        const freshRow = fresh.navigation.find(candidate => candidate.id === row.id && candidate.statement_id === row.statement_id);
        if (!freshRow || JSON.stringify(freshRow.effective_range) !== JSON.stringify(row.effective_range)) return undefined;
        if (pick) {
          const candidates = macro ? macroOriginPicks(freshRow) : writtenSourcePicks(freshRow);
          if (macro && pick.frameIndex !== undefined &&
              JSON.stringify([freshRow.macro_frames[pick.frameIndex]?.kind, freshRow.macro_frames[pick.frameIndex]?.variable, freshRow.macro_frames[pick.frameIndex]?.value]) !==
              JSON.stringify([row.macro_frames[pick.frameIndex]?.kind, row.macro_frames[pick.frameIndex]?.variable, row.macro_frames[pick.frameIndex]?.value])) return undefined;
          if (!candidates.some(candidate => candidate.frameIndex === pick.frameIndex && candidate.site === pick.site &&
              (candidate.target.document_version === pick.target.document_version || (loadedDocument && pick.target.document_version === null &&
                candidate.target.document_version === loadedDocument.version)) && sameWrittenLocation(candidate.target, pick.target))) return undefined;
        }
        return { row: freshRow, fileEpoch, root: fresh.root_uri, revision: fresh.revision };
      }
      return undefined;
    };
    try {
      if (!await validated()) { if (current()) { stale(state); unavailable(); } return; }
      const pick = !macro && picks.length === 1 ? picks[0] : await vscode.window.showQuickPick(picks, {
        placeHolder: macro ? "Choose a verified macro origin" : "Choose the written portion of this model row", matchOnDescription: true, matchOnDetail: true,
      });
      if (!pick || !current()) return;
      if (!await validated(pick)) { if (current()) { stale(state); unavailable(); } return; }
      state.loadingOperation = request.operation;
      let acceptedProof: Proof | undefined;
      await service.openLocation(pick.target, state.session.root, async document => {
        if (!current()) return false;
        const loaded = document.version;
        if (document.isClosed || !sameUri(document.uri.toString(), pick.target.uri) || !targetFitsDocument(pick.target, document) ||
            (pick.target.document_version !== null && loaded !== pick.target.document_version) ||
            (pick.target.document_version === null && document.isDirty)) { stale(state); return false; }
        const proof = await validated(pick, document);
        const proofCurrent = (): boolean => !!proof && proof.fileEpoch === state.fileEpoch && proof.revision === payload.revision &&
          sameUri(proof.root, state.session.root.toString()) && current() && !document.isClosed && document.version === loaded &&
          (pick.target.document_version !== null || !document.isDirty);
        if (!proofCurrent()) { if (current()) { stale(state); unavailable(); } return false; }
        return { isCurrent: (): boolean => {
          if (!proofCurrent()) return false;
          acceptedProof = proof;
          return true;
        } };
      }, { viewColumn: vscode.ViewColumn.Beside });
      if (acceptedProof && acceptedProof.fileEpoch === state.fileEpoch && ownsOperation()) {
        state.pendingFile = false; state.stale = false; updateContext();
      }
    } catch (error) { if (current()) { stale(state); service.log(String(error)); unavailable(); } } finally {
      if (state.loadingOperation === request.operation) {
        state.loadingOperation = undefined;
        if (state.pendingFile && ownsOperation()) stale(state);
      }
    }
  };
  const subscriptions = [previews.onDidCreate(attach), previews.onDidClose(detach),
    vscode.commands.registerCommand("dygnosis.goToWrittenSource", () => navigate(false)),
    vscode.commands.registerCommand("dygnosis.showMacroOrigins", () => navigate(true)),
    vscode.commands.registerCommand("dygnosis.refreshEffectiveModel", refresh),
    vscode.window.onDidChangeActiveTextEditor(updateContext), vscode.window.onDidChangeTextEditorSelection(updateContext),
    vscode.workspace.onDidChangeConfiguration(updateContext)];
  for (const session of previews.sessions()) attach(session);
  updateContext();
  return new vscode.Disposable(() => {
    disposed = true;
    for (const session of [...states.keys()]) detach(session);
    for (const subscription of subscriptions) subscription.dispose();
    updateContext();
  });
}
