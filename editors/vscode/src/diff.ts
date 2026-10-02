import * as vscode from "vscode";
import { randomBytes } from "node:crypto";
import * as path from "node:path";
import { DygnosisClient } from "./client";
import { record } from "./protocol";
import { listSetting } from "./settings";
import { changeKinds, diffSections, DiffChoices, DiffPreferences, DiffRow, DiffSide, DiffSnapshot, normalizeChoices, parseDiff, sameTarget } from "./diff_view";

export function diffPreferences(root: vscode.Uri, log: (message: string) => void): DiffPreferences {
  const config = vscode.workspace.getConfiguration("dynare", { uri: root, languageId: "dynare" });
  const choice = <T extends string>(key: string, allowed: T[], fallback: T): T => {
    const value = config.get<unknown>(key, fallback);
    if (typeof value === "string" && allowed.includes(value as T)) return value as T;
    log(`Invalid dynare.${key}; using ${fallback}.`); return fallback;
  };
  return {
    layout: choice("diff.layout", ["auto", "sideBySide", "stacked"], "auto"),
    expansion: choice("diff.defaultExpansion", ["changes", "all", "none"], "changes"),
    sections: listSetting("diff.sections", root, [...diffSections], [...diffSections], log) as DiffPreferences["sections"],
    changeKinds: listSetting("diff.defaultChangeKinds", root, [...changeKinds], [...changeKinds], log) as DiffPreferences["changeKinds"],
  };
}

export function diffHtml(webview: vscode.Webview, assets: vscode.Uri): string {
  const nonce = randomBytes(24).toString("base64");
  const script = webview.asWebviewUri(vscode.Uri.joinPath(assets, "diff_view.js"));
  const style = webview.asWebviewUri(vscode.Uri.joinPath(assets, "diff_view.css"));
  return `<!DOCTYPE html><html lang="en"><head><meta charset="UTF-8"><meta name="viewport" content="width=device-width, initial-scale=1.0">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src ${webview.cspSource}; script-src 'nonce-${nonce}';">
<link rel="stylesheet" href="${style.toString()}"><title>Dygnosis model Diff</title></head><body>
<main><h1>Model Diff</h1><div id="models"></div><p id="status" role="status" aria-live="polite">Loading comparison…</p>
<div class="toolbar"><button id="refresh" type="button">Refresh</button><label>Search <input id="search" type="search" placeholder="Names, values, or equations"></label>
<label>Scope <select id="scope"><option value="all">All scopes</option></select></label>
<label>Layout <select id="layout"><option value="auto">Auto</option><option value="sideBySide">Side by side</option><option value="stacked">Stacked</option></select></label>
<label>Expansion <select id="expansion"><option value="changes">Changes</option><option value="all">All</option><option value="none">None</option></select></label></div>
<fieldset id="kinds"><legend>Change kinds</legend></fieldset><fieldset id="sections"><legend>Sections</legend></fieldset>
<p id="counts" aria-live="polite"></p><div id="results"></div></main><script nonce="${nonce}" src="${script.toString()}"></script></body></html>`;
}

interface ComparisonView {
  panel: vscode.WebviewPanel; before: vscode.Uri; after: vscode.Uri; key: string;
  choices: DiffChoices; snapshot?: DiffSnapshot; instance: number; generation: number;
  status: "loading" | "ready" | "stale" | "incomplete" | "failure"; message: string;
  cancellation?: vscode.CancellationTokenSource; disposed: boolean; subscriptions: vscode.Disposable[];
}

/** A view owns one explicit direction and the engine's row identities. */
export function registerDiff(service: DygnosisClient): vscode.Disposable {
  const assets = vscode.Uri.file(path.resolve(__dirname, "../media"));
  const views = new Map<string, ComparisonView>(), remembered = new Map<string, DiffChoices>();
  let disposed = false, opening = 0;
  const send = (view: ComparisonView): void => {
    if (view.disposed) return;
    void view.panel.webview.postMessage({ type: "render", key: view.key, token: view.generation,
      before: view.before.toString(), after: view.after.toString(), status: view.status, message: view.message,
      rows: view.snapshot?.rows ?? [], choices: view.choices });
  };
  const cancel = (view: ComparisonView): void => { view.cancellation?.cancel(); view.cancellation?.dispose(); view.cancellation = undefined; };
  const stale = (view: ComparisonView, message = "Out of date. Refresh to compare current inputs and enable source actions."): void => {
    if (view.disposed) return;
    ++view.generation; cancel(view); view.status = "stale"; view.message = message; send(view);
  };
  const capability = (): boolean => {
    const experimental: unknown = service.client?.initializeResult?.capabilities.experimental;
    return record(experimental) && record(experimental.dygnosis) && record(experimental.dygnosis.compareModels) &&
      experimental.dygnosis.compareModels.navigation_schema_version === 1;
  };
  const current = (view: ComparisonView, generation: number, instance: number): boolean =>
    !disposed && !view.disposed && view.generation === generation && service.currentInstance === instance;
  const bothRoots = <T>(view: ComparisonView, query: (root: vscode.Uri, side: DiffSide) => Promise<T>): Promise<[T, T]> => {
    const before = query(view.before, "before");
    const after = view.before.toString() === view.after.toString() ? before : query(view.after, "after");
    return Promise.all([before, after]);
  };
  const refresh = async (view: ComparisonView): Promise<void> => {
    cancel(view);
    const generation = ++view.generation, instance = service.currentInstance;
    const cancellation = new vscode.CancellationTokenSource(); view.cancellation = cancellation;
    view.status = "loading"; view.message = "Loading comparison…"; view.snapshot = undefined; send(view);
    try {
      if (!capability()) throw new Error("This engine does not support model Diff navigation. Update dynare.serverPath or use the bundled binary.");
      // Both fresh requests establish their own root settings, overlays, and dependency watches.
      const inputs = await bothRoots(view, root => service.modelInfo(root, root, true));
      if (!current(view, generation, instance)) return;
      if (inputs.some(info => !info)) throw new Error("Model Diff is unavailable. Use the bundled binary or check Dygnosis Output.");
      if (inputs.some(info => !info?.complete)) {
        view.status = "incomplete"; view.message = "Incomplete expansion. Fix the model inputs and Refresh before comparing."; send(view); return;
      }
      const result = await service.execute("dynare/compareModels", [view.before.toString(), view.after.toString()], cancellation.token);
      if (!current(view, generation, instance) || cancellation.token.isCancellationRequested) return;
      const snapshot = parseDiff(result, view.before.toString(), view.after.toString());
      if (!snapshot.complete) {
        view.status = "incomplete"; view.message = "Incomplete expansion. Fix the model inputs and Refresh before comparing."; send(view); return;
      }
      if (snapshot.before.revision !== inputs[0]?.revision || snapshot.after.revision !== inputs[1]?.revision) {
        stale(view); return;
      }
      const verified = await bothRoots(view, (root, side) => service.revalidate(root, snapshot[side].revision!, instance));
      if (!current(view, generation, instance)) return;
      if (verified.some(info => !info)) { stale(view); return; }
      view.snapshot = snapshot; view.instance = instance; view.status = "ready";
      view.message = snapshot.rows.length ? "Current comparison" : "No structural changes."; send(view);
    } catch (error) {
      if (!current(view, generation, instance) || cancellation.token.isCancellationRequested) return;
      view.snapshot = undefined; view.status = "failure"; view.message = String(error); service.log(view.message); send(view);
      void service.failure(view.message);
    } finally {
      if (view.cancellation === cancellation) { cancellation.dispose(); view.cancellation = undefined; }
    }
  };
  const validateRow = async (view: ComparisonView, row: DiffRow, side: DiffSide, generation: number): Promise<boolean> => {
    const snapshot = view.snapshot, instance = view.instance;
    if (!snapshot?.complete || view.status !== "ready" || !current(view, generation, instance)) return false;
    const inputs = await bothRoots(view, (root, side) => service.revalidate(root, snapshot[side].revision!, instance));
    if (!current(view, generation, instance) || inputs.some(info => !info)) return false;
    const result = parseDiff(await service.execute("dynare/compareModels", [view.before.toString(), view.after.toString()]), view.before.toString(), view.after.toString());
    const fresh = result.rows.find(candidate => candidate.id === row.id);
    return current(view, generation, instance) && result.complete && result.before.revision === snapshot.before.revision &&
      result.after.revision === snapshot.after.revision && !!fresh && fresh.navigation.kind === row.navigation.kind &&
      sameTarget(fresh.navigation.before, row.navigation.before) && sameTarget(fresh.navigation.after, row.navigation.after) && !!fresh.navigation[side];
  };
  const openSource = async (view: ComparisonView, row: DiffRow, side: DiffSide): Promise<void> => {
    const generation = view.generation, instance = view.instance, target = row.navigation[side];
    if (!target?.written_locations.length || view.status !== "ready") return;
    const refuse = (message?: string): void => {
      // A superseded source action must not demote a newer comparison.
      if (!disposed && !view.disposed && view.generation === generation) stale(view, message);
    };
    try {
      if (!await validateRow(view, row, side, generation)) { refuse(); return; }
      const locations = target.written_locations;
      const selected = locations.length === 1 ? locations[0] : (await vscode.window.showQuickPick(locations.map(location => ({
        label: vscode.workspace.asRelativePath(vscode.Uri.parse(location.uri)),
        description: `${side === "before" ? "Before" : "After"} · line ${location.range.start.line + 1}`, location,
      })), { placeHolder: "Choose a contributing written source" }))?.location;
      if (!selected || !current(view, generation, instance)) return;
      const root = side === "before" ? view.before : view.after;
      await service.openLocation(selected, root, async () => {
        const valid = await validateRow(view, row, side, generation);
        if (!valid) refuse();
        return valid;
      });
    } catch (error) { service.log(String(error)); refuse("Source could not be verified. Refresh the comparison before opening source."); }
  };
  const closeView = (view: ComparisonView): void => {
    if (view.disposed) return;
    view.disposed = true; ++view.generation; cancel(view); remembered.set(view.key, view.choices); views.delete(view.key);
    for (const subscription of view.subscriptions) subscription.dispose();
  };
  const command = vscode.commands.registerCommand("dygnosis.diffWith", async () => {
    const document = vscode.window.activeTextEditor?.document, request = ++opening;
    if (!document || disposed) return;
    const version = document.version;
    try {
      const before = await service.rootForDocument(document, true);
      if (!before || disposed || request !== opening) return;
      await service.ensureStarted();
      const instance = service.currentInstance;
      const selected = await vscode.window.showOpenDialog({ canSelectFiles: true, canSelectFolders: false, canSelectMany: false,
        title: "Model Diff: choose After (active model is Before)", openLabel: "Compare as After", filters: { "Dynare models": ["mod", "dyn"] } });
      if (!selected?.[0] || disposed || request !== opening || document.version !== version || document.isClosed ||
        service.currentInstance !== instance || vscode.window.activeTextEditor?.document !== document) return;
      const owner = await service.rootForDocument(document);
      if (owner?.toString() !== before.toString() || disposed || request !== opening || document.version !== version ||
        document.isClosed || service.currentInstance !== instance || vscode.window.activeTextEditor?.document !== document) return;
      const after = selected[0];
      if (after.scheme !== "file" || !/\.(mod|dyn)$/i.test(after.path)) throw new Error("Choose a .mod or .dyn file for After.");
      const key = JSON.stringify([before.toString(), after.toString()]);
      const existing = views.get(key);
      if (existing) { existing.panel.reveal(vscode.ViewColumn.Beside); return; }
      const panel = vscode.window.createWebviewPanel("dygnosis.diff", "Dygnosis model Diff", vscode.ViewColumn.Beside,
        { enableScripts: true, localResourceRoots: [assets], enableCommandUris: false, retainContextWhenHidden: false });
      const preferences = diffPreferences(before, service.log);
      const view: ComparisonView = { panel, before, after, key, choices: normalizeChoices(remembered.get(key), preferences), instance,
        generation: 0, status: "loading", message: "Loading comparison…", disposed: false, subscriptions: [] };
      views.set(key, view);
      view.subscriptions.push(panel.onDidDispose(() => closeView(view)), panel.webview.onDidReceiveMessage((message: unknown) => {
        if (!record(message) || view.disposed) return;
        if (message.type === "ready") {
          if (message.key === key) view.choices = { ...normalizeChoices(message.choices, preferences), sections: view.choices.sections };
          send(view);
        } else if (message.type === "choices" && message.key === key) {
          view.choices = normalizeChoices(message.choices, preferences); remembered.set(key, view.choices);
        } else if (message.type === "refresh") void refresh(view);
        else if (message.type === "openSource" && message.token === view.generation && typeof message.rowId === "string" &&
          (message.side === "before" || message.side === "after") && view.status === "ready") {
          const row = view.snapshot?.rows.find(row => row.id === message.rowId);
          if (row) void openSource(view, row, message.side);
        }
      }), panel.onDidChangeViewState(() => { if (panel.visible) send(view); }));
      panel.webview.html = diffHtml(panel.webview, assets);
      await refresh(view);
    } catch (error) { if (!disposed && request === opening) await service.failure(String(error)); }
  });
  const invalidated = service.onDidChange(() => { for (const view of views.values()) stale(view); });
  // Client invalidation also covers overlays, watched dependencies, root settings and restarts.
  const settings = vscode.workspace.onDidChangeConfiguration(event => {
    for (const view of views.values()) if (event.affectsConfiguration("dynare.diff.sections", view.before)) {
      view.choices.sections = diffPreferences(view.before, service.log).sections; remembered.set(view.key, view.choices); send(view);
    }
  });
  return vscode.Disposable.from(command, invalidated, settings, new vscode.Disposable(() => {
    disposed = true; ++opening;
    for (const view of [...views.values()]) { closeView(view); view.panel.dispose(); }
    remembered.clear();
  }));
}
