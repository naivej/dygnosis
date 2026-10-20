import * as vscode from "vscode";
import { DygnosisClient, isAnalysisDocument, isRootUri } from "../client/client";
import { ModelInfo } from "../client/protocol";
import { booleanSetting, listSetting } from "../client/settings";

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
  const reasons = Array.isArray(info.incomplete_reasons)
    ? info.incomplete_reasons
      .map(reason => typeof reason?.message === "string" ? reason.message.trim() : "")
      .filter(message => message.length > 0)
    : [];
  if (reasons.length > 0) return reasons.join("\n");
  return info.message ?? "Model expansion is incomplete.";
}

/** Sum scope counts for the status bar; retain each scope's facts in the hover. */
export function modelStatus(info: ModelInfo, counts: StatusCount[]): StatusContent {
  if (!info.complete) {
    const detail = incompleteDetail(info);
    return {
      text: "$(info) Dynare: incomplete",
      tooltip: `${detail}\nCounts are unavailable.`,
      accessibilityLabel: "Dynare model expansion is incomplete. Counts are unavailable. Focus Outline.",
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
    text: "$(info) Dynare: unavailable",
    tooltip: "Model counts are unavailable.",
    accessibilityLabel: "Dynare model counts are unavailable. Focus Outline.",
  };
  const tooltip = new vscode.MarkdownString(), report: string[] = [];
  for (const group of groups) {
    if (report.length) tooltip.appendMarkdown("\n\n");
    tooltip.appendMarkdown("**").appendText(group.title).appendMarkdown("**\n\n");
    const lines = countLines(group.facts, counts);
    lines.forEach((line, index) => {
      if (index) tooltip.appendMarkdown("  \n");
      tooltip.appendText(line);
    });
    report.push(group.title, ...lines);
  }
  return {
    text: shown.map(count => `$(${count.icon}) ${String(count.value)}`).join("  "),
    tooltip,
    accessibilityLabel: `Dynare model counts. Total: ${shown.map(count => `${String(count.value)} ${count.label}`).join(", ")}. ${report.join(". ")}. Focus Outline.`,
  };
}

/** One item follows the displayed root document and the client's chosen owner. */
export function registerStatus(service: DygnosisClient): vscode.Disposable {
  const item = vscode.window.createStatusBarItem("dygnosis.modelCounts", vscode.StatusBarAlignment.Left, 40);
  item.name = "Dynare model counts";
  item.command = "outline.focus";
  let generation = 0;
  let disposed = false;
  const content = (value: StatusContent): void => {
    item.text = value.text;
    item.tooltip = value.tooltip;
    item.accessibilityInformation = { label: value.accessibilityLabel };
    item.show();
  };
  const preferences = (document: vscode.TextDocument): { enabled: boolean; counts: StatusCount[] } => ({
    enabled: booleanSetting("statusBar.enabled", document.uri, true, service.log),
    counts: listSetting("statusBar.counts", document.uri, statusCounts, statusCounts, service.log) as StatusCount[],
  });
  const refresh = (): void => {
    const request = ++generation;
    item.hide();
    item.text = "";
    item.tooltip = undefined;
    item.accessibilityInformation = undefined;
    const document = vscode.window.activeTextEditor?.document;
    if (disposed || !document || document.isClosed || !isAnalysisDocument(document) || !isRootUri(document.uri)) return;
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
    content({
      text: "$(sync~spin) Dynare: updating",
      tooltip: "Refreshing model counts…",
      accessibilityLabel: "Dynare model counts are updating. Focus Outline.",
    });
    const unavailable = (): void => content({
      text: "$(info) Dynare: unavailable",
      tooltip: service.supportsModelInfo
        ? "Model counts are unavailable. Choose a model root, or open Dygnosis Output for details."
        : "Model counts are unavailable. Restart the Dygnosis language server, or use the bundled binary in Settings.",
      accessibilityLabel: "Dynare model counts are unavailable. Focus Outline.",
    });
    const update = async (): Promise<void> => {
      const root = await service.rootForDocument(document);
      if (!current()) return;
      if (!root) {
        content({
          text: "$(info) Dynare: choose model owner",
          tooltip: "This file needs a model owner for model counts. Run Dygnosis: Choose model owner from the Command Palette. Open its parent model first if no owner is listed.",
          accessibilityLabel: "Dynare model counts need a model owner. Run Dygnosis: Choose model owner from the Command Palette.",
        });
        return;
      }
      const client = service.client;
      const info = await service.modelInfo(root, document.uri);
      if (!current() || client !== service.client) return;
      if (!info) { unavailable(); return; }
      if (info.root_uri !== root.toString() || info.document_uri !== uri || info.document_version !== version) {
        unavailable(); return;
      }
      content(modelStatus(info, settings.counts));
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
  ];
  refresh();
  return new vscode.Disposable(() => {
    disposed = true;
    ++generation;
    for (const listener of listeners) listener.dispose();
    item.hide();
    item.dispose();
  });
}
