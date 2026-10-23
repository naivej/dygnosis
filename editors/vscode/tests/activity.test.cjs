const { compiledModule } = require("./helpers/extension_modules.cjs");
const assert = require("node:assert/strict");
const test = require("node:test");
const Module = require("node:module");
const { URL } = require("node:url");

class Emitter {
  listeners = new Set();
  event = listener => { this.listeners.add(listener); return { dispose: () => this.listeners.delete(listener) }; };
  fire(value) { for (const listener of [...this.listeners]) listener(value); }
  dispose() { this.listeners.clear(); }
}
function uri(value) {
  const parsed = new URL(value);
  return { scheme: parsed.protocol.slice(0, -1), fsPath: decodeURIComponent(parsed.pathname), toString: () => value };
}
const vscode = { EventEmitter: Emitter, Uri: { parse: uri } };
const originalLoad = Module._load;
Module._load = function(id, ...args) { return id === "vscode" ? vscode : originalLoad.call(this, id, ...args); };
const { RequestActivity } = require(compiledModule("activity"));
Module._load = originalLoad;

const root = uri("file:///workspace/main.mod");
const document = { textDocument: { uri: root.toString() } };
const expansion = { command: "dynare/showEffectiveModel", arguments: [{ root_uri: root.toString() }] };
function deferred() {
  let resolve, reject;
  const promise = new Promise((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}
function cancellation() {
  const listeners = new Set();
  return { listeners, isCancellationRequested: false,
    onCancellationRequested(callback) { listeners.add(callback); return { dispose: () => listeners.delete(callback) }; },
    cancel() { this.isCancellationRequested = true; for (const callback of [...listeners]) callback(); },
  };
}
function setup(context) {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const activity = new RequestActivity(), events = [];
  activity.onDidChange(() => events.push(activity.forDocument(root, root)));
  context.after(() => activity.dispose());
  return { activity, middleware: activity.middleware({}), clock: context.mock.timers, events };
}

test("slow macro expansion exposes model activity while fast requests keep their results without flashing", async context => {
  // Guards Expand macros waiting silently and routine cached responses flashing a spinner.
  const { activity, middleware, clock, events } = setup(context), pending = deferred(), token = cancellation();
  const result = middleware.sendRequest({ method: "workspace/executeCommand" }, expansion, token, (type, value, cancelled) => {
    assert.equal(type.method, "workspace/executeCommand"); assert.equal(value, expansion); assert.equal(cancelled, token);
    return pending.promise;
  });
  clock.tick(249); assert.equal(activity.forDocument(root), undefined); assert.equal(events.length, 0);
  clock.tick(1); assert.equal(activity.forDocument(root).label, "Expanding macros");
  pending.resolve({ source: "var y;" }); assert.deepEqual(await result, { source: "var y;" });
  assert.equal(activity.forDocument(root), undefined); assert.equal(token.listeners.size, 0);
  assert.equal(await middleware.sendRequest("textDocument/hover", document, undefined, async () => "hover"), "hover");
  clock.tick(250); assert.equal(events.length, 2);
});

test("overlapping model requests retain activity until the last result or failure", async context => {
  // Guards a background completion hiding an unfinished user action or a rejected request leaving a stuck spinner.
  const { activity, middleware, clock } = setup(context), expand = deferred(), hover = deferred();
  const first = middleware.sendRequest("workspace/executeCommand", expansion, undefined, () => expand.promise);
  const second = middleware.sendRequest("textDocument/hover", document, undefined, () => hover.promise);
  const failure = new Error("connection closed"), rejected = assert.rejects(second, error => error === failure);
  clock.tick(250);
  assert.equal(activity.forDocument(root).label, "Expanding macros");
  assert.equal(activity.forDocument(root).pending, 2);
  assert.match(activity.forDocument(root).detail, /Expanding macros[\s\S]*Loading hover/);
  expand.resolve(); await first;
  assert.equal(activity.forDocument(root).label, "Loading hover");
  hover.reject(failure); await rejected;
  assert.equal(activity.forDocument(root), undefined);
});

test("visible work keeps overlapping requests busy while another model keeps its own delay", async context => {
  // Guards a false idle interval when an older request finishes before newer overlapping work reaches 250 ms.
  const { activity, middleware, clock } = setup(context), expand = deferred(), hover = deferred(), definition = deferred(), unrelated = deferred();
  const other = uri("file:///workspace/other.mod");
  const first = middleware.sendRequest("workspace/executeCommand", expansion, undefined, () => expand.promise);
  clock.tick(200);
  const second = middleware.sendRequest("textDocument/hover", document, undefined, () => hover.promise);
  const another = middleware.sendRequest("textDocument/hover", { textDocument: { uri: other.toString() } }, undefined, () => unrelated.promise);
  clock.tick(50); assert.equal(activity.forDocument(root).pending, 2);
  expand.resolve(); await first;
  assert.equal(activity.forDocument(root).label, "Loading hover");
  assert.equal(activity.forDocument(other), undefined);
  const third = middleware.sendRequest("textDocument/definition", document, undefined, () => definition.promise);
  assert.equal(activity.forDocument(root).pending, 2);
  hover.resolve(); await second;
  assert.equal(activity.forDocument(root).label, "Finding definition");
  clock.tick(199); assert.equal(activity.forDocument(other), undefined);
  clock.tick(1); assert.equal(activity.forDocument(other).label, "Loading hover");
  definition.resolve(); unrelated.resolve(); await Promise.all([third, another]);
  assert.equal(activity.forDocument(root), undefined); assert.equal(activity.forDocument(other), undefined);
});

test("model activity excludes other roots, workspace jobs and comparisons", async context => {
  // Guards an unrelated or workspace request replacing the active model's counts.
  const { activity, middleware, clock } = setup(context), work = [];
  const requests = [
    ["textDocument/hover", { textDocument: { uri: "file:///workspace/other.mod" } }],
    ["workspace/diagnostic", {}], ["workspace/symbol", { query: "y" }],
    ["workspace/executeCommand", { command: "dynare/workspaceStatus", arguments: [] }],
    ["workspace/executeCommand", { command: "dynare/recheckWorkspace", arguments: [] }],
    ["workspace/executeCommand", { command: "dynare/compareModelSnapshots", arguments: [{ root_uri: root.toString() }] }],
    ["workspace/executeCommand", { command: "dynare/modelInfo", arguments: [{ root_uri: "file:///workspace/other.mod", document_uri: root.toString() }] }],
  ];
  for (const [type, params] of requests) {
    const pending = deferred();
    work.push({ pending, result: middleware.sendRequest(type, params, undefined, () => pending.promise) });
  }
  clock.tick(250); assert.equal(activity.forDocument(root, root), undefined);
  assert.equal(activity.forDocument(uri("file:///workspace/other.mod")).label, "Loading hover");
  for (const { pending } of work) pending.resolve();
  await Promise.all(work.map(entry => entry.result));
});

test("the known model owner scopes include requests and explicit roots retain their authority", async context => {
  // Guards a helper action leaking to another owner after the active configuration context changes.
  const { activity, clock } = setup(context), helper = uri("file:///workspace/part.inc"), pending = deferred();
  const middleware = activity.middleware({}, value => value === helper.toString() ? root.toString() : undefined);
  const result = middleware.sendRequest("textDocument/definition", { textDocument: { uri: helper.toString() } }, undefined, () => pending.promise);
  clock.tick(250);
  assert.equal(activity.forDocument(root, root).label, "Finding definition");
  assert.equal(activity.forDocument(helper, root).label, "Finding definition");
  assert.equal(activity.forDocument(helper, uri("file:///workspace/other.mod")), undefined);
  pending.resolve(); await result;
});

test("call hierarchy and source or Rename commands use their actual document targets", async context => {
  // Guards supported single-model wire shapes without a top-level textDocument losing their activity.
  const { activity, middleware, clock } = setup(context);
  const cases = [
    ["callHierarchy/incomingCalls", { item: { uri: root.toString() } }, "Finding callers"],
    ["callHierarchy/outgoingCalls", { item: { uri: root.toString() } }, "Finding calls"],
    ["workspace/executeCommand", { command: "dynare/captureRename", arguments: [document] }, "Preparing Rename"],
    ["workspace/executeCommand", { command: "dynare/sourceCandidates", arguments: [document] }, "Finding source candidates"],
  ];
  for (const [method, params, label] of cases) {
    const pending = deferred(), result = middleware.sendRequest(method, params, undefined, () => pending.promise);
    clock.tick(250); assert.equal(activity.forDocument(root).label, label);
    pending.resolve(); await result;
  }
  assert.equal(await middleware.sendRequest("workspace/executeCommand", { command: "dynare/explainDiagnostic", arguments: ["E063"] }, undefined, async () => "reference"), "reference");
  clock.tick(250); assert.equal(activity.forDocument(root), undefined);
});

test("cancellation releases unanswered activity and a late reply cannot hide newer work", async context => {
  // Guards superseded requests keeping activity alive when the server ignores cancellation.
  const { activity, middleware, clock } = setup(context), pending = deferred(), next = deferred(), token = cancellation();
  const old = middleware.sendRequest("textDocument/hover", document, token, () => pending.promise);
  clock.tick(250); token.cancel();
  assert.equal(activity.forDocument(root), undefined); assert.equal(token.listeners.size, 0);
  const current = middleware.sendRequest("textDocument/definition", document, undefined, () => next.promise);
  clock.tick(250); assert.equal(activity.forDocument(root).label, "Finding definition");
  pending.resolve("late"); assert.equal(await old, "late"); assert.equal(activity.forDocument(root).label, "Finding definition");
  const alreadyCancelled = cancellation(); alreadyCancelled.cancel();
  assert.equal(await middleware.sendRequest("textDocument/references", document, alreadyCancelled, async () => "cancelled"), "cancelled");
  assert.equal(alreadyCancelled.listeners.size, 0);
  next.resolve(); await current; assert.equal(activity.forDocument(root), undefined);
});

test("replacement scopes reject retired activity and preserve live feature middleware", async context => {
  // Guards old server replies clearing a replacement's activity and a wrapper hiding later feature middleware.
  const { activity, clock } = setup(context), pending = deferred(), next = deferred();
  let forwarded = 0;
  const base = { sendRequest: (type, param, token, continuation) => { ++forwarded; return continuation(type, param, token); } };
  const previous = activity.middleware(base);
  const old = previous.sendRequest("textDocument/hover", document, undefined, () => pending.promise);
  clock.tick(250); assert.equal(activity.forDocument(root).label, "Loading hover");
  const replacement = activity.middleware(base); assert.equal(activity.forDocument(root), undefined);
  base.provideHover = () => "new feature";
  assert.equal(replacement.provideHover(), "new feature");
  const current = replacement.sendRequest("textDocument/references", document, undefined, () => next.promise);
  clock.tick(250); assert.equal(activity.forDocument(root).label, "Finding references");
  pending.resolve(); await old;
  await previous.sendRequest("textDocument/definition", document, undefined, async () => "retired");
  assert.equal(activity.forDocument(root).label, "Finding references"); assert.equal(forwarded, 3);
  next.resolve(); await current; assert.equal(activity.forDocument(root), undefined);
});

test("reset and disposal clear delays, cancellation subscriptions and visible activity", async context => {
  // Guards Restart Language Server or extension shutdown retaining pending activity.
  const { activity, middleware, clock, events } = setup(context), pending = deferred(), token = cancellation();
  const old = middleware.sendRequest("textDocument/hover", document, token, () => pending.promise);
  activity.reset(); clock.tick(250);
  assert.equal(events.length, 0); assert.equal(token.listeners.size, 0);
  const replacement = activity.middleware({}), next = deferred(), currentToken = cancellation();
  const current = replacement.sendRequest("custom/futureRequest", document, currentToken, () => next.promise);
  clock.tick(250); assert.equal(activity.forDocument(root).label, "Working");
  activity.dispose(); assert.equal(activity.forDocument(root), undefined); assert.equal(currentToken.listeners.size, 0);
  pending.resolve(); next.resolve(); await Promise.all([old, current]);
  const previousEvents = events.length;
  await replacement.sendRequest("textDocument/hover", document, undefined, async () => "disposed");
  clock.tick(250); assert.equal(events.length, previousEvents);
});
