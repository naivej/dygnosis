import * as vscode from "vscode";
import { DygnosisClient, isAnalysisDocument } from "./client";
import { location, ModelInfo, record } from "./protocol";
import { booleanSetting, Log } from "./settings";

export type TintStyle = "off" | "subtle" | "model";
export const tintStyles: TintStyle[] = ["off", "subtle", "model"];

/** Settings declarations mirror the engine registry; they do not classify text. */
export const tintCategoryDefaults: ReadonlyArray<readonly [string, TintStyle]> = [
  ["model.aggregate", "model"], ["model.heterogeneous", "model"],
  ["model_replace", "subtle"], ["steady_state_model", "subtle"], ["initval", "subtle"],
  ["endval.standard", "subtle"], ["endval.learnt_in", "subtle"], ["histval", "subtle"],
  ["filter_initial_state", "subtle"], ["shocks.standard", "subtle"],
  ["shocks.surprise", "subtle"], ["shocks.learnt_in", "subtle"],
  ["shocks.heterogeneous", "subtle"], ["mshocks.standard", "subtle"],
  ["mshocks.learnt_in", "subtle"], ["heteroskedastic_shocks", "subtle"],
  ["shock_paths.standard", "subtle"], ["shock_paths.learnt_in", "subtle"],
  ["perfect_foresight_controlled_paths.standard", "subtle"],
  ["perfect_foresight_controlled_paths.learnt_in", "subtle"],
  ["conditional_forecast_paths", "subtle"], ["estimated_params", "subtle"],
  ["estimated_params_init", "subtle"], ["estimated_params_bounds", "subtle"],
  ["estimated_params_remove", "subtle"], ["observation_trends", "subtle"],
  ["deterministic_trends", "subtle"], ["matched_moments", "subtle"],
  ["matched_irfs", "subtle"], ["matched_irfs_weights", "subtle"],
  ["generate_irfs", "subtle"], ["irf_calibration", "subtle"],
  ["moment_calibration", "subtle"], ["optim_weights", "subtle"],
  ["osr_params_bounds", "subtle"], ["ramsey_constraints", "subtle"],
  ["homotopy_setup", "subtle"], ["occbin_constraints", "subtle"],
  ["svar_identification", "subtle"], ["shock_groups", "subtle"], ["init2shocks", "subtle"],
  ["pac_target_info", "subtle"], ["epilogue", "subtle"], ["verbatim", "subtle"], ["priors", "subtle"],
];

export interface TintPreferences {
  enabled: boolean;
  styles: ReadonlyMap<string, TintStyle>;
  heterogeneousModels: ReadonlyMap<string, TintStyle>;
}
function style(value: unknown): value is TintStyle {
  return value === "off" || value === "subtle" || value === "model";
}
export function dimensionOverrides(value: unknown, log: Log): Map<string, TintStyle> {
  const result = new Map<string, TintStyle>();
  if (!record(value)) {
    log("Invalid dynare.blockTint.heterogeneousModels; using no dimension overrides.");
    return result;
  }
  for (const [dimension, choice] of Object.entries(value)) {
    if (dimension.length > 0 && style(choice)) result.set(dimension, choice);
    else log(`Invalid dynare.blockTint.heterogeneousModels entry '${dimension}' was ignored.`);
  }
  return result;
}
/** Read presentation against the displayed file, using the engine's defaults. */
export function tintPreferences(document: vscode.TextDocument, registry: ModelInfo["block_categories"], log: Log): TintPreferences {
  const enabled = booleanSetting("blockTint.enabled", document.uri, true, log);
  const config = vscode.workspace.getConfiguration("dynare", { uri: document.uri, languageId: "dynare" });
  const styles = new Map<string, TintStyle>();
  for (const entry of registry) {
    const fallback = style(entry.default) ? entry.default : "off";
    const key = `blockTint.${entry.category}`;
    const value = config.get<unknown>(key, fallback);
    if (!style(value)) log(`Invalid dynare.${key}; using ${fallback}.`);
    styles.set(entry.category, style(value) ? value : fallback);
  }
  return { enabled, styles,
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

/** Shared model cache; this surface owns only visible-editor decorations. */
export function registerColors(service: DygnosisClient): vscode.Disposable {
  const decorations = {
    model: vscode.window.createTextEditorDecorationType({ isWholeLine: true,
      rangeBehavior: vscode.DecorationRangeBehavior.ClosedClosed,
      backgroundColor: new vscode.ThemeColor("dynare.blockTint.modelBackground") }),
    subtle: vscode.window.createTextEditorDecorationType({ isWholeLine: true,
      rangeBehavior: vscode.DecorationRangeBehavior.ClosedClosed,
      backgroundColor: new vscode.ThemeColor("dynare.blockTint.subtleBackground") }),
  };
  interface EditorState {
    document: vscode.TextDocument;
    client: DygnosisClient["client"];
    instance: number;
    root?: string;
  }
  const tracked = new Map<vscode.TextEditor, EditorState>();
  let generation = 0, disposed = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const cancelTimer = (): void => { if (timer !== undefined) clearTimeout(timer); timer = undefined; };
  const clear = (editor: vscode.TextEditor): void => {
    editor.setDecorations(decorations.model, []);
    editor.setDecorations(decorations.subtle, []);
  };
  const update = async (editor: vscode.TextEditor, state: EditorState, request: number): Promise<void> => {
    const { document, client, instance } = state;
    const version = document.version, uri = document.uri.toString();
    const current = (): boolean => !disposed && request === generation && tracked.get(editor) === state &&
      vscode.window.visibleTextEditors.includes(editor) && editor.document === document &&
      !document.isClosed && document.version === version && document.uri.toString() === uri &&
      client === service.client && instance === service.currentInstance &&
      isAnalysisDocument(document) && booleanSetting("blockTint.enabled", document.uri, true, service.log);
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
      const ranges = tintRanges(info, document, tintPreferences(document, info.block_categories, service.log));
      // VS Code moves the existing decorations with edits while this request is
      // pending. Replace them only with current facts; do not repaint old offsets.
      editor.setDecorations(decorations.model, ranges.model);
      editor.setDecorations(decorations.subtle, ranges.subtle);
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
          !booleanSetting("blockTint.enabled", document.uri, true, service.log)) { clear(editor); continue; }
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
      let affected = false;
      for (const editor of tracked.keys()) {
        if (event.affectsConfiguration("dynare.blockTint", editor.document.uri)) { clear(editor); affected = true; }
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
    decorations.model.dispose(); decorations.subtle.dispose();
  });
}
