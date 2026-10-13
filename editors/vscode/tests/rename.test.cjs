// Verify native Rename guards before the language-client converter drops versions.
const { compiledModule } = require("./helpers/extension_modules.cjs");
const assert = require("node:assert/strict");
const test = require("node:test");
const Module = require("node:module");
const { URL } = require("node:url");

class Emitter {
  listeners = new Set();
  event = callback => { this.listeners.add(callback); return { dispose: () => this.listeners.delete(callback) }; };
  fire(event) { for (const callback of this.listeners) callback(event); }
}
const events = Object.fromEntries(["edit", "close", "folder", "configuration", "invalidate"].map(name => [name, new Emitter()]));
let host;
const vscode = { Position: class { constructor(line, character) { this.line = line; this.character = character; } }, Uri: { parse: value => { const uri = new URL(value); return { toString: () => value, scheme: uri.protocol.slice(0, -1), fsPath: decodeURIComponent(uri.pathname), authority: uri.host, query: uri.search.slice(1), fragment: uri.hash.slice(1) }; } }, workspace: {
  get textDocuments() { return [...host.documents.values()]; },
  onDidChangeTextDocument: events.edit.event, onDidCloseTextDocument: events.close.event,
  onDidChangeWorkspaceFolders: events.folder.event, onDidChangeConfiguration: events.configuration.event,
  openTextDocument: async uri => {
    const target = host.document(uri.toString(), host.disk.get(uri.toString()));
    await host.load?.(target);
    return target;
  },
} };
const TextDocumentEdit = { is: value => !!value && !!value.textDocument && Array.isArray(value.edits) };
const original = Module._load;
Module._load = function(name, ...args) { return name === "vscode" ? vscode : name === "vscode-languageclient/node" ? { TextDocumentEdit } : original.call(this, name, ...args); };
const { withCheckedRenameEdit } = require(compiledModule("rename_guard"));
Module._load = original;
const root = "file:///project/root.mod", child = "file:///project/equations.txt";
const range = { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } };
const token = { isCancellationRequested: false };
function deferred() { let resolve; const promise = new Promise(done => { resolve = done; }); return { promise, resolve }; }
function setup() {
  host = { documents: new Map(), disk: new Map([[child, "p"]]), requests: 0, converted: [],
    document(uri, text) { const document = { uri: vscode.Uri.parse(uri), version: 1, text, getText() { return this.text; } }; this.documents.set(uri, document); return document; } };
  const document = host.document(root, "p");
  const envelope = () => ({ schema_version: 1, owners: [{ root_uri: root, revision: "current" }],
    edit: { documentChanges: [root, child].map(uri => ({ textDocument: { uri, version: host.documents.get(uri)?.version ?? null }, edits: [{ range, newText: "rho" }] })) },
    sources: [root, child].map(uri => ({ uri, text: "p", version: host.documents.get(uri)?.version ?? null })) });
  const service = { currentInstance: 1, onDidInvalidate: events.invalidate.event,
    execute: async (command, args) => { assert.equal(command, "dynare/captureRename"); assert.equal(args[0].newName, "rho"); ++host.requests; return await host.capture?.(host.requests) ?? envelope(); },
    client: { initializeResult: { capabilities: { executeCommandProvider: { commands: ["dynare/captureRename"] } } },
      protocol2CodeConverter: { asWorkspaceEdit: async edit => { host.converted.push(edit); await host.convert?.(); return { converted: edit, guards: [], insert(uri, position, text) { this.guards.push({ uri: uri.toString(), position, text }); } }; } } },
  };
  return { service, document, envelope, run: consume => withCheckedRenameEdit(service, document, { line: 0, character: 0 }, "rho", token, consume ?? (edit => edit)) };
}

test("saved targets load before conversion; raw versions and a second complete decision survive", async () => {
  const { run } = setup();
  const edit = await run();
  assert.ok(edit); assert.equal(host.requests, 2); assert.equal(host.converted.length, 1);
  assert.ok(host.documents.has(child)); assert.equal(edit.converted.documentChanges[1].textDocument.version, 1);
});

// Guards the native Rename path replacing a server refusal with a generic result.
test("capture refusal keeps the server's cause and never reaches edit conversion", async () => {
  const { run } = setup();
  host.capture = () => ({ error: "The new name is already declared in an affected model.", code: "RENAME_REFUSED" });
  await assert.rejects(run(), /already declared/);
  assert.equal(host.converted.length, 0);
});

test("a raw stale version or saved source mismatch withholds the entire edit", async () => {
  for (const wrong of ["version", "saved", "unsyncedOpen"]) {
    const { run, envelope } = setup();
    if (wrong === "version") host.capture = () => { const offered = envelope(); offered.sources[0].version = 0; offered.edit.documentChanges[0].textDocument.version = 0; return offered; };
    if (wrong === "saved") host.disk.set(child, "changed p");
    if (wrong === "unsyncedOpen") { host.document(child, "changed p"); host.capture = () => { const offered = envelope(); offered.sources[1].version = null; offered.edit.documentChanges[1].textDocument.version = null; return offered; }; }
    await assert.rejects(run(), /Rename|engine/, wrong); assert.equal(host.converted.length, 0);
  }
});

test("edits, dependency file invalidation, settings and replacement during loading cannot return old edits", async () => {
  for (const reason of ["edit", "file", "settings", "instance", "owner"]) {
    const { run, service, envelope } = setup();
    host.load = document => {
      if (reason === "edit") { ++document.version; events.edit.fire({ document, contentChanges: [{}] }); }
      if (reason === "file") events.invalidate.fire({ reason: "file" });
      if (reason === "settings") events.configuration.fire({ affectsConfiguration: key => key === "dynare" });
      if (reason === "instance") ++service.currentInstance;
      if (reason === "owner") host.capture = () => { const changed = envelope(); changed.owners.push({ root_uri: "file:///project/other.mod", revision: "other" }); return changed; };
    };
    await assert.rejects(run(), /Rename|engine/, reason); assert.equal(host.converted.length, 0);
  }
});

test("the second decision cannot change targets, edit ranges or source text", async () => {
  for (const reason of ["range", "source", "target"]) {
    const { run, envelope } = setup();
    host.capture = request => {
      const value = envelope();
      if (request === 2) {
        if (reason === "range") value.edit.documentChanges[1].edits[0].range = { ...range, end: { line: 0, character: 2 } };
        if (reason === "source") value.sources[1].text = "different";
        if (reason === "target") { value.sources.pop(); value.edit.documentChanges.pop(); }
      }
      return value;
    };
    await assert.rejects(run(), /Rename|engine/, reason); assert.equal(host.converted.length, 0);
  }
});

test("document changes during asynchronous conversion cannot escape the final version guard", async () => {
  const { run } = setup();
  const pending = deferred(); host.convert = () => pending.promise;
  const result = run();
  while (!host.converted.length) await new Promise(resolve => setImmediate(resolve));
  ++host.documents.get(child).version;
  pending.resolve();
  await assert.rejects(result, /Rename/);
});

test("an unsupported engine cannot bypass the checked application path", async () => {
  const { service, run } = setup(); service.client.initializeResult.capabilities.executeCommandProvider.commands = [];
  await assert.rejects(run(), /Rename|engine/, undefined);
  assert.equal(host.requests, 0);
});

test("guard listeners remain active through consumption and are disposed afterwards", async () => {
  const { run } = setup(); const pending = deferred();
  const result = run(async () => { assert.ok(events.edit.listeners.size); return await pending.promise; });
  while (!host.converted.length) await new Promise(resolve => setImmediate(resolve));
  assert.ok(events.edit.listeners.size); pending.resolve(true); assert.equal(await result, true);
  assert.equal(events.edit.listeners.size, 0);
});

test("unedited model inputs carry no-op application guards without becoming genuine rename targets", async () => {
  const { run, envelope } = setup(); const dependency = "file:///project/settings.inc";
  host.disk.set(dependency, "@#define variant=1");
  host.capture = () => { const value = envelope(); value.sources.push({ uri: dependency, text: host.disk.get(dependency), version: null }); return value; };
  const result = await run(); assert.ok(result);
  assert.deepEqual(result.guards, [{ uri: dependency, position: new vscode.Position(0, 0), text: "" }]);
  assert.ok(!result.converted.documentChanges.some(change => change.textDocument.uri === dependency));
});

test("native Windows saved-to-open URI aliases preserve the complete recaptured edit", { skip: process.platform !== "win32" }, async () => {
  const { run, envelope } = setup();
  const saved = "file:///C:/Project/equations.inc", opened = "file:///c%3A/Project/equations.inc";
  host.disk.set(saved, "p");
  host.load = target => { if (target.uri.toString() === saved) { host.documents.delete(saved); target.uri = vscode.Uri.parse(opened); host.documents.set(opened, target); } };
  host.capture = request => {
    const value = envelope(), uri = request === 1 ? saved : opened;
    value.edit.documentChanges[1].textDocument = { uri, version: request === 1 ? null : 1 };
    value.sources[1] = { uri, text: "p", version: request === 1 ? null : 1 };
    // The same native identity can occur in the owner set too.
    value.owners[0].root_uri = request === 1 ? "file:///C:/Project/root.mod" : "file:///c%3A/Project/root.mod";
    return value;
  };
  assert.ok(await run());
  assert.equal(host.requests, 2); assert.equal(host.converted.length, 1);
  assert.equal(host.converted[0].documentChanges[1].textDocument.uri, opened);
});

test("URI aliases do not permit duplicate identities or genuinely changed targets", { skip: process.platform !== "win32" }, async () => {
  for (const reason of ["sources", "edits", "owners", "different"]) {
    const { run, envelope } = setup();
    host.capture = () => {
      const value = envelope();
      const first = "file:///C:/Project/root.mod", alias = "file:///c%3A/Project/root.mod";
      if (reason === "sources") value.sources.push({ uri: first, text: "p", version: null }, { uri: alias, text: "p", version: null });
      if (reason === "edits") value.edit.documentChanges.push({ textDocument: { uri: first, version: null }, edits: [] }, { textDocument: { uri: alias, version: null }, edits: [] });
      if (reason === "owners") value.owners.push({ root_uri: first, revision: "one" }, { root_uri: alias, revision: "two" });
      if (reason === "different") value.sources[1].uri = "file:///different/equations.inc";
      return value;
    };
    await assert.rejects(run(), /Rename|engine/, reason); assert.equal(host.converted.length, 0);
  }
});

test("recapture cannot merge distinct native query or fragment resources", { skip: process.platform !== "win32" }, async () => {
  for (const suffix of ["?snapshot=other", "#other"]) {
    const { run, envelope } = setup();
    host.capture = request => {
      const value = envelope();
      if (request === 2) {
        value.sources[1].uri += suffix;
        value.edit.documentChanges[1].textDocument.uri += suffix;
      }
      return value;
    };
    await assert.rejects(run(), /Rename/, suffix); assert.equal(host.converted.length, 0);
  }
});
