import * as vscode from "vscode";
import type { DygnosisClient, InputInvalidation, NavigationGuard } from "./client";
import { sameWrittenLocation } from "./model_view";
import type { EffectivePreviewRegistry, EffectivePreviewSession } from "./preview";
import { effectivePreviewArguments } from "./preview";
import { location, record } from "./protocol";
import type { Location, Position, Range } from "./protocol";
import { listSetting } from "./settings";

/** The input-only event and native placement option are integrated with 0.11.2. */
export interface OriginJumpClient extends Pick<DygnosisClient, "client" | "currentInstance" | "log" | "failure" | "revalidate" | "execute"> {
  readonly onDidInvalidate: vscode.Event<InputInvalidation>;
  openLocation(location: Location, root?: vscode.Uri, guard?: NavigationGuard, options?: { viewColumn?: vscode.ViewColumn; reuseOpen?: boolean }): Promise<void>;
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
export interface SourceRegion {
  id: string; effective_range: Range; written_location: PreviewLocation;
  kind: "copy" | "substitution" | "identifier";
}
export interface PreviewNavigation {
  effective_text: string; navigation_schema_version: 1; root_uri: string; revision: string;
  document_version: number | null; complete: boolean; navigation: PreviewRow[]; dependency_candidates: string[];
  source_navigation_schema_version?: 1; source_navigation?: SourceRegion[];
}
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
function parseSourceRegions(value: unknown, lines: string[]): SourceRegion[] | undefined {
  if (!Array.isArray(value)) return undefined;
  const ids = new Set<string>();
  const regions: SourceRegion[] = [];
  for (const row of value) {
    if (!record(row) || typeof row.id !== "string" || !row.id || ids.has(row.id) || !boundedRange(row.effective_range, lines) ||
        !source(row.written_location) || !["copy", "substitution", "identifier"].includes(String(row.kind))) return undefined;
    ids.add(row.id);
    regions.push(row as unknown as SourceRegion);
  }
  return regions;
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
  if (value.source_navigation_schema_version !== undefined || value.source_navigation !== undefined) {
    if (value.source_navigation_schema_version !== 1) return bad();
    const regions = parseSourceRegions(value.source_navigation, lines);
    if (!regions || (!value.complete && regions.length)) return bad();
    return { ...(value as unknown as PreviewNavigation), source_navigation_schema_version: 1, source_navigation: regions };
  }
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
/** Half-open region hit test; a nonempty selection must fit inside one region. */
export function previewRegionAt(regions: SourceRegion[], selection: Range): SourceRegion | undefined {
  const empty = !before(selection.start, selection.end);
  const matches = regions.filter(region => empty
    ? !before(selection.start, region.effective_range.start) && before(selection.start, region.effective_range.end)
    : !before(selection.start, region.effective_range.start) && !before(region.effective_range.end, selection.end));
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
  return `${row.scope === "aggregate" ? "Aggregate" : `Dimension ${row.dimension ?? "(unnamed)"}`} · ${row.number === null ? row.kind : `Equation ${String(row.number)}`} · Preview line ${String(row.effective_range.start.line + 1)}`;
}
interface SourcePick extends vscode.QuickPickItem { target: PreviewLocation }
export function writtenSourcePicks(row: PreviewRow): SourcePick[] {
  return row.written_locations.map(target => ({ label: sourceLabel(target), description: rowLabel(row), target }));
}
function utf16Offset(lines: string[], position: Position): number {
  let offset = 0;
  for (let line = 0; line < position.line; line++) offset += (lines[line]?.length ?? 0) + 1;
  return offset + position.character;
}
function advance(origin: Position, text: string, units: number): Position {
  let line = origin.line, character = origin.character;
  for (let index = 0; index < units && index < text.length; index++) {
    if (text[index] === "\n") { line += 1; character = 0; }
    else character += 1;
  }
  return { line, character };
}
function sliceText(text: string, range: Range): string {
  const lines = text.split("\n");
  return text.slice(utf16Offset(lines, range.start), utf16Offset(lines, range.end));
}
/** Copy projects a subrange; substitution and identifier select the whole written target. */
export function projectRegionTarget(payload: PreviewNavigation, region: SourceRegion, selection: Range): PreviewLocation {
  if (region.kind !== "copy") return region.written_location;
  const lines = payload.effective_text.split("\n");
  const text = sliceText(payload.effective_text, region.effective_range);
  const base = utf16Offset(lines, region.effective_range.start);
  const empty = !before(selection.start, selection.end);
  const startUnits = Math.max(0, Math.min(utf16Offset(lines, selection.start) - base, text.length));
  const endUnits = empty ? startUnits : Math.max(startUnits, Math.min(utf16Offset(lines, selection.end) - base, text.length));
  return {
    ...region.written_location,
    range: { start: advance(region.written_location.range.start, text, startUnits), end: advance(region.written_location.range.start, text, endUnits) },
  };
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
    const regions = state?.payload?.source_navigation;
    const region = state && editor && usable(state) && regions ? previewRegionAt(regions, editor.selection) : undefined;
    const row = state && editor && usable(state) && !regions ? previewRowAt(state.payload!.navigation, editor.selection) : undefined;
    for (const [key, value] of Object.entries({ effectivePreview: !!state, previewWrittenSource: !!(region || row?.written_locations.length),
      previewToolbarActions: actions.includes("toolbar"), previewContextActions: actions.includes("contextMenu") }))
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
      const result = await service.execute("dynare/showEffectiveModel", effectivePreviewArguments(service, state.session.root), request.token);
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
  const openTarget = async (state: State, editor: vscode.TextEditor, target: PreviewLocation, proofMatch: (fresh: PreviewNavigation, loadedDocument?: vscode.TextDocument) => boolean): Promise<void> => {
    const request = begin(state), generation = state.session.generation, documentVersion = editor.document.version, instance = state.instance;
    const selection = JSON.stringify(editor.selection), payload = state.payload!;
    const ownsOperation = (): boolean => !disposed && !state.closed && state.operation === request.operation && state.epoch === request.epoch &&
      state.session.generation === generation && state.instance === instance && service.currentInstance === instance && !request.token.isCancellationRequested;
    const loadProofCurrent = (): boolean => state.loadingOperation === request.operation && state.pendingFile && state.payload?.complete === true &&
      supportsNavigation(service) && !state.session.document.isClosed && state.session.document.getText() === state.session.text;
    const current = (): boolean => ownsOperation() && (usable(state) || loadProofCurrent()) && vscode.window.activeTextEditor === editor &&
      editor.document.version === documentVersion && JSON.stringify(editor.selection) === selection;
    interface Proof { fileEpoch: number; root: string; revision: string }
    const validated = async (loadedDocument?: vscode.TextDocument): Promise<Proof | undefined> => {
      for (let attempt = 0; attempt < 4; ++attempt) {
        if (!current()) return undefined;
        const fileEpoch = state.fileEpoch;
        const result = await service.execute("dynare/showEffectiveModel", effectivePreviewArguments(service, state.session.root), request.token);
        if (!current()) return undefined;
        const fresh = parsePreviewNavigation(result, state.session.root);
        if (!fresh.complete || fresh.revision !== payload.revision || fresh.effective_text !== payload.effective_text || !rootVersionMatches(state, fresh)) return undefined;
        if (fresh.document_version !== payload.document_version && !(loadedDocument && payload.document_version === null &&
            sameUri(loadedDocument.uri.toString(), state.session.root.toString()) && fresh.document_version === loadedDocument.version)) return undefined;
        const info = await service.revalidate(state.session.root, payload.revision, state.instance, state.session.root, request.token);
        if (!current()) return undefined;
        if (fileEpoch !== state.fileEpoch && loadedDocument) continue;
        if (!info && loadedDocument) continue;
        if (!info?.complete) return undefined;
        if (!proofMatch(fresh, loadedDocument)) return undefined;
        return { fileEpoch, root: fresh.root_uri, revision: fresh.revision };
      }
      return undefined;
    };
    try {
      if (!await validated()) { if (current()) { stale(state); unavailable(); } return; }
      state.loadingOperation = request.operation;
      let acceptedProof: Proof | undefined;
      await service.openLocation(target, state.session.root, async document => {
        if (!current()) return false;
        const loaded = document.version;
        if (document.isClosed || !sameUri(document.uri.toString(), target.uri) || !targetFitsDocument(target, document) ||
            (target.document_version !== null && loaded !== target.document_version) ||
            (target.document_version === null && document.isDirty)) { stale(state); return false; }
        const proof = await validated(document);
        const proofCurrent = (): boolean => !!proof && proof.fileEpoch === state.fileEpoch && proof.revision === payload.revision &&
          sameUri(proof.root, state.session.root.toString()) && current() && !document.isClosed && document.version === loaded &&
          (target.document_version !== null || !document.isDirty);
        if (!proofCurrent()) { if (current()) { stale(state); unavailable(); } return false; }
        return { isCurrent: (): boolean => {
          if (!proofCurrent()) return false;
          acceptedProof = proof;
          return true;
        } };
      }, { reuseOpen: true });
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
  const navigate = async (): Promise<void> => {
    const state = activeState(), editor = vscode.window.activeTextEditor;
    if (!state || !editor || !usable(state)) return;
    const regions = state.payload!.source_navigation;
    if (regions) {
      const region = previewRegionAt(regions, editor.selection);
      if (!region) return;
      const target = projectRegionTarget(state.payload!, region, editor.selection);
      await openTarget(state, editor, target, (fresh, loadedDocument) => {
        const freshRegion = fresh.source_navigation?.find(candidate => candidate.id === region.id);
        if (!freshRegion || JSON.stringify(freshRegion.effective_range) !== JSON.stringify(region.effective_range) ||
            freshRegion.kind !== region.kind) return false;
        const versionOk = freshRegion.written_location.document_version === region.written_location.document_version ||
          (!!loadedDocument && region.written_location.document_version === null &&
            freshRegion.written_location.document_version === loadedDocument.version);
        return versionOk && sameWrittenLocation(freshRegion.written_location, region.written_location);
      });
      return;
    }
    const row = previewRowAt(state.payload!.navigation, editor.selection);
    if (!row) return;
    const picks = writtenSourcePicks(row);
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
        const result = await service.execute("dynare/showEffectiveModel", effectivePreviewArguments(service, state.session.root), request.token);
        if (!current()) return undefined;
        const fresh = parsePreviewNavigation(result, state.session.root);
        if (!fresh.complete || fresh.revision !== payload.revision || fresh.effective_text !== payload.effective_text || !rootVersionMatches(state, fresh)) return undefined;
        if (fresh.document_version !== payload.document_version && !(loadedDocument && payload.document_version === null &&
            sameUri(loadedDocument.uri.toString(), state.session.root.toString()) && fresh.document_version === loadedDocument.version)) return undefined;
        const info = await service.revalidate(state.session.root, payload.revision, state.instance, state.session.root, request.token);
        if (!current()) return undefined;
        if (fileEpoch !== state.fileEpoch && loadedDocument) continue;
        if (!info && loadedDocument) continue;
        if (!info?.complete) return undefined;
        const freshRow = fresh.navigation.find(candidate => candidate.id === row.id && candidate.statement_id === row.statement_id);
        if (!freshRow || JSON.stringify(freshRow.effective_range) !== JSON.stringify(row.effective_range)) return undefined;
        if (pick) {
          const candidates = writtenSourcePicks(freshRow);
          if (!candidates.some(candidate =>
              (candidate.target.document_version === pick.target.document_version || (loadedDocument && pick.target.document_version === null &&
                candidate.target.document_version === loadedDocument.version)) && sameWrittenLocation(candidate.target, pick.target))) return undefined;
        }
        return { row: freshRow, fileEpoch, root: fresh.root_uri, revision: fresh.revision };
      }
      return undefined;
    };
    try {
      if (!await validated()) { if (current()) { stale(state); unavailable(); } return; }
      const pick = picks.length === 1 ? picks[0] : await vscode.window.showQuickPick(picks, {
        placeHolder: "Choose the written portion of this model row", matchOnDescription: true, matchOnDetail: true,
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
      }, { reuseOpen: true });
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
    vscode.commands.registerCommand("dygnosis.goToWrittenSource", () => navigate()),
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
