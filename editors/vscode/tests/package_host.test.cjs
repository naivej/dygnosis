const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const os = require("node:os");
const path = require("node:path");
const test = require("node:test");
const { setTimeout: delay } = require("node:timers/promises");
const { runHost } = require("../scripts/package-host.cjs");

async function fixture(t, mode) {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), "dygnosis host logs "));
  t.after(async () => {
    const owned = path.resolve(root);
    assert.equal(path.dirname(owned), path.resolve(os.tmpdir()));
    assert.ok(path.basename(owned).startsWith("dygnosis host logs "));
    if (!t.passed) {
      for (const file of ["run.json", "launcher-stderr.log", "launcher-stdout.log"]) {
        try { process.stderr.write(`${file}: ${await fs.readFile(path.join(root, "run with spaces", "host-evidence", file), "utf8")}\n`); } catch { /* Unix has no launcher log. */ }
      }
    }
    await fs.rm(owned, { recursive: true, force: true });
  });
  // Use a separate profile directory, as the short macOS profile does.
  const profile = path.join(root, "outside run", "profile with spaces");
  const runRoot = path.join(root, "run with spaces");
  await fs.mkdir(runRoot);
  const host = path.join(root, "fake host.cjs");
  await fs.writeFile(host, `
const fs = require("node:fs");
const path = require("node:path");
const { spawn } = require("node:child_process");
const [mode, profile, root] = process.argv.slice(2);
const logRoot = path.join(profile, "logs", "session", "window1", "exthost");
fs.mkdirSync(logRoot, { recursive: true });
fs.writeFileSync(path.join(profile, "logs", "session", "main.log"), "editor log\\n");
fs.writeFileSync(path.join(logRoot, "exthost.log"), "extension host log\\n");
process.stdout.write("complete stdout ".repeat(8192) + "stdout end\\n");
process.stderr.write("complete stderr ".repeat(8192) + "stderr end\\n");
if (mode === "timeout" || mode === "fail-worker") {
  const worker = spawn(process.execPath, ["-e", "const fs=require('node:fs'); const path=require('node:path'); setInterval(()=>{if(!fs.existsSync(path.dirname(process.argv[1]))) process.exit(); fs.appendFileSync(process.argv[1],'.');},20); setTimeout(()=>process.exit(),10000)", path.join(root, "heartbeat")], { stdio: "ignore" });
  fs.writeFileSync(path.join(root, "worker-pid"), String(worker.pid));
  worker.unref();
  if (mode === "timeout") setInterval(() => {}, 1000);
  else {
    const ready = setInterval(() => {
      const heartbeat = path.join(root, "heartbeat");
      if (!fs.existsSync(heartbeat) || fs.readFileSync(heartbeat, "utf8").length < 3) return;
      clearInterval(ready);
      fs.writeFileSync(path.join(root, "installed-host.json"), JSON.stringify({ passed: false }));
      process.exitCode = 7;
    }, 20);
  }
} else {
  fs.writeFileSync(path.join(root, "installed-host.json"), JSON.stringify({ passed: mode === "pass" }));
  process.exitCode = mode === "pass" ? 0 : 7;
}
`);
  return { root: runRoot, profile, executable: process.execPath, args: [host, mode, profile, runRoot], env: process.env, identity: { runId: mode, target: "test", requested_vscode: "test" }, timeoutMs: mode === "timeout" ? 3000 : 10000 };
}

async function assertLogs(root) {
  const evidence = path.join(root, "host-evidence");
  assert.equal(await fs.readFile(path.join(evidence, "stdout.log"), "utf8"), "complete stdout ".repeat(8192) + "stdout end\n");
  assert.equal(await fs.readFile(path.join(evidence, "stderr.log"), "utf8"), "complete stderr ".repeat(8192) + "stderr end\n");
  assert.equal(await fs.readFile(path.join(evidence, "editor-logs", "session", "main.log"), "utf8"), "editor log\n");
  assert.equal(await fs.readFile(path.join(evidence, "editor-logs", "session", "window1", "exthost", "exthost.log"), "utf8"), "extension host log\n");
  return JSON.parse(await fs.readFile(path.join(evidence, "run.json"), "utf8"));
}

async function assertWorkerStopped(root, record) {
  assert.equal(record.cleanup_verified, true);
  assert.equal(record.cleanup_error, undefined, "Successful cleanup must not report a second stop against an exited PID");
  if (process.platform !== "win32") assert.throws(() => process.kill(-record.root_pid, 0), { code: "ESRCH" }, "The owned Unix group must be absent before logs are copied");
  await delay(100);
  const heartbeat = path.join(root, "heartbeat");
  const stopped = await fs.readFile(heartbeat, "utf8");
  assert.ok(stopped.length > 0, "The descendant must run before root exit or timeout");
  await delay(200);
  assert.equal(await fs.readFile(heartbeat, "utf8"), stopped, "The owned descendant must stop before logs are collected");
}

test("installed host preserves full output and separate profile logs on success", async t => {
  const options = await fixture(t, "pass");
  await runHost(options);
  const record = await assertLogs(options.root);
  assert.equal(record.passed, true);
  assert.equal(record.runId, "pass");
  assert.equal(JSON.parse(await fs.readFile(path.join(options.root, "host-evidence", "installed-host.json"), "utf8")).passed, true);
});

test("installed host keeps failed output and result without passing verification", async t => {
  const options = await fixture(t, "fail");
  await assert.rejects(runHost(options), /VS Code host exited/);
  const record = await assertLogs(options.root);
  assert.equal(record.passed, false);
  assert.equal(JSON.parse(await fs.readFile(path.join(options.root, "host-evidence", "installed-host.json"), "utf8")).passed, false);
});

test("installed host timeout stops owned descendants and retains logs without host JSON", async t => {
  const options = await fixture(t, "timeout");
  await assert.rejects(runHost(options), /did not finish within its timeout/);
  const record = await assertLogs(options.root);
  assert.equal(record.passed, false);
  assert.equal(record.timed_out, true);
  await assert.rejects(fs.stat(path.join(options.root, "host-evidence", "installed-host.json")), { code: "ENOENT" });
  await assertWorkerStopped(options.root, record);
});

test("installed host nonzero root exit stops its already running owned descendant", async t => {
  const options = await fixture(t, "fail-worker");
  await assert.rejects(runHost(options), /VS Code host exited/);
  const record = await assertLogs(options.root);
  assert.equal(record.passed, false);
  if (process.platform === "win32") assert.equal(record.launcher.exit_code, 7);
  else assert.equal(record.code, 7);
  await assertWorkerStopped(options.root, record);
});
