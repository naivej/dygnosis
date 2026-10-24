import * as vscode from "vscode";
import { DygnosisClient, isAnalysisDocument } from "../client/client";
import { ModelInfo } from "../client/protocol";
import { booleanSetting, listSetting } from "../client/settings";
import type { EffectivePreviewRegistry } from "./preview";

export const statusCounts = ["endogenous", "exogenous", "equations"];
export type StatusCount = "endogenous" | "exogenous" | "equations";
interface StatusContent { text: string; tooltip: string | vscode.MarkdownString; accessibilityLabel: string }
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

function incompleteDetail(info: ModelInfo): string {
  const reasons = new Set<string>();
  const cause = info.analysis?.cause?.message.trim();
  if (cause) reasons.add(cause);
  for (const reason of Array.isArray(info.incomplete_reasons) ? info.incomplete_reasons : []) {
    const message = typeof reason?.message === "string" ? reason.message.trim() : "";
    if (message) reasons.add(message);
  }
  if (reasons.size > 0) return [...reasons].join("\n");
  return info.message ?? "Model expansion is incomplete.";
}

function configurationHover(info: ModelInfo): vscode.MarkdownString {
  const analysis = info.analysis;
  const equations = analysis?.configuration.equation_selection;
  const name = /^Analysis preview(?: \(modified\))?$/.test(analysis?.selection.name ?? "") ? undefined : analysis?.selection.name;
  const choices = [
    ...(analysis?.configuration.defines ?? []).map(input => `Macro: ${input.name}=${input.expression}`),
    ...(equations ? equations.values.map(value =>
      `Equation: ${equations.tag ?? "name"}='${value}' (${equations.mode === "include" ? "included" : "excluded"})`) : []),
    ...(analysis?.configuration.overrides ?? []).map(operation => operation.kind === "definition"
      ? `Definition override = ${operation.expression}`
      : operation.kind === "skip_definition" ? "Skip written definition" : operation.branch_id === null ? "Preview no branch" : "Forced branch preview"),
  ];
  const tooltip = new vscode.MarkdownString("[Current config](command:dygnosis.selectAnalysisConfiguration)");
  if (analysis?.state === "stale") tooltip.appendText(" (stale)");
  for (const choice of choices.length ? choices : [name ?? "Source"])
    tooltip.appendMarkdown("  \n").appendText(choice);
  if (name && choices.length) tooltip.appendMarkdown("  \n").appendText(name);
  for (const receipt of analysis?.overrides ?? []) {
    if (receipt.application !== "applied" || receipt.cause) tooltip.appendMarkdown("  \n").appendText(
      `${receipt.operation.kind === "branch" ? "Branch preview" : "Definition override"}: ${receipt.application === "not_reached" ? "not reached" : receipt.application}${receipt.cause ? ` — ${receipt.cause}` : ""}`);
  }
  if (analysis?.include_paths !== null && analysis?.include_paths !== undefined)
    tooltip.appendMarkdown("  \n").appendText(`Include paths: ${analysis.include_paths.join(", ") || "none"}`);
  if (info.complete && analysis?.cause) tooltip.appendMarkdown("\n\n").appendText(analysis.cause.message);
  tooltip.isTrusted = { enabledCommands: ["dygnosis.selectAnalysisConfiguration"] };
  return tooltip;
}

/** Sum scope counts for the status bar; retain each scope's facts in the hover. */
export function modelStatus(info: ModelInfo, counts: StatusCount[]): StatusContent {
  const analysis = info.analysis;
  const configured = analysis && (analysis.selection.name || analysis.configuration.defines?.length ||
    analysis.configuration.overrides?.length || analysis.configuration.equation_selection || analysis.include_paths !== null);
  const label = configured ? "Current config" : "Source";
  const tooltip = configurationHover(info);
  if (!info.complete) {
    const detail = incompleteDetail(info);
    return {
      text: `$(info) ${label}: ${analysis?.state === "stale" ? "source only" : "incomplete"}`,
      tooltip: tooltip.appendMarkdown("\n\n").appendText(`${detail}\nCounts are unavailable.`),
      accessibilityLabel: "Dynare model expansion is incomplete. Counts are unavailable. Choose the analysis configuration.",
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
    tooltip: tooltip.appendMarkdown("\n\n").appendText("Model counts are unavailable."),
    accessibilityLabel: "Dynare model counts are unavailable. Choose the analysis configuration.",
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
    accessibilityLabel: `Dynare model counts. Total: ${shown.map(count => `${String(count.value)} ${count.label}`).join(", ")}. ${report.join(". ")}. Choose the analysis configuration.`,
  };
}

/** One item follows the displayed root document and the client's chosen owner. */
export function registerStatus(service: DygnosisClient, previews?: EffectivePreviewRegistry): vscode.Disposable {
  const item = vscode.window.createStatusBarItem("dygnosis.modelCounts", vscode.StatusBarAlignment.Left, 40);
  item.name = "Dynare model counts";
  item.command = "dygnosis.selectAnalysisConfiguration";
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
      tooltip: activity.detail,
      accessibilityLabel: `Dynare: ${activity.label}. ${activity.pending} pending model request${activity.pending === 1 ? "" : "s"}. Choose the analysis configuration.`,
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
      tooltip: "Refreshing model counts…",
      accessibilityLabel: "Dynare model counts are updating. Choose the analysis configuration.",
    } };
    const updateContent = (value: StatusContent): void => {
      if (!current() || !displayed) return;
      displayed.value = value;
      render();
    };
    render();
    const unavailable = (): void => updateContent({
      text: "$(info) Dynare: unavailable",
      tooltip: service.supportsModelInfo
        ? "Model counts are unavailable. Choose a model root, or open Dygnosis Output for details."
        : "Model counts are unavailable. Restart the Dygnosis language server, or use the bundled binary in Settings.",
      accessibilityLabel: "Dynare model counts are unavailable. Choose the analysis configuration.",
    });
    const update = async (): Promise<void> => {
      const root = preview?.root ?? await service.rootForDocument(document);
      if (!current()) return;
      if (!root) {
        updateContent({
          text: "$(info) Dynare: choose model owner",
          tooltip: "This file needs a model owner for model counts. Run Dygnosis: Choose model owner from the Command Palette. Open its parent model first if no owner is listed.",
          accessibilityLabel: "Dynare model counts need a model owner. Run Dygnosis: Choose model owner from the Command Palette.",
        });
        return;
      }
      displayed!.root = root;
      render();
      const client = service.client;
      const requestedDocument = preview ? root : document.uri;
      const info = await service.modelInfo(root, requestedDocument);
      if (!current() || client !== service.client) return;
      if (!info) { unavailable(); return; }
      if (info.root_uri !== root.toString() || info.document_uri !== requestedDocument.toString() || !preview && info.document_version !== version) {
        unavailable(); return;
      }
      updateContent(modelStatus(info, settings.counts));
    };
    void update().catch((error: unknown) => {
      if (current()) { service.log(String(error)); unavailable(); }
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
