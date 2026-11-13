import * as vscode from "vscode";
import { randomBytes } from "node:crypto";
import { listSetting } from "../client/settings";
import { changeKinds, DiffPreferences, normalizeChangeKinds } from "./diff_view";

export function diffPreferences(root: vscode.Uri, log: (message: string) => void): DiffPreferences {
  return {
    changeKinds: normalizeChangeKinds(listSetting("diff.defaultChangeKinds", root, [...changeKinds], [...changeKinds], log), [...changeKinds]),
  };
}

export function diffHtml(webview: vscode.Webview, assets: vscode.Uri): string {
  const nonce = randomBytes(24).toString("base64");
  const script = webview.asWebviewUri(vscode.Uri.joinPath(assets, "diff_view.js"));
  const style = webview.asWebviewUri(vscode.Uri.joinPath(assets, "diff_view.css"));
  return `<!DOCTYPE html><html lang="en"><head><meta charset="UTF-8"><meta name="viewport" content="width=device-width, initial-scale=1.0">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src ${webview.cspSource}; script-src 'nonce-${nonce}';">
<link rel="stylesheet" href="${style.toString()}"><title>Dygnosis Changes</title></head><body>
<main><h1 class="sr-only">Changes</h1>
<div class="comparison-body">
<header class="comparison-heading"><div id="models" class="comparison-inputs"></div></header>
<div id="filterTools" class="tools"><label class="search"><span class="sr-only">Find changes</span><input id="search" type="search" placeholder="Find changes"></label>
<div id="kindToggles" class="kind-toggles" role="group" aria-label="Change kinds"></div>
<details id="kinds" class="change-type-filter"></details>
</div>
<p id="counts" aria-live="polite"></p>
<div class="capture-status"><p id="status" role="status" aria-live="polite">Loading comparison…</p><button id="choosePath" type="button" hidden>Choose model path</button></div>
<p id="limitsStatus" class="comparison-limits" role="status" hidden></p>
<div id="reviewContent" class="review-content"><div id="results" aria-label="Model changes"></div></div>
</div>
<footer class="footer"><button id="help" type="button">Help</button></footer>
</main><script nonce="${nonce}" src="${script.toString()}"></script></body></html>`;
}
