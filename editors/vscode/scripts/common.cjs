const assert = require("node:assert/strict");
const { Buffer } = require("node:buffer");
const { execFileSync, spawn } = require("node:child_process");
const crypto = require("node:crypto");
const fs = require("node:fs/promises");
const os = require("node:os");
const path = require("node:path");
const { pathToFileURL } = require("node:url");
const { clearTimeout } = require("node:timers");

const extensionRoot = path.resolve(__dirname, "..");
const productRoot = path.resolve(extensionRoot, "../..");
const targets = {
  "win32-x64": { rust: "x86_64-pc-windows-msvc", platform: "win32", arch: "x64", vscode: "win32-x64-archive" },
  "win32-arm64": { rust: "aarch64-pc-windows-msvc", platform: "win32", arch: "arm64", vscode: "win32-arm64-archive" },
  "darwin-x64": { rust: "x86_64-apple-darwin", platform: "darwin", arch: "x64", vscode: "darwin" },
  "darwin-arm64": { rust: "aarch64-apple-darwin", platform: "darwin", arch: "arm64", vscode: "darwin-arm64" },
  "linux-x64": { rust: "x86_64-unknown-linux-gnu", platform: "linux", arch: "x64", vscode: "linux-x64" },
  "linux-arm64": { rust: "aarch64-unknown-linux-gnu", platform: "linux", arch: "arm64", vscode: "linux-arm64" },
};
function targetInfo(target) {
  assert.ok(Object.hasOwn(targets, target), `Unsupported target: ${target}`);
  return targets[target];
}
function assertNative(target) {
  const info = targetInfo(target);
  assert.equal(process.platform, info.platform, "Launch checks require the target OS");
  assert.equal(process.arch, info.arch, "Launch checks require a native target Node process");
  return info;
}
function execute(file, args, options = {}) {
  if (file === "tar" && process.platform === "win32") {
    assert.ok(process.env.SystemRoot, "Windows archive checks require SystemRoot");
    file = path.join(process.env.SystemRoot, "System32", "tar.exe");
  }
  return execFileSync(file, args, { encoding: "utf8", windowsHide: true, timeout: 120000, maxBuffer: 32 * 1024 * 1024, ...options });
}
async function sha256(file) { return crypto.createHash("sha256").update(await fs.readFile(file)).digest("hex"); }
async function writeJson(file, value) { await fs.writeFile(file, JSON.stringify(value, null, 2) + "\n"); }
function binaryName(target) { return targetInfo(target).platform === "win32" ? "dygnosis.exe" : "dygnosis"; }
function hostFacts() { return { platform: process.platform, arch: process.arch, os: os.release(), node: process.version }; }

// Both protocols have bounded buffers and waits. A failed/early exit cannot count
// as a successful launch. The test owns and terminates every child it starts.
function rpc(executable, args, framed) {
  const child = spawn(executable, args, { stdio: "pipe", windowsHide: true, shell: false });
  const pending = new Map();
  let buffer = Buffer.alloc(0), nextId = 1, failure;
  const fail = error => {
    failure = error;
    for (const waiter of pending.values()) { clearTimeout(waiter.timer); waiter.reject(error); }
    pending.clear();
  };
  child.on("error", fail);
  child.on("exit", (code, signal) => fail(new Error(`Engine exited (${code ?? signal}) before the reply`)));
  child.stdin.on("error", fail);
  child.stderr.resume();
  child.stdout.on("data", chunk => {
    try {
      buffer = Buffer.concat([buffer, chunk]);
      assert.ok(buffer.length <= 8 * 1024 * 1024, "Oversized protocol response");
      while (buffer.length) {
        let body, end;
        if (framed) {
          const headerEnd = buffer.indexOf("\r\n\r\n");
          if (headerEnd < 0) return;
          const length = /Content-Length:\s*(\d+)/i.exec(buffer.subarray(0, headerEnd).toString());
          assert.ok(length, "LSP response lacks Content-Length");
          end = headerEnd + 4 + Number(length[1]);
          if (buffer.length < end) return;
          body = buffer.subarray(headerEnd + 4, end);
        } else {
          end = buffer.indexOf("\n") + 1;
          if (!end) return;
          body = buffer.subarray(0, end);
        }
        buffer = buffer.subarray(end);
        const message = JSON.parse(body.toString());
        const waiter = pending.get(message.id);
        if (!waiter) continue;
        clearTimeout(waiter.timer); pending.delete(message.id);
        if (message.error) waiter.reject(new Error(`${waiter.method}: ${JSON.stringify(message.error)}`));
        else waiter.resolve(message.result);
      }
    } catch (error) { fail(error); }
  });
  const send = message => {
    if (failure) throw failure;
    const body = Buffer.from(JSON.stringify(message));
    child.stdin.write(framed ? Buffer.concat([Buffer.from(`Content-Length: ${body.length}\r\n\r\n`), body]) : Buffer.concat([body, Buffer.from("\n")]));
  };
  return {
    notify(method, params) { send({ jsonrpc: "2.0", method, params }); },
    request(method, params) {
      return new Promise((resolve, reject) => {
        if (failure) { reject(failure); return; }
        const id = nextId++;
        const timer = setTimeout(() => { pending.delete(id); reject(new Error(`Timed out: ${method}`)); }, 15000);
        pending.set(id, { resolve, reject, timer, method });
        try { send({ jsonrpc: "2.0", id, method, params }); } catch (error) { fail(error); }
      });
    },
    close() { fail(new Error("Probe finished")); child.kill(); },
  };
}
const model = "var y; parameters p; p=1/2; model; y=p; end;\n";
const mcpProbeCases = {
  dynare_auto_fix: { file_content: model },
  dynare_compare_models: { file_content_a: model, file_content_b: model.replace("1/2", "1/3") },
  dynare_diagnose: { files: { "/main.mod": "var y; model;\n@#include \"body.inc\"\nend;", "/body.inc": "y=0;" }, active_file: "/main.mod" },
  dynare_equations: { file_content: model },
  dynare_expand: { file_content: model },
  dynare_explain: { code: "E001" },
  dynare_extract: { file_content: model, names: ["y"] },
  dynare_find_references: { file_content: model, symbol: "y" },
  dynare_format: { file_content: model, formatIndent: 4 },
  dynare_list_diagnostic_codes: {},
  dynare_list_options: { command: "stoch_simul" },
  dynare_model_info: { file_content: model },
  dynare_related_files: { files: { "/main.mod": "var y; model;\n@#include \"body.inc\"\nend;", "/body.inc": "y=0;" }, active_file: "/main.mod" },
  dynare_rename: { file_content: model, old_name: "y", new_name: "output" },
  dynare_workspace_diagnose: { files: { "/main.mod": model }, roots: ["/main.mod"] },
};
async function probeMcp(executable, version) {
  const session = rpc(executable, ["mcp"], false);
  try {
    const initialized = await session.request("initialize", { protocolVersion: "2024-11-05", capabilities: {}, clientInfo: { name: "dygnosis-package-check", version: "1" } });
    assert.equal(initialized.serverInfo.name, "dygnosis");
    assert.equal(initialized.serverInfo.version, version);
    assert.ok(initialized.capabilities.tools);
    session.notify("notifications/initialized", {});
    const list = await session.request("tools/list", {});
    assert.deepEqual(list.tools.map(tool => tool.name).sort(), Object.keys(mcpProbeCases).sort());
    const results = {};
    for (const [name, args] of Object.entries(mcpProbeCases)) {
      const response = await session.request("tools/call", { name, arguments: args });
      assert.ok(!response.isError, `${name} returned an error`);
      results[name] = response.content.filter(item => item.type === "text").map(item => item.text).join("\n");
      assert.ok(results[name].length, `${name} returned no text`);
    }
    const info = JSON.parse(results.dynare_model_info);
    assert.equal(info.n_endogenous, 1); assert.equal(info.n_equations, 1);
    const reference = JSON.parse(await fs.readFile(path.join(productRoot, "help/reference.json"), "utf8"));
    assert.deepEqual(list.tools, reference.tools, "Packaged MCP metadata differs from Help");
    assert.match(results.dynare_explain, /E001/);
    assert.match(results.dynare_rename, /output/);
    return { initialize: true, discovery: true, tool: "dynare_model_info", tools: Object.keys(results), metadata_matches_help: true, n_equations: info.n_equations };
  } finally { session.close(); }
}
async function probeBinary(executable, version, workspace) {
  assert.equal(execute(executable, ["--version"]).trim(), `dygnosis ${version}`);
  const file = path.join(workspace, "launch model.mod");
  await fs.writeFile(file, model);
  const session = rpc(executable, [], true);
  try {
    const uri = pathToFileURL(file).href;
    const initialized = await session.request("initialize", { processId: process.pid, rootUri: pathToFileURL(workspace).href, capabilities: {}, workspaceFolders: null });
    assert.ok(initialized.capabilities.documentSymbolProvider);
    session.notify("initialized", {});
    session.notify("textDocument/didOpen", { textDocument: { uri, languageId: "dynare", version: 1, text: model } });
    const symbols = await session.request("textDocument/documentSymbol", { textDocument: { uri } });
    assert.ok(symbols.length > 0, "LSP returned no model symbols");
    await session.request("shutdown");
    session.notify("exit");
  } finally { session.close(); }
  return { version, lsp: { initialize: true, symbols: true }, mcp: await probeMcp(executable, version) };
}
module.exports = { extensionRoot, productRoot, targets, targetInfo, assertNative, execute, sha256, writeJson, binaryName, hostFacts, probeBinary, probeMcp, model, mcpProbeCases, rpc };
