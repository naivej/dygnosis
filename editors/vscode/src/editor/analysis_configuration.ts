import * as vscode from "vscode";
import * as path from "node:path";
import { DygnosisClient } from "../client/client";
import { AnalysisConfiguration, AnalysisContext, AnalysisMetadata, AnalysisOverride, EquationSelection, PresetBinding, analysisConfiguration, parseAnalysisMetadata, record } from "../client/protocol";
import { CommandCancelled, refuse, runCommand } from "../client/refusal";
import type { EffectivePreviewRegistry } from "./preview";
import { automaticConfigurationName, configurationSummary } from "../client/analysis_presentation";

export interface AnalysisSide { analysis: AnalysisConfiguration; include_paths: string[] | null; preset_bindings?: PresetBinding[] }
interface Preset { name: string; root: string; analysis: AnalysisConfiguration; includePaths?: string[]; bindings: PresetBinding[] }
interface Target { root: vscode.Uri; document: vscode.Uri; current(): boolean }

export function analysisLabel(analysis: AnalysisMetadata): string {
  return `${configurationSummary({ ...analysis.configuration, name: analysis.selection.name ?? analysis.configuration.name }, analysis.include_paths)}${analysis.state === "stale" ? " (stale)" : ""}`;
}
export function resultAnalysis(result: unknown): AnalysisMetadata | undefined {
  if (!record(result) || result.analysis === undefined) return undefined;
  try { return parseAnalysisMetadata(result.analysis); } catch { return undefined; }
}
export function parseMacroInput(raw: string): { name: string; expression: string } | undefined {
  const match = /^\s*([A-Za-z_]\w*)\s*=(?!=)([\s\S]+)$/.exec(raw);
  const expression = match?.[2].trim();
  return match && expression ? { name: match[1], expression } : undefined;
}
export function parseEquationSelection(raw: string): Omit<EquationSelection, "mode"> | undefined {
  let text = raw.trim();
  if (text.startsWith("[") && text.endsWith("]")) text = text.slice(1, -1).trim();
  const assignment = /^([A-Za-z_]\w*)\s*=\s*([\s\S]+)$/.exec(text);
  if (!assignment) return undefined;
  let remaining = assignment[2].trim();
  const multiple = remaining.startsWith("(") && remaining.endsWith(")");
  if (multiple) remaining = remaining.slice(1, -1).trim();
  const values: string[] = [];
  while (remaining) {
    // Dynare tag strings use single quotes; commas inside them are literal.
    const value = /^(?:'([^']+)'|([^\s,'"()[\]=]+))\s*/.exec(remaining);
    if (!value) return undefined;
    values.push(value[1] ?? value[2]);
    remaining = remaining.slice(value[0].length);
    if (!remaining) break;
    if (!multiple || !remaining.startsWith(",")) return undefined;
    remaining = remaining.slice(1).trimStart();
    if (!remaining) return undefined;
  }
  return values.length && new Set(values).size === values.length ? { tag: assignment[1], values } : undefined;
}
function equationSelectionExpression(selection: EquationSelection): string {
  const values = selection.values.map(value => `'${value}'`);
  return `${selection.tag ?? "name"}=${values.length === 1 ? values[0] : `(${values.join(", ")})`}`;
}

function fileUri(file: string): vscode.Uri {
  return /^[a-z][a-z\d+.-]*:\/\//i.test(file) ? vscode.Uri.parse(file) : vscode.Uri.file(file);
}
function relative(folder: vscode.WorkspaceFolder, uri: vscode.Uri): string {
  return path.relative(folder.uri.fsPath, uri.fsPath).split(path.sep).join("/");
}
function resolved(folder: vscode.WorkspaceFolder, file: string): string {
  return path.isAbsolute(file) ? file : path.resolve(folder.uri.fsPath, file);
}
function preset(value: unknown): value is Preset {
  return record(value) && typeof value.name === "string" && value.name.length > 0 && typeof value.root === "string" &&
    analysisConfiguration(value.analysis) && Array.isArray(value.bindings) &&
    (value.includePaths === undefined || Array.isArray(value.includePaths) && value.includePaths.every(path => typeof path === "string"));
}
function presets(root: vscode.Uri): { folder?: vscode.WorkspaceFolder; values: Preset[] } {
  const folder = root.scheme === "file" ? vscode.workspace.getWorkspaceFolder(root) : undefined;
  if (!folder) return { values: [] };
  const raw = vscode.workspace.getConfiguration("dynare", folder.uri).inspect<unknown>("analysisConfigurations")?.workspaceFolderValue;
  const rootPath = relative(folder, root);
  return { folder, values: Array.isArray(raw) ? raw.filter(preset).filter(value => value.root === rootPath) : [] };
}
function presetSide(value: Preset, folder: vscode.WorkspaceFolder): AnalysisSide {
  const analysis = structuredClone(value.analysis);
  analysis.name = value.name;
  for (const operation of analysis.overrides ?? []) if (operation.target.file !== null) operation.target.file = resolved(folder, operation.target.file);
  return { analysis, include_paths: value.includePaths?.map(path => resolved(folder, path)) ?? null,
    preset_bindings: value.bindings.map(binding => ({ ...binding, file: binding.file === null ? null : resolved(folder, binding.file) })) };
}

/** Choose Before without changing or saving the current configuration. */
export async function chooseAnalysisBaseline(root: vscode.Uri, token: vscode.CancellationToken): Promise<AnalysisSide | undefined> {
  const saved = presets(root);
  const pick = await vscode.window.showQuickPick([
    { label: "Written source", side: { analysis: {}, include_paths: null } },
    ...saved.values.map(value => ({ label: value.name, description: "Saved configuration", side: presetSide(value, saved.folder!) })),
  ], { title: `Compare ${path.posix.basename(root.path)} with`, placeHolder: "Choose written source or a saved configuration" }, token);
  return token.isCancellationRequested ? undefined : pick?.side;
}
function modified(context: AnalysisContext): AnalysisConfiguration {
  const choices = structuredClone(context.analysis.configuration);
  const name = context.analysis.selection.name;
  if (automaticConfigurationName(name)) delete choices.name;
  else choices.name = name!.endsWith(" (modified)") ? name! : `${name} (modified)`;
  return choices;
}
function replaceOverride(choices: AnalysisConfiguration, operation: AnalysisOverride): void {
  choices.overrides = [...(choices.overrides ?? []).filter(previous => previous.target.file !== operation.target.file || previous.target.site_id !== operation.target.site_id), operation];
}

/** Native commands share the same root client state in source and expansion. */
export function registerAnalysisConfigurations(service: DygnosisClient, previews: EffectivePreviewRegistry): vscode.Disposable {
  let disposed = false;
  const target = async (written = false): Promise<Target> => {
    const document = vscode.window.activeTextEditor?.document;
    if (!document || document.isClosed) refuse("Open a Dynare source or macro expansion before selecting its analysis configuration.");
    const session = previews.get(document.uri);
    if (written && session) refuse("Use Go to written source, then select the controlling macro directive for this analysis preview.");
    const root = session?.root ?? await service.rootForDocument(document, true);
    if (!root) refuse("Choose the model root for this source before selecting an analysis configuration.");
    await service.ensureStarted();
    if (!service.supportsAnalysisConfiguration) refuse("This engine does not support analysis configurations. Use the bundled binary or update dynare.serverPath.");
    const instance = service.currentInstance, version = document.version;
    return { root, document: session ? root : document.uri, current: () => !disposed && !document.isClosed && document.version === version &&
      vscode.window.activeTextEditor?.document === document && service.currentInstance === instance && (!session || previews.get(document.uri) === session) };
  };
  const context = async (invocation: Target, prepare = false): Promise<AnalysisContext> => {
    const value = await service.analysisContext(invocation.root, invocation.document, undefined, prepare);
    if (!value || !invocation.current()) refuse("The source, server, or chosen root changed. Request the analysis action again.");
    return value;
  };
  const apply = async (invocation: Target, proof: AnalysisContext, side: AnalysisSide): Promise<void> => {
    if (!invocation.current()) refuse("The source or chosen root changed. Request the analysis action again.");
    await service.setAnalysisConfiguration(invocation.root, side.analysis, proof, side.include_paths, side.preset_bindings, () => invocation.current());
  };
  type Choice = vscode.QuickPickItem & { side?: AnalysisSide; action?: "save" | "input" | "include" | "exclude" | "clearEquations" | "clearBranches"; index?: number };
  const choose = async (invocation: Target, proof: AnalysisContext): Promise<Choice> => {
    const saved = presets(invocation.root);
    const entries: Choice[] = [
      { label: "Source", description: "Reset analysis configuration", side: { analysis: {}, include_paths: null } },
      ...saved.values.map(value => ({ label: value.name, description: "Saved configuration", side: presetSide(value, saved.folder!) })),
      ...(proof.analysis.configuration.defines ?? []).map((input, index) => ({ label: `${input.name}=${input.expression}`, description: "Edit macro input", action: "input" as const, index })),
      { label: "Add macro input…", action: "input", index: -1 },
      { label: "Include equations…", action: "include" }, { label: "Exclude equations…", action: "exclude" },
      ...(proof.analysis.configuration.equation_selection ? [{ label: "Clear equation selection", action: "clearEquations" as const }] : []),
      ...(proof.analysis.configuration.overrides?.some(operation => operation.kind === "branch")
        ? [{ label: "Clear branch previews", description: "Use source conditions", action: "clearBranches" as const }] : []),
      { label: "Save current configuration…", action: "save" },
    ];
    const pick = await vscode.window.showQuickPick(entries, { placeHolder: "Choose an analysis configuration or edit its inputs", title: `Dynare: ${analysisLabel(proof.analysis)}` });
    if (!pick) throw new CommandCancelled();
    if (!invocation.current()) refuse("The source or chosen root changed. Open the selector again.");
    return pick;
  };
  const save = async (invocation?: Target): Promise<void> => {
    invocation ??= await target();
    const { folder } = presets(invocation.root);
    if (!folder) refuse("Save this model in a workspace folder before saving a path-based analysis configuration. Untitled models can keep transient choices.");
    const proof = await context(invocation, true);
    if (proof.analysis.state === "stale") refuse(proof.analysis.cause?.message ?? "Repair or reset the stale analysis configuration before saving it.");
    const name = await vscode.window.showInputBox({ title: "Save Analysis Configuration", prompt: "Configuration name", value: proof.analysis.selection.name ?? "", validateInput: value => value.trim() ? undefined : "Enter a configuration name." });
    if (name === undefined) throw new CommandCancelled();
    if (!invocation.current()) refuse("The source or server changed. Save the configuration again.");
    const current = await context(invocation, true);
    if (current.revision !== proof.revision || current.analysis.selection_generation !== proof.analysis.selection_generation) refuse("The analysis configuration changed. Save the current configuration again.");
    const analysis = structuredClone(current.analysis.configuration);
    analysis.name = name.trim();
    for (const operation of analysis.overrides ?? []) if (operation.target.file !== null) operation.target.file = relative(folder, fileUri(operation.target.file));
    const value: Preset = { name: name.trim(), root: relative(folder, invocation.root), analysis,
      bindings: (current.preset_bindings ?? []).map(binding => ({ ...binding, file: binding.file === null ? null : relative(folder, fileUri(binding.file)) })) };
    if (current.analysis.include_paths !== null) value.includePaths = current.analysis.include_paths.map(file => relative(folder, fileUri(file)));
    const configuration = vscode.workspace.getConfiguration("dynare", folder.uri);
    const raw = configuration.inspect<unknown>("analysisConfigurations")?.workspaceFolderValue;
    const existing = Array.isArray(raw) ? raw.filter(preset) : [];
    const duplicate = existing.some(saved => saved.root === value.root && saved.name === value.name);
    if (duplicate && await vscode.window.showWarningMessage(`Replace saved analysis configuration '${value.name}'?`, { modal: true }, "Replace") !== "Replace") throw new CommandCancelled();
    if (!invocation.current()) refuse("The source or server changed. Save the configuration again.");
    await configuration.update("analysisConfigurations", [...existing.filter(saved => saved.root !== value.root || saved.name !== value.name), value], vscode.ConfigurationTarget.WorkspaceFolder);
  };
  const inputs = async (invocation: Target, proof: AnalysisContext, index: number): Promise<void> => {
    const choices = modified(proof), current = choices.defines ?? [];
    if (!proof.analysis.selection.name) choices.name = "Macro inputs";
    const previous = current[index];
    const invalid = 'Enter a macro assignment, such as FLAG=true, N=2 or COUNTRY="US".';
    const raw = await vscode.window.showInputBox({
      title: "Set macro input", prompt: `Dynare macro assignment, for example FLAG=true or COUNTRY="US"${previous ? "; clear to remove this input" : ""}`,
      placeHolder: "FLAG=true", value: previous ? `${previous.name}=${previous.expression}` : "",
      validateInput: value => previous && !value.trim() || parseMacroInput(value) ? undefined : invalid,
    });
    if (raw === undefined) throw new CommandCancelled();
    const input = parseMacroInput(raw);
    if (!input && (!previous || raw.trim())) refuse(invalid);
    choices.defines = previous
      ? current.flatMap((entry, position) => position === index ? input ? [input] : [] : [entry])
      : [...current, ...(input ? [input] : [])];
    await apply(invocation, proof, { analysis: choices, include_paths: proof.analysis.include_paths });
  };
  const equations = async (invocation: Target, proof: AnalysisContext, mode: "include" | "exclude"): Promise<void> => {
    const choices = modified(proof), previous = choices.equation_selection;
    const invalid = "Enter a tag expression, such as name='policy', endogenous='rate' or name=('policy','demand'). Use single quotes for tag values and give each value once.";
    const raw = await vscode.window.showInputBox({
      title: mode === "include" ? "Include equations" : "Exclude equations", prompt: "Dynare tag expression, for example name='policy', endogenous='rate' or name=('policy','demand')",
      placeHolder: "name='policy'", value: previous ? equationSelectionExpression(previous) : "",
      validateInput: value => parseEquationSelection(value) ? undefined : invalid,
    });
    if (raw === undefined) throw new CommandCancelled();
    const selection = parseEquationSelection(raw);
    if (!selection) refuse(invalid);
    choices.equation_selection = { mode, ...selection };
    await apply(invocation, proof, { analysis: choices, include_paths: proof.analysis.include_paths });
  };
  const previewBranch = async (): Promise<void> => {
    const invocation = await target(true), proof = await context(invocation);
    const position = vscode.window.activeTextEditor!.selection.active;
    let operation: Extract<AnalysisOverride, { kind: "branch" }> | undefined;
    for (const site of proof.sites) {
      if (site.kind !== "conditional") continue;
      const arm = site.branches.find(arm => arm.location.uri === invocation.document.toString() &&
        arm.location.range.start.line <= position.line && (position.line < arm.location.range.end.line ||
          position.line === arm.location.range.end.line && arm.location.range.end.character > 0));
      if (arm) {
        operation = { kind: "branch", target: site.target, branch_id: arm.branch_id };
        break;
      }
    }
    if (!operation) refuse(proof.source_coverage.cause ?? "Place the cursor on a macro condition or @#else to preview that branch.");
    const choices = modified(proof);
    replaceOverride(choices, operation);
    await apply(invocation, proof, { analysis: choices, include_paths: proof.analysis.include_paths });
  };
  const override = async (skip = false): Promise<void> => {
    const invocation = await target(true), proof = await context(invocation);
    const position = vscode.window.activeTextEditor!.selection.active;
    const sites = proof.sites.filter(site => site.kind !== "conditional" && site.location.uri === invocation.document.toString() &&
      site.location.range.start.line <= position.line && position.line <= site.location.range.end.line);
    if (!sites.length) refuse(`Place the cursor on an @#define directive before ${skip ? "skipping" : "overriding"} it for analysis. To select a conditional branch, use Dynare: Preview Macro Branch.`);
    const site = sites.length === 1 ? sites[0] : (await vscode.window.showQuickPick(sites.map(site => ({ label: site.expected_text, site })), { placeHolder: "Select the written macro definition" }))?.site;
    if (!site) throw new CommandCancelled();
    const choices = modified(proof);
    if (site.kind === "function_definition") refuse(`Function-like macro definitions cannot be ${skip ? "skipped" : "overridden"} for analysis. Select a value definition.`);
    if (skip) replaceOverride(choices, { kind: "skip_definition", target: site.target });
    else {
      const previous = choices.overrides?.find(operation => operation.target.file === site.target.file && operation.target.site_id === site.target.site_id);
      const expression = await vscode.window.showInputBox({ title: `Override ${site.name ?? "macro"} · ${path.posix.basename(invocation.document.path)}:${site.location.range.start.line + 1}`, prompt: `Source: ${site.expected_text.replace(/\s+/g, " ").slice(0, 100)}. Enter a replacement expression.`, value: previous?.kind === "definition" ? previous.expression : "", validateInput: value => value.trim() ? undefined : "Enter a macro expression." });
      if (expression === undefined) throw new CommandCancelled();
      if (!expression.trim()) refuse("Enter a Dynare macro expression. Use Skip Macro Definition for Analysis to skip this definition.");
      replaceOverride(choices, { kind: "definition", target: site.target, expression });
    }
    await apply(invocation, proof, { analysis: choices, include_paths: proof.analysis.include_paths });
  };
  const registrations = [
    vscode.commands.registerCommand("dygnosis.selectAnalysisConfiguration", () => runCommand(service, async () => {
      const invocation = await target(), proof = await context(invocation);
      const selected = await choose(invocation, proof);
      if (selected.action === "save") await save(invocation);
      else if (selected.action === "input") await inputs(invocation, proof, selected.index!);
      else if (selected.action === "include" || selected.action === "exclude") await equations(invocation, proof, selected.action);
      else if (selected.action === "clearEquations" || selected.action === "clearBranches") {
        const choices = modified(proof);
        if (selected.action === "clearEquations") delete choices.equation_selection;
        else choices.overrides = choices.overrides?.filter(operation => operation.kind !== "branch");
        await apply(invocation, proof, { analysis: choices, include_paths: proof.analysis.include_paths });
      } else await apply(invocation, proof, selected.side!);
    }, () => !disposed)),
    vscode.commands.registerCommand("dygnosis.resetAnalysisConfiguration", () => runCommand(service, async () => { const invocation = await target(); await apply(invocation, await context(invocation), { analysis: {}, include_paths: null }); }, () => !disposed)),
    vscode.commands.registerCommand("dygnosis.saveAnalysisConfiguration", () => runCommand(service, () => save(), () => !disposed)),
    vscode.commands.registerCommand("dygnosis.overrideMacroDefinition", () => runCommand(service, () => override(), () => !disposed)),
    vscode.commands.registerCommand("dygnosis.skipMacroDefinition", () => runCommand(service, () => override(true), () => !disposed)),
    vscode.commands.registerCommand("dygnosis.previewMacroBranch", () => runCommand(service, previewBranch, () => !disposed)),
  ];
  return new vscode.Disposable(() => { disposed = true; for (const registration of registrations) registration.dispose(); });
}
