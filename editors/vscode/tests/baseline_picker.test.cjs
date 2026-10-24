// Exercise native picker events while Git results arrive or outlive the picker.
const assert = require("node:assert/strict");
const test = require("node:test");
const Module = require("node:module");
const path = require("node:path");

class Event {
  listeners = new Set();
  subscribe = listener => { this.listeners.add(listener); return { dispose: () => this.listeners.delete(listener) }; };
  fire(value) { for (const listener of this.listeners) listener(value); }
}
function deferred() { let resolve, reject; const promise = new Promise((done, fail) => { resolve = done; reject = fail; }); return { promise, resolve, reject }; }
let picker;
const vscode = { window: { createQuickPick() {
  const value = new Event(), accept = new Event(), hide = new Event();
  let items = [], disposed = false;
  picker = {
    value: "", selectedItems: [], activeItems: [], valueEvent: value, acceptEvent: accept, hideEvent: hide,
    get items() { return items; }, set items(next) {
      assert.equal(disposed, false, "a closed picker cannot receive a late result"); items = next;
      if (!next.includes(this.activeItems[0])) this.activeItems = next.slice(0, 1);
    },
    onDidChangeValue: value.subscribe, onDidAccept: accept.subscribe, onDidHide: hide.subscribe,
    show() {}, dispose() { disposed = true; },
    type(text) { this.value = text; value.fire(text); }, choose(item) { this.selectedItems = [item]; accept.fire(); },
  };
  return picker;
} } };
const originalLoad = Module._load;
Module._load = function (id, ...args) { return id === "vscode" ? vscode : originalLoad.call(this, id, ...args); };
const { pickBaseline } = require(path.join(__dirname, "../out/comparison/baseline_picker.js"));
Module._load = originalLoad;
function cancellation() { const changed = new Event(); return { isCancellationRequested: false, onCancellationRequested: changed.subscribe,
  cancel() { this.isCancellationRequested = true; changed.fire(); } }; }

test("an available ref can be accepted while other Git reads remain pending", async () => {
  // Guards allowing a ready Git ref while history is still loading.
  const pending = deferred(), token = cancellation(); let publish;
  const result = pickBaseline("Compare main.mod with", [{ label: "config…", mode: "config" }], (_more, update) => { publish = update; return pending.promise; }, token);
  assert.equal(picker.busy, true);
  const ref = { label: "main", mode: "ref", revision: { hash: "a".repeat(40) } };
  publish([ref]);
  picker.choose(picker.items.find(item => item.mode === "ref"));
  assert.equal(await result, ref);
  publish([{ label: "late history" }]);
  pending.resolve([{ label: "late refs" }]);
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(picker.valueEvent.listeners.size, 0);
  assert.equal(token.isCancellationRequested, false);
});

test("typing filters retained choices without starting more Git reads or a second input", async () => {
  // Guards a repository read on each typed prefix and accepting a stale typed ref.
  const pending = deferred(); let reads = 0;
  const result = pickBaseline("main.mod", [], () => { ++reads; return pending.promise; }, cancellation());
  for (const text of ["H", "HEAD", "HEAD~", "HEAD~2"]) picker.type(text);
  assert.equal(reads, 1);
  const row = picker.items.find(item => item.mode === "expression");
  assert.equal(row.label, "Use revision: HEAD~2");
  picker.choose(row);
  assert.equal((await result).expression, "HEAD~2");
  pending.reject(new Error("cancelled Git read"));
  await new Promise(resolve => setImmediate(resolve));
});

test("arriving history retains the active ref rather than selecting the new first row", async () => {
  // Guards Enter comparing a different revision after an asynchronous item update.
  const pending = deferred(); let publish;
  const result = pickBaseline("main.mod", [], (_more, update) => { publish = update; return pending.promise; }, cancellation());
  const ref = { label: "policy", mode: "ref", revision: { hash: "b".repeat(40), requested_ref: "policy" } };
  publish([ref]); picker.activeItems = [ref];
  const refreshedRef = { ...ref };
  publish([{ label: "earlier ref", mode: "ref", revision: { hash: "a".repeat(40) } }, refreshedRef]);
  assert.equal(picker.activeItems[0], refreshedRef);
  picker.choose(picker.activeItems[0]);
  assert.equal((await result).revision.hash, ref.revision.hash);
  pending.resolve([]); await new Promise(resolve => setImmediate(resolve));
});

test("cancellation closes the picker and ignores late Git completion", async () => {
  // Guards an obsolete picker publishing after the user starts another comparison.
  const pending = deferred(), token = cancellation();
  const result = pickBaseline("main.mod", [], () => pending.promise, token);
  token.cancel();
  assert.equal(await result, undefined);
  pending.resolve([{ label: "late revision", mode: "revision" }]);
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(picker.acceptEvent.listeners.size, 0);
});

test("a failed older page keeps loaded choices and permits a successful retry", async () => {
  // Guards losing valid revisions, retry state, or retaining an obsolete failure after loading more.
  const revision = { label: "aaaaaaa", mode: "revision", revision: { hash: "a".repeat(40) } };
  const more = { label: "Load more commits…", action: "more" };
  const older = { label: "Older commit", mode: "revision", revision: { hash: "b".repeat(40) } };
  let attempts = 0;
  const result = pickBaseline("main.mod", [], async loadingMore => {
    if (!loadingMore) return [revision, more];
    if (++attempts === 1) throw new Error("older page unavailable");
    return [revision, older];
  }, cancellation());
  await new Promise(resolve => setImmediate(resolve));
  picker.choose(more); await new Promise(resolve => setImmediate(resolve));
  assert.ok(picker.items.includes(revision));
  assert.ok(picker.items.includes(more), "the failed page can be retried");
  assert.equal(picker.items.find(item => item.label === "More commits unavailable").description, "older page unavailable");
  picker.choose(more); await new Promise(resolve => setImmediate(resolve));
  assert.equal(attempts, 2);
  assert.equal(picker.items.some(item => item.label === "More commits unavailable"), false);
  picker.choose(older);
  assert.equal(await result, older);
});
