const assert = require("node:assert/strict");
const test = require("node:test");
const Module = require("node:module");
const { URL } = require("node:url");
const protocol = require("vscode-languageserver-protocol/node");
const { Lexer, marked } = require("marked");
class Disposable {
  constructor(callback = () => {}) { this.callback = callback; }
  dispose() { this.callback(); this.callback = () => {}; }
  static from(...items) { return new Disposable(() => items.forEach(item => item.dispose())); }
}
class Emitter {
  listeners = new Set();
  event = listener => { this.listeners.add(listener); return new Disposable(() => this.listeners.delete(listener)); };
  fire(value) { for (const listener of this.listeners) listener(value); }
}
class Uri {
  constructor(value) { this.value = value; const url = new URL(value); this.scheme = url.protocol.slice(0, -1); this.path = url.pathname; }
  toString() { return this.value; }
  static parse(value) { return new Uri(value); }
  static from({ scheme, path }) { return new Uri(`${scheme}:${path}`); }
}
class Position {
  constructor(line, character) { this.line = line; this.character = character; }
  isBeforeOrEqual(other) { return this.line < other.line || (this.line === other.line && this.character <= other.character); }
}
class Range {
  constructor(a, b, c, d) { this.start = typeof a === "number" ? new Position(a, b) : a; this.end = typeof a === "number" ? new Position(c, d) : b; }
  isEqual(other) { return this.start.line === other.start.line && this.start.character === other.start.character && this.end.line === other.end.line && this.end.character === other.end.character; }
}
class Diagnostic {
  constructor(range, message, severity) { this.range = range; this.message = message; this.severity = severity; }
}
class CodeAction {
  constructor(title, kind) { this.title = title; this.kind = kind; }
}
class Kind {
  constructor(value) { this.value = value; }
  contains(other) { return this.value === "" || other.value === this.value || other.value.startsWith(`${this.value}.`); }
  static QuickFix = new Kind("quickfix");
  static Refactor = new Kind("refactor");
}
class Base {}
let host;
const vscode = {
  Disposable, EventEmitter: Emitter, Uri, Position, Range, Diagnostic, CodeAction, CodeActionKind: Kind,
  CompletionItem: Base, CodeLens: Base, DocumentLink: Base, CallHierarchyItem: Base, TypeHierarchyItem: Base, SymbolInformation: Base, InlayHint: Base,
  DiagnosticSeverity: { Error: 0, Warning: 1, Information: 2, Hint: 3 }, DiagnosticTag: { Unnecessary: 1, Deprecated: 2 },
  DiagnosticRelatedInformation: class { constructor(location, message) { this.location = location; this.message = message; } },
  Location: class { constructor(uri, range) { this.uri = uri; this.range = range; } },
  StatusBarAlignment: { Left: 1 },
  CodeActionTriggerKind: { Invoke: 1, Automatic: 2 },
  window: {
    createStatusBarItem: () => { const item = { shown: false, show() { this.shown = true; }, hide() { this.shown = false; }, dispose() { this.shown = false; this.disposed = true; } }; host.status.push(item); return item; },
    showQuickPick: items => Promise.resolve(items.find(item => item.code === host.selection) ?? items[0]),
    showInputBox: () => Promise.resolve(host.input),
  },
  commands: {
    registerCommand: (name, callback) => { host.commands.set(name, callback); return new Disposable(() => host.commands.delete(name)); },
    executeCommand: (name, ...args) => { host.executed.push({ name, args }); return Promise.resolve(); },
  },
  workspace: {
    get textDocuments() { return host.documents; },
    getConfiguration: (_section, resource) => ({ get: (key, fallback) => host.settings[resource.uri.toString()]?.[key] ?? fallback }),
    registerTextDocumentContentProvider: (scheme, provider) => { host.providers.set(scheme, provider); return new Disposable(() => host.providers.delete(scheme)); },
    openTextDocument: uri => { const document = { uri, version: 1, languageId: "plaintext" }; host.documents.push(document); host.opened.push(document); return host.loadDocument?.(document) ?? Promise.resolve(document); },
    onDidChangeWorkspaceFolders: listener => host.folder.event(listener),
    onDidCloseTextDocument: listener => host.close.event(listener),
  },
  languages: { setTextDocumentLanguage: (document, language) => { document.languageId = language; return host.setLanguage?.(document) ?? Promise.resolve(document); } },
};
const languageClient = { ...protocol, vsdiag: { DocumentDiagnosticReportKind: { full: "full", unChanged: "unChanged" } } };
const originalLoad = Module._load;
Module._load = function(id, ...args) {
  if (id === "vscode") return vscode;
  if (id === "vscode-languageclient/node") return languageClient;
  if (id === "./client" && args[0].filename.endsWith("quickfix.js")) return { isAnalysisDocument: document => document.languageId === "dynare" && ["file", "untitled"].includes(document.uri.scheme) };
  return originalLoad.call(this, id, ...args);
};
const { registerDiagnosticActions, diagnosticCode, safeExplanation } = require("../out/quickfix");
// Exercise the installed v9 converters: data survives only on ProtocolDiagnostic instances.
const p2c = require("vscode-languageclient/lib/common/protocolConverter").createConverter();
const c2p = require("vscode-languageclient/lib/common/codeConverter").createConverter();
Module._load = originalLoad;
const token = { isCancellationRequested: false };
function reset() {
  host = { status: [], commands: new Map(), executed: [], providers: new Map(), documents: [], settings: {}, folder: new Emitter(), close: new Emitter(), changed: new Emitter(), opened: [], refreshes: 0, calls: [], logs: [], progress: new Map() };
  const provider = { onDidChangeDiagnosticsEmitter: { fire() { ++host.refreshes; } } };
  host.invalidated = new Emitter();
  const service = { middleware: {}, currentInstance: 1, onDidChange: host.changed.event, onDidInvalidate: host.invalidated.event,
    ensureStarted: () => Promise.resolve(),
    log: message => host.logs.push(message), failure: message => { host.logs.push(message); return Promise.resolve(); },
    execute: (command, args) => { host.calls.push({ command, args }); return Promise.resolve(host.markdown ?? "# W010\nExplanation"); },
    client: { protocol2CodeConverter: p2c, code2ProtocolConverter: c2p,
      initializeResult: { capabilities: { diagnosticProvider: { identifier: "dygnosis" } } },
      getFeature: () => ({ getProvider: () => provider }),
      onProgress: (_type, id, callback) => { host.progress.set(id, callback); return new Disposable(() => host.progress.delete(id)); },
      sendRequest: (_type, params) => { host.request = params; return host.workspaceRequest?.(params) ?? Promise.resolve({ items: [] }); },
    },
  };
  return service;
}
function document(name = "a.mod") { const doc = { uri: Uri.parse(`file:///project/${name}`), version: 1, languageId: "dynare" }; host.documents.push(doc); return doc; }
function wire(code = "W010", root = "file:///project/a.mod", severity = 2) {
  return { range: { start: { line: 0, character: 2 }, end: { line: 0, character: 3 } }, message: `Check ${code}`, source: "dygnosis", code,
    severity, data: { root, input_revision: "checked-1", related_context: [{ origin_frames: [{ kind: "macro", value: "1" }] }] },
    tags: [1], relatedInformation: [{ location: { uri: "file:///project/first.inc", range: { start: { line: 3, character: 1 }, end: { line: 3, character: 2 } } }, message: "First occurrence" }] };
}
const note = (...args) => p2c.asDiagnostic(wire(...args));
function push(service, doc, items) { const displayed = []; service.middleware.handleDiagnostics(doc.uri, items, (_uri, values) => displayed.push(values)); return displayed; }
const command = (name, ...args) => host.commands.get(name)(...args);
async function offers(service, doc, items, returned = [], only) {
  return service.middleware.provideCodeActions(doc, new Range(0, 0, 0, 20), { diagnostics: items, only }, token,
    (_document, _range, context) => { host.actionContext = context; return returned; });
}
async function hide(service, doc, diagnostic) {
  const result = await offers(service, doc, [diagnostic]);
  const action = result.find(action => action.command?.command === "dygnosis.ignoreDiagnostic");
  assert.ok(action, "Ignore is offered");
  command("dygnosis.ignoreDiagnostic", ...action.command.arguments);
}
function deferred() { let resolve; const promise = new Promise(done => { resolve = done; }); return { promise, resolve }; }

test("push hide applies across files/classes and restoration replays the latest raw objects", async () => {
  const service = reset(), registration = registerDiagnosticActions(service), a = document(), b = document("b.mod");
  const original = note(), error = note("E001", undefined, 1), info = note("I208", undefined, 3);
  const displayed = push(service, a, [original, error, info]);
  for (const diagnostic of [original, error, info]) {
    const actions = await offers(service, a, [diagnostic]);
    assert.equal(actions.filter(action => action.diagnostics?.includes(diagnostic)).length, 2);
  }
  await hide(service, a, original);
  assert.deepEqual(displayed.at(-1), [error, info]);
  const next = note(); next.message = "Latest result";
  const latestDisplay = push(service, a, [next, error]);
  const otherFile = push(service, b, [note()]);
  assert.deepEqual(otherFile.at(-1), []);
  assert.equal(host.status[0].text, "$(eye-closed) 1 hidden check");
  assert.equal(host.status[0].command, "dygnosis.showDiagnostic");
  await command("dygnosis.showDiagnostic");
  assert.equal(otherFile.at(-1).length, 1);
  assert.equal(host.status[0].shown, false);
  assert.equal(c2p.asDiagnostic(original).data, original.data);
  assert.equal(c2p.asDiagnostic(original).relatedInformation[0].location.uri, "file:///project/first.inc");
  // Replaying the second push restores exactly that newer diagnostic.
  assert.equal(latestDisplay.at(-1)[0], next);
  registration.dispose();
});

test("Show cannot replay a push after a dependency or folder change at the same document version", async () => {
  for (const change of ["dependency", "folder"]) {
    const service = reset(), registration = registerDiagnosticActions(service), doc = document();
    const diagnostic = note(), displayed = push(service, doc, [diagnostic]);
    await hide(service, doc, diagnostic);
    const count = displayed.length;
    if (change === "dependency") { host.invalidated.fire({ reason: "file" }); host.changed.fire(); } else host.folder.fire();
    await command("dygnosis.showAllDiagnostics");
    assert.equal(displayed.length, count, "old push was not replayed");
    const fresh = note(); fresh.message = "Fresh input";
    const latest = push(service, doc, [fresh]);
    await hide(service, doc, fresh);
    await command("dygnosis.showAllDiagnostics");
    assert.equal(latest.at(-1)[0], fresh, "a subsequent push remains usable");
    registration.dispose();
  }
});

test("owner presentation changes keep a current push available to Show", async () => {
  const service = reset(), registration = registerDiagnosticActions(service), doc = document();
  const diagnostic = note(), displayed = push(service, doc, [diagnostic]);
  await hide(service, doc, diagnostic);
  host.changed.fire();
  await command("dygnosis.showAllDiagnostics");
  assert.equal(displayed.at(-1)[0], diagnostic);
  registration.dispose();
});

test("a server revision notification preserves its new push and invalidates only older root facts", async () => {
  const service = reset(), registration = registerDiagnosticActions(service), doc = document(), other = document("other.mod");
  const diagnostic = note(), displayed = push(service, doc, [diagnostic]);
  const independent = note("W010", other.uri.toString()), otherDisplay = push(service, other, [independent]);
  await hide(service, doc, diagnostic);
  host.invalidated.fire({ reason: "input", root: doc.uri.toString(), inputRevision: "checked-1" }); host.changed.fire();
  await command("dygnosis.showAllDiagnostics");
  assert.equal(displayed.at(-1)[0], diagnostic, "notification after a current push retains it");
  await hide(service, doc, diagnostic);
  const count = displayed.length;
  host.invalidated.fire({ reason: "input", root: doc.uri.toString(), inputRevision: "changed-2" }); host.changed.fire();
  await command("dygnosis.showAllDiagnostics");
  assert.equal(displayed.length, count + 1, "retired contribution clears its display on Show");
  assert.deepEqual(displayed.at(-1), [], "old revision cannot replay");
  assert.equal(otherDisplay.at(-1)[0], independent, "other roots retain their current push");
  registration.dispose();
});

test("shared includes retain a current owner's rows when another owner becomes stale", async () => {
  const service = reset(), registration = registerDiagnosticActions(service), doc = document("shared.inc");
  const left = note("W010", "file:///project/left.mod"), right = note("W010", "file:///project/right.mod");
  const displayed = push(service, doc, [left, right]);
  await hide(service, doc, left);
  host.invalidated.fire({ reason: "input", root: "file:///project/left.mod", inputRevision: "changed" }); host.changed.fire();
  await command("dygnosis.showAllDiagnostics");
  assert.deepEqual(displayed.at(-1), [right]);
  registration.dispose();
});

test("legacy metadata-less pushes survive their notification but expire on local input changes", async () => {
  const service = reset(), registration = registerDiagnosticActions(service), doc = document();
  const value = wire(); delete value.data;
  const diagnostic = p2c.asDiagnostic(value), displayed = push(service, doc, [diagnostic]);
  await hide(service, doc, diagnostic);
  host.invalidated.fire({ reason: "input", root: doc.uri.toString(), inputRevision: "current" }); host.changed.fire();
  await command("dygnosis.showAllDiagnostics");
  assert.equal(displayed.at(-1)[0], diagnostic);
  await hide(service, doc, diagnostic);
  const count = displayed.length;
  host.invalidated.fire({ reason: "file" }); host.changed.fire();
  await command("dygnosis.showAllDiagnostics");
  assert.equal(displayed.length, count);
  registration.dispose();
});

test("actions retain engine fixes and raw root/data context, hiding only diagnostic-bound actions", async () => {
  const service = reset(), registration = registerDiagnosticActions(service), doc = document("shared.inc");
  const left = note("I208", "file:///project/left.mod", 3), right = note("I208", "file:///project/right.mod", 3), warning = note();
  push(service, doc, [left, right, warning]);
  // Another VS Code caller can supply a new diagnostic instance with the same engine context.
  const supplied = p2c.asDiagnostic(c2p.asDiagnostic(right));
  const leftFix = new CodeAction("Name equations", Kind.QuickFix); leftFix.diagnostics = [left];
  const refactor = new CodeAction("Shock template", Kind.Refactor);
  const independent = { title: "Independent action", command: "test.independent" };
  const actions = await offers(service, doc, [supplied], [leftFix, refactor, independent]);
  assert.equal(host.actionContext.diagnostics.length, 1);
  assert.equal(host.actionContext.diagnostics[0], right);
  assert.ok(actions.includes(leftFix));
  assert.equal((await c2p.asCodeActionContext(host.actionContext)).diagnostics[0].data.root, "file:///project/right.mod");
  await hide(service, doc, left);
  const filtered = await offers(service, doc, [warning], [leftFix, refactor, independent]);
  assert.ok(!filtered.includes(leftFix));
  assert.ok(filtered.includes(refactor)); assert.ok(filtered.includes(independent));
  assert.deepEqual(await offers(service, doc, [], [refactor], Kind.Refactor), [refactor]);
  registration.dispose();
});

test("pull full/unchanged and related document reports keep a raw cache and empty every display path", async () => {
  const service = reset(), registration = registerDiagnosticActions(service), doc = document(), child = document("child.inc"), diagnostic = note();
  const displayed = push(service, doc, [diagnostic]); await hide(service, doc, diagnostic);
  const raw = note("W010"), related = note("W010", doc.uri.toString());
  const report = await service.middleware.provideDiagnostics(doc, undefined, token, () => ({ kind: "full", resultId: "two", items: [raw], relatedDocuments: { [child.uri.toString()]: { kind: "full", resultId: "child", items: [related] } } }));
  assert.equal(report.kind, "full"); assert.equal(report.resultId, "two");
  assert.deepEqual(report.items, []); assert.deepEqual(report.relatedDocuments[child.uri.toString()].items, []);
  assert.equal(report.relatedDocuments[child.uri.toString()].resultId, "child");
  // Show replays the push collection, not the pull cache.
  command("dygnosis.showAllDiagnostics"); assert.equal(host.refreshes, 2);
  assert.equal(displayed.at(-1)[0], diagnostic);
  const unchanged = await service.middleware.provideDiagnostics(doc, "two", token, () => ({ kind: "unChanged", resultId: "two", relatedDocuments: { [child.uri.toString()]: { kind: "unChanged", resultId: "child" } } }));
  assert.equal(unchanged.kind, "full"); assert.equal(unchanged.resultId, "two");
  assert.deepEqual(unchanged.items, []);
  assert.deepEqual(unchanged.relatedDocuments[child.uri.toString()].items, []);
  assert.equal(unchanged.relatedDocuments[child.uri.toString()].resultId, "child");
  // Empty full withdrawal clears the pull cache; a later unchanged still paints nothing.
  const cleared = await service.middleware.provideDiagnostics(doc, "two", token, () => ({ kind: "full", resultId: "three", items: [] }));
  assert.deepEqual(cleared.items, []); assert.equal(cleared.resultId, "three");
  const afterClear = await service.middleware.provideDiagnostics(doc, "three", token, () => ({ kind: "unChanged", resultId: "three" }));
  assert.equal(afterClear.kind, "full"); assert.deepEqual(afterClear.items, []);
  registration.dispose();
});

test("workspace pull empties partial and final rows, keeps result IDs, and frees progress", async () => {
  const service = reset(), registration = registerDiagnosticActions(service), doc = document();
  const diagnostic = note(); push(service, doc, [diagnostic]); await hide(service, doc, diagnostic);
  host.workspaceRequest = params => {
    host.progress.get(params.partialResultToken)({ items: [{ uri: doc.uri.toString(), version: 1, kind: "full", resultId: "partial", items: [wire()] }] });
    return Promise.resolve({ items: [{ uri: "file:///project/unopened.mod", version: null, kind: "full", resultId: "final", items: [wire()] }] });
  };
  const chunks = [];
  const collection = new Map(), resultIds = new Map();
  // This is the v9 consumer: diagnostics and result IDs come only from reporter;
  // its awaited provider return is ignored (diagnostic.js:374-388).
  const consume = async previous => {
    await service.middleware.provideWorkspaceDiagnostics(previous, token, chunk => {
      chunks.push(chunk);
      for (const item of chunk?.items ?? []) {
        if (item.kind === "full") collection.set(item.uri.toString(), item.items);
        resultIds.set(item.uri.toString(), item.resultId);
      }
    }, () => { throw new Error("v9 next's unfiltered reporter must not be used"); });
  };
  await consume([{ uri: doc.uri, value: "prior" }]);
  assert.equal(host.request.identifier, "dygnosis");
  assert.deepEqual(host.request.previousResultIds, [{ uri: doc.uri.toString(), value: "prior" }]);
  assert.equal(chunks.length, 2, "one partial and one final delivery");
  assert.deepEqual(collection.get(doc.uri.toString()), []);
  assert.deepEqual(collection.get("file:///project/unopened.mod"), []);
  assert.equal(resultIds.get(doc.uri.toString()), "partial");
  assert.equal(resultIds.get("file:///project/unopened.mod"), "final");
  assert.equal(host.progress.size, 0);
  command("dygnosis.showAllDiagnostics");
  host.workspaceRequest = () => Promise.resolve({ items: [{ uri: "file:///project/unopened.mod", version: null, kind: "unchanged", resultId: "final" }] });
  await consume([]);
  assert.equal(chunks.length, 3, "unchanged final is delivered once");
  assert.equal(chunks.at(-1).items[0].kind, "full");
  assert.deepEqual(collection.get("file:///project/unopened.mod"), []);
  assert.equal(resultIds.get("file:///project/unopened.mod"), "final");
  registration.dispose();
});

test("push then pull and pull then push keep one painted list and push-owned actions", async () => {
  const service = reset(), registration = registerDiagnosticActions(service), doc = document();
  const first = note(), delayed = note("E001", undefined, 1);
  // Push first: pull returns empty and cannot replace action context.
  const painted = push(service, doc, [first]);
  assert.equal(painted.at(-1)[0], first);
  const afterPush = await service.middleware.provideDiagnostics(doc, undefined, token, () => ({ kind: "full", resultId: "pull-1", items: [delayed] }));
  assert.deepEqual(afterPush.items, []);
  const actions = await offers(service, doc, [first]);
  assert.equal(host.actionContext.diagnostics[0], first);
  assert.equal((await c2p.asCodeActionContext(host.actionContext)).diagnostics[0].data.root, "file:///project/a.mod");
  assert.ok(actions.some(action => action.command?.command === "dygnosis.ignoreDiagnostic"));
  assert.ok(actions.some(action => action.command?.command === "dygnosis.explainDiagnostic"));
  // Pull first at the same version creates no painted list; the later push owns display and actions.
  const other = document("order.mod");
  const pullFirst = note("W010", other.uri.toString());
  const beforePush = await service.middleware.provideDiagnostics(other, undefined, token, () => ({ kind: "full", resultId: "early", items: [pullFirst] }));
  assert.deepEqual(beforePush.items, []);
  assert.deepEqual(await offers(service, other, []), []);
  const later = note("I208", other.uri.toString(), 3);
  const secondPaint = push(service, other, [later]);
  assert.equal(secondPaint.at(-1)[0], later);
  const latePull = await service.middleware.provideDiagnostics(other, "early", token, () => ({ kind: "full", resultId: "late", items: [pullFirst] }));
  assert.deepEqual(latePull.items, []);
  await offers(service, other, [later]);
  assert.equal(host.actionContext.diagnostics[0], later);
  assert.equal((await c2p.asCodeActionContext(host.actionContext)).diagnostics[0].data, later.data);
  // Empty push withdrawal is authoritative for Show replay.
  const withdrawn = push(service, doc, []);
  assert.deepEqual(withdrawn.at(-1), []);
  command("dygnosis.showAllDiagnostics");
  assert.deepEqual(withdrawn.at(-1), []);
  registration.dispose();
});

test("stale document, server, folder, and canceled request results never replace retained data", async () => {
  for (const invalidate of [(_service, doc) => { ++doc.version; }, service => { ++service.currentInstance; host.changed.fire(); }, () => host.folder.fire(), () => host.changed.fire(), (_service, _doc, cancellation) => { cancellation.isCancellationRequested = true; }]) {
    const service = reset(), registration = registerDiagnosticActions(service), doc = document(), pending = deferred();
    const cancellation = { isCancellationRequested: false };
    const result = service.middleware.provideDiagnostics(doc, undefined, cancellation, () => pending.promise);
    invalidate(service, doc, cancellation);
    pending.resolve({ kind: "full", items: [note()] });
    assert.equal(await result, undefined); registration.dispose();
  }
});

test("Show restores one of several codes; server restart retains hiding and disposal restores hooks", async () => {
  const service = reset(), doc = document(), first = note(), second = note("E001", undefined, 1);
  const original = (_uri, items, next) => next(_uri, items);
  service.middleware.handleDiagnostics = original;
  const registration = registerDiagnosticActions(service);
  const displayed = push(service, doc, [first, second]);
  await hide(service, doc, first); await hide(service, doc, second);
  assert.deepEqual(displayed.at(-1), []); assert.equal(host.status[0].text, "$(eye-closed) 2 hidden checks");
  host.selection = "W010"; await command("dygnosis.showDiagnostic");
  assert.deepEqual(displayed.at(-1), [first]); assert.equal(host.status[0].text, "$(eye-closed) 1 hidden check");
  ++service.currentInstance; host.changed.fire();
  const restarted = push(service, doc, [first, second]); assert.deepEqual(restarted.at(-1), [first]);
  registration.dispose(); assert.equal(service.middleware.handleDiagnostics, original);
  assert.equal(host.status[0].shown, false);
});

test("pending action/Explain/workspace results cannot reopen UI after edits or disposal", async () => {
  const service = reset(), registration = registerDiagnosticActions(service), doc = document(), diagnostic = note();
  push(service, doc, [diagnostic]);
  const delayed = deferred();
  const pendingAction = service.middleware.provideCodeActions(doc, diagnostic.range, { diagnostics: [diagnostic] }, token, () => delayed.promise);
  host.changed.fire(); delayed.resolve([new CodeAction("Old fix", Kind.QuickFix)]);
  assert.deepEqual(await pendingAction, []);
  const actions = await offers(service, doc, [diagnostic]);
  const explain = actions.find(action => action.command.command === "dygnosis.explainDiagnostic");
  const explanation = deferred(); service.execute = () => explanation.promise;
  const pendingExplain = command("dygnosis.explainDiagnostic", ...explain.command.arguments);
  const workspace = deferred(); host.workspaceRequest = () => workspace.promise;
  const chunks = [];
  const pendingWorkspace = service.middleware.provideWorkspaceDiagnostics([], token, chunk => chunks.push(chunk), () => undefined);
  assert.equal(host.progress.size, 1); registration.dispose(); assert.equal(host.progress.size, 0);
  explanation.resolve("# Old explanation"); workspace.resolve({ items: [] });
  await pendingExplain; assert.equal(await pendingWorkspace, undefined);
  assert.deepEqual(host.opened, []); assert.deepEqual(chunks, []);
});

test("resource settings disable only offers; invalid values fall back; foreign diagnostics stay untouched", async () => {
  const service = reset(), registration = registerDiagnosticActions(service), doc = document(), diagnostic = note();
  const displayed = push(service, doc, [diagnostic]); await hide(service, doc, diagnostic);
  host.settings[doc.uri.toString()] = { "diagnosticActions.ignore": false, "diagnosticActions.explain": false };
  command("dygnosis.showAllDiagnostics"); assert.equal(displayed.at(-1)[0], diagnostic);
  assert.deepEqual(await offers(service, doc, [diagnostic]), []);
  host.settings[doc.uri.toString()] = { "diagnosticActions.ignore": "no", "diagnosticActions.explain": false };
  assert.equal((await offers(service, doc, [diagnostic])).length, 1); assert.ok(host.logs[0].includes("Invalid dynare.diagnosticActions.ignore"));
  const foreign = note(); foreign.source = "Other extension";
  const other = document("foreign.mod");
  assert.deepEqual(await offers(service, other, [foreign]), []);
  assert.equal(foreign.source, "Other extension"); registration.dispose();
});

test("folder changes clear hidden codes and stale commands cannot hide after an edit", async () => {
  const service = reset(), registration = registerDiagnosticActions(service), doc = document(), diagnostic = note();
  const displayed = push(service, doc, [diagnostic]);
  const actions = await offers(service, doc, [diagnostic]); const saved = actions[0].command.arguments;
  ++doc.version; command("dygnosis.ignoreDiagnostic", ...saved); assert.equal(host.status[0].shown, false);
  push(service, doc, [diagnostic]); await hide(service, doc, diagnostic);
  host.folder.fire(); assert.equal(host.status[0].shown, false);
  assert.equal(displayed.at(-1)[0], diagnostic);
  registration.dispose(); assert.equal(host.commands.size, 0); assert.equal(host.providers.size, 0); assert.equal(host.status[0].disposed, true);
  assert.equal(service.middleware.handleDiagnostics, undefined);
});

test("Explain requests backend Markdown, opens exact-code Help, and strips command links", async () => {
  const service = reset(), registration = registerDiagnosticActions(service), doc = document(), diagnostic = note();
  push(service, doc, [diagnostic]); host.markdown = "# W010\n[Run](command:danger)\n<img src=x onerror=alert(1)>\n[Docs](https://www.dynare.org/)";
  const actions = await offers(service, doc, [diagnostic]);
  const explain = actions.find(action => action.command.command === "dygnosis.explainDiagnostic");
  await command("dygnosis.explainDiagnostic", ...explain.command.arguments);
  assert.deepEqual(host.calls[0], { command: "dynare/explainDiagnostic", args: ["W010"] });
  assert.equal(host.executed[0].name, "dygnosis.openHelp");
  assert.equal(host.executed[0].args[0].code, "W010");
  assert.equal(host.executed[0].args[0].markdown.includes("command:"), false);
  assert.equal(host.opened.length, 0);
  host.input = "E001"; await command("dygnosis.explainDiagnostic"); assert.equal(host.calls[1].args[0], "E001");
  assert.doesNotMatch(marked.parse(safeExplanation("[bad](command%3Adanger)\n[bad][ref]\n[ref]: command:danger\n<a href='command:danger'>x</a>")), /(?:href|src)="(?:command|javascript|data|vscode):/i);
  assert.equal(diagnosticCode(p2c.asDiagnostic({ ...wire(), code: 100, codeDescription: { href: "https://example.org/check" } })), "100");
  registration.dispose();
});

test("Markdown destinations are normalized by the parser; safe formatting and literal code survive", () => {
  const attacks = [
    "[Run](command\\:danger)",
    "[Run](\ncommand:danger\n)",
    "[Run](<command:danger>)",
    "[Run][ref]\n\n[ref]:\n  command\\:danger",
    "[Run](command&colon;danger)",
    "[Run](command%3Adanger)",
    "[Run](command&#58;danger)",
    "[Run]<i></i>(command:danger)",
    "[Run](javascript:alert(1))",
    "![Run](command:danger)",
  ];
  for (const attack of attacks) {
    const safe = safeExplanation(attack);
    const links = [];
    marked.walkTokens(Lexer.lex(safe), token => { if (token.type === "link" || token.type === "image") links.push(token.href); });
    assert.deepEqual(links, [], `${attack} became ${safe}`);
    assert.doesNotMatch(marked.parse(safe), /<(?:script|img)|(?:href|src)="(?:command|javascript|data|vscode):/i);
  }
  const ordinary = "# Heading\n\n**Bold** and *italic*. [Docs](https://www.dynare.org/manual/)\n\n- First\n- Second\n\n> A quote\n\n```dynare\nvar y;\n```\n\n`[Run](command:literal)`";
  assert.equal(marked.parse(safeExplanation(ordinary)), marked.parse(ordinary));
});

test("palette Explain starts the first engine and rejects a restart during its backend reply", async () => {
  const service = reset(), registration = registerDiagnosticActions(service);
  service.currentInstance = 0;
  service.ensureStarted = () => { service.currentInstance = 1; host.changed.fire(); return Promise.resolve(); };
  host.input = "W010";
  await command("dygnosis.explainDiagnostic");
  assert.equal(host.executed[0].name, "dygnosis.openHelp");
  host.executed.length = 0; host.opened.length = 0;
  service.ensureStarted = () => Promise.resolve();
  const pending = deferred(); service.execute = () => pending.promise;
  const result = command("dygnosis.explainDiagnostic");
  await new Promise(resolve => setImmediate(resolve));
  ++service.currentInstance; host.changed.fire(); pending.resolve("# Old engine");
  await result;
  assert.deepEqual(host.executed, []); assert.deepEqual(host.opened, []);
  registration.dispose();
});
