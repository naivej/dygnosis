const assert = require("node:assert/strict");
const { Buffer } = require("node:buffer");
const { spawn } = require("node:child_process");
const { createHash } = require("node:crypto");
const fs = require("node:fs/promises");
const Module = require("node:module");
const os = require("node:os");
const path = require("node:path");
const test = require("node:test");
const { pathToFileURL, fileURLToPath } = require("node:url");

class Disposable {
  constructor(callback = () => {}) { this.callback = callback; }
  dispose() { this.callback(); this.callback = () => {}; }
  static from(...values) { return new Disposable(() => values.forEach(value => value.dispose())); }
}
class Uri {
  constructor(value) { this.value = value; this.scheme = value.split(":")[0]; this.fsPath = this.scheme === "file" ? fileURLToPath(value) : value; }
  toString() { return this.value; }
  static file(value) { return new Uri(pathToFileURL(value).href); }
  static parse(value) { return new Uri(value); }
  static joinPath(base, ...parts) { return Uri.file(path.join(base.fsPath, ...parts)); }
}
let host;
const vscode = {
  Disposable, Uri,
  Range: class { constructor(start, end) { this.start = start; this.end = end; } },
  WorkspaceEdit: class { replace(uri, range, text) { this.change = { uri, range, text }; } },
  extensions: { getExtension: () => host.git },
  commands: {
    registerCommand: (id, callback) => { host.commands.set(id, callback); return new Disposable(() => host.commands.delete(id)); },
    executeCommand: (...args) => { host.executed.push(args); return Promise.resolve(); },
  },
  window: {
    showTextDocument: document => { host.opened.push(document); return Promise.resolve({ document }); },
    showQuickPick: items => { host.picks.push(items); return Promise.resolve(host.cancelPick ? undefined : items[host.pickIndex ?? 0]); },
    showInformationMessage: message => { host.messages.push(message); return Promise.resolve(undefined); },
    showWarningMessage: message => { host.messages.push(message); return Promise.resolve(host.reviewAnswer); },
    showErrorMessage: message => { host.errors.push(message); return Promise.resolve(undefined); },
  },
  workspace: {
    get isTrusted() { return host.trusted; },
    get workspaceFolders() { return host.folders; },
    get textDocuments() { return host.documents; },
    onDidCloseTextDocument: callback => { host.close = callback; return new Disposable(() => { host.close = undefined; }); },
    registerTextDocumentContentProvider: (scheme, provider) => { host.providers.set(scheme, provider); return new Disposable(() => host.providers.delete(scheme)); },
    openTextDocument: async uri => {
      let document = host.documents.find(candidate => candidate.uri.toString() === uri.toString());
      if (!document) { document = makeDocument(uri, await fs.readFile(uri.fsPath, "utf8")); host.documents.push(document); }
      return document;
    },
    applyEdit: async edit => {
      if (host.refuseEdit) return false;
      const document = host.documents.find(candidate => candidate.uri.toString() === edit.change.uri.toString());
      document.text = edit.change.text; ++document.version; document.isDirty = true;
      await host.afterEdit?.(document);
      return true;
    },
  },
};
const originalLoad = Module._load;
Module._load = function(id, ...args) {
  if (id === "vscode") return vscode;
  if (id === "./binary") return {
    resolveBinary: async () => { ++host.resolved; return host.binary; },
    validateMcpBinary: async () => { ++host.validated; },
  };
  return originalLoad.call(this, id, ...args);
};
const { compareVersions, mergeProjectConfig, chooseProjectRoot, ensureManagedBinary, verifyPackagedBundle, withProjectLock, writeProjectConfig, registerProjectMcp } = require("../out/project_mcp");
Module._load = originalLoad;
function reset() {
  host = { trusted: true, folders: [], documents: [], commands: new Map(), providers: new Map(), opened: [], picks: [], executed: [], messages: [], errors: [], resolved: 0, validated: 0, saved: 0, binary: { path: path.resolve("user-managed engine"), override: true, version: "test" } };
}
function makeDocument(uri, text, dirty = false) {
  return { uri, text, version: 7, isDirty: dirty, isClosed: false, getText() { return this.text; }, positionAt(offset) { return { offset }; },
    async save() { ++host.saved; if (host.refuseSave) return false; await fs.writeFile(this.uri.fsPath, this.text); this.isDirty = false; return true; } };
}
async function directory(t) {
  const folder = await fs.mkdtemp(path.join(os.tmpdir(), "dygnosis-project-mcp-"));
  t.after(() => fs.rm(folder, { recursive: true, force: true }));
  return folder;
}
function bundle(version, contents = `engine ${version}`, target = `${process.platform}-${process.arch}`) {
  const bytes = Buffer.from(contents), commit = "a".repeat(40);
  const rust = { "win32-x64": "x86_64-pc-windows-msvc", "win32-arm64": "aarch64-pc-windows-msvc", "darwin-x64": "x86_64-apple-darwin", "darwin-arm64": "aarch64-apple-darwin", "linux-x64": "x86_64-unknown-linux-gnu", "linux-arm64": "aarch64-unknown-linux-gnu" };
  return { bytes, provenance: { schema_version: 1, name: "dygnosis", publisher: "CoconutWater", version, target, rust_target: rust[target], commit, source: `https://github.com/naivej/dygnosis/tree/${commit}`, source_archive: `https://github.com/naivej/dygnosis/archive/${commit}.tar.gz`, binary_sha256: createHash("sha256").update(bytes).digest("hex"), release: false, tag: null, dirty: true, test_provenance: { retained: true } } };
}
const fixtureVersion = async file => `dygnosis ${(await fs.readFile(file, "utf8")).split(" ")[1]}`;
const options = { version: fixtureVersion, lockAttempts: 50, lockDelayMs: 5 };
function files(folder, platform = process.platform) {
  const bin = path.join(folder, "bin"); return { bin, binary: path.join(bin, platform === "win32" ? "dygnosis.exe" : "dygnosis"), active: path.join(bin, "managed.json"), pending: path.join(bin, "pending.json"), stage: path.join(bin, ".dygnosis.pending"), lock: path.join(bin, ".dygnosis.lock") };
}

test("strict project merge preserves unrelated text, server data and large numeric literals", () => {
  const original = '{\r\n  "other": 9007199254740993,\r\n  "mcpServers": {"other": {"type":"http", "url":"https://example.test"}},\r\n  "keep": [true,null,{"quote":"a\\"b"}]\r\n}\r\n';
  const command = path.resolve('path with spaces', 'engine"name');
  const result = mergeProjectConfig(original, command);
  assert.equal(result.conflict, false); assert.equal(result.identical, false);
  assert.ok(result.text.includes('"other": 9007199254740993'));
  assert.ok(result.text.includes('"other": {"type":"http", "url":"https://example.test"}'));
  assert.ok(result.text.includes('"keep": [true,null,{"quote":"a\\"b"}]'));
  assert.deepEqual(JSON.parse(result.text).mcpServers.dygnosis, { type: "stdio", command, args: ["mcp"] });
});
test("adds mcpServers to empty and nonempty roots and respects identical property ordering", () => {
  const command = path.resolve("engine");
  for (const source of ["{}\n", '{"unrelated":{}}', '{"mcpServers": {}}']) assert.equal(JSON.parse(mergeProjectConfig(source, command).text).mcpServers.dygnosis.command, command);
  const identical = JSON.stringify({ mcpServers: { dygnosis: { args: ["mcp"], command, type: "stdio" } } });
  assert.deepEqual(mergeProjectConfig(identical, command), { text: identical, identical: true, conflict: false });
});
test("different existing entry is a conflict and only that value changes", () => {
  const source = '{"mcpServers":{"dygnosis":{"type":"http","url":"https://old.test"}, "keep": {"a":1}}, "x": 2}';
  const result = mergeProjectConfig(source, path.resolve("engine"));
  assert.equal(result.conflict, true); assert.ok(result.text.endsWith(', "keep": {"a":1}}, "x": 2}'));
});
test("rejects non-strict JSON, invalid containers and duplicate keys without a repaired guess", () => {
  for (const source of ['{"mcpServers":[]}', '{"mcpServers":null}', '{"mcpServers":1}', '[]', 'null', '{"x":1,}', '{/*comment*/}', '{"mcpServers":{},"mcpServers":{}}', '{"other":{"a":1,"a":2}}']) assert.throws(() => mergeProjectConfig(source, path.resolve("engine")));
  assert.throws(() => mergeProjectConfig("{}", "relative/engine"), /absolute/);
});
test("semantic versions compare core, prerelease precedence and ignore build labels", () => {
  const order = ["0.11.2-alpha", "0.11.2-alpha.1", "0.11.2-alpha.2", "0.11.2-alpha.10", "0.11.2-beta", "0.11.2-rc.1", "0.11.2", "0.11.10", "0.12.0"];
  for (let i = 1; i < order.length; ++i) { assert.equal(compareVersions(order[i - 1], order[i]), -1); assert.equal(compareVersions(order[i], order[i - 1]), 1); }
  assert.equal(compareVersions("0.11.2+first", "0.11.2+second"), 0);
  for (const version of ["0.11", "01.2.0", "0.11.2-01", "0.11.2-a..b", "0.11.2+.."]) assert.throws(() => compareVersions(version, "0.11.2"));
});
test("Git root choice uses the nearest containing file repository, with visible folder fallback", () => {
  const root = Uri.file(path.resolve("repo")), nested = Uri.file(path.join(root.fsPath, "nested")), folder = Uri.file(path.join(nested.fsPath, "models"));
  assert.equal(chooseProjectRoot(folder, [{ rootUri: root }, { rootUri: nested }]).toString(), nested.toString());
  assert.equal(chooseProjectRoot(folder, [{ rootUri: Uri.file(`${root.fsPath}-different`) }, { rootUri: Uri.parse("vscode-vfs:/repo") }]).toString(), folder.toString());
});

test("managed install writes frozen bytes and complete source provenance, then is a no-op", async t => {
  const folder = await directory(t), source = bundle("0.11.2"), selected = await ensureManagedBinary(folder, source, options);
  assert.equal(selected.changed, true); assert.equal(selected.version, "0.11.2"); assert.ok(path.isAbsolute(selected.path));
  assert.deepEqual(await fs.readFile(selected.path), source.bytes);
  assert.deepEqual(JSON.parse(await fs.readFile(files(folder).active, "utf8")), source.provenance);
  const before = await fs.stat(selected.path);
  assert.deepEqual(await ensureManagedBinary(folder, source, options), { path: selected.path, version: "0.11.2", changed: false });
  assert.equal((await fs.stat(selected.path)).mtimeMs, before.mtimeMs);
});
test("two windows cannot downgrade a managed copy or install different bytes for one version", async t => {
  const folder = await directory(t);
  const results = await Promise.all([ensureManagedBinary(folder, bundle("0.11.10"), options), ensureManagedBinary(folder, bundle("0.11.2"), options)]);
  assert.equal(JSON.parse(await fs.readFile(files(folder).active, "utf8")).version, "0.11.10");
  assert.ok(results.every(result => ["0.11.2", "0.11.10"].includes(result.version)));
  await assert.rejects(ensureManagedBinary(folder, bundle("0.11.10", "different bytes"), options), /different package bytes/);
  assert.equal(await fs.readFile(files(folder).binary, "utf8"), "engine 0.11.10");
});
test("staging version/hash failures preserve the working engine and source record", async t => {
  const folder = await directory(t); await ensureManagedBinary(folder, bundle("0.11.1"), options);
  const active = await fs.readFile(files(folder).active);
  await assert.rejects(ensureManagedBinary(folder, bundle("0.11.2"), { ...options, version: async () => "dygnosis 9.0.0" }), /version verification/);
  const mutated = bundle("0.11.2"); mutated.bytes[0] = 42;
  await assert.rejects(ensureManagedBinary(folder, mutated, options), /changed before installation/);
  assert.equal(await fs.readFile(files(folder).binary, "utf8"), "engine 0.11.1"); assert.deepEqual(await fs.readFile(files(folder).active), active);
});
test("Windows sharing violations retain old engine and verified pending update then replace at the same path", async t => {
  const folder = await directory(t), windows = { ...options, platform: "win32" }, old = await ensureManagedBinary(folder, bundle("0.11.1"), windows);
  const locked = await ensureManagedBinary(folder, bundle("0.11.2"), { ...windows, replace: async () => { throw Object.assign(new Error("locked by MCP"), { code: "EPERM" }); } });
  assert.deepEqual(locked, { path: old.path, version: "0.11.1", pendingVersion: "0.11.2", changed: false });
  assert.equal(await fs.readFile(old.path, "utf8"), "engine 0.11.1");
  assert.equal(JSON.parse(await fs.readFile(files(folder, "win32").pending, "utf8")).version, "0.11.2");
  // An older extension retries the already-verified newer pending version, never its own older bundle.
  const updated = await ensureManagedBinary(folder, bundle("0.11.1"), windows);
  assert.equal(updated.path, old.path); assert.equal(updated.version, "0.11.2");
  assert.equal(await fs.readFile(old.path, "utf8"), "engine 0.11.2");
});
test("non-sharing replacement failure and interrupted metadata commit are recoverable", async t => {
  const folder = await directory(t); await ensureManagedBinary(folder, bundle("0.11.1"), options);
  await assert.rejects(ensureManagedBinary(folder, bundle("0.11.2"), { ...options, replace: async () => { throw Object.assign(new Error("disk failure"), { code: "EIO" }); } }), /disk failure/);
  assert.equal(await fs.readFile(files(folder).binary, "utf8"), "engine 0.11.1");
  // Simulate process interruption after binary rename, before the active metadata rename.
  await fs.rename(files(folder).stage, files(folder).binary);
  const result = await ensureManagedBinary(folder, bundle("0.11.1"), options);
  assert.equal(result.version, "0.11.2"); assert.equal(result.changed, false);
  assert.equal(JSON.parse(await fs.readFile(files(folder).active, "utf8")).version, "0.11.2");
  await assert.rejects(fs.stat(files(folder).pending), { code: "ENOENT" });
});
test("corrupt working copy, wrong source/target and incomplete newer pending stage never overwrite", async t => {
  const folder = await directory(t); await ensureManagedBinary(folder, bundle("0.11.1"), options);
  await fs.writeFile(files(folder).binary, "tampered");
  await assert.rejects(ensureManagedBinary(folder, bundle("0.11.2"), options), /recorded checksum/);
  assert.equal(await fs.readFile(files(folder).binary, "utf8"), "tampered");
  const bad = bundle("0.11.2"); bad.provenance.source = "https://other.test/source";
  await assert.rejects(ensureManagedBinary(folder, bad, options), /provenance/);
  await fs.writeFile(files(folder).binary, bundle("0.11.1").bytes);
  await fs.writeFile(files(folder).pending, JSON.stringify(bundle("0.11.3").provenance));
  await assert.rejects(ensureManagedBinary(folder, bundle("0.11.2"), options), /incomplete/);
  const other = bundle("0.11.3", "engine 0.11.3", process.platform === "win32" ? "linux-x64" : "win32-x64");
  await fs.writeFile(files(folder).pending, JSON.stringify(other.provenance));
  await assert.rejects(ensureManagedBinary(folder, bundle("0.11.2"), options), /another native target/);
});
test("Unix staged install retains executable permissions", { skip: process.platform === "win32" }, async t => {
  const folder = await directory(t), result = await ensureManagedBinary(folder, bundle("0.11.2"), options);
  assert.equal((await fs.stat(result.path)).mode & 0o777, 0o755);
});
test("actual bundled engine is frozen, staged and version-verified using real --version", async t => {
  const extensionPath = path.resolve(__dirname, "..");
  try { await fs.stat(path.join(extensionPath, "SOURCE.json")); } catch { t.skip("No generated native candidate; installed-package gate remains separate"); return; }
  const context = { extensionPath, extension: { packageJSON: JSON.parse(await fs.readFile(path.join(extensionPath, "package.json"), "utf8")) } };
  const frozen = await verifyPackagedBundle(context), folder = await directory(t);
  const installed = await ensureManagedBinary(folder, frozen);
  assert.deepEqual(await fs.readFile(installed.path), frozen.bytes);
  assert.equal(installed.version, frozen.provenance.version);
});
test("package verification rejects mismatched extension metadata before launching", async t => {
  const folder = await directory(t), source = bundle("0.11.2");
  await fs.writeFile(path.join(folder, "SOURCE.json"), JSON.stringify(source.provenance));
  const context = { extensionPath: folder, extension: { packageJSON: { name: "dygnosis", publisher: "different", version: "0.11.2" } } };
  await assert.rejects(verifyPackagedBundle(context), /provenance/);
  context.extension.packageJSON.publisher = "CoconutWater"; context.extension.packageJSON.version = "0.11.1";
  await assert.rejects(verifyPackagedBundle(context), /version or native target/);
});

test("a real child process owns the lock; contender waits and enters only after release", async t => {
  const folder = await directory(t), lock = path.join(folder, "cross-window.lock"), signal = path.join(folder, "release");
  const script = `const Module=require('node:module'), fs=require('node:fs/promises');const load=Module._load;Module._load=function(id,...args){return id==='vscode'?{}:load.call(this,id,...args)};const {withProjectLock}=require(process.argv[1]);withProjectLock(process.argv[2],async()=>{process.stdout.write('acquired\\n');while(true){try{await fs.stat(process.argv[3]);break}catch{}await new Promise(r=>setTimeout(r,10))}process.stdout.write('released\\n')}).catch(e=>{process.stderr.write(e.stack);process.exitCode=1});`;
  const child = spawn(process.execPath, ["-e", script, path.resolve(__dirname, "../out/project_mcp"), lock, signal], { windowsHide: true });
  t.after(() => { child.kill(); });
  const acquired = new Promise((resolve, reject) => { child.stdout.once("data", resolve); child.once("error", reject); child.once("exit", code => { if (code !== 0) reject(new Error(`Lock child exited ${code}`)); }); });
  await acquired;
  let entered = false;
  const contender = withProjectLock(lock, async () => { entered = true; }, 100, 10);
  await new Promise(resolve => setTimeout(resolve, 30)); assert.equal(entered, false);
  await fs.writeFile(signal, "release"); await contender; assert.equal(entered, true);
  await assert.rejects(fs.stat(lock), { code: "ENOENT" });
});
test("dead owner is recovered, live owner and malformed lock time out without unsafe deletion", async t => {
  const folder = await directory(t), lock = path.join(folder, "lock");
  const child = spawn(process.execPath, ["-e", ""], { windowsHide: true });
  await new Promise(resolve => child.once("exit", resolve));
  await fs.writeFile(lock, JSON.stringify({ pid: child.pid, token: "dead" }));
  await withProjectLock(lock, async () => {}, 3, 1);
  for (const owner of [JSON.stringify({ pid: process.pid, token: "live" }), "incomplete"]) {
    await fs.writeFile(lock, owner);
    await assert.rejects(withProjectLock(lock, async () => {}, 2, 1), /Another Dygnosis window/);
    assert.equal(await fs.readFile(lock, "utf8"), owner);
  }
});

test("dirty buffer review preserves unsaved keys and saves exactly the approved merge", async t => {
  reset(); const folder = await directory(t), uri = Uri.file(path.join(folder, ".mcp.json"));
  await fs.writeFile(uri.fsPath, '{"disk":true}');
  const document = makeDocument(uri, '{"unsaved":42,"mcpServers":{"other":{"command":"keep","args":[]}}}', true); host.documents.push(document);
  let reviewed;
  assert.equal(await writeProjectConfig(uri, path.resolve("engine"), async (before, after) => { assert.equal(before, document.text); reviewed = after; return true; }), "saved");
  assert.equal(await fs.readFile(uri.fsPath, "utf8"), reviewed); assert.equal(document.text, reviewed); assert.equal(document.isDirty, false);
  assert.equal(JSON.parse(reviewed).unsaved, 42); assert.equal(JSON.parse(reviewed).mcpServers.other.command, "keep"); assert.equal(host.saved, 1);
});
test("cancelled conflict/dirty review does not touch buffer or disk", async t => {
  reset(); const folder = await directory(t), uri = Uri.file(path.join(folder, ".mcp.json")), disk = '{"mcpServers":{"dygnosis":{"command":"old"}}}';
  await fs.writeFile(uri.fsPath, disk); const document = makeDocument(uri, disk, true); host.documents.push(document);
  assert.equal(await writeProjectConfig(uri, path.resolve("engine"), async () => false), "cancelled");
  assert.equal(document.text, disk); assert.equal(await fs.readFile(uri.fsPath, "utf8"), disk); assert.equal(host.saved, 0); assert.equal(document.version, 7);
});
test("identical dirty entry is a no-op and never saves unrelated unsaved text", async t => {
  reset(); const folder = await directory(t), uri = Uri.file(path.join(folder, ".mcp.json")), command = path.resolve("engine");
  await fs.writeFile(uri.fsPath, "{}"); const source = JSON.stringify({ unsaved: 42, mcpServers: { dygnosis: { type: "stdio", command, args: ["mcp"] } } });
  const document = makeDocument(uri, source, true); host.documents.push(document);
  assert.equal(await writeProjectConfig(uri, command, async () => { assert.fail("no review needed"); }), "identical");
  assert.equal(await fs.readFile(uri.fsPath, "utf8"), "{}"); assert.equal(document.isDirty, true); assert.equal(host.saved, 0);
});
test("buffer edits and disk edits during review abort before applying or saving", async t => {
  for (const kind of ["buffer", "disk", "close"]) {
    reset(); const folder = await directory(t), uri = Uri.file(path.join(folder, ".mcp.json")), source = '{"mcpServers":{"dygnosis":{}}}';
    await fs.writeFile(uri.fsPath, source); const document = makeDocument(uri, source, true); host.documents.push(document);
    await assert.rejects(writeProjectConfig(uri, path.resolve("engine"), async () => {
      if (kind === "disk") await fs.writeFile(uri.fsPath, '{"concurrent":true}');
      else if (kind === "buffer") { document.text = '{"concurrent":true}'; ++document.version; }
      else document.isClosed = true;
      return true;
    }), /changed/);
    assert.equal(host.saved, 0); assert.equal(document.version, kind === "buffer" ? 8 : 7);
    assert.equal(await fs.readFile(uri.fsPath, "utf8"), kind === "disk" ? '{"concurrent":true}' : source);
  }
});
test("edits at apply/save boundary withhold the save and leave current buffer for review", async t => {
  for (const kind of ["buffer", "disk"]) {
    reset(); const folder = await directory(t), uri = Uri.file(path.join(folder, ".mcp.json")), source = '{"mcpServers":{"dygnosis":{}}}';
    await fs.writeFile(uri.fsPath, source); const document = makeDocument(uri, source, true); host.documents.push(document);
    host.afterEdit = async value => { if (kind === "buffer") { value.text += " "; ++value.version; } else await fs.writeFile(uri.fsPath, '{"concurrent":true}'); };
    await assert.rejects(writeProjectConfig(uri, path.resolve("engine"), async () => true), /changed while applying/);
    assert.equal(host.saved, 0); assert.equal(await fs.readFile(uri.fsPath, "utf8"), kind === "disk" ? '{"concurrent":true}' : source);
  }
});
test("invalid configuration and symlink destination stay intact", async t => {
  reset(); const folder = await directory(t), uri = Uri.file(path.join(folder, ".mcp.json"));
  for (const source of ['{"mcpServers": []}', '{broken', '{"mcpServers":{},"mcpServers":{}}']) {
    host.documents = []; await fs.writeFile(uri.fsPath, source);
    await assert.rejects(writeProjectConfig(uri, path.resolve("engine"), async () => true));
    assert.equal(await fs.readFile(uri.fsPath, "utf8"), source);
  }
  await fs.unlink(uri.fsPath); const target = path.join(folder, "elsewhere.json"); await fs.writeFile(target, "{}");
  try { await fs.symlink(target, uri.fsPath); } catch (error) { if (error.code === "EPERM") { t.diagnostic("Windows symlink creation unavailable"); return; } throw error; }
  await assert.rejects(writeProjectConfig(uri, path.resolve("engine"), async () => true), /regular file/);
  assert.equal(await fs.readFile(target, "utf8"), "{}");
});
test("missing config is atomically created and opened; competing setup writes preserve first result", async t => {
  reset(); const folder = await directory(t), uri = Uri.file(path.join(folder, ".mcp.json"));
  const first = writeProjectConfig(uri, path.resolve("first"), async () => true);
  const second = writeProjectConfig(uri, path.resolve("second"), async () => true);
  const results = await Promise.allSettled([first, second]);
  assert.equal(results.filter(result => result.status === "fulfilled").length, 1);
  assert.equal(results.filter(result => result.status === "rejected").length, 1);
  assert.ok([path.resolve("first"), path.resolve("second")].includes(JSON.parse(await fs.readFile(uri.fsPath, "utf8")).mcpServers.dygnosis.command));
  assert.equal(host.opened.length, 1);
});

test("an external config creator at the atomic commit boundary is never overwritten", async t => {
  reset(); const folder = await directory(t), uri = Uri.file(path.join(folder, ".mcp.json")), concurrent = '{"concurrent":true}';
  const originalLink = fs.link;
  let intercepted = false;
  fs.link = async (staged, destination) => {
    assert.equal(destination, uri.fsPath);
    assert.ok(staged.startsWith(`${uri.fsPath}.`));
    intercepted = true;
    // The absence check has completed. A non-Dygnosis writer now creates the real destination.
    await fs.writeFile(destination, concurrent, { flag: "wx" });
    return originalLink(staged, destination);
  };
  try {
    await assert.rejects(writeProjectConfig(uri, path.resolve("engine"), async () => true), /was created during setup.*no configuration was saved/);
  } finally { fs.link = originalLink; }
  assert.equal(intercepted, true);
  assert.equal(await fs.readFile(uri.fsPath, "utf8"), concurrent);
  assert.equal(host.saved, 0); assert.equal(host.opened.length, 0);
  assert.deepEqual(await fs.readdir(folder), [".mcp.json"]);
});

test("command is available without a model and refuses untrusted/missing/non-file workspaces", async t => {
  for (const condition of ["untrusted", "missing", "virtual"]) {
    reset(); const folder = await directory(t), context = { extensionPath: folder, globalStorageUri: Uri.file(path.join(folder, "storage")), extension: { packageJSON: {} } };
    host.trusted = condition !== "untrusted";
    if (condition === "virtual") host.folders = [{ name: "virtual", uri: Uri.parse("vscode-vfs:/project") }];
    const registration = registerProjectMcp(context); assert.equal(host.commands.has("dygnosis.setupProjectMcp"), true);
    await host.commands.get("dygnosis.setupProjectMcp")(); assert.equal(host.resolved, 0); assert.equal(host.saved, 0);
    registration.dispose(); assert.equal(host.commands.size, 0); assert.equal(host.providers.size, 0);
  }
});
test("multi-root command cancellation does not resolve a binary or write a config", async t => {
  reset(); const folder = await directory(t); host.folders = [{ name: "one", uri: Uri.file(folder) }, { name: "two", uri: Uri.file(path.join(folder, "two")) }]; host.cancelPick = true;
  const registration = registerProjectMcp({ extensionPath: folder, globalStorageUri: Uri.file(path.join(folder, "storage")), extension: { packageJSON: {} } });
  await host.commands.get("dygnosis.setupProjectMcp")(); assert.equal(host.picks.length, 1); assert.equal(host.resolved, 0);
  await assert.rejects(fs.stat(path.join(folder, ".mcp.json")), { code: "ENOENT" }); registration.dispose();
});
test("public Git root is used; explicit override remains at its user path and is never copied", async t => {
  reset(); const folder = await directory(t), child = path.join(folder, "models"); await fs.mkdir(child);
  host.folders = [{ name: "models", uri: Uri.file(child) }]; host.git = { isActive: false, activate: async () => ({ getAPI: version => { assert.equal(version, 1); return { repositories: [{ rootUri: Uri.file(folder) }] }; } }) };
  const storage = path.join(folder, "storage"), registration = registerProjectMcp({ extensionPath: folder, globalStorageUri: Uri.file(storage), extension: { packageJSON: {} } });
  await host.commands.get("dygnosis.setupProjectMcp")(); assert.equal(host.validated, 1); assert.deepEqual(host.errors, []);
  const config = JSON.parse(await fs.readFile(path.join(folder, ".mcp.json"), "utf8")); assert.equal(config.mcpServers.dygnosis.command, host.binary.path);
  await assert.rejects(fs.stat(path.join(child, ".mcp.json")), { code: "ENOENT" }); await assert.rejects(fs.stat(storage), { code: "ENOENT" });
  assert.ok(host.messages.some(message => message.includes(path.join(folder, ".mcp.json")))); registration.dispose();
});
test("conflicting command shows a native read-only diff, cancellation preserves config", async t => {
  reset(); const folder = await directory(t), configPath = path.join(folder, ".mcp.json"), source = '{"mcpServers":{"dygnosis":{"command":"old"}},"keep":42}'; await fs.writeFile(configPath, source);
  host.folders = [{ name: "project", uri: Uri.file(folder) }]; host.reviewAnswer = undefined;
  const registration = registerProjectMcp({ extensionPath: folder, globalStorageUri: Uri.file(path.join(folder, "storage")), extension: { packageJSON: {} } });
  await host.commands.get("dygnosis.setupProjectMcp")(); assert.equal(await fs.readFile(configPath, "utf8"), source);
  const [id, before, after, title] = host.executed[0]; assert.equal(id, "vscode.diff"); assert.ok(title.includes(configPath));
  const provider = host.providers.get("dygnosis-mcp-review"); assert.equal(provider.provideTextDocumentContent(before), source); assert.equal(JSON.parse(provider.provideTextDocumentContent(after)).keep, 42);
  registration.dispose();
});
