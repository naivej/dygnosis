import * as vscode from "vscode";
import { DygnosisClient, isAnalysisDocument, isRootUri } from "./client";
import { Dimension, ModelInfo } from "./protocol";
import { booleanSetting, listSetting } from "./settings";

export const statusCounts = ["endogenous", "exogenous", "equations"];
export type StatusCount = "endogenous" | "exogenous" | "equations";
interface StatusContent { text: string; tooltip: string; accessibilityLabel: string }
const countDetails: Record<StatusCount, { field: "n_endogenous" | "n_exogenous" | "n_equations"; icon: string; label: string }> = {
  endogenous: { field: "n_endogenous", icon: "symbol-variable", label: "Endogenous" },
  exogenous: { field: "n_exogenous", icon: "symbol-field", label: "Exogenous" },
  equations: { field: "n_equations", icon: "list-ordered", label: "Equations" },
};

function timingLine(info: Pick<ModelInfo, "static" | "predetermined" | "forward_looking" | "mixed">): string {
  return `Timing: ${info.static?.length ?? 0} static, ${info.predetermined?.length ?? 0} predetermined, ${info.forward_looking?.length ?? 0} forward-looking, ${info.mixed?.length ?? 0} mixed`;
}
function dimensionLines(dimension: Dimension): string[] {
  return [
    `Dimension ${dimension.dimension}: ${dimension.n_endogenous} endogenous, ${dimension.n_exogenous} exogenous, ${dimension.n_equations} equations`,
    timingLine(dimension),
  ];
}

/** Format only engine facts. Aggregate and dimension counts stay separate. */
export function modelStatus(info: ModelInfo, counts: StatusCount[], modelLabel: string): StatusContent {
  if (!info.complete) return {
    text: "$(info) Dynare: incomplete",
    tooltip: `${modelLabel}\n${info.message ?? "Model expansion is incomplete."}\nCounts are unavailable.\nClick to focus Outline.`,
    accessibilityLabel: "Dynare model expansion is incomplete. Counts are unavailable. Focus Outline.",
  };
  const shown = counts.map(count => ({ ...countDetails[count], value: info[countDetails[count].field] }));
  if (shown.some(count => typeof count.value !== "number" || !Number.isSafeInteger(count.value) || count.value < 0)) return {
    text: "$(info) Dynare: unavailable",
    tooltip: `${modelLabel}\nModel counts are unavailable.\nClick to focus Outline.`,
    accessibilityLabel: "Dynare model counts are unavailable. Focus Outline.",
  };
  const labels = shown.map(count => `${count.label}: ${String(count.value)}`);
  return {
    text: shown.map(count => `$(${count.icon}) ${String(count.value)}`).join("  "),
    tooltip: [
      modelLabel,
      "Aggregate model before transformation",
      ...labels,
      timingLine(info),
      ...(info.heterogeneity_dimensions ?? []).flatMap(dimensionLines),
      "Click to focus Outline.",
    ].join("\n"),
    accessibilityLabel: `Dynare aggregate model. ${labels.join(". ")}. Focus Outline.`,
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
      tooltip: "Refreshing model counts.\nClick to focus Outline.",
      accessibilityLabel: "Dynare model counts are updating. Focus Outline.",
    });
    const unavailable = (): void => content({
      text: "$(info) Dynare: unavailable",
      tooltip: service.supportsModelInfo
        ? "Model counts are unavailable. Choose a model root, or open Dygnosis Output for details.\nClick to focus Outline."
        : "Model counts are unavailable. Restart the Dygnosis language server, or use the bundled binary in Settings.\nClick to focus Outline.",
      accessibilityLabel: "Dynare model counts are unavailable. Focus Outline.",
    });
    const update = async (): Promise<void> => {
      const root = await service.rootForDocument(document);
      if (!current()) return;
      if (!root) { unavailable(); return; }
      const client = service.client;
      const info = await service.modelInfo(root, document.uri);
      if (!current() || client !== service.client) return;
      if (!info) { unavailable(); return; }
      if (info.root_uri !== root.toString() || info.document_uri !== uri || info.document_version !== version) {
        unavailable(); return;
      }
      content(modelStatus(info, settings.counts, vscode.workspace.asRelativePath(root)));
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
