import * as vscode from "vscode";
import { randomBytes } from "node:crypto";
import { listSetting } from "./settings";
import { allDiffSections, changeKinds, DiffPreferences } from "./diff_view";

export function diffPreferences(root: vscode.Uri, log: (message: string) => void): DiffPreferences {
  const config = vscode.workspace.getConfiguration("dynare", { uri: root, languageId: "dynare" });
  const choice = <T extends string>(key: string, allowed: T[], fallback: T): T => {
    const value = config.get<unknown>(key, fallback);
    if (typeof value === "string" && allowed.includes(value as T)) return value as T;
    log(`Invalid dynare.${key}; using ${fallback}.`); return fallback;
  };
  return {
    presentation: choice<"focusedReview" | "changeList">("diff.presentation", ["focusedReview", "changeList"], "focusedReview"),
    layout: choice("diff.layout", ["auto", "sideBySide", "stacked"], "auto"),
    expansion: choice("diff.defaultExpansion", ["changes", "all", "none"], "changes"),
    sections: listSetting("diff.sections", root, [...allDiffSections], [...allDiffSections], log) as DiffPreferences["sections"],
    changeKinds: listSetting("diff.defaultChangeKinds", root, [...changeKinds], [...changeKinds], log) as DiffPreferences["changeKinds"],
  };
}

export function diffHtml(webview: vscode.Webview, assets: vscode.Uri): string {
  const nonce = randomBytes(24).toString("base64");
  const script = webview.asWebviewUri(vscode.Uri.joinPath(assets, "diff_view.js"));
  const style = webview.asWebviewUri(vscode.Uri.joinPath(assets, "diff_view.css"));
  return `<!DOCTYPE html><html lang="en"><head><meta charset="UTF-8"><meta name="viewport" content="width=device-width, initial-scale=1.0">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src ${webview.cspSource}; script-src 'nonce-${nonce}';">
<link rel="stylesheet" href="${style.toString()}"><title>Dygnosis Changes</title></head><body>
<main><h1>Changes</h1><div class="comparison-heading"><div id="models"></div><label>Presentation <select id="presentation"><option value="focusedReview">Focused review</option><option value="changeList">Change list</option></select></label></div><p id="folders"></p><p id="status" role="status" aria-live="polite">Loading comparison…</p>
<div class="toolbar"><button id="changeComparison" type="button">Change comparison…</button><button id="swap" type="button">Swap sides</button><button id="rootTextDiff" type="button">Root file text diff</button><button id="updateRevision" type="button">Update revision</button><button id="choosePath" type="button">Choose model path</button><button id="details" type="button">Details</button></div>
<div class="toolbar"><button id="refresh" type="button">Refresh</button><button id="help" type="button">Help</button><label>Search <input id="search" type="search" placeholder="Names, values, or equations"></label>
<label>Scope <select id="scope"><option value="all">All scopes</option></select></label>
<label>Layout <select id="layout"><option value="auto">Auto</option><option value="sideBySide">Side by side</option><option value="stacked">Stacked</option></select></label>
<label>Expansion <select id="expansion"><option value="changes">Changes</option><option value="all">All</option><option value="none">None</option></select></label></div>
<fieldset id="kinds"><legend>Change kinds</legend></fieldset><fieldset id="sections"><legend>Sections</legend></fieldset>
<div id="tabs" role="tablist" aria-label="Comparison detail"><button id="modelTab" type="button" role="tab" aria-controls="results">Model changes</button><button id="sourceTab" type="button" role="tab" aria-controls="results">Source changes</button><button id="coverageTab" type="button" role="tab" aria-controls="results">Coverage</button></div>
<p id="counts" aria-live="polite"></p><div id="results" role="tabpanel"></div></main><script nonce="${nonce}" src="${script.toString()}"></script></body></html>`;
}
