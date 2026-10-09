import * as vscode from "vscode";
import { randomBytes } from "node:crypto";
import { listSetting } from "./settings";
import { changeKinds, DiffPreferences } from "./diff_view";

export function diffPreferences(root: vscode.Uri, log: (message: string) => void): DiffPreferences {
  return {
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
<main><div id="reviewContent" class="review-content"><h1 class="sr-only">Changes</h1><p class="breadcrumb">Dygnosis <span aria-hidden="true">›</span> Changes</p>
<header class="comparison-heading"><div class="comparison-selector"><div id="models" class="comparison-inputs"></div>
<button id="changeComparison" type="button">Change comparison…</button><button id="swap" type="button" title="Swap Before and After">Swap sides</button></div><div class="comparison-actions"><button id="refresh" type="button">Refresh</button>
<button id="rootTextDiff" type="button">Text diff</button><button id="capturedTextDiff" type="button" hidden>Text diff…</button></div></header>
<div id="filterTools" class="tools"><label class="search"><span class="sr-only">Find changes</span><input id="search" type="search" placeholder="Find changes"></label>
<label id="kindFilter">Kind <select id="kinds" aria-label="Change kind"></select></label>
</div>
<div class="capture-status"><p id="status" role="status" aria-live="polite">Loading comparison…</p><button id="choosePath" type="button" hidden>Choose model path</button></div>
<p id="counts" aria-live="polite"></p><div id="results" aria-label="Model changes"></div></div>
<footer class="footer" aria-label="Change colors"><span><b class="change-mark added" aria-hidden="true">+</b> Added</span><span><b class="change-mark removed" aria-hidden="true">−</b> Removed</span><span><b class="change-mark changed" aria-hidden="true">~</b> Changed</span><span><b class="change-mark unpaired" aria-hidden="true">?</b> Unpaired</span><span id="sourceBoundary" class="boundary">Captured source boundary unavailable</span><button id="help" type="button">Help</button></footer>
</main><script nonce="${nonce}" src="${script.toString()}"></script></body></html>`;
}
