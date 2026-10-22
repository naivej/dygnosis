import * as vscode from "vscode";
import * as path from "node:path";
import { DygnosisClient } from "../client/client";
import { AnalysisConfiguration, AnalysisContext, AnalysisMetadata, AnalysisOverride, PresetBinding, analysisConfiguration, parseAnalysisMetadata, record } from "../client/protocol";
import { CommandCancelled, refuse, runCommand } from "../client/refusal";
import type { EffectivePreviewRegistry } from "./preview";

export interface AnalysisSide { analysis: AnalysisConfiguration; include_paths: string[] | null; preset_bindings?: PresetBinding[] }
export type CompareAnalysis = (root: vscode.Uri, left: AnalysisSide, right: AnalysisSide) => Promise<void>;
interface Preset { name: string; root: string; analysis: AnalysisConfiguration; includePaths?: string[]; bindings: PresetBinding[] }
interface Target { root: vscode.Uri; document: vscode.Uri; current(): boolean }

export function analysisLabel(analysis: AnalysisMetadata): string {
  const selection = analysis.selection;
  const name = selection.name ?? (selection.preview ? "Analysis preview" : (analysis.configuration.defines?.length ?? 0) ? "Macro inputs" : "Source");
  return `${name}${selection.preview ? " (preview)" : ""}${analysis.state === "stale" ? " (stale)" : ""}`;
}
export function resultAnalysis(result: unknown): AnalysisMetadata | undefined {
  if (!record(result) || result.analysis === undefined) return undefined;
  try { return parseAnalysisMetadata(result.analysis); } catch { return undefined; }
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
function modified(context: AnalysisContext): AnalysisConfiguration {
  const choices = structuredClone(context.analysis.configuration);
  const name = context.analysis.selection.name;
  choices.name = name ? name.endsWith(" (modified)") ? name : `${name} (modified)` : "Analysis preview";
  return choices;
}
function replaceOverride(choices: AnalysisConfiguration, operation: AnalysisOverride): void {
  choices.overrides = [...(choices.overrides ?? []).filter(previous => previous.target.file !== operation.target.file || previous.target.site_id !== operation.target.site_id), operation];
}

/** Native commands share the same root client state in source and expansion. */
export function registerAnalysisConfigurations(service: DygnosisClient, previews: EffectivePreviewRegistry, compare?: CompareAnalysis): vscode.Disposable {
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
  const choose = async (invocation: Target, proof: AnalysisContext, placeHolder: string, actions = false): Promise<AnalysisSide | "save" | "inputs"> => {
    const saved = presets(invocation.root);
    const entries: (vscode.QuickPickItem & { side?: AnalysisSide; action?: "save" | "inputs" })[] = [
      { label: "Source", description: "Use the written macro inputs", side: { analysis: {}, include_paths: null } },
      ...saved.values.map(value => ({ label: value.name, description: "Saved configuration", side: presetSide(value, saved.folder!) })),
    ];
    if (actions) entries.push({ label: "Set macro inputs…", action: "inputs" }, { label: "Save current configuration…", action: "save" });
    const pick = await vscode.window.showQuickPick(entries, { placeHolder, title: `Dynare: ${analysisLabel(proof.analysis)}` });
    if (!pick) throw new CommandCancelled();
    if (!invocation.current()) refuse("The source or chosen root changed. Open the selector again.");
    return pick.action ?? pick.side!;
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
  const inputs = async (invocation: Target, proof: AnalysisContext): Promise<void> => {
    const choices = modified(proof), current = choices.defines ?? [];
    if (!proof.analysis.selection.name) choices.name = "Macro inputs";
    const pick = await vscode.window.showQuickPick([{ label: "Add macro input", index: -1 }, ...current.map((input, index) => ({ label: input.name, description: input.expression, index }))], { placeHolder: "Set an initial macro value" });
    if (!pick) throw new CommandCancelled();
    const previous = current[pick.index];
    const name = previous?.name ?? await vscode.window.showInputBox({ prompt: "Macro input name", validateInput: value => /^[A-Za-z_][A-Za-z_\d]*$/.test(value) ? undefined : "Enter a macro identifier." });
    if (name === undefined) throw new CommandCancelled();
    const expression = await vscode.window.showInputBox({ prompt: "Dynare macro expression; leave empty to remove this input", value: previous?.expression ?? "" });
    if (expression === undefined) throw new CommandCancelled();
    choices.defines = previous
      ? current.flatMap((input, index) => index === pick.index ? expression.trim() ? [{ name, expression }] : [] : [input])
      : [...current, ...(expression.trim() ? [{ name, expression }] : [])];
    await apply(invocation, proof, { analysis: choices, include_paths: proof.analysis.include_paths });
  };
  const override = async (branch: boolean): Promise<void> => {
    const invocation = await target(true), proof = await context(invocation);
    const position = vscode.window.activeTextEditor!.selection.active;
    const sites = proof.sites.filter(site => (branch ? site.kind === "conditional" : site.kind !== "conditional") &&
      site.location.uri === invocation.document.toString() && (site.location.range.start.line <= position.line && position.line <= site.location.range.end.line ||
        branch && site.branches.some(arm => arm.location.range.start.line <= position.line && position.line <= arm.location.range.end.line)));
    const site = sites.length === 1 ? sites[0] : (await vscode.window.showQuickPick(sites.map(site => ({ label: site.expected_text, site })), { placeHolder: branch ? "Select the controlling conditional" : "Select the written macro definition" }))?.site;
    if (!site) refuse(branch ? "Place the cursor on a conditional opener before previewing one of its branches." : "Place the cursor on a macro definition before overriding it for analysis.");
    const choices = modified(proof);
    if (branch) {
      const pick = await vscode.window.showQuickPick([{ label: "No branch", branch_id: null as string | null }, ...site.branches.map(arm => ({ label: arm.expected_text, branch_id: arm.branch_id }))], { placeHolder: "Preview one written branch; enclosing loops and conditions still apply" });
      if (!pick) throw new CommandCancelled();
      replaceOverride(choices, { kind: "branch", target: site.target, branch_id: pick.branch_id });
    } else {
      if (site.kind === "function_definition") refuse("Function-like macro definitions cannot be overridden for analysis. Select a value definition.");
      const pick = await vscode.window.showQuickPick(["Replacement expression", "Skip this definition"], { placeHolder: site.expected_text });
      if (!pick) throw new CommandCancelled();
      if (pick === "Skip this definition") replaceOverride(choices, { kind: "skip_definition", target: site.target });
      else {
        const expression = await vscode.window.showInputBox({ prompt: "Dynare macro expression for this written definition" });
        if (expression === undefined) throw new CommandCancelled();
        replaceOverride(choices, { kind: "definition", target: site.target, expression });
      }
    }
    await apply(invocation, proof, { analysis: choices, include_paths: proof.analysis.include_paths });
  };
  const registrations = [
    vscode.commands.registerCommand("dygnosis.selectAnalysisConfiguration", () => runCommand(service, async () => {
      const invocation = await target(), proof = await context(invocation);
      const selected = await choose(invocation, proof, "Select the analysis configuration for this model root", true);
      if (selected === "save") await save(invocation); else if (selected === "inputs") await inputs(invocation, proof); else await apply(invocation, proof, selected);
    }, () => !disposed)),
    vscode.commands.registerCommand("dygnosis.resetAnalysisConfiguration", () => runCommand(service, async () => { const invocation = await target(); await apply(invocation, await context(invocation), { analysis: {}, include_paths: null }); }, () => !disposed)),
    vscode.commands.registerCommand("dygnosis.saveAnalysisConfiguration", () => runCommand(service, () => save(), () => !disposed)),
    vscode.commands.registerCommand("dygnosis.overrideMacroDefinition", () => runCommand(service, () => override(false), () => !disposed)),
    vscode.commands.registerCommand("dygnosis.previewMacroBranch", () => runCommand(service, () => override(true), () => !disposed)),
    vscode.commands.registerCommand("dygnosis.compareAnalysisConfigurations", () => runCommand(service, async () => {
      if (!compare) refuse("This engine cannot capture analysis-configuration comparisons. Use the bundled engine and restart the server.");
      const invocation = await target(), proof = await context(invocation);
      const left = await choose(invocation, proof, "Choose configuration A"), right = await choose(invocation, proof, "Choose configuration B");
      if (typeof left === "string" || typeof right === "string") return;
      await compare(invocation.root, left, right);
    }, () => !disposed)),
  ];
  return new vscode.Disposable(() => { disposed = true; for (const registration of registrations) registration.dispose(); });
}
