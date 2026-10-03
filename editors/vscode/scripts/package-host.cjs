const { spawn } = require("node:child_process");
const fs = require("node:fs/promises");
const path = require("node:path");
const { performance } = require("node:perf_hooks");
const { clearTimeout } = require("node:timers");
const { setTimeout: delay } = require("node:timers/promises");
const { writeJson } = require("./common.cjs");

const cleanupTimeoutMs = 10000;
async function stopGroup(pid) {
  try { process.kill(-pid, "SIGKILL"); } catch (error) { if (error.code !== "ESRCH") throw error; }
  const deadline = Date.now() + cleanupTimeoutMs;
  while (true) {
    try { process.kill(-pid, 0); } catch (error) { if (error.code === "ESRCH") return; throw error; }
    if (Date.now() >= deadline) throw new Error("Owned host process group did not stop within 10 seconds");
    await delay(20);
  }
}

async function waitForWindowsLauncher(exit, lifecycle, record, startupTimeoutMs, timeoutMs) {
  const started = performance.now();
  const startupDeadline = started + startupTimeoutMs;
  let hostDeadline, completed;
  exit.then(result => { completed = result; }, error => { completed = { error }; });
  while (true) {
    try {
      const state = JSON.parse((await fs.readFile(lifecycle, "utf8")).replace(/^\uFEFF/, ""));
      record.launcher_stage = state.stage;
      if (state.host_ready === true && hostDeadline === undefined) {
        record.startup_ms = performance.now() - started;
        hostDeadline = performance.now() + timeoutMs + cleanupTimeoutMs + 5000;
      }
    } catch (error) {
      if (error.code !== "ENOENT" && !(error instanceof SyntaxError)) record.launcher_read_error = String(error);
    }
    if (completed) {
      if (completed.error) throw completed.error;
      record.startup_ms ??= performance.now() - started;
      return completed;
    }
    if (performance.now() >= (hostDeadline ?? startupDeadline)) {
      record.startup_ms ??= performance.now() - started;
      return { timed_out: true, startup_timed_out: hostDeadline === undefined };
    }
    await delay(20);
  }
}

async function runHost({ executable, args, env, root, profile, identity, timeoutMs = 120000, startupTimeoutMs = 30000,
  launcherScript = path.join(__dirname, "launch-host.ps1") }) {
  const windows = process.platform === "win32";
  const evidenceRoot = path.join(root, "host-evidence");
  await fs.mkdir(evidenceRoot, { recursive: true });
  const stdout = path.join(evidenceRoot, "stdout.log"), stderr = path.join(evidenceRoot, "stderr.log");
  const lifecycle = path.join(evidenceRoot, "launcher-result.json");
  const record = { ...identity, executable, args, started: new Date().toISOString(), timeout_ms: timeoutMs, passed: false };
  if (windows) record.startup_timeout_ms = startupTimeoutMs;
  await writeJson(path.join(evidenceRoot, "run.json"), record);
  const handles = [];
  let child, failure;
  try {
    const launchFile = path.join(root, "host-launch.json");
    if (windows) {
      await writeJson(launchFile, { executable, args, stdout, stderr, lifecycle, timeoutMs });
      await writeJson(lifecycle, { stage: "launcher_starting", host_ready: false, cleanup_verified: false });
      await Promise.all([fs.writeFile(stdout, ""), fs.writeFile(stderr, "")]);
    }
    for (const file of windows ? ["launcher-stdout.log", "launcher-stderr.log"] : ["stdout.log", "stderr.log"]) {
      handles.push(await fs.open(path.join(evidenceRoot, file), "w"));
    }
    child = spawn(windows ? "powershell.exe" : executable,
      windows ? ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", launcherScript, "-LaunchFile", launchFile, "-Lifecycle", lifecycle] : args,
      { stdio: ["ignore", handles[0].fd, handles[1].fd], detached: !windows, windowsHide: true, shell: false, env });
    record.root_pid = child.pid;
    const exit = new Promise((resolve, reject) => {
      child.once("error", reject);
      child.once("close", (code, signal) => resolve({ code, signal }));
    });
    // The Windows launcher owns a job before the host can create descendants.
    // Startup has its own bound; host runtime begins only after job ownership.
    // The host's outer deadline also covers failed termination.
    let timer;
    const deadline = new Promise(resolve => {
      if (!windows) timer = setTimeout(() => resolve({ timed_out: true }), timeoutMs);
    });
    let result;
    try {
      result = windows ? await waitForWindowsLauncher(exit, lifecycle, record, startupTimeoutMs, timeoutMs) : await Promise.race([exit, deadline]);
    } finally { clearTimeout(timer); }
    Object.assign(record, result);
    if (result.timed_out) {
      failure = new Error(result.startup_timed_out
        ? `Windows host launcher did not become ready within its startup timeout (last stage: ${record.launcher_stage})`
        : "Installed VSIX host did not finish within its timeout");
      if (windows) child.kill("SIGKILL"); // Closing the launcher's owned job also stops its descendants.
      else {
        record.cleanup_verified = false;
        try { await stopGroup(child.pid); record.cleanup_verified = true; }
        catch (error) { record.cleanup_error = String(error); }
      }
      let stopTimer;
      try {
        Object.assign(record, await Promise.race([exit, new Promise(resolve => { stopTimer = setTimeout(() => resolve({ cleanup_error: "Host did not close after termination" }), cleanupTimeoutMs); })]));
      } finally { clearTimeout(stopTimer); }
    } else if (result.code !== 0) failure = new Error(`VS Code host exited ${result.code ?? result.signal}`);
    if (!windows && record.cleanup_verified === undefined) {
      record.cleanup_verified = false;
      try { await stopGroup(child.pid); record.cleanup_verified = true; }
      catch (error) { record.cleanup_error = String(error); failure ??= error; }
    }
    if (windows) {
      try {
        record.launcher = JSON.parse((await fs.readFile(lifecycle, "utf8")).replace(/^\uFEFF/, ""));
        if (!record.launcher.host_ready && record.launcher.error) {
          failure = new Error(`Windows host launcher failed during ${record.launcher.stage}: ${record.launcher.error}`);
        }
        if (!record.launcher.host_ready) failure ??= new Error(`Windows host launcher exited before host readiness (last stage: ${record.launcher.stage})`);
        if (record.launcher.timed_out) {
          record.timed_out = true;
          failure = new Error("Installed VSIX host did not finish within its timeout");
        }
        record.cleanup_verified = record.launcher.cleanup_verified === true;
        if (!record.cleanup_verified) {
          record.cleanup_error = record.launcher.cleanup_error ?? "Could not verify that the owned host job stopped";
          failure ??= new Error(record.cleanup_error);
        }
      } catch (error) {
        record.launcher_error = String(error);
        record.cleanup_verified = false;
        record.cleanup_error = "Could not read the owned host job cleanup proof";
        failure ??= error;
      }
    }
    if (failure) throw failure;
    record.passed = true;
  } catch (error) {
    failure = error;
    record.error = String(error);
  } finally {
    await Promise.all(handles.map(handle => handle.close()));
    for (const [source, destination] of [[path.join(profile, "logs"), "editor-logs"], [path.join(root, "installed-host.json"), "installed-host.json"]]) {
      try { await fs.cp(source, path.join(evidenceRoot, destination), { recursive: true }); }
      catch (error) { if (error.code !== "ENOENT") { record.evidence_error = String(error); failure ??= error; } }
    }
    record.finished = new Date().toISOString();
    if (failure) record.passed = false;
    await writeJson(path.join(evidenceRoot, "run.json"), record);
  }
  if (failure) throw failure;
  return evidenceRoot;
}

module.exports = { runHost };
