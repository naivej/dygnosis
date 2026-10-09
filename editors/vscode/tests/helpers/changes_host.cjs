// Native-like VS Code surface for comparison document, capture, and source lifetime tests.
const Module = require("node:module");
const path = require("node:path");
const { URL, pathToFileURL, fileURLToPath } = require("node:url");
const { resourceQuery, resourceName, changesScheme, changesViewType, historyScheme } = require("../../out/changes_resource");

class Disposable {
  constructor(callback) { this.callback = callback; }
  dispose() { const callback = this.callback; this.callback = undefined; callback?.(); }
  static from(...items) { return new Disposable(() => items.forEach(item => item.dispose())); }
}
class Emitter {
  listeners = new Set();
  event = listener => { this.listeners.add(listener); return new Disposable(() => this.listeners.delete(listener)); };
  fire(value) { for (const listener of [...this.listeners]) listener(value); }
  dispose() { this.listeners.clear(); }
}
class CancellationTokenSource {
  changed = new Emitter();
  token = { isCancellationRequested: false, onCancellationRequested: this.changed.event };
  cancel() { if (!this.token.isCancellationRequested) { this.token.isCancellationRequested = true; this.changed.fire(); } }
  dispose() { this.changed.dispose(); }
}
class Uri {
  constructor(value) { const parsed = new URL(value); this.scheme = parsed.protocol.slice(0, -1); this.authority = parsed.host; this.path = decodeURIComponent(parsed.pathname); this.query = decodeURIComponent(parsed.search.slice(1)); }
  get fsPath() { return this.scheme === "file" ? fileURLToPath(this.toString()) : this.path; }
  toString() { return `${this.scheme}://${this.authority}${this.path.split("/").map(encodeURIComponent).join("/")}${this.query ? `?${encodeURIComponent(this.query)}` : ""}`; }
  static parse(value) { return new Uri(value); }
  static file(value) { return new Uri(pathToFileURL(path.resolve(value)).href); }
  static from(value) { const uri = new Uri(`${value.scheme}://${value.authority ?? ""}${value.path ?? "/"}`); uri.query = value.query ?? ""; return uri; }
  static joinPath(base, ...parts) { return Uri.from({ scheme: base.scheme, authority: base.authority, path: path.posix.join(base.path, ...parts) }); }
}
class Range { constructor(line, character, endLine, endCharacter) { this.start = { line, character }; this.end = { line: endLine, character: endCharacter }; } }
class RelativePattern { constructor(base, pattern) { this.baseUri = typeof base === "string" ? Uri.file(base) : base; this.pattern = pattern; } }
class TabInputCustom { constructor(uri, viewType) { this.uri = uri; this.viewType = viewType; } }
const rootPath = path.resolve(__dirname, "fixture-models");
const rootUri = Uri.file(rootPath), anchorUri = Uri.file(path.join(rootPath, "main.mod")), baselineUri = Uri.file(path.join(rootPath, "baseline.mod"));
const commits = { old: "a".repeat(40), opened: "b".repeat(40), head: "c".repeat(40), moved: "d".repeat(40) };
const working = (uri = anchorUri) => ({ kind: "working", root_uri: uri.toString() });
const historical = (commit = commits.opened, root_file = "main.mod", requested_ref = commit) => ({ kind: "git", repository_uri: rootUri.toString(), commit, root_file, requested_ref });
const comparison = (before = historical(commits.old), after = working(), anchor = after) => ({ schema_version: 1, before, after, anchor, context_uri: anchorUri.toString() });
const deferred = () => { let resolve, reject; const promise = new Promise((done, fail) => { resolve = done; reject = fail; }); return { promise, resolve, reject }; };
const flush = async () => { for (let index = 0; index < 12; index++) await new Promise(resolve => setImmediate(resolve)); };

function diffData(changed = true) {
  const data = { symbols_changed: [], changed_parameter_values: [], added_equations: [], removed_equations: [], changed_equations: [], unmatched_same_name: [], heterogeneous_equations: [], shock_setup_changes: [] };
  for (const category of ["endogenous", "exogenous", "parameters"]) for (const kind of ["added", "removed", "common"]) data[`${kind}_${category}`] = [];
  if (changed) data.changed_equations.push({ index_old: 1, index_new: 1, text_old: "y = 1", text_new: "y = 2", name_old: "goods", name_new: "goods", tags_old: { name: "goods" }, tags_new: { name: "goods" } });
  return data;
}
function captured(resource, history, host, instance = 1, changed = true) {
  const holder = {}, texts = {}, sourceUris = {}, inputs = {}, workingInputs = [];
  for (const side of ["before", "after"]) {
    const input = resource[side], root = input.kind === "git" ? input.root_file : input.root_uri;
    const include = input.kind === "git" ? "written.data" : Uri.joinPath(Uri.parse(input.root_uri), "..", `${path.basename(Uri.parse(input.root_uri).fsPath, ".mod")}.data`).toString();
    const label = input.kind === "git" ? input.commit.slice(0, 7) : path.basename(Uri.parse(input.root_uri).fsPath);
    texts[side] = { [root]: `@#include "${path.posix.basename(include)}"\r\n// ${label} root\r\n`, [include]: `// 😀 ${label}\nmodel; y = ${label.startsWith("a") ? "1" : "2"}; end;\n` };
    sourceUris[side] = input.kind === "git" ? history.retain(holder, input, texts[side]) : new Map(Object.keys(texts[side]).map(key => [key, Uri.parse(key)]));
    if (input.kind === "working") { for (const [key, text] of Object.entries(texts[side])) host.document(Uri.parse(key), text); workingInputs.push({ root: Uri.parse(input.root_uri), expected: `revision:${input.root_uri}` }); }
    inputs[side] = { kind: input.kind, input_id: side, root_file: root, revision: input.kind === "git" ? input.commit : `revision:${input.root_uri}`, complete: true, file_keys: Object.keys(texts[side]), dependency_candidates: input.kind === "working" ? [Uri.joinPath(Uri.parse(input.root_uri), "..", "missing.data").toString()] : [], search_paths: ["common"] };
  }
  const location = side => ({ uri: sourceUris[side].get(Object.keys(texts[side])[1]).toString(), range: { start: { line: 0, character: 3 }, end: { line: 0, character: 5 } } });
  const target = side => ({ occurrence_id: `${side}:equation:1`, domain: "aggregate", dimension: null, written_locations: [location(side)] });
  const row = { id: "/changed_equations/0", section: "aggregateEquations", group: "Aggregate equations", kind: "changed", label: "goods", before: "y=1", after: "y=2", scopes: ["aggregate"], sideScopes: { before: "aggregate", after: "aggregate" }, navigation: { id: "/changed_equations/0", kind: "equation", before: target("before"), after: target("after") } };
  return { snapshot: { before: { root_uri: "before", revision: inputs.before.revision, complete: true }, after: { root_uri: "after", revision: inputs.after.revision, complete: true }, rows: changed ? [row] : [], complete: true }, inputs, texts, sourceUris, working: workingInputs, holder, instance };
}
function snapshotResponse(request, changed = true) {
  const inputs = { schema_version: 1 }, sources = {}, navigation = { schema_version: 2, rows: [] };
  for (const side of ["before", "after"]) {
    const input = request[side], root = input.kind === "git" ? input.root_file : input.root_uri;
    const text = input.kind === "git" ? Object.fromEntries(Object.entries(input.sources).filter(([, value]) => value.kind === "text").map(([key, value]) => [key, value.text])) : { [root]: "var y; model; y = 2; end;\n" };
    inputs[side] = { kind: input.kind, input_id: input.input_id, root_file: root, revision: input.kind === "git" ? `git:${input.commit}` : input.expected_revision, complete: true, file_keys: Object.keys(text), dependency_candidates: [], search_paths: input.search_paths ?? [],
      ...(input.kind === "git" ? { repository_uri: input.repository_uri, commit: input.commit, requested_ref: input.requested_ref, source_policy: "git_tree" } : { root_uri: input.root_uri, expected_revision: input.expected_revision, source_policy: "editor_buffers_and_disk" }) };
    sources[side] = text; navigation[side] = { ...inputs[side] };
  }
  if (changed) {
    const target = side => ({ occurrence_id: `${side}:1`, domain: "aggregate", dimension: null, written_locations: [{ input_id: request[side].input_id, file_key: Object.keys(sources[side]).at(-1), ...(request[side].kind === "git" ? { commit: request[side].commit } : {}), range: { start: { line: 0, character: 0 }, end: { line: 0, character: 3 } } }] });
    navigation.rows.push({ id: "/changed_equations/0", kind: "equation", before: target("before"), after: target("after") });
  }
  return { state: "result", inputs, sources, navigation, diff: diffData(changed) };
}

function load(name, vscode, injections = {}) {
  for (const moduleName of ["changes_editor", "snapshot_compare", "history_sources", "diff", "settings", "extension"]) delete require.cache[require.resolve(`../../out/${moduleName}`)];
  const originalLoad = Module._load;
  Module._load = function(id, ...args) {
    if (id === "vscode") return vscode;
    if (Object.hasOwn(injections, id)) return injections[id];
    return originalLoad.call(this, id, ...args);
  };
  try { return require(`../../out/${name}`); } finally { Module._load = originalLoad; }
}
function createHost() {
  const host = { commands: new Map(), calls: [], providers: new Map(), texts: new Map(), custom: new Map(), panels: [], picks: [], dialogs: [], inputs: [], progress: [], shown: [], logs: [], failures: [], watchers: [], settings: new Map(), scopes: [], captures: [], revalidations: [], owners: [], changed: new Emitter(), modelUpdated: new Emitter(), configChanged: new Emitter(), closedText: new Emitter(), editorChanged: new Emitter(), tabsChanged: new Emitter(), tabGroupsChanged: new Emitter(), editor: undefined, activeTab: undefined };
  host.document = (uri, text = "var y; model; y=2; end;\n") => {
    const found = host.texts.get(uri.toString()); if (found) { found.text = text; return found; }
    const document = { uri, version: 1, languageId: "dynare", isClosed: false, text, getText() { return this.text; } }; host.texts.set(uri.toString(), document); return document;
  };
  host.editor = { document: host.document(anchorUri) };
  host.panel = document => {
    const received = new Emitter(), closed = new Emitter(), visible = new Emitter();
    const panel = { document, received, closed, visibility: visible, visible: true, viewColumn: 2, messages: [], disposed: false,
      webview: { options: {}, cspSource: "vscode-webview:", asWebviewUri: uri => uri, onDidReceiveMessage: received.event, postMessage: async message => { panel.messages.push(message); } },
      onDidDispose: closed.event, onDidChangeViewState: visible.event, dispose() { if (!this.disposed) { this.disposed = true; closed.fire(); } } };
    host.panels.push(panel); return panel;
  };
  const token = () => new CancellationTokenSource().token;
  const vscode = { Disposable, EventEmitter: Emitter, CancellationTokenSource, Uri, Range, RelativePattern, TabInputCustom, QuickPickItemKind: { Separator: -1 }, ProgressLocation: { Window: 10 }, ViewColumn: { Active: -1, Beside: 2 },
    commands: {
      registerCommand(id, callback) { host.commands.set(id, callback); return new Disposable(() => host.commands.delete(id)); },
      async executeCommand(id, ...args) {
        host.calls.push({ id, args });
        if (host.commands.has(id)) return host.commands.get(id)(...args);
        if (id !== "vscode.openWith") return undefined;
        const [uri] = args; let document = host.custom.get(uri.toString());
        host.activeTab = { input: new TabInputCustom(uri, changesViewType) };
        if (document && host.panels.some(panel => panel.document === document && !panel.disposed)) return undefined;
        document ??= host.provider.openCustomDocument(uri, {}, token()); host.custom.set(uri.toString(), document);
        const panel = host.panel(document); await host.provider.resolveCustomEditor(document, panel, token()); return undefined;
      },
    },
    workspace: {
      get textDocuments() { return [...host.texts.values()]; },
      async openTextDocument(uri) {
        if (host.loadText) await host.loadText(uri);
        if (host.missing?.has(uri.toString())) throw new Error("source absent");
        let document = host.texts.get(uri.toString());
        if (!document) {
          const provider = host.providers.get(uri.scheme), text = provider ? await provider.provideTextDocumentContent(uri, token()) : "var y; model; y=2; end;\n";
          document = host.document(uri, text);
        }
        return document;
      },
      registerTextDocumentContentProvider(scheme, provider) { host.providers.set(scheme, provider); return new Disposable(() => host.providers.delete(scheme)); },
      onDidCloseTextDocument: host.closedText.event, onDidChangeConfiguration: host.configChanged.event,
      getWorkspaceFolder: () => ({ uri: rootUri }), asRelativePath: uri => uri.path,
      getConfiguration(_section, scope) {
        host.scopes.push(scope); const settings = host.settings.get(scope?.uri?.toString()) ?? {};
        return { get: (key, fallback) => settings[key] ?? fallback, inspect: () => undefined };
      },
      createFileSystemWatcher(pattern) {
        const change = new Emitter(), create = new Emitter(), remove = new Emitter();
        const watcher = { pattern, change, create, remove, onDidChange: change.event, onDidCreate: create.event, onDidDelete: remove.event, disposed: false, dispose() { this.disposed = true; change.dispose(); create.dispose(); remove.dispose(); } }; host.watchers.push(watcher); return watcher;
      },
    },
    languages: { setTextDocumentLanguage: async (document, language) => { document.languageId = language; return document; } },
    window: {
      get activeTextEditor() { return host.editor; },
      tabGroups: { activeTabGroup: { get activeTab() { return host.activeTab; } }, onDidChangeTabs: host.tabsChanged.event, onDidChangeTabGroups: host.tabGroupsChanged.event },
      onDidChangeActiveTextEditor: host.editorChanged.event,
      registerCustomEditorProvider(viewType, provider, options) { host.provider = provider; host.providerOptions = options; host.viewType = viewType; return new Disposable(() => { host.provider = undefined; }); },
      async showQuickPick(items, options, cancel) { host.picks.push({ items, options, cancel }); return host.pick ? host.pick(items, options, cancel) : items.find(item => item.mode === "previous") ?? items[0]; },
      async showOpenDialog(options) { host.dialogs.push(options); return host.select ? host.select(options) : [baselineUri]; },
      async showInputBox(options, cancel) { host.inputs.push({ ...options, cancel }); return host.input ? host.input(options, cancel) : "HEAD~1"; },
      showInformationMessage: async message => { host.logs.push(message); },
      async withProgress(options, action) { const source = new CancellationTokenSource(); host.progress.push({ options, source }); return action({}, source.token); },
      showTextDocument: async (document, options) => { host.shown.push({ document, options }); return { document }; },
    },
  };
  const service = { currentInstance: 1, client: { initializeResult: { capabilities: { experimental: { dygnosis: { compareModelSnapshots: { schema_version: 1, navigation_schema_version: 2 } } }, executeCommandProvider: { commands: ["dynare/showEffectiveModel"] } } } },
    onDidInvalidate: host.changed.event, onDidChange: host.changed.event, onDidUpdateModelInfo: host.modelUpdated.event, log: message => host.logs.push(message), failure: async message => host.failures.push(message), ensureStarted: async () => {},
    rootForDocument: async document => host.rootFor ? host.rootFor(document) : document.uri,
    knownOwners: uri => host.knownOwners?.(uri) ?? [], knownIncludedFiles: () => host.includedFiles ?? [],
    selectOwner: (source, root) => host.owners.push({ source, root }),
    modelInfo: async (root, document, fresh, cancel) => host.modelInfo ? host.modelInfo(root, document, fresh, cancel) : ({ revision: `revision:${root.toString()}`, complete: true, related_files: [] }),
    revalidate: async (root, expected, instance) => { host.revalidations.push({ root, expected, instance }); return host.validate ? host.validate(root, expected, instance) : { revision: expected, complete: true }; },
    execute: async (command, args, cancel) => { host.engineCalls ??= []; host.engineCalls.push({ command, args: JSON.parse(JSON.stringify(args)), cancel }); return host.engine ? host.engine(command, args, cancel) : snapshotResponse(args[0]); },
  };
  const repo = { rootUri }, commit = ref => ({ hash: /^[a-f0-9]{40}$/.test(ref) ? ref : ref === "HEAD~1" ? commits.opened : commits.head, requested_ref: ref, parents: [], message: "repository commit", commitDate: new Date("2026-10-08T00:00:00Z") });
  const git = { clear() {}, repositoryFor: async () => repo, fileKey: (_repository, uri) => path.relative(rootPath, uri.fsPath).split(path.sep).join("/"),
    resolve: async (_repository, ref) => commit(ref),
    previous: async (_repository, anchor) => { host.previous ??= []; host.previous.push(anchor); const revision = commit(anchor ? commits.old : "HEAD"); return { revision, description: anchor ? `Parent of ${anchor.slice(0, 7)} · aaaaaaa` : "Last committed model · ccccccc" }; },
    history: async () => ({ commits: [commit(commits.old), commit(commits.opened)] }),
    refs: async () => [{ kind: "branch", name: "main", revision: commit("main") }, { kind: "remote", name: "origin/main", revision: commit("origin/main") }, { kind: "tag", name: "release", revision: commit(commits.old) }],
    rename: async () => host.rename,
    async capture(_repository, revision, rootFile) {
      const available = { "main.mod": { mode: "100644", object_id: "1".repeat(40) }, "computed.data": { mode: "100644", object_id: "2".repeat(40) }, "unused.data": { mode: "100644", object_id: "3".repeat(40) } };
      const manifest = host.manifest ?? available, sources = { [rootFile]: { kind: "text", text: '@#include computed_name\r\nvar y;\r\n' } };
      const capture = { manifest, sources, stats: { blobReads: 1 },
        input: (inputId, searchPaths) => ({ kind: "git", input_id: inputId, root_file: rootFile, repository_uri: rootUri.toString(), commit: revision.hash, requested_ref: revision.requested_ref, search_paths: searchPaths, manifest, sources: { ...sources } }),
        async load(keys) { host.loaded ??= []; host.loaded.push(...keys); for (const key of keys) sources[key] = { kind: "text", text: `// 😀 ${key}\nmodel; y=1; end;\n` }; },
        source: key => sources[key] ?? { kind: "failure", code: "source_not_loaded", message: "not loaded" }, modelPaths: () => Object.keys(manifest).filter(key => /\.(mod|dyn)$/.test(key)),
      }; return capture;
    },
    async provenance(document) {
      if (host.provenance) return host.provenance(document);
      const value = JSON.parse(document.uri.query); return { repository: repo, commit: commit(value.ref), file_key: path.basename(value.path) };
    },
  };
  host.install = () => {
    const { ComparisonFailure } = load("snapshot_compare", vscode);
    host.ComparisonFailure = ComparisonFailure;
    const capture = async (_service, _git, history, resource, cancel) => {
      host.captures.push({ resource, cancel, history });
      return host.capture ? host.capture(resource, cancel, history) : captured(resource, history, host, service.currentInstance);
    };
    const { registerChanges } = load("changes_editor", vscode, { "./git_source_host": { createGitSources: async () => { if (host.gitUnavailable) throw new Error("Git is unavailable"); return git; } }, "./snapshot_compare": { captureComparison: capture, ComparisonFailure } });
    host.registration = registerChanges(service); return host.registration;
  };
  host.open = argument => host.commands.get("dygnosis.openChanges")(argument);
  host.file = argument => host.commands.get("dygnosis.diffWith")(argument);
  host.openSaved = resource => vscode.commands.executeCommand("vscode.openWith", Uri.from({ scheme: changesScheme, path: `/${resourceName(resource)}`, query: resourceQuery(resource) }), changesViewType);
  host.last = (panel = host.panels.at(-1)) => panel.messages.at(-1);
  host.message = (message, panel = host.panels.at(-1)) => panel.received.fire(message);
  host.source = (side = "before", panel = host.panels.at(-1), extra = {}) => host.message({ type: "openSource", token: host.last(panel).token, rowId: "/changed_equations/0", side, ...extra }, panel);
  host.split = async document => { const panel = host.panel(document); await host.provider.resolveCustomEditor(document, panel, token()); return panel; };
  host.close = panel => { panel.dispose(); if (!host.panels.some(other => other.document === panel.document && !other.disposed)) { panel.document.dispose(); host.custom.delete(panel.document.uri.toString()); } };
  host.closeText = document => { document.isClosed = true; host.closedText.fire(document); host.texts.delete(document.uri.toString()); };
  return { host, service, vscode, git, token };
}
module.exports = { createHost, load, captured, snapshotResponse, diffData, deferred, flush, working, historical, comparison, commits, Uri, rootUri, rootPath, anchorUri, baselineUri, Disposable, Emitter, CancellationTokenSource, historyScheme, changesViewType };
