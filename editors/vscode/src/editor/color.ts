import * as vscode from "vscode";
import { DygnosisClient, isAnalysisDocument } from "../client/client";
import { location, ModelInfo, Position, Range, record } from "../client/protocol";
import { booleanSetting, Log } from "../client/settings";

export type TintStyle = "off" | "subtle" | "model";

export interface TintPreferences {
  enabled: boolean;
  native: TintStyle;
  styles: ReadonlyMap<string, TintStyle>;
  heterogeneousModels: ReadonlyMap<string, TintStyle>;
}
function style(value: unknown): value is TintStyle {
  return value === "off" || value === "subtle" || value === "model";
}
function tintChoice(value: unknown): TintStyle | undefined {
  switch (value) {
    case "Off": return "off";
    case "Subtle": return "subtle";
    case "Standard": return "model";
  }
}
const nativeTintKey = "MATLAB/OctaveTint";
const resourceConfiguration = (document: vscode.TextDocument) =>
  vscode.workspace.getConfiguration("dynare", { uri: document.uri, languageId: "dynare" });

export function dimensionOverrides(value: unknown, log: Log): Map<string, TintStyle> {
  const result = new Map<string, TintStyle>();
  if (!record(value)) {
    log("Invalid dynare.blockTint.heterogeneousModels; using no dimension overrides.");
    return result;
  }
  for (const [dimension, choice] of Object.entries(value)) {
    const tint = tintChoice(choice);
    if (dimension.length > 0 && tint) result.set(dimension, tint);
    else log(`Invalid dynare.blockTint.heterogeneousModels entry '${dimension}' was ignored.`);
  }
  return result;
}
/** Read presentation against the displayed file, using the engine's defaults. */
export function tintPreferences(document: vscode.TextDocument, registry: ModelInfo["block_categories"], log: Log): TintPreferences {
  const enabled = booleanSetting("blockTint.enabled", document.uri, true, log);
  const config = resourceConfiguration(document);
  const styles = new Map<string, TintStyle>(registry.map(entry => [entry.category, style(entry.default) ? entry.default : "off"]));
  const overrides = config.get<unknown>("blockTint.styles", {});
  if (record(overrides)) {
    for (const [block, value] of Object.entries(overrides)) {
      const tint = tintChoice(value);
      if (styles.has(block) && tint) styles.set(block, tint);
      else log(`Invalid dynare.blockTint.styles entry '${block}' was ignored.`);
    }
  } else {
    log("Invalid dynare.blockTint.styles; using the default block styles.");
  }
  const native = tintChoice(config.get<unknown>(nativeTintKey, "Off"));
  if (!native) log(`Invalid dynare.${nativeTintKey}; using Off.`);
  return { enabled, native: native ?? "off", styles,
    heterogeneousModels: dimensionOverrides(config.get<unknown>("blockTint.heterogeneousModels", {}), log) };
}

/** Only independently proven portions of this written file receive backgrounds. */
export function tintRanges(info: ModelInfo, document: vscode.TextDocument, preferences: TintPreferences): { model: vscode.Range[]; subtle: vscode.Range[] } {
  const result: { model: vscode.Range[]; subtle: vscode.Range[] } = { model: [], subtle: [] };
  if (!preferences.enabled) return result;
  const portions = new Map<string, { choice: TintStyle; range: vscode.Range }>();
  for (const statement of info.statements) {
    if (!statement.complete || statement.native || !statement.category || !preferences.styles.has(statement.category)) continue;
    let choice = preferences.styles.get(statement.category) ?? "off";
    if (statement.category === "model.heterogeneous" && statement.dimension !== null) {
      choice = preferences.heterogeneousModels.get(statement.dimension) ?? choice;
    }
    for (const segment of statement.segments) {
      if (!location(segment) || segment.uri !== document.uri.toString()) continue;
      const { start, end } = segment.range;
      if (start.line === end.line && start.character === end.character) continue;
      if (end.line >= document.lineCount ||
          start.character > document.lineAt(start.line).text.length ||
          end.character > document.lineAt(end.line).text.length) continue;
      // LSP's exclusive end at column zero must not color the following line.
      const lastLine = end.character === 0 && end.line > start.line ? end.line - 1 : end.line;
      const lastCharacter = lastLine !== end.line ? document.lineAt(lastLine).text.length : end.character;
      const key = `${start.line}:${start.character}-${lastLine}:${lastCharacter}`;
      const existing = portions.get(key);
      // One written site can stand for several macro dimensions. Disagreement
      // is not a license to pick one occurrence's presentation preference.
      if (existing) { if (existing.choice !== choice) existing.choice = "off"; }
      else portions.set(key, { choice, range: new vscode.Range(start.line, start.character, lastLine, lastCharacter) });
    }
  }
  for (const { choice, range } of portions.values()) if (choice !== "off") result[choice].push(range);
  return result;
}

/** Activity is proved by the selected execution, never inferred from block colors. */
export function inactiveRanges(info: ModelInfo, document: vscode.TextDocument): vscode.Range[] {
  return documentRanges(info, document, info.inactive_ranges ?? []);
}

function documentRanges(info: ModelInfo, document: vscode.TextDocument, ranges: Range[]): vscode.Range[] {
  const result: vscode.Range[] = [];
  if (info.document_uri !== document.uri.toString()) return result;
  for (const range of ranges) {
    if (!location({ uri: info.document_uri, range })) continue;
    const { start, end } = range;
    if (start.line === end.line && start.character === end.character || end.line >= document.lineCount ||
        start.character > document.lineAt(start.line).text.length || end.character > document.lineAt(end.line).text.length) continue;
    result.push(new vscode.Range(start.line, start.character, end.line, end.character));
  }
  return result;
}

type TintRanges = ReturnType<typeof tintRanges>;
type TintPaint = { whole: TintRanges; exact: TintRanges };
const positionOrder = (a: Position, b: Position): number => a.line - b.line || a.character - b.character;
const editorRange = (start: Position, end: Position): vscode.Range => new vscode.Range(start.line, start.character, end.line, end.character);

function tintPaint(info: ModelInfo, document: vscode.TextDocument, preferences: TintPreferences): TintPaint {
  const blocks = tintRanges(info, document, preferences);
  const native = documentRanges(info, document, info.native_ranges ?? []);
  const paint: TintPaint = { whole: { model: [], subtle: [] }, exact: { model: [], subtle: [] } };
  const lineEnd = (line: number): Position => ({ line, character: document.lineAt(line).text.length });
  const nativeEnds = native.map(range => range.end.character === 0 && range.end.line > range.start.line ? lineEnd(range.end.line - 1) : range.end);
  if (preferences.native !== "off") paint.exact[preferences.native] = native.slice();

  for (const strength of ["model", "subtle"] as const) {
    const whole = paint.whole[strength], exact = paint.exact[strength];
    const gap = (start: Position, end: Position, exactStart: boolean, exactEnd: boolean): void => {
      if (positionOrder(start, end) >= 0) return;
      if (start.line === end.line) {
        (exactStart || exactEnd ? exact : whole).push(editorRange(start, end));
        return;
      }
      if (exactStart) {
        exact.push(editorRange(start, lineEnd(start.line)));
        start = { line: start.line + 1, character: 0 };
      }
      if (exactEnd) {
        exact.push(editorRange({ line: end.line, character: 0 }, end));
        end = lineEnd(end.line - 1);
      }
      if (positionOrder(start, end) <= 0) whole.push(editorRange(start, end));
    };
    for (const block of blocks[strength]) {
      let start: Position = block.start, exactStart = false;
      let low = 0, high = native.length;
      while (low < high) {
        const mid = Math.floor((low + high) / 2);
        if (nativeEnds[mid].line < start.line) low = mid + 1;
        else high = mid;
      }
      const clip = (point: Position): Position => positionOrder(point, block.start) < 0 ? block.start :
        positionOrder(point, block.end) > 0 ? block.end : point;
      for (let index = low; index < native.length && native[index].start.line <= block.end.line; index++) {
        const before = clip(native[index].start), after = clip(nativeEnds[index]);
        gap(start, before, exactStart, true);
        start = after;
        exactStart = true;
      }
      gap(start, block.end, exactStart, false);
    }
  }
  return paint;
}

function tintDecorations(palette: unknown) {
  const prefix = palette === "amber" || palette === "violet" || palette === "slate" ? `dynare.tintPalette.${palette}` : undefined;
  const model = new vscode.ThemeColor(prefix ? `${prefix}.standardBackground` : "dynare.blockTint.modelBackground");
  const subtle = new vscode.ThemeColor(prefix ? `${prefix}.subtleBackground` : "dynare.blockTint.subtleBackground");
  const create = (backgroundColor: vscode.ThemeColor, isWholeLine: boolean) => vscode.window.createTextEditorDecorationType({
    isWholeLine, backgroundColor, rangeBehavior: vscode.DecorationRangeBehavior.ClosedClosed,
  });
  return { whole: { model: create(model, true), subtle: create(subtle, true) }, exact: { model: create(model, false), subtle: create(subtle, false) } };
}

/** Shared model cache; this surface owns only visible-editor decorations. */
export function registerColors(service: DygnosisClient): vscode.Disposable {
  const palette = () => vscode.workspace.getConfiguration("dynare").get<unknown>("tintPalette");
  let decorations = tintDecorations(palette());
  const inactive = vscode.window.createTextEditorDecorationType({ opacity: "0.65", rangeBehavior: vscode.DecorationRangeBehavior.ClosedClosed });
  const paintTint = (editor: vscode.TextEditor, paint?: TintPaint): void => {
    for (const kind of ["whole", "exact"] as const) {
      for (const strength of ["model", "subtle"] as const) editor.setDecorations(decorations[kind][strength], paint?.[kind][strength] ?? []);
    }
  };
  const disposeTints = (): void => {
    for (const kind of Object.values(decorations)) for (const decoration of Object.values(kind)) decoration.dispose();
  };
  interface EditorState {
    document: vscode.TextDocument;
    client: DygnosisClient["client"];
    instance: number;
    root?: string;
    paint?: TintPaint;
  }
  const tracked = new Map<vscode.TextEditor, EditorState>();
  let generation = 0, disposed = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const cancelTimer = (): void => { if (timer !== undefined) clearTimeout(timer); timer = undefined; };
  const clear = (editor: vscode.TextEditor): void => {
    const state = tracked.get(editor);
    if (state) state.paint = undefined;
    paintTint(editor);
    editor.setDecorations(inactive, []);
  };
  const enabled = (document: vscode.TextDocument): boolean =>
    booleanSetting("blockTint.enabled", document.uri, true, service.log) ||
    (tintChoice(resourceConfiguration(document).get<unknown>(nativeTintKey)) ?? "off") !== "off" ||
    booleanSetting("inactiveCode.enabled", document.uri, true, service.log);
  const update = async (editor: vscode.TextEditor, state: EditorState, request: number): Promise<void> => {
    const { document, client, instance } = state;
    const version = document.version, uri = document.uri.toString();
    const current = (): boolean => !disposed && request === generation && tracked.get(editor) === state &&
      vscode.window.visibleTextEditors.includes(editor) && editor.document === document &&
      !document.isClosed && document.version === version && document.uri.toString() === uri &&
      client === service.client && instance === service.currentInstance &&
      isAnalysisDocument(document) && enabled(document);
    try {
      if (!current()) return;
      const root = await service.rootForDocument(document);
      if (!current()) return;
      if (!root) { clear(editor); state.root = undefined; return; }
      if (state.root !== undefined && state.root !== root.toString()) clear(editor);
      state.root = root.toString();
      const info = await service.modelInfo(root, document.uri);
      if (!current()) return;
      if (!info || info.client_instance !== instance || info.root_uri !== root.toString() ||
          info.document_uri !== uri || info.document_version !== version) { clear(editor); return; }
      const selected = await service.rootForDocument(document);
      if (!current()) return;
      if (selected?.toString() !== root.toString()) { clear(editor); state.root = undefined; return; }
      state.paint = tintPaint(info, document, tintPreferences(document, info.block_categories, service.log));
      // VS Code moves the existing decorations with edits while this request is
      // pending. Replace them only with current facts; do not repaint old offsets.
      paintTint(editor, state.paint);
      editor.setDecorations(inactive,
        booleanSetting("inactiveCode.enabled", document.uri, true, service.log) ? inactiveRanges(info, document) : []);
    } catch (error: unknown) {
      if (current()) { clear(editor); service.log(String(error)); }
    }
  };
  const refresh = (delay = 0): void => {
    if (disposed) return;
    const request = ++generation;
    cancelTimer();
    const visible = vscode.window.visibleTextEditors;
    for (const [editor, state] of tracked) {
      if (!visible.includes(editor) || editor.document !== state.document) { clear(editor); tracked.delete(editor); }
    }
    const pending: Array<readonly [vscode.TextEditor, EditorState]> = [];
    for (const editor of visible) {
      const document = editor.document;
      let state = tracked.get(editor);
      if (!state || state.client !== service.client || state.instance !== service.currentInstance) {
        if (state) clear(editor);
        state = { document, client: service.client, instance: service.currentInstance };
        tracked.set(editor, state);
      }
      if (document.isClosed || !isAnalysisDocument(document) ||
          !enabled(document)) { clear(editor); continue; }
      // A changed input invalidates activity proof immediately. Block outlines
      // can keep moving with edits while the selected execution is refreshed.
      editor.setDecorations(inactive, []);
      if (state.paint?.exact.model.length) editor.setDecorations(decorations.exact.model, []);
      if (state.paint?.exact.subtle.length) editor.setDecorations(decorations.exact.subtle, []);
      state.paint = undefined;
      pending.push([editor, state]);
    }
    const run = (): void => {
      timer = undefined;
      for (const [editor, state] of pending) void update(editor, state, request);
    };
    // Invalidate pending replies now, but keep the editor's tint through the
    // typing pause and the next response. Lifecycle changes still clear above.
    if (pending.length && delay) timer = setTimeout(run, delay);
    else run();
  };
  const hasDocument = (document: vscode.TextDocument): boolean => [...tracked.keys()].some(editor => editor.document === document);
  const listeners = [
    vscode.window.onDidChangeVisibleTextEditors(() => refresh()),
    vscode.window.onDidChangeActiveTextEditor(() => refresh()),
    vscode.workspace.onDidChangeTextDocument(event => {
      if (event.contentChanges.length && hasDocument(event.document)) refresh(200);
    }),
    vscode.workspace.onDidOpenTextDocument(document => { if (hasDocument(document)) refresh(); }),
    vscode.workspace.onDidCloseTextDocument(document => { if (hasDocument(document)) refresh(); }),
    vscode.workspace.onDidChangeConfiguration(event => {
      if (event.affectsConfiguration("dynare.tintPalette")) {
        disposeTints();
        decorations = tintDecorations(palette());
        for (const [editor, state] of tracked) paintTint(editor, state.paint);
      }
      let affected = false;
      for (const editor of tracked.keys()) {
        if (event.affectsConfiguration("dynare.blockTint", editor.document.uri) ||
            event.affectsConfiguration(`dynare.${nativeTintKey}`, editor.document.uri) ||
            event.affectsConfiguration("dynare.inactiveCode", editor.document.uri)) { clear(editor); affected = true; }
      }
      if (affected) refresh();
    }),
    service.onDidChange(() => refresh(200)),
  ];
  refresh();
  return new vscode.Disposable(() => {
    disposed = true;
    ++generation;
    cancelTimer();
    for (const listener of listeners) listener.dispose();
    for (const editor of tracked.keys()) clear(editor);
    tracked.clear();
    disposeTints(); inactive.dispose();
  });
}
