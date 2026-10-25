const { compiledModule } = require("./helpers/extension_modules.cjs");
const assert = require("node:assert/strict");
const test = require("node:test");
const Module = require("node:module");

class Disposable { constructor(action) { this.action = action; } dispose() { this.action?.(); } }
class Range { constructor(line, character, endLine, endCharacter) { this.start = { line, character }; this.end = { line: endLine, character: endCharacter }; } }
let host;
const vscode = {
  Disposable, Range,
  commands: { registerCommand(id, action) { host.commands.set(id, action); return new Disposable(() => host.commands.delete(id)); } },
  window: {
    get activeTextEditor() { return host.editor; },
    async showQuickPick(items) { host.picks.push(items); return host.pick ? host.pick(items) : items[0]; },
    async showInformationMessage(message) { host.messages.push(message); },
  },
  workspace: { asRelativePath() { return "main.mod"; } },
};
const originalLoad = Module._load;
Module._load = function (id, ...args) { if (id === "vscode") return vscode; return originalLoad.call(this, id, ...args); };
const { registerSourceCandidates } = require(compiledModule("source_candidates"));
Module._load = originalLoad;

function setup() {
  const uri = { scheme: "file", toString: () => "file:///model/main.mod" };
  const document = { uri, version: 1, isClosed: false, lineCount: 1, lineAt: () => ({ text: "var dormant;" }),
    getText(range) { return range ? "var dormant;".slice(range.start.character, range.end.character) : "var dormant;"; } };
  const proof = { schema_version: 1, document_uri: uri.toString(), document_version: 1, source_revision: "source-a",
    message: "Choose a written declaration. Each candidate is unverified.", candidates: [{ source_id: "written:1", name: "dormant", label: "source candidate · unverified", availability: "inactive",
      location: { uri: uri.toString(), range: { start: { line: 0, character: 4 }, end: { line: 0, character: 11 } } } }] };
  host = { commands: new Map(), editor: { document, selection: { active: { line: 0, character: 5 } } }, picks: [], shown: [], messages: [], calls: 0 };
  const service = { currentInstance: 1, log() {}, async ensureStarted() {},
    async execute() { host.calls++; return globalThis.structuredClone(host.fresh?.() ?? proof); },
    async openLocation(location, root, guard) {
      const decision = await guard(document);
      if (!decision || typeof decision !== "boolean" && !decision.isCurrent()) throw new Error("The written source changed; request the source jump again.");
      host.shown.push({ location, root });
    } };
  const registration = registerSourceCandidates(service);
  return { document, proof, service, registration, invoke: () => host.commands.get("dygnosis.goToSourceCandidate")() };
}

test("one unverified candidate still needs an explicit picker and a fresh source proof", async () => {
  // Prevent a single written match from silently becoming accepted Definition navigation.
  const state = setup();
  await state.invoke();
  assert.equal(host.picks.length, 1);
  assert.match(host.picks[0][0].description, /declaration · inactive/);
  assert.equal(host.calls, 2);
  assert.equal(host.shown.length, 1);
  assert.equal(host.shown[0].root, undefined);
  state.registration.dispose();
});

test("an edit while the candidate picker is open withholds the source jump", async () => {
  // A delayed picker cannot reveal a candidate from an earlier editor version.
  const state = setup();
  host.pick = items => { state.document.version++; return items[0]; };
  await state.invoke();
  assert.equal(host.shown.length, 0);
  assert.match(host.messages[0], /source or server changed/);
  state.registration.dispose();
});

test("a changed source revision during origin revalidation withholds the jump", async () => {
  // Matching names and positions cannot substitute for the original written-source identity.
  const state = setup();
  host.fresh = () => host.calls > 1 ? { ...state.proof, source_revision: "source-b" } : state.proof;
  await state.invoke();
  assert.equal(host.calls, 2);
  assert.equal(host.shown.length, 0);
  assert.match(host.messages[0], /written source changed/);
  state.registration.dispose();
});

test("dismissing the candidate picker cancels without a jump or refusal message", async () => {
  // User cancellation must not choose the sole unverified row automatically.
  const state = setup(); host.pick = () => undefined;
  await state.invoke();
  assert.equal(host.shown.length, 0);
  assert.deepEqual(host.messages, []);
  state.registration.dispose();
});
