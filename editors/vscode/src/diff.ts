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
<main><h1 class="sr-only">Changes</h1><p class="breadcrumb">Dygnosis <span aria-hidden="true">›</span> Changes</p>
<header class="comparison-heading"><div id="models" class="comparison-inputs"></div><label class="presentation-choice">Presentation <select id="presentation"><option value="focusedReview">Focused review</option><option value="changeList">Change list</option></select></label>
<div class="comparison-actions"><button id="changeComparison" type="button">Change comparison…</button><button id="swap" type="button" title="Swap Before and After">Swap sides</button><button id="refresh" type="button">Refresh</button>
<details id="moreActions" class="more-actions"><summary>More actions</summary><div class="more-actions-panel"><button id="rootTextDiff" type="button">Root file text diff</button><button id="updateRevision" type="button">Update revision</button><button id="details" type="button">Details</button><button id="help" type="button">Help</button><label>Expansion <select id="expansion"><option value="changes">Changes</option><option value="all">All</option><option value="none">None</option></select></label><p id="folders"></p></div></details></div></header>
<div id="tabs" role="tablist" aria-label="Comparison detail"><button id="modelTab" type="button" role="tab" aria-controls="results">Model changes <span id="modelCount" class="count"></span></button><button id="sourceTab" type="button" role="tab" aria-controls="results">Source changes <span id="sourceCount" class="count"></span></button><button id="coverageTab" type="button" role="tab" aria-controls="results">Coverage</button></div>
<div id="filterTools" class="tools"><label class="search"><span class="sr-only">Find changes</span><input id="search" type="search" placeholder="Find changes"></label>
<label id="kindFilter">Kind <select id="kinds" aria-label="Change kind"></select></label><label id="sectionFilter">Section <select id="sections" aria-label="Section"></select></label><label id="scopeFilter">Scope <select id="scope"><option value="all">All scopes</option></select></label>
<label>Detail <select id="layout" aria-label="Before and After layout"><option value="auto">Auto</option><option value="sideBySide">Side by side</option><option value="stacked">Stacked</option></select></label></div>
<div class="capture-status"><p id="status" role="status" aria-live="polite">Loading comparison…</p><button id="choosePath" type="button" hidden>Choose model path</button></div>
<p id="counts" aria-live="polite"></p><div id="results" role="tabpanel"></div>
<footer class="footer" aria-label="Change colors"><span><b class="change-mark added" aria-hidden="true">+</b> Added</span><span><b class="change-mark removed" aria-hidden="true">−</b> Removed</span><span><b class="change-mark changed" aria-hidden="true">~</b> Changed</span><span><b class="change-mark unpaired" aria-hidden="true">?</b> Unpaired</span><span id="sourceBoundary" class="boundary">Review Coverage for the captured source boundary</span></footer>
</main><script nonce="${nonce}" src="${script.toString()}"></script></body></html>`;
}
