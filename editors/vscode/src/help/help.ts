import * as vscode from "vscode";
import { randomBytes } from "node:crypto";
import { readFileSync } from "node:fs";
import * as path from "node:path";
import { escapeHtml, helpMarkdown, PreparedHelpBundle, resolveHelpAssets, userDiagnosticMarkdown } from "./help_content";
import { record } from "../client/protocol";
import { changesViewType } from "../comparison/changes_resource";

export interface HelpRequest { topic?: string; code?: string; markdown?: string; engineVersion?: string }

function helpDestination(destination: string): string {
  const [topic, section] = destination.split("#", 2);
  if (topic === "troubleshoot") return section === "report-a-problem" ? "about#report-a-problem" : "about#support";
  return destination;
}

/** Help owns one offline panel. Reading never starts the analysis engine. */
export function registerHelp(context: vscode.ExtensionContext): vscode.Disposable {
  const root = vscode.Uri.joinPath(context.extensionUri, "help");
  const media = vscode.Uri.joinPath(context.extensionUri, "media");
  let panel: vscode.WebviewPanel | undefined;
  let ready = false;
  let walkthroughHelpOpened = false;
  let pending: HelpRequest | undefined;
  let pendingBundle: PreparedHelpBundle | undefined;
  let subscriptions: vscode.Disposable[] = [];
  let topicIds: Set<string> | undefined;
  let settingsTopics: PreparedHelpBundle["settingsTopics"] = {};
  const readBundle = (): PreparedHelpBundle => {
    const bundle = JSON.parse(readFileSync(path.join(root.fsPath, "bundle.json"), "utf8")) as PreparedHelpBundle;
    topicIds ??= new Set(bundle.topics.map(topic => topic.id));
    settingsTopics = bundle.settingsTopics;
    return bundle;
  };
  const explanationPage = (request: HelpRequest) => request.code && request.markdown ? {
    id: `check:${request.code}`, title: `${request.code}: active engine explanation`,
    html: helpMarkdown(`${userDiagnosticMarkdown(request.markdown)}\n\n[Use diagnostic actions](help:diagnostics) · [Diagnostic code index](help:reference#diagnostic-codes)`, () => undefined),
    edition: request.engineVersion ? `Explanation from engine ${request.engineVersion}` : "Explanation from the active engine",
    explanation: { code: request.code, markdown: request.markdown, engineVersion: request.engineVersion },
  } : undefined;
  const send = (request: HelpRequest): void => {
    if (!panel || !ready) { pending = request; return; }
    const destination = request.code ? `check:${request.code}` : helpDestination(request.topic ?? "get-started");
    const known = topicIds?.has(destination.split("#")[0]);
    const extra = explanationPage(request);
    void panel.webview.postMessage({ type: "open", destination: known || extra ? destination : "reference", extra });
  };
  const attach = (next: vscode.WebviewPanel): void => {
    panel?.dispose();
    panel = next; ready = false;
    next.webview.options = { enableScripts: true, localResourceRoots: [root, media] };
    const nonce = randomBytes(24).toString("base64");
    const uri = (file: string): string => next.webview.asWebviewUri(vscode.Uri.joinPath(media, file)).toString();
    next.webview.html = `<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src ${next.webview.cspSource}; style-src ${next.webview.cspSource}; script-src 'nonce-${nonce}';">
  <title>Dygnosis Help</title>
  <link rel="stylesheet" href="${escapeHtml(uri("help.css"))}">
</head>
<body>
  <header>
    <div class="search">
      <label class="sr-only" for="search">Search Help</label>
      <input id="search" type="search" maxlength="300" placeholder="Search Help" aria-controls="results">
      <button id="clear" type="button" hidden>Clear search</button>
    </div>
    <div class="actions" role="group" aria-label="Topic actions">
      <button id="back" type="button">Back</button>
      <button id="forward" type="button">Forward</button>
      <button id="permalink" type="button">Copy topic link</button>
    </div>
  </header>
  <div class="layout">
    <details id="topics" open>
      <summary>Topics</summary>
      <nav aria-label="Help topics" id="contents"></nav>
    </details>
    <main tabindex="-1" id="main">
      <section id="results" aria-label="Search results" hidden>
        <p id="result-count" role="status"></p>
        <ul id="result-list"></ul>
      </section>
      <p id="breadcrumb" hidden></p>
      <p id="edition" hidden></p>
      <article id="article"></article>
    </main>
  </div>
  <p id="notice" role="status"></p>
  <dialog id="enlarged"><button id="close-image" type="button">Close image</button><img alt=""></dialog>
  <script nonce="${nonce}" src="${escapeHtml(uri("help.js"))}"></script>
</body>
</html>`;
    subscriptions.push(next.onDidDispose(() => {
      if (panel !== next) return;
      panel = undefined; ready = false; pending = undefined; pendingBundle = undefined;
      const listeners = subscriptions; subscriptions = [];
      listeners.forEach(listener => { listener.dispose(); });
    }), next.webview.onDidReceiveMessage(async (message: unknown) => {
      if (panel !== next || !record(message)) return;
      try {
      if (message.type === "ready") {
        const historyDetails = Array.isArray(message.history) ? message.history.slice(-100).map((entry: unknown) => {
          if (!record(entry) || typeof entry.destination !== "string") return undefined;
          const destination = helpDestination(entry.destination);
          const source = entry.explanation;
          if (!record(source) || typeof source.code !== "string" || !/^[A-Z]\d{3}$/.test(source.code) || destination.split("#")[0] !== `check:${source.code}` || typeof source.markdown !== "string" || source.markdown.length > 1024 * 1024) return { destination };
          const extra = explanationPage({ code: source.code, markdown: source.markdown, engineVersion: typeof source.engineVersion === "string" ? source.engineVersion.slice(0, 300) : undefined });
          return { destination, extra };
        }) : [];
        const bundle = pendingBundle ?? readBundle();
        pendingBundle = undefined;
        const topics = resolveHelpAssets(bundle, file => next.webview.asWebviewUri(vscode.Uri.joinPath(root, file)).toString());
        const delivered = await next.webview.postMessage({ type: "bundle", historyDetails, topics });
        if (panel !== next) return;
        ready = delivered;
        if (!walkthroughHelpOpened && delivered) {
          walkthroughHelpOpened = true;
          void vscode.commands.executeCommand("setContext", "dygnosis.walkthroughHelpOpened", true);
        }
        if (pending) { const request = pending; pending = undefined; send(request); }
      } else if (message.type === "permalink" && typeof message.destination === "string" && topicIds?.has(message.destination.split("#")[0])) {
        await vscode.env.clipboard.writeText(`${vscode.env.uriScheme}://${context.extension.id}/help?topic=${encodeURIComponent(message.destination)}`);
        await next.webview.postMessage({ type: "notice", text: "Topic link copied." });
      } else if (message.type === "copy" && typeof message.text === "string" && message.text.length < 1024 * 1024) {
        await vscode.env.clipboard.writeText(message.text);
        await next.webview.postMessage({ type: "notice", text: "Example copied." });
      } else if (message.type === "link" && typeof message.target === "string") {
        const target = message.target;
        if (target.startsWith("settings:") && Object.hasOwn(settingsTopics, target.slice(9))) await vscode.commands.executeCommand("workbench.action.openSettings", `@id:${target.slice(9)}`);
        else if (target === "action:shortcuts") await vscode.commands.executeCommand("workbench.action.openGlobalKeybindings", "dygnosis");
        else if (target === "action:output") await vscode.commands.executeCommand("dygnosis.showOutput");
        else if (target === "action:restart") await vscode.commands.executeCommand("dygnosis.restartServer");
        else if (target === "action:settings") await vscode.commands.executeCommand("dygnosis.openSettings");
        else if (/^https?:\/\//.test(target)) await vscode.env.openExternal(vscode.Uri.parse(target));
      }
      } catch (error) {
        const action = record(message) ? message.type === "copy" ? "Copy example" : message.type === "permalink" ? "Copy topic link" : message.type === "link" && typeof message.target === "string" ? `Open ${message.target}` : "Help action" : "Help action";
        await next.webview.postMessage({ type: "notice", text: `${action} failed: ${error instanceof Error ? error.message : String(error)}` });
      }
    }));
  };
  const command = vscode.commands.registerCommand("dygnosis.openHelp", (argument?: unknown) => {
    const request: HelpRequest | undefined = typeof argument === "string" ? (
      argument.toLowerCase() === context.extension.id.toLowerCase() ? undefined : { topic: argument }
    ) : record(argument) ? {
      topic: typeof argument.topic === "string" ? argument.topic : undefined,
      code: typeof argument.code === "string" && /^[A-Z]\d{3}$/.test(argument.code) ? argument.code : undefined,
      markdown: typeof argument.markdown === "string" ? argument.markdown : undefined,
      engineVersion: typeof argument.engineVersion === "string" ? argument.engineVersion : undefined,
    } : undefined;
    if (panel) {
      panel.reveal();
      if (request) send(request);
      return;
    }
    const document = vscode.window.activeTextEditor?.document;
    const expansion = document?.uri.scheme === "dygnosis-effective";
    const input = vscode.window.tabGroups.activeTabGroup.activeTab?.input;
    const changes = input instanceof vscode.TabInputCustom && input.viewType === changesViewType;
    const column = expansion || changes || document?.languageId === "dynare" ? vscode.ViewColumn.Beside : vscode.ViewColumn.Active;
    attach(vscode.window.createWebviewPanel("dygnosis.help", "Dygnosis Help", column, { enableFindWidget: true, enableScripts: true, retainContextWhenHidden: true, localResourceRoots: [root, media] }));
    send(request ?? { topic: expansion ? "effective-model" : "get-started" });
  });
  const serializer = vscode.window.registerWebviewPanelSerializer("dygnosis.help", { deserializeWebviewPanel: next => { attach(next); return Promise.resolve(); } });
  const links = vscode.window.registerUriHandler({ handleUri: uri => {
    const topic = helpDestination(new URLSearchParams(uri.query).get("topic") ?? "");
    if (uri.path !== "/help" || !topic) return;
    const bundle = topicIds ? undefined : readBundle();
    if (!topicIds?.has(topic.split("#")[0])) return;
    pendingBundle ??= bundle;
    void vscode.commands.executeCommand("dygnosis.openHelp", topic);
  } });
  return vscode.Disposable.from(command, serializer, links, new vscode.Disposable(() => { panel?.dispose(); subscriptions.forEach(subscription => { subscription.dispose(); }); }));
}
