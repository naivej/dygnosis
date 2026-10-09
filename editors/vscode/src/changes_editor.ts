import * as vscode from "vscode";
import * as path from "node:path";
import { randomBytes } from "node:crypto";
import { DygnosisClient } from "./client";
import { diffHtml, diffPreferences } from "./diff";
import { ComparisonResource, changesScheme, changesViewType, GitSelector, historyScheme, inputLabel, ModelSelector, resourceData, resourceName, resourceQuery } from "./changes_resource";
import { DiffChoices, DiffRow, DiffSide, normalizeChoices, parseDiff, sameTarget } from "./diff_view";
import { createGitSources } from "./git_source_host";
import { GitRepository, GitSources, HistoryCursor, PinnedCommit } from "./git_source";
import { decodeGitDocument, normalizeGitText } from "./git_provenance";
import { HistoricalSources, historicalSource } from "./history_sources";
import { captureComparison, CapturedComparison, ComparisonFailure } from "./snapshot_compare";
import { record } from "./protocol";
import { CapturedTextSources } from "./captured_text";
import { SourceFile } from "./semantic_view";

interface EditorView { panel: vscode.WebviewPanel; choices: DiffChoices; subscriptions: vscode.Disposable[]; sectionsChanged?: boolean }
interface InputSelection { token: vscode.CancellationToken; signal: AbortSignal; dispose(): void }
function checkSelection(token: vscode.CancellationToken): void {
  if (token.isCancellationRequested) throw new ComparisonFailure("CANCELLED", "Input selection was cancelled.");
}
type ViewStatus = "loading" | "ready" | "stale" | "incomplete" | "failure";
class ChangesDocument implements vscode.CustomDocument {
  readonly views = new Set<EditorView>();
  readonly subscriptions: vscode.Disposable[] = [];
  readonly closed = new vscode.EventEmitter<void>();
  readonly onDidDispose = this.closed.event;
  generation = 0; instance = 0; disposed = false;
  captureId = "";
  status: ViewStatus = "loading"; message = "Loading comparison…";
  result?: CapturedComparison; capture?: vscode.CancellationTokenSource; loading?: Promise<void>;
  failureSide?: DiffSide;
  constructor(readonly uri: vscode.Uri, readonly resource: ComparisonResource, readonly close: (document: ChangesDocument) => void) {}
  dispose(): void {
    if (this.disposed) return;
    this.disposed = true; ++this.generation; this.capture?.cancel(); this.capture?.dispose();
    this.close(this); for (const item of this.subscriptions) item.dispose();
    for (const view of this.views) { for (const item of view.subscriptions) item.dispose(); view.panel.dispose(); }
    this.views.clear(); this.closed.fire(); this.closed.dispose();
  }
}

/** A custom document owns capture state; each split owns its display choices. */
export function registerChanges(service: DygnosisClient): vscode.Disposable {
  let gitSources: Promise<GitSources> | undefined, disposed = false, opening = 0;
  let invocation: vscode.CancellationTokenSource | undefined;
  const documents = new Map<string, ChangesDocument>(), remembered = new Map<string, DiffChoices>();
  const beginSelection = (doc?: ChangesDocument, view?: EditorView): InputSelection => {
    invocation?.cancel(); invocation?.dispose(); invocation = new vscode.CancellationTokenSource(); ++opening;
    const source = invocation, abort = new AbortController();
    const subscriptions = [source.token.onCancellationRequested(() => abort.abort())];
    if (doc) subscriptions.push(doc.onDidDispose(() => source.cancel()));
    if (view) subscriptions.push(view.panel.onDidDispose(() => source.cancel()));
    return { token: source.token, signal: abort.signal, dispose: () => { for (const item of subscriptions) item.dispose(); } };
  };
  const assets = vscode.Uri.file(path.resolve(__dirname, "../media"));
  const capturedText = new CapturedTextSources();
  const git = (): Promise<GitSources> => gitSources ??= createGitSources().catch(error => { gitSources = undefined; throw error; });
  const history = new HistoricalSources(async (source, token) => {
    const abort = new AbortController(), subscription = token.onCancellationRequested(() => abort.abort());
    if (token.isCancellationRequested) abort.abort();
    try {
      const sources = await git(), repository = await sources.repositoryFor(vscode.Uri.parse(source.repository_uri), abort.signal);
      const commit = await sources.resolve(repository, source.commit, abort.signal), capture = await sources.capture(repository, commit, source.file_key, abort.signal);
      const fact = capture.source(source.file_key);
      if (fact.kind === "failure") throw new Error(fact.message);
      return fact.text;
    } finally { subscription.dispose(); }
  });
  const current = (doc: ChangesDocument, generation: number, instance: number): boolean =>
    !disposed && !doc.disposed && doc.generation === generation && service.currentInstance === instance;
  const send = (doc: ChangesDocument): void => {
    for (const view of doc.views) void view.panel.webview.postMessage({ type: "render", key: doc.uri.toString(), token: doc.generation,
      before: inputLabel(doc.resource.before), after: inputLabel(doc.resource.after), status: doc.status, message: doc.message,
      rows: doc.result?.snapshot.rows ?? [], choices: view.choices, hasHistory: doc.resource.before.kind === "git" || doc.resource.after.kind === "git",
      capture: doc.captureId, defaults: diffPreferences(vscode.Uri.parse(doc.resource.context_uri), service.log), semantic: doc.result?.snapshot.semantic,
      sourceChanges: doc.result?.snapshot.sourceChanges, coverage: doc.result?.snapshot.coverage, references: doc.result?.snapshot.references, semanticMessage: doc.result?.snapshot.semanticMessage,
      details: doc.result ? { inputs: doc.result.inputs, sourcePolicy: "Current extra include folders; each historical side uses its own written directives." } : undefined });
  };
  const release = (doc: ChangesDocument): void => { if (doc.result) { history.release(doc.result.holder); capturedText.release(doc.result.holder); } doc.result = undefined; };
  const stale = (doc: ChangesDocument, message = "Out of date. Refresh to compare current inputs and enable source actions."): void => {
    if (doc.disposed || doc.status === "stale") return;
    ++doc.generation; doc.capture?.cancel(); doc.status = "stale"; doc.message = message; send(doc);
  };
  const watchInputs = (doc: ChangesDocument): void => {
    for (const item of doc.subscriptions.splice(0)) item.dispose();
    const candidates = new Set<string>();
    for (const side of ["before", "after"] as const) if (doc.resource[side].kind === "working") {
      for (const uri of doc.result?.sourceUris[side].values() ?? []) candidates.add(uri.toString());
      for (const uri of doc.result?.inputs[side].dependency_candidates ?? []) candidates.add(uri);
    }
    // Watch exact positive and negative lookup paths, including files with arbitrary extensions.
    for (const key of candidates) {
      const uri = vscode.Uri.parse(key); if (uri.scheme !== "file") continue;
      const watcher = vscode.workspace.createFileSystemWatcher(new vscode.RelativePattern(vscode.Uri.file(path.dirname(uri.fsPath)), "*"));
      const changed = (file: vscode.Uri): void => {
        if (file.scheme === "file" && (process.platform === "win32" ? file.fsPath.toLowerCase() === uri.fsPath.toLowerCase() : file.toString() === uri.toString())) stale(doc);
      };
      doc.subscriptions.push(watcher, watcher.onDidChange(changed), watcher.onDidCreate(changed), watcher.onDidDelete(changed));
    }
  };
  const refresh = async (doc: ChangesDocument): Promise<void> => {
    doc.capture?.cancel(); doc.capture?.dispose();
    const token = new vscode.CancellationTokenSource(); doc.capture = token;
    const generation = ++doc.generation; doc.instance = service.currentInstance;
    release(doc); doc.status = "loading"; doc.message = "Loading comparison…"; doc.failureSide = undefined; send(doc);
    try {
      const result = await vscode.window.withProgress({ location: vscode.ProgressLocation.Window, title: "Comparing model sources", cancellable: true }, async (_progress, cancelled) => {
        const cancel = cancelled.onCancellationRequested(() => token.cancel());
        try { return await captureComparison(service, git, history, doc.resource, token.token); }
        finally { cancel.dispose(); }
      });
      if (!current(doc, generation, result.instance) || token.token.isCancellationRequested) { history.release(result.holder); return; }
      doc.instance = result.instance; doc.result = result; doc.status = "ready";
      doc.captureId = randomBytes(16).toString("hex");
      doc.message = result.snapshot.rows.length ? "Current comparison" : result.snapshot.sourceChanges?.files.length ? "No model changes. Captured text differs; use Captured file text diff in More actions." : result.snapshot.semantic ? "No model rows. Review Comparison limits before concluding that the model is unchanged." : "No structural changes. Written text can still differ.";
      watchInputs(doc); send(doc);
    } catch (error) {
      if (!current(doc, generation, service.currentInstance)) return;
      if (token.token.isCancellationRequested) { doc.status = "failure"; doc.message = "Comparison capture was cancelled. Refresh to try again."; release(doc); send(doc); return; }
      doc.status = error instanceof ComparisonFailure && error.code === "INCOMPLETE_INPUT" ? "incomplete" : "failure";
      doc.message = String(error); if (error instanceof ComparisonFailure) doc.failureSide = error.side;
      release(doc); service.log(doc.message); send(doc);
    } finally { if (doc.capture === token) { token.dispose(); doc.capture = undefined; } }
  };
  const modelUri = (input: ModelSelector): vscode.Uri => input.kind === "working" ? vscode.Uri.parse(input.root_uri) : vscode.Uri.joinPath(vscode.Uri.parse(input.repository_uri), input.root_file);
  const openResource = async (resource: ComparisonResource, column?: vscode.ViewColumn, choices?: DiffChoices): Promise<void> => {
    const uri = vscode.Uri.from({ scheme: changesScheme, path: `/${resourceName(resource)}`, query: resourceQuery(resource) });
    if (choices) remembered.set(uri.toString(), { ...choices, expanded: {} });
    await vscode.commands.executeCommand("vscode.openWith", uri, changesViewType, { viewColumn: column ?? vscode.ViewColumn.Active, preview: false });
  };
  const rootForHistoricalInclude = async (input: GitSelector, invokingKey: string, token: vscode.CancellationToken, signal: AbortSignal): Promise<GitSelector | undefined> => {
    checkSelection(token);
    const sources = await git(); checkSelection(token);
    const repository = await sources.repositoryFor(vscode.Uri.parse(input.repository_uri), signal);
    const capture = await sources.capture(repository, await sources.resolve(repository, input.commit, signal), input.root_file, signal);
    checkSelection(token);
    const currentCandidates = service.knownOwners(vscode.Uri.joinPath(repository.rootUri, invokingKey)).flatMap(root => {
      try { return [sources.fileKey(repository, root)]; } catch { return []; }
    });
    const choices = [...new Set([...currentCandidates, ...capture.modelPaths()])];
    const root = await vscode.window.showQuickPick(choices.map(root_file => ({ label: root_file, description: currentCandidates.includes(root_file) ? "Current owner candidate; checked at this revision" : "Model at this revision", root_file })), { placeHolder: "Choose the model that includes this historical source" }, token);
    checkSelection(token); if (!root) return undefined;
    const selected = { ...input, root_file: root.root_file }, context_uri = modelUri(selected).toString();
    const proof = await captureComparison(service, git, history, { schema_version: 1, before: selected, after: selected, anchor: selected, context_uri }, token);
    try {
      checkSelection(token);
      if (!proof.inputs.after.file_keys.includes(invokingKey)) throw new Error(`${root.root_file} does not include ${invokingKey} at ${input.commit.slice(0, 7)}.`);
      return selected;
    } finally { history.release(proof.holder); }
  };
  const anchorForDocument = async (document: vscode.TextDocument, token: vscode.CancellationToken, signal: AbortSignal): Promise<ModelSelector | undefined> => {
    checkSelection(token);
    if (document.uri.scheme === "file" || document.uri.scheme === "untitled") {
      let root = await service.rootForDocument(document, true, token); checkSelection(token);
      if (!root) {
        const owners = service.knownOwners(document.uri);
        if (owners.length === 1) root = owners[0];
        else if (owners.length > 1) {
          const selected = await vscode.window.showQuickPick(owners.map(root => ({ label: vscode.workspace.asRelativePath(root), description: root.fsPath, root })), { placeHolder: "Choose the model that owns this include" }, token);
          checkSelection(token);
          if (!selected) return undefined;
          root = selected.root;
        } else {
          const selected = await vscode.window.showOpenDialog({ title: "Choose the model that owns this include", filters: { "Dynare models": ["mod", "dyn"] }, canSelectMany: false });
          checkSelection(token);
          if (!selected?.[0]) return undefined;
          root = selected[0];
        }
        const info = await service.modelInfo(root, root, true, token);
        checkSelection(token);
        if (!info?.related_files.some(row => row.kind === "include" && row.path && (process.platform === "win32" ? vscode.Uri.file(row.path).fsPath.toLowerCase() === document.uri.fsPath.toLowerCase() : vscode.Uri.file(row.path).toString() === document.uri.toString()))) throw new Error("The selected model does not include this source.");
        // Keep the proven owner across the close/open events of a language change.
        service.selectOwner(document.uri, root);
        if (document.languageId !== "dynare") {
          await vscode.languages.setTextDocumentLanguage(document, "dynare");
          checkSelection(token);
        }
      }
      return { kind: "working", root_uri: root.toString() };
    }
    let input: GitSelector, key: string;
    if (document.uri.scheme === historyScheme) {
      const owners = history.owners(document.uri);
      if (owners.length === 1) return owners[0];
      if (owners.length > 1) {
        const owner = await vscode.window.showQuickPick(owners.map(input => ({ label: input.root_file, input })), { placeHolder: "Choose the historical model owner" }, token);
        checkSelection(token); return owner?.input;
      }
      const source = historicalSource(document.uri);
      if (!source) throw new Error("This historical source has invalid provenance. Choose its source revision.");
      key = source.file_key; input = { kind: "git", repository_uri: source.repository_uri, commit: source.commit, root_file: key, requested_ref: source.commit };
    } else {
      const sources = await git(); checkSelection(token);
      let provenance;
      try { provenance = await sources.provenance(document, signal); checkSelection(token); }
      catch (error) {
        checkSelection(token);
        const decoded = decodeGitDocument(document.uri);
        if (decoded.kind === "unavailable" && decoded.code === "unsupported_source") throw error;
        const value: unknown = (() => { try { return JSON.parse(document.uri.query) as unknown; } catch { return undefined; } })();
        const selected = record(value) && typeof value.path === "string" && path.isAbsolute(value.path) && !/[\0\r\n]/.test(value.path) ? vscode.Uri.file(value.path) :
          (await vscode.window.showOpenDialog({ title: "Choose the written model source for this document", canSelectMany: false }))?.[0];
        checkSelection(token);
        if (!selected) return undefined;
        const repository = await sources.repositoryFor(selected, signal); checkSelection(token);
        const ref = await vscode.window.showInputBox({ prompt: "Enter the fixed source revision for the displayed document" }, token);
        checkSelection(token);
        if (!ref) return undefined;
        const commit = await sources.resolve(repository, ref, signal), file_key = sources.fileKey(repository, selected), capture = await sources.capture(repository, commit, file_key, signal), fact = capture.source(file_key);
        checkSelection(token);
        if (fact.kind === "failure" || fact.text !== normalizeGitText(document.getText())) throw new Error("The displayed text does not match that source revision. Open the source at the chosen commit.", { cause: error });
        provenance = { repository, commit, file_key, capture };
      }
      key = provenance.file_key; input = { kind: "git", repository_uri: provenance.repository.rootUri.toString(), commit: provenance.commit.hash, root_file: key, requested_ref: provenance.commit.requested_ref };
    }
    const known = service.knownOwners(modelUri(input));
    if (/\.(mod|dyn)$/i.test(key) && !known.length) return input;
    return rootForHistoricalInclude(input, key, token, signal);
  };
  const activeComparison = (): ChangesDocument | undefined => {
    const input = vscode.window.tabGroups.activeTabGroup.activeTab?.input;
    return input instanceof vscode.TabInputCustom && input.viewType === changesViewType ? documents.get(input.uri.toString()) : undefined;
  };
  const anchorAtInvocation = async (argument: unknown, token: vscode.CancellationToken, signal: AbortSignal): Promise<{ anchor: ModelSelector; context_uri: string } | undefined> => {
    checkSelection(token);
    const explicit = record(argument) && record(argument.resourceUri) ? argument.resourceUri : argument;
    const uri = explicit && typeof explicit === "object" && "scheme" in explicit && "toString" in explicit ? explicit as vscode.Uri : undefined;
    if (!uri) {
      const doc = activeComparison(); if (doc) return { anchor: doc.resource.anchor, context_uri: doc.resource.context_uri };
    }
    let document = uri ? undefined : vscode.window.activeTextEditor?.document;
    if (uri?.scheme === "file") {
      try { document = await vscode.workspace.openTextDocument(uri); checkSelection(token); }
      catch (error) { checkSelection(token); if (/\.(mod|dyn)$/i.test(uri.path)) return { anchor: { kind: "working", root_uri: uri.toString() }, context_uri: uri.toString() }; throw new Error("The selected include has no available model owner.", { cause: error }); }
    } else if (uri) document = await vscode.workspace.openTextDocument(uri);
    checkSelection(token);
    if (!document) return undefined;
    const anchor = await anchorForDocument(document, token, signal);
    checkSelection(token);
    return anchor ? { anchor, context_uri: modelUri(anchor).toString() } : undefined;
  };
  const revisionPicker = async (sources: GitSources, repository: GitRepository, token: vscode.CancellationToken, signal: AbortSignal): Promise<PinnedCommit | undefined> => {
    let cursor: HistoryCursor | undefined, commits: PinnedCommit[] = [];
    for (;;) {
      checkSelection(token);
      const page = await sources.history(repository, cursor, signal); checkSelection(token);
      commits = [...commits, ...page.commits]; cursor = page.next;
      const items: (vscode.QuickPickItem & { revision?: PinnedCommit; action?: string })[] = [
        { label: "Enter revision…", action: "enter" },
        ...commits.map(revision => ({ label: revision.hash.slice(0, 7), description: revision.message.split("\n")[0], detail: (revision.commitDate ?? revision.authorDate)?.toLocaleDateString(), revision })),
        ...(cursor ? [{ label: "Load more", action: "more" }] : []),
      ];
      const choice = await vscode.window.showQuickPick(items, { placeHolder: "Choose a repository revision for Before" }, token);
      checkSelection(token);
      if (!choice) return undefined;
      if (choice.revision) return choice.revision;
      if (choice.action === "enter") {
        const ref = await vscode.window.showInputBox({ prompt: "Enter a local Git revision for Before", placeHolder: "HEAD~2, a commit, branch, tag, or stash@{0}" }, token);
        checkSelection(token); return ref ? sources.resolve(repository, ref, signal) : undefined;
      }
      if (commits.length >= 4096) throw new Error("History display reached its limit. Use Enter revision to select an older commit.");
    }
  };
  const chooseBaseline = async (anchor: ModelSelector, fileOnly: boolean, token: vscode.CancellationToken, signal: AbortSignal): Promise<ModelSelector | undefined> => {
    checkSelection(token);
    let repository: GitRepository | undefined, sources: GitSources | undefined, previous: Awaited<ReturnType<GitSources["previous"]>> | undefined;
    let unavailable = "", previousUnavailable = "";
    if (!fileOnly) try {
      sources = await git(); checkSelection(token); repository = await sources.repositoryFor(modelUri(anchor), signal); checkSelection(token);
      try { previous = await sources.previous(repository, anchor.kind === "git" ? anchor.commit : undefined, signal); }
      catch (error) { checkSelection(token); previousUnavailable = String(error); }
    } catch (error) { checkSelection(token); unavailable = String(error); previousUnavailable = unavailable; }
    checkSelection(token);
    const choice = fileOnly ? "file" : (await vscode.window.showQuickPick([
      { label: "With previous revision", description: previous?.description ?? previousUnavailable, mode: "previous", available: !!previous },
      { label: "With revision…", description: unavailable, mode: "revision", available: !!repository },
      { label: "With branch or tag…", description: unavailable, mode: "ref", available: !!repository },
      { label: "With .mod file…", mode: "file", available: true },
    ], { placeHolder: `Choose Before · After is ${inputLabel(anchor)}` }, token))?.mode;
    checkSelection(token);
    if (!choice) return undefined;
    if (choice === "file") {
      const files = await vscode.window.showOpenDialog({ title: "Open changes: choose Before (open model is After)", openLabel: "Compare as Before", filters: { "Dynare models": ["mod", "dyn"] }, canSelectMany: false, canSelectFiles: true, canSelectFolders: false });
      checkSelection(token);
      if (!files?.[0]) return undefined;
      return { kind: "working", root_uri: files[0].toString() };
    }
    if (!sources || !repository || choice === "previous" && !previous) throw new Error(choice === "previous" ? previousUnavailable : unavailable);
    let commit: PinnedCommit | undefined;
    if (choice === "previous") commit = previous!.revision;
    else if (choice === "revision") commit = await revisionPicker(sources, repository, token, signal);
    else {
      const refs = await sources.refs(repository, signal); checkSelection(token);
      const items: (vscode.QuickPickItem & { revision?: PinnedCommit })[] = [];
      for (const [kind, label] of [["branch", "Local branches"], ["remote", "Remote-tracking branches"], ["tag", "Tags"]] as const) {
        items.push({ label, kind: vscode.QuickPickItemKind.Separator });
        items.push(...refs.filter(ref => ref.kind === kind).map(ref => ({ label: ref.name, description: ref.revision.hash.slice(0, 7), revision: ref.revision })));
      }
      commit = (await vscode.window.showQuickPick(items, { placeHolder: "Choose a branch or tag for Before" }, token))?.revision;
    }
    checkSelection(token);
    if (!commit) return undefined;
    let root_file = sources.fileKey(repository, modelUri(anchor));
    const capture = await sources.capture(repository, commit, root_file, signal); checkSelection(token);
    if (!Object.hasOwn(capture.manifest, root_file)) {
      const rename = await sources.rename(repository, commit.hash, anchor.kind === "git" ? anchor.commit : undefined, root_file, signal); checkSelection(token);
      if (rename) {
        const choice = await vscode.window.showQuickPick([{ label: `Use ${rename.before} → ${rename.after}`, key: rename.before }, { label: "Choose model path at this revision", key: "" }], { placeHolder: "Git found a root rename. Choose the Before path." }, token);
        checkSelection(token);
        if (!choice) return undefined;
        if (choice.key) root_file = choice.key;
        else {
          const selected = await vscode.window.showQuickPick(capture.modelPaths(), { placeHolder: "Choose the Before model at this revision" }, token);
          checkSelection(token); if (!selected) return undefined; root_file = selected;
        }
      }
    }
    return { kind: "git", repository_uri: repository.rootUri.toString(), commit: commit.hash, root_file, requested_ref: commit.requested_ref };
  };
  const start = async (argument?: unknown, fileOnly = false, from?: ChangesDocument, view?: EditorView): Promise<void> => {
    const originating = from ?? (argument === undefined ? activeComparison() : undefined);
    const selection = beginSelection(originating, view), request = opening, { token, signal } = selection;
    try {
      const context = originating ? { anchor: originating.resource.anchor, context_uri: originating.resource.context_uri } : await anchorAtInvocation(argument, token, signal);
      if (!context || token.isCancellationRequested) return;
      const before = await chooseBaseline(context.anchor, fileOnly, token, signal);
      if (!before || disposed || request !== opening || token.isCancellationRequested) return;
      await openResource({ schema_version: 1, before, after: context.anchor, ...context }, view?.panel.viewColumn, view?.choices);
    } catch (error) { if (!disposed && request === opening && !token.isCancellationRequested) await service.failure(String(error)); }
    finally { selection.dispose(); }
  };
  const validateWorking = async (doc: ChangesDocument, row?: Pick<DiffRow, "id" | "navigation">): Promise<boolean> => {
    const generation = doc.generation, result = doc.result, instance = doc.instance;
    if (!result || doc.status !== "ready" || !current(doc, generation, instance)) return false;
    const verified = await Promise.all(result.working.map(input => service.revalidate(input.root, input.expected, instance)));
    if (!current(doc, generation, instance) || verified.some(info => !info)) return false;
    if (row && result.legacy) {
      const fresh = parseDiff(await service.execute("dynare/compareModels", [modelUri(doc.resource.before).toString(), modelUri(doc.resource.after).toString()]), modelUri(doc.resource.before).toString(), modelUri(doc.resource.after).toString());
      const matched = fresh.rows.find(candidate => candidate.id === row.id);
      if (!matched || !sameTarget(matched.navigation.before, row.navigation.before) || !sameTarget(matched.navigation.after, row.navigation.after)) return false;
    }
    return current(doc, generation, instance);
  };
  const openSource = async (doc: ChangesDocument, row: Pick<DiffRow, "id" | "navigation">, side: DiffSide): Promise<void> => {
    const generation = doc.generation, instance = doc.instance, result = doc.result, target = row.navigation[side];
    if (!result || !target?.written_locations.length) return;
    const refuse = (): void => { if (current(doc, generation, instance)) stale(doc); };
    try {
      if (!await validateWorking(doc, row)) { refuse(); return; }
      const selected = target.written_locations.length === 1 ? target.written_locations[0] : (await vscode.window.showQuickPick(target.written_locations.map(location => ({
        label: location.uri, description: `${side === "before" ? "Before" : "After"} · line ${location.range.start.line + 1}`, location,
      })), { placeHolder: "Choose a contributing written source" }))?.location;
      if (!selected || !current(doc, generation, instance)) return;
      const source = vscode.Uri.parse(selected.uri);
      let written = await vscode.workspace.openTextDocument(source);
      if (source.scheme === historyScheme) written = await vscode.languages.setTextDocumentLanguage(written, "dynare");
      if (!await validateWorking(doc, row) || !current(doc, generation, instance)) { refuse(); return; }
      const key = [...result.sourceUris[side]].find(([, uri]) => uri.toString() === source.toString())?.[0];
      if (!key || !result.legacy && normalizeGitText(written.getText()) !== normalizeGitText(result.texts[side][key])) { refuse(); return; }
      const input = doc.resource[side]; if (input.kind === "working") service.selectOwner(written.uri, modelUri(input));
      const range = new vscode.Range(selected.range.start.line, selected.range.start.character, selected.range.end.line, selected.range.end.character);
      await vscode.window.showTextDocument(written, { selection: range });
    } catch (error) { service.log(String(error)); refuse(); }
  };
  const openCaptured = async (doc: ChangesDocument, file: SourceFile): Promise<void> => {
    const generation = doc.generation, instance = doc.instance, result = doc.result;
    if (!result || doc.status !== "ready") return;
    try {
      if (!await validateWorking(doc) || !current(doc, generation, instance)) { if (current(doc, generation, instance)) stale(doc); return; }
      const uri = (which: DiffSide): vscode.Uri | undefined => {
        const own = file[which];
        if (own && (!own.exact_text_available || !Object.hasOwn(result.texts[which], own.file_key))) return undefined;
        const input = doc.resource[which], label = `${which} ${own?.file_key ?? "absent"}${input.kind === "git" ? ` ${input.commit.slice(0, 7)}` : " captured"}`;
        if (input.kind === "git" && own) return result.sourceUris[which].get(own.file_key);
        return capturedText.retain(result.holder, `${file.pointer}:${which}`, label, own ? result.texts[which][own.file_key] : "");
      };
      const before = uri("before"), after = uri("after");
      if (before && after && await validateWorking(doc) && current(doc, generation, instance)) await vscode.commands.executeCommand("vscode.diff", before, after, `Captured file text: ${file.before?.file_key ?? "absent"} → ${file.after?.file_key ?? "absent"}`);
    } catch (error) { service.log(String(error)); if (current(doc, generation, instance)) stale(doc); }
  };
  const chooseCaptured = async (doc: ChangesDocument): Promise<void> => {
    const generation = doc.generation, instance = doc.instance;
    const files = doc.result?.snapshot.sourceChanges?.files ?? [];
    if (!files.length) return;
    if (!await validateWorking(doc) || !current(doc, generation, instance)) { if (current(doc, generation, instance)) stale(doc); return; }
    const selected = await vscode.window.showQuickPick(files.map(file => {
      const available = (["before", "after"] as const).every(side => !file[side] || file[side].exact_text_available && Object.hasOwn(doc.result!.texts[side], file[side].file_key));
      return {
        label: file.after?.file_key ?? file.before!.file_key,
        description: available ? `${file.change}${file.correspondence === "unpaired" ? " · file correspondence not established" : ""}` : "Text diff unavailable: complete captured text is missing",
        detail: `Before: ${file.before?.file_key ?? "absent"} → After: ${file.after?.file_key ?? "absent"}`,
        file, available,
      };
    }), { placeHolder: "Choose a changed captured file for text diff" });
    if (!selected || !current(doc, generation, instance)) return;
    if (!selected.available) { void vscode.window.showInformationMessage("Complete captured text is unavailable for this file. Review Comparison limits."); return; }
    await openCaptured(doc, selected.file);
  };
  const updateRevision = async (doc: ChangesDocument, view: EditorView): Promise<void> => {
    const selection = beginSelection(doc, view), { token, signal } = selection;
    try {
      checkSelection(token); const sources = await git(); checkSelection(token);
      const resource = { ...doc.resource }; let changed = false;
      for (const side of ["before", "after"] as const) {
        const input = resource[side]; if (input.kind !== "git") continue;
        const repository = await sources.repositoryFor(vscode.Uri.parse(input.repository_uri), signal);
        const revision = await sources.resolve(repository, input.requested_ref, signal); checkSelection(token);
        if (revision.hash !== input.commit) { resource[side] = { ...input, commit: revision.hash }; changed = true; }
      }
      if (changed) await openResource(resource, view.panel.viewColumn, view.choices);
      else void vscode.window.showInformationMessage("The selected revisions have not moved. Refresh keeps the captured commits.");
    } catch (error) { if (!token.isCancellationRequested) throw error; }
    finally { selection.dispose(); }
  };
  const choosePath = async (doc: ChangesDocument, view: EditorView): Promise<void> => {
    const selection = beginSelection(doc, view), { token, signal } = selection;
    try {
      checkSelection(token);
      const side = doc.failureSide ?? "before", input = doc.resource[side]; let selected: ModelSelector | undefined;
      if (input.kind === "git") {
        const sources = await git(); checkSelection(token);
        const repository = await sources.repositoryFor(vscode.Uri.parse(input.repository_uri), signal);
        const capture = await sources.capture(repository, await sources.resolve(repository, input.commit, signal), input.root_file, signal);
        checkSelection(token);
        const root_file = await vscode.window.showQuickPick(capture.modelPaths(), { placeHolder: `Choose ${side === "before" ? "Before" : "After"} model path at ${input.commit.slice(0, 7)}` }, token);
        checkSelection(token); if (root_file) selected = { ...input, root_file };
      } else {
        const uri = (await vscode.window.showOpenDialog({ title: `Choose the ${side} model`, filters: { "Dynare models": ["mod", "dyn"] }, canSelectMany: false }))?.[0];
        checkSelection(token); if (uri) selected = { kind: "working", root_uri: uri.toString() };
      }
      if (selected) await openResource({ ...doc.resource, [side]: selected }, view.panel.viewColumn, view.choices);
    } catch (error) { if (!token.isCancellationRequested) throw error; }
    finally { selection.dispose(); }
  };
  const provider: vscode.CustomReadonlyEditorProvider<ChangesDocument> = {
    openCustomDocument(uri, _context, token) {
      if (disposed || token.isCancellationRequested || uri.scheme !== changesScheme || uri.query.length > 262144) throw new Error("This comparison resource is unavailable.");
      const resource = resourceData(JSON.parse(uri.query));
      const doc = new ChangesDocument(uri, resource, current => { release(current); documents.delete(current.uri.toString()); });
      documents.set(uri.toString(), doc); return doc;
    },
    async resolveCustomEditor(doc, panel) {
      panel.webview.options = { enableScripts: true, localResourceRoots: [assets], enableCommandUris: false };
      const defaults = diffPreferences(vscode.Uri.parse(doc.resource.context_uri), service.log), key = doc.uri.toString();
      const view: EditorView = { panel, choices: normalizeChoices(remembered.get(key), defaults), subscriptions: [] }; doc.views.add(view);
      view.subscriptions.push(panel.onDidDispose(() => { remembered.set(key, view.choices); doc.views.delete(view); for (const item of view.subscriptions) item.dispose(); }),
        panel.webview.onDidReceiveMessage((message: unknown) => {
          if (!record(message) || doc.disposed) return;
          if (message.type === "ready") {
            if (message.key === key) {
              const restored = normalizeChoices(message.choices, defaults);
              if (view.sectionsChanged) {
                restored.sections = view.choices.sections;
                restored.customSections = [...view.choices.sections];
              }
              view.choices = restored; view.sectionsChanged = false;
            }
            send(doc);
          }
          else if (message.type === "choices" && message.key === key) { view.choices = normalizeChoices(message.choices, defaults); remembered.set(key, view.choices); }
          else if (message.type === "refresh") void refresh(doc);
          else if (message.type === "help") void vscode.commands.executeCommand("dygnosis.openHelp", "structural-diff");
          else if (message.type === "changeComparison") void start(undefined, false, doc, view);
          else if (message.type === "swap") void openResource({ ...doc.resource, before: doc.resource.after, after: doc.resource.before }, panel.viewColumn, view.choices);
          else if (message.type === "updateRevision") void updateRevision(doc, view).catch(error => service.failure(String(error)));
          else if (message.type === "choosePath") void choosePath(doc, view).catch(error => service.failure(String(error)));
          else if (message.type === "details" && doc.result) void vscode.window.showInformationMessage(`Extra include folders (current list): ${doc.result.inputs.after.search_paths.join(", ") || "none"}. Historical sources come only from their selected commit trees. Working sources include open unsaved text.`);
          else if (message.type === "rootTextDiff" && message.token === doc.generation && doc.status === "ready") {
            const generation = doc.generation;
            void validateWorking(doc).then(valid => {
              if (!valid) { if (doc.generation === generation) stale(doc); return; }
              const result = doc.result!, before = result.sourceUris.before.get(result.inputs.before.root_file), after = result.sourceUris.after.get(result.inputs.after.root_file);
              if (before && after && doc.generation === generation) return vscode.commands.executeCommand("vscode.diff", before, after, `Root file text: ${inputLabel(doc.resource.before)} → ${inputLabel(doc.resource.after)}`);
            }).catch(error => service.failure(String(error)));
          } else if (message.type === "openSource" && message.token === doc.generation && doc.status === "ready" && typeof message.rowId === "string" && (message.side === "before" || message.side === "after")) {
            const row = doc.result?.snapshot.rows.find(row => row.id === message.rowId); if (row) void openSource(doc, row, message.side);
          } else if (message.type === "openReference" && message.token === doc.generation && doc.status === "ready" && typeof message.pointer === "string" && (message.side === "before" || message.side === "after")) {
            const reference = doc.result?.snapshot.references?.find(reference => reference.pointer === message.pointer && reference.side === message.side);
            if (reference) void openSource(doc, { id: reference.pointer, navigation: reference.navigation }, reference.side);
          } else if (message.type === "capturedTextDiff" && message.token === doc.generation && doc.status === "ready") {
            void chooseCaptured(doc).catch(error => service.failure(String(error)));
          }
        }), panel.onDidChangeViewState(() => { if (panel.visible) send(doc); }));
      panel.webview.html = diffHtml(panel.webview, assets); send(doc);
      if (!doc.result && !doc.loading) doc.loading = refresh(doc).finally(() => { doc.loading = undefined; });
      await doc.loading;
    },
  };
  const invalidated = service.onDidInvalidate(event => {
    if (event.reason === "lifecycle") invocation?.cancel();
    for (const doc of documents.values()) {
      if (event.reason === "lifecycle" || doc.instance && doc.instance !== service.currentInstance) { stale(doc, "The engine stopped or restarted. Refresh this comparison."); continue; }
      if (doc.status === "loading" || !doc.result) continue;
      const workingSides = (["before", "after"] as const).filter(side => doc.resource[side].kind === "working");
      const matches = workingSides.some(side => event.root ? modelUri(doc.resource[side]).toString() === event.root && event.modelRevision !== doc.result!.working.find(input => input.root.toString() === event.root)?.expected :
        event.uri ? [...doc.result!.sourceUris[side].values()].some(uri => uri.toString() === event.uri) || doc.result!.inputs[side].dependency_candidates.includes(event.uri) : false);
      if (matches) stale(doc);
    }
  });
  const settings = vscode.workspace.onDidChangeConfiguration(event => {
    for (const doc of documents.values()) {
      const context = vscode.Uri.parse(doc.resource.context_uri);
      if (event.affectsConfiguration("dynare.serverPath")) stale(doc, "The engine selection changed. Refresh this comparison.");
      else if (event.affectsConfiguration("dynare.searchPaths", context) || (["before", "after"] as const).some(side => doc.resource[side].kind === "working" && event.affectsConfiguration("dynare.searchPaths", modelUri(doc.resource[side])))) stale(doc);
      if (event.affectsConfiguration("dynare.diff.sections", context)) {
        for (const view of doc.views) {
          view.choices.sections = diffPreferences(context, service.log).sections;
          view.choices.customSections = [...view.choices.sections];
          view.sectionsChanged = true;
        }
        send(doc);
      }
    }
  });
  return vscode.Disposable.from(history, capturedText, invalidated, settings,
    vscode.window.registerCustomEditorProvider(changesViewType, provider, { supportsMultipleEditorsPerDocument: true, webviewOptions: { retainContextWhenHidden: false } }),
    vscode.commands.registerCommand("dygnosis.openChanges", (argument?: unknown) => start(argument)),
    vscode.commands.registerCommand("dygnosis.diffWith", (argument?: unknown) => start(argument, true)),
    new vscode.Disposable(() => { disposed = true; ++opening; invocation?.cancel(); invocation?.dispose(); for (const doc of [...documents.values()]) doc.dispose(); remembered.clear(); void gitSources?.then(sources => sources.clear()); }));
}
