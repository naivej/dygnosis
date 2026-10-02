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

/** Exact manifest additions owned by this slice, available to integration/tests. */
export const colorContributions = {
  semanticTokenTypes: [
    { id: "dynareEndogenous", superType: "variable", description: "A Dynare endogenous variable." },
    { id: "dynareExogenous", superType: "variable", description: "A Dynare exogenous or deterministic exogenous variable." },
    { id: "dynareParameter", superType: "variable", description: "A Dynare model parameter." },
    { id: "dynareModelLocal", superType: "variable", description: "A Dynare model-local variable." },
  ],
  semanticTokenModifiers: [
    { id: "forwardLooking", description: "Forward-looking timing of a Dynare endogenous variable." },
    { id: "predetermined", description: "Predetermined timing of a Dynare endogenous variable." },
  ],
  semanticTokenScopes: [{ language: "dynare", scopes: {
    dynareEndogenous: ["variable.other.readwrite.dynare.endogenous"],
    dynareExogenous: ["variable.other.readwrite.dynare.exogenous"],
    dynareParameter: ["variable.other.readwrite.dynare.parameter"],
    dynareModelLocal: ["variable.other.readwrite.dynare.modelLocal"],
  } }],
  colors: [
    { id: "dynare.blockTint.modelBackground", description: "Whole-line background for Model-strength Dynare block tinting. Supports transparency.",
      defaults: { dark: "#569CD61A", light: "#007ACC12", highContrast: "#00000000", highContrastLight: "#00000000" } },
    { id: "dynare.blockTint.subtleBackground", description: "Whole-line background for Subtle Dynare block tinting. Supports transparency.",
      defaults: { dark: "#569CD608", light: "#007ACC06", highContrast: "#00000000", highContrastLight: "#00000000" } },
  ],
  configuration: { title: "Dygnosis: Appearance", properties: {
    "dynare.blockTint.enabled": { type: "boolean", default: true, scope: "resource", description: "Tint recognized Dynare blocks using the current theme's background colors." },
    ...Object.fromEntries(tintCategoryDefaults.map(([category, fallback]) => [`dynare.blockTint.${category}`, {
      type: "string", enum: tintStyles, enumItemLabels: ["Off", "Subtle", "Model-strength"],
      default: fallback, scope: "resource", description: `Background style for Dynare ${category} blocks.`,
    }])),
    "dynare.blockTint.heterogeneousModels": { type: "object", default: {}, scope: "resource",
      additionalProperties: { type: "string", enum: tintStyles }, propertyNames: { minLength: 1 },
      markdownDescription: "Override the heterogeneous model tint by written dimension name, for example `{\"households\": \"off\"}`. Takes priority over the heterogeneous model style. [Edit in settings.json](command:dygnosis.editSettingsJson)." },
  } },
};

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
  const tracked = new Set<vscode.TextEditor>();
  let generation = 0, disposed = false;
  const clear = (editor: vscode.TextEditor): void => {
    editor.setDecorations(decorations.model, []);
    editor.setDecorations(decorations.subtle, []);
  };
  const refresh = (): void => {
    const request = ++generation;
    for (const editor of tracked) clear(editor);
    tracked.clear();
    if (disposed) return;
    for (const editor of vscode.window.visibleTextEditors) {
      tracked.add(editor);
      const document = editor.document;
      if (document.isClosed || !isAnalysisDocument(document) ||
          !booleanSetting("blockTint.enabled", document.uri, true, service.log)) continue;
      const version = document.version, uri = document.uri.toString();
      const current = (): boolean => !disposed && request === generation &&
        vscode.window.visibleTextEditors.includes(editor) && editor.document === document &&
        !document.isClosed && document.version === version && document.uri.toString() === uri &&
        isAnalysisDocument(document) && booleanSetting("blockTint.enabled", document.uri, true, service.log);
      const update = async (): Promise<void> => {
        const root = await service.rootForDocument(document);
        if (!current() || !root) return;
        const client = service.client, instance = service.currentInstance;
        const info = await service.modelInfo(root, document.uri);
        if (!current() || client !== service.client || instance !== service.currentInstance || !info ||
            info.client_instance !== instance || info.root_uri !== root.toString() ||
            info.document_uri !== uri || info.document_version !== version) return;
        const selected = await service.rootForDocument(document);
        if (!current() || client !== service.client || instance !== service.currentInstance || selected?.toString() !== root.toString()) return;
        const ranges = tintRanges(info, document, tintPreferences(document, info.block_categories, service.log));
        editor.setDecorations(decorations.model, ranges.model);
        editor.setDecorations(decorations.subtle, ranges.subtle);
      };
      void update().catch((error: unknown) => { if (current()) service.log(String(error)); });
    }
  };
  const listeners = [
    vscode.window.onDidChangeVisibleTextEditors(refresh),
    vscode.window.onDidChangeActiveTextEditor(refresh),
    vscode.workspace.onDidChangeTextDocument(event => {
      if ([...tracked].some(editor => editor.document === event.document)) refresh();
    }),
    vscode.workspace.onDidOpenTextDocument(document => {
      if ([...tracked].some(editor => editor.document === document)) refresh();
    }),
    vscode.workspace.onDidCloseTextDocument(document => {
      if ([...tracked].some(editor => editor.document === document)) refresh();
    }),
    vscode.workspace.onDidChangeConfiguration(event => {
      if ([...tracked].some(editor => event.affectsConfiguration("dynare.blockTint", editor.document.uri))) refresh();
    }),
    service.onDidChange(refresh),
  ];
  refresh();
  return new vscode.Disposable(() => {
    disposed = true;
    ++generation;
    for (const listener of listeners) listener.dispose();
    for (const editor of tracked) clear(editor);
    tracked.clear();
    decorations.model.dispose(); decorations.subtle.dispose();
  });
}
