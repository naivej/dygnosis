import * as vscode from "vscode";
import { randomBytes } from "node:crypto";
import { readFileSync } from "node:fs";
import * as path from "node:path";
import { escapeHtml, HelpBundle, helpMarkdown, helpPages } from "./help_content";
import { record } from "./protocol";

export interface HelpRequest { topic?: string; code?: string; markdown?: string; engineVersion?: string }

/** Help owns one offline panel. Reading never starts the analysis engine. */
export function registerHelp(context: vscode.ExtensionContext): vscode.Disposable {
  const root = vscode.Uri.joinPath(context.extensionUri, "help");
  const media = vscode.Uri.joinPath(context.extensionUri, "media");
  let panel: vscode.WebviewPanel | undefined;
  let ready = false;
  let pending: HelpRequest | undefined;
  let disposed = false;
  let subscriptions: vscode.Disposable[] = [];
  const bundle = JSON.parse(readFileSync(path.join(root.fsPath, "bundle.json"), "utf8")) as HelpBundle;
  const pages = helpPages(bundle);
  const send = (request: HelpRequest): void => {
    if (!panel || !ready) { pending = request; return; }
    const destination = request.code ? `check:${request.code}` : request.topic ?? "get-started";
    const page = pages.find(topic => topic.id === destination.split("#")[0]);
    const extra = request.code && request.markdown ? { id: destination, title: `${request.code}: active engine explanation`, keywords: [request.code],
      html: helpMarkdown(request.markdown, () => undefined), edition: `Active engine ${request.engineVersion ?? "unknown version"}; Help ${bundle.version}` } : undefined;
    void panel.webview.postMessage({ type: "open", destination: page || extra ? destination : "reference", extra });
  };
  const attach = (next: vscode.WebviewPanel): void => {
    panel?.dispose();
    panel = next; ready = false;
    next.webview.options = { enableScripts: true, localResourceRoots: [root, media] };
    const nonce = randomBytes(24).toString("base64");
    const uri = (file: string): string => next.webview.asWebviewUri(vscode.Uri.joinPath(media, file)).toString();
    next.webview.html = `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src ${next.webview.cspSource}; style-src ${next.webview.cspSource}; script-src 'nonce-${nonce}';"><title>Dygnosis Help</title><link rel="stylesheet" href="${escapeHtml(uri("help.css"))}"></head><body><header><strong>Dygnosis Help</strong><span id="edition"></span><button id="back" aria-label="Back" type="button">←</button><button id="forward" aria-label="Forward" type="button">→</button><button id="permalink" type="button">Copy topic link</button><label for="search">Search</label><input id="search" type="search" placeholder="Feature, command, setting or check"><button id="clear" type="button">Clear search</button></header><div class="layout"><nav aria-label="Help contents" id="contents"></nav><main tabindex="-1" id="main"><p id="breadcrumb"></p><div id="results" role="region" aria-label="Search results"></div><article id="article"></article><p id="notice" role="status" aria-live="polite"></p></main></div><dialog id="enlarged"><button id="close-image" type="button">Close image</button><img alt=""></dialog><script nonce="${nonce}" src="${escapeHtml(uri("help.js"))}"></script></body></html>`;
    subscriptions.push(next.onDidDispose(() => {
      if (panel !== next) return;
      panel = undefined; ready = false;
      const listeners = subscriptions; subscriptions = [];
      listeners.forEach(listener => { listener.dispose(); });
    }), next.webview.onDidReceiveMessage(async (message: unknown) => {
      if (panel !== next || !record(message)) return;
      try {
      if (message.type === "ready") {
        ready = true;
        await next.webview.postMessage({ type: "bundle", version: bundle.version, topics: pages.map(page => ({ ...page, html: helpMarkdown(page.markdown, file => next.webview.asWebviewUri(vscode.Uri.joinPath(root, file)).toString()) })) });
        if (pending) { const request = pending; pending = undefined; send(request); }
      } else if (message.type === "permalink" && typeof message.destination === "string" && pages.some(page => page.id === String(message.destination).split("#")[0])) {
        await vscode.env.clipboard.writeText(`${vscode.env.uriScheme}://${context.extension.id}/help?topic=${encodeURIComponent(message.destination)}`);
        await next.webview.postMessage({ type: "notice", text: "Topic link copied." });
      } else if (message.type === "copy" && typeof message.text === "string" && message.text.length < 1024 * 1024) {
        await vscode.env.clipboard.writeText(message.text);
        await next.webview.postMessage({ type: "notice", text: "Example copied." });
      } else if (message.type === "link" && typeof message.target === "string") {
        const target = message.target;
        if (target.startsWith("settings:") && Object.hasOwn(bundle.settingsTopics, target.slice(9))) await vscode.commands.executeCommand("workbench.action.openSettings", `@id:${target.slice(9)}`);
        else if (target === "action:shortcuts") await vscode.commands.executeCommand("workbench.action.openGlobalKeybindings", "dygnosis");
        else if (target === "action:output") await vscode.commands.executeCommand("dygnosis.showOutput");
        else if (target === "action:restart") await vscode.commands.executeCommand("dygnosis.restartServer");
        else if (/^https?:\/\//.test(target)) await vscode.env.openExternal(vscode.Uri.parse(target));
      }
      } catch (error) {
        await next.webview.postMessage({ type: "notice", text: `The Help action failed: ${String(error)}` });
      }
    }));
  };
  const command = vscode.commands.registerCommand("dygnosis.openHelp", (argument?: unknown) => {
    const request: HelpRequest = typeof argument === "string" ? { topic: argument } : record(argument) ? {
      topic: typeof argument.topic === "string" ? argument.topic : undefined,
      code: typeof argument.code === "string" && /^[A-Z]\d{3}$/.test(argument.code) ? argument.code : undefined,
      markdown: typeof argument.markdown === "string" ? argument.markdown : undefined,
      engineVersion: typeof argument.engineVersion === "string" ? argument.engineVersion : undefined,
    } : { topic: vscode.window.activeTextEditor?.document.uri.scheme === "dygnosis-effective" ? "effective-model" : "get-started" };
    if (!panel) attach(vscode.window.createWebviewPanel("dygnosis.help", "Dygnosis Help", vscode.ViewColumn.Active, { enableScripts: true, retainContextWhenHidden: true, localResourceRoots: [root, media] }));
    panel?.reveal(); send(request);
  });
  const serializer = vscode.window.registerWebviewPanelSerializer("dygnosis.help", { deserializeWebviewPanel: next => { attach(next); return Promise.resolve(); } });
  const links = vscode.window.registerUriHandler({ handleUri: uri => {
    const topic = new URLSearchParams(uri.query).get("topic");
    if (uri.path === "/help" && topic && pages.some(page => page.id === topic.split("#")[0])) void vscode.commands.executeCommand("dygnosis.openHelp", topic);
  } });
  if (!context.globalState.get<boolean>("helpInvitationShown")) {
    void context.globalState.update("helpInvitationShown", true).then(async () => {
      if (disposed) return;
      const action = await vscode.window.showInformationMessage("Dygnosis is ready. Open Help to start a model or explore its features.", "Open Help");
      if (action) await vscode.commands.executeCommand("dygnosis.openHelp", "get-started");
    });
  }
  return vscode.Disposable.from(command, serializer, links, new vscode.Disposable(() => { disposed = true; panel?.dispose(); subscriptions.forEach(subscription => { subscription.dispose(); }); }));
}
