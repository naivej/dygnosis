import * as vscode from "vscode";
import { DygnosisClient, isAnalysisDocument } from "../client/client";
import { AnalysisOverride, ModelInfo } from "../client/protocol";
import { booleanSetting, listSetting } from "../client/settings";
import type { EffectivePreviewRegistry } from "./preview";
import { failureText, fileLabel, reasonText } from "../client/failure";
import { automaticConfigurationName, configurationSummary } from "../client/analysis_presentation";

export const statusCounts = ["endogenous", "exogenous", "equations"];
export type StatusCount = "endogenous" | "exogenous" | "equations";
interface StatusContent { text: string; tooltip: string | vscode.MarkdownString; accessibilityLabel: string }
const outlineClickInstruction = "Click the status item to open Outline.";
const countDetails: Record<StatusCount, { field: "n_endogenous" | "n_exogenous" | "n_equations"; icon: string; label: string }> = {
  endogenous: { field: "n_endogenous", icon: "symbol-variable", label: "endogenous" },
  exogenous: { field: "n_exogenous", icon: "symbol-field", label: "exogenous" },
  equations: { field: "n_equations", icon: "symbol-operator", label: "equations" },
};

type CountFacts = Pick<ModelInfo, "n_endogenous" | "n_exogenous" | "n_equations" | "static" | "predetermined" | "forward_looking" | "mixed">;

function countLines(info: CountFacts, counts: StatusCount[]): string[] {
  return counts.map(count => {
    const { field, label } = countDetails[count];
    const line = `${String(info[field])} ${label}`;
    return count === "endogenous"
      ? `${line} (${info.static?.length ?? 0} static, ${info.predetermined?.length ?? 0} predetermined, ${info.forward_looking?.length ?? 0} forward-looking, ${info.mixed?.length ?? 0} mixed)`
      : line;
  });
}

function incompleteDetail(info: ModelInfo): { detail: string; leadingCause: string } {
  const reasons = new Map<string, string>();
  const cause = info.analysis?.cause?.message.trim();
  if (cause) reasons.set(cause, cause);
  for (const reason of Array.isArray(info.incomplete_reasons) ? info.incomplete_reasons : []) {
    const message = typeof reason?.message === "string" ? reason.message.trim() : "";
    if (message) reasons.set(message, reasonText({ ...reason, message }));
  }
  const detail = reasons.size > 0 ? [...reasons.values()].join("\n") : info.message ?? "Model expansion is incomplete.";
  const leading = reasons.values().next().value ?? detail;
  return { detail, leadingCause: leading.length > 512 ? leading.slice(0, 511) + "…" : leading };
}

function configurationHover(info: ModelInfo): vscode.MarkdownString {
  const analysis = info.analysis;
  const equations = analysis?.configuration.equation_selection;
  const name = automaticConfigurationName(analysis?.selection.name) ? undefined : analysis?.selection.name;
  const names = new Map<string, number>();
  for (const receipt of analysis?.overrides ?? []) if (receipt.name) names.set(receipt.name, (names.get(receipt.name) ?? 0) + 1);
  const overrideChoice = (operation: AnalysisOverride, name?: string | null, branchLabel?: string | null): string => operation.kind === "definition"
    ? `Macro: ${name ? `${name}=` : ""}${operation.expression}`
    : operation.kind === "skip_definition" ? `Skip ${name ?? "macro"} definition` : `Branch: ${branchLabel ?? (operation.branch_id === null ? "No branch" : "selected")}`;
  const overrideChoices = analysis?.overrides.length
    ? analysis.overrides.map((receipt, index) => {
      let choice = overrideChoice(receipt.operation, receipt.name, receipt.branch_label ?? analysis.override_branch_labels?.[index]);
      if (receipt.name && (names.get(receipt.name) ?? 0) > 1) choice += ` · ${fileLabel(receipt.operation.target.file ?? info.root_uri)}${receipt.source_line ? `:${receipt.source_line}` : ""}`;
      if (receipt.application === "applied" && !receipt.cause) return choice;
      const application = receipt.application === "not_reached" ? "not reached" : receipt.application;
      return `${choice} (${application})${receipt.cause ? ` — ${receipt.cause}` : ""}`;
    })
    : (analysis?.configuration.overrides ?? []).map((operation, index) => overrideChoice(operation, analysis?.override_names?.[index], analysis?.override_branch_labels?.[index]));
  const choices = [
    ...(analysis?.configuration.defines ?? []).map(input => `Macro: ${input.name}=${input.expression}`),
    ...(equations ? equations.values.map(value =>
      `Equation: ${equations.tag ?? "name"}='${value}' (${equations.mode === "include" ? "included" : "excluded"})`) : []),
    ...overrideChoices,
  ];
  const tooltip = new vscode.MarkdownString("[Current config](command:dygnosis.selectAnalysisConfiguration)");
  if (analysis?.state === "stale") tooltip.appendText(" (stale)");
  for (const choice of choices.length ? choices : [name ?? "Source"])
    tooltip.appendMarkdown("  \n").appendText(choice);
  if (name && choices.length) tooltip.appendMarkdown("  \n").appendText(name);
  if (analysis?.include_paths !== null && analysis?.include_paths !== undefined)
    tooltip.appendMarkdown("  \n").appendText(`Include paths: ${analysis.include_paths.join(", ") || "none"}`);
  if (info.root_uri !== info.document_uri) tooltip.appendMarkdown("\n\n").appendText(`Model: ${fileLabel(info.root_uri)}`);
  if (info.complete && analysis?.cause) tooltip.appendMarkdown("\n\n").appendText(analysis.cause.message);
  tooltip.isTrusted = { enabledCommands: ["dygnosis.selectAnalysisConfiguration"] };
  return tooltip;
}

/** Sum scope counts for the status bar; retain each scope's facts in the hover. */
export function modelStatus(info: ModelInfo, counts: StatusCount[]): StatusContent {
  const analysis = info.analysis;
  const configured = analysis && (!automaticConfigurationName(analysis.selection.name) || analysis.configuration.defines?.length ||
    analysis.configuration.overrides?.length || analysis.configuration.equation_selection || analysis.include_paths !== null);
  const label = configured ? "Current config" : "Source";
  const spoken = analysis ? configurationSummary({ ...analysis.configuration, name: analysis.selection.name ?? analysis.configuration.name }, analysis.include_paths) : "Source";
  const tooltip = configurationHover(info);
  if (!info.complete) {
    const { detail, leadingCause } = incompleteDetail(info);
    return {
      text: `$(info) ${label}: ${analysis?.state === "stale" ? "source only" : "incomplete"}`,
      tooltip: tooltip.appendMarkdown("\n\n").appendText(`${detail}\nCounts are unavailable.`),
      accessibilityLabel: `${fileLabel(info.root_uri)}. ${spoken}: ${leadingCause}. Counts are unavailable. ${outlineClickInstruction}`,
    };
  }
  const groups = [
    { title: "Aggregate model", facts: info },
    ...(info.heterogeneity_dimensions ?? []).map(dimension => ({ title: `Heterogeneous model: ${dimension.dimension}`, facts: dimension })),
  ];
  const validCount = (value: unknown): value is number => typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
  const shown = counts.map(count => {
    const values = groups.map(group => group.facts[countDetails[count].field]);
    return { ...countDetails[count], value: values.every(validCount) ? values.reduce((total, value) => total + value, 0) : undefined };
  });
  if (shown.some(count => !validCount(count.value))) return {
    text: `$(info) ${label}: unavailable`,
    tooltip: tooltip.appendMarkdown("\n\n").appendText("Counts withheld: the engine returned an invalid count."),
    accessibilityLabel: `${fileLabel(info.root_uri)}. Counts withheld: the engine returned an invalid count. ${outlineClickInstruction}`,
  };
  const report: string[] = [];
  for (const group of groups) {
    tooltip.appendMarkdown("\n\n");
    tooltip.appendMarkdown("**").appendText(group.title).appendMarkdown("**\n\n");
    const lines = countLines(group.facts, counts);
    lines.forEach((line, index) => {
      if (index) tooltip.appendMarkdown("  \n");
      tooltip.appendText(line);
    });
    report.push(group.title, ...lines);
  }
  return {
    text: `${label}: ${shown.map(count => `$(${count.icon}) ${String(count.value)}`).join("  ")}`,
    tooltip,
    accessibilityLabel: `${fileLabel(info.root_uri)}. ${spoken}. Total: ${shown.map(count => `${String(count.value)} ${count.label}`).join(", ")}. ${report.join(". ")}. ${outlineClickInstruction}`,
  };
}

/** One item follows the displayed root document and the client's chosen owner. */
export function registerStatus(service: DygnosisClient, previews?: EffectivePreviewRegistry): vscode.Disposable {
  const item = vscode.window.createStatusBarItem("dygnosis.modelCounts", vscode.StatusBarAlignment.Left, 40);
  item.name = "Dynare model counts";
  item.command = "outline.focus";
  let generation = 0;
  let disposed = false;
  let displayed: { document: vscode.TextDocument; root?: vscode.Uri; value: StatusContent; current(): boolean } | undefined;
  const content = (value: StatusContent): void => {
    item.text = value.text;
    item.tooltip = value.tooltip;
    item.accessibilityInformation = { label: value.accessibilityLabel };
    item.show();
  };
  const render = (): void => {
    if (!displayed?.current()) return;
    const activity = service.modelActivity(displayed.document.uri, displayed.root);
    content(activity ? {
      text: `$(sync~spin) Dynare: ${activity.label}…`,
      tooltip: new vscode.MarkdownString().appendText(`${activity.detail}\n\n`).appendMarkdown(typeof displayed.value.tooltip === "string" ? displayed.value.tooltip : displayed.value.tooltip.value),
      accessibilityLabel: `${fileLabel((displayed.root ?? displayed.document.uri).toString())}. ${activity.label}. ${outlineClickInstruction}`,
    } : displayed.value);
  };
  const preferences = (document: vscode.TextDocument): { enabled: boolean; counts: StatusCount[] } => ({
    enabled: booleanSetting("statusBar.enabled", document.uri, true, service.log),
    counts: listSetting("statusBar.counts", document.uri, statusCounts, statusCounts, service.log) as StatusCount[],
  });
  const refresh = (): void => {
    const request = ++generation;
    displayed = undefined;
    item.hide();
    item.text = "";
    item.tooltip = undefined;
    item.accessibilityInformation = undefined;
    const document = vscode.window.activeTextEditor?.document;
    const preview = document && previews?.get(document.uri);
    if (disposed || !document || document.isClosed || !isAnalysisDocument(document) && !preview) return;
    const settings = preferences(document);
    if (!settings.enabled || settings.counts.length === 0) return;
    const uri = document.uri.toString(), version = document.version;
    const current = (): boolean => {
      const active = vscode.window.activeTextEditor?.document;
      if (disposed || request !== generation || active !== document || document.isClosed ||
          active.uri.toString() !== uri || active.version !== version) return false;
      const latest = preferences(document);
      return latest.enabled && latest.counts.join("\n") === settings.counts.join("\n");
    };
    displayed = { document, current, value: {
      text: "$(sync~spin) Dynare: updating",
      tooltip: `Model: ${fileLabel(uri)}\nRefreshing model counts…`,
      accessibilityLabel: `Dynare model counts are updating. ${outlineClickInstruction}`,
    } };
    const updateContent = (value: StatusContent): void => {
      if (!current() || !displayed) return;
      displayed.value = value;
      render();
    };
    render();
    const unavailable = (cause: string): void => updateContent({
      text: "$(info) Dynare: unavailable",
      tooltip: `Model: ${fileLabel((displayed?.root ?? document.uri).toString())}\nCounts unavailable — ${cause}`,
      accessibilityLabel: `${fileLabel((displayed?.root ?? document.uri).toString())}. Counts unavailable: ${cause} ${outlineClickInstruction}`,
    });
    const update = async (): Promise<void> => {
      const root = preview?.root ?? await service.rootForDocument(document);
      if (!current()) return;
      if (!root) {
        updateContent({
          text: "$(info) Dynare: choose model owner",
          tooltip: "This file needs a model owner for model counts. Run Dygnosis: Choose model owner from the Command Palette. Open its parent model first if no owner is listed.",
          accessibilityLabel: `Dynare model counts need a model owner. Run Dygnosis: Choose model owner from the Command Palette. ${outlineClickInstruction}`,
        });
        return;
      }
      displayed!.root = root;
      displayed!.value.tooltip = `Model: ${fileLabel(root.toString())}\nRefreshing model counts…`;
      render();
      const client = service.client;
      const requestedDocument = preview ? root : document.uri;
      const info = await service.modelInfo(root, requestedDocument);
      if (!current() || client !== service.client) return;
      if (!info) { unavailable(service.supportsModelInfo ? "The request ended without current model information. Refresh the model." : "This engine does not support model information. Use the bundled binary or update Server Path."); return; }
      if (info.root_uri !== root.toString() || info.document_uri !== requestedDocument.toString() || !preview && info.document_version !== version) {
        unavailable("The response no longer matches this model or document. Counts were withheld."); return;
      }
      updateContent(modelStatus(info, settings.counts));
    };
    void update().catch((error: unknown) => {
      if (current()) { service.log(String(error)); unavailable(failureText(error)); }
    });
  };
  const listeners = [
    vscode.window.onDidChangeActiveTextEditor(refresh),
    vscode.workspace.onDidChangeTextDocument(event => {
      if (event.document === vscode.window.activeTextEditor?.document) refresh();
    }),
    vscode.workspace.onDidCloseTextDocument(document => {
      if (document === vscode.window.activeTextEditor?.document) refresh();
    }),
    vscode.workspace.onDidChangeConfiguration(event => {
      const document = vscode.window.activeTextEditor?.document;
      if (event.affectsConfiguration("dynare.statusBar", document?.uri)) refresh();
    }),
    service.onDidChange(refresh),
    service.onDidChangeActivity(render),
  ];
  refresh();
  return new vscode.Disposable(() => {
    disposed = true;
    displayed = undefined;
    ++generation;
    for (const listener of listeners) listener.dispose();
    item.hide();
    item.dispose();
  });
}
