const assert = require("node:assert/strict");
const { spawn } = require("node:child_process");
const crypto = require("node:crypto");
const fs = require("node:fs/promises");
const path = require("node:path");
const { parseArgs } = require("node:util");
const { clearTimeout } = require("node:timers");
const { downloadAndUnzipVSCode } = require("@vscode/test-electron");
const { inspectVsix } = require("./vsix.cjs");
const { resolveCli } = require("./vscode-cli.cjs");
const { assertNative, binaryName, execute, extensionRoot, hostFacts, probeBinary, sha256, writeJson } = require("./common.cjs");

async function runHost(executable, args, env, root) {
  const windows = process.platform === "win32";
  const launchFile = path.join(root, "host-launch.json");
  if (windows) await writeJson(launchFile, { executable, args });
  const command = windows ? "powershell.exe" : executable;
  const commandArgs = windows ? ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", path.join(__dirname, "launch-host.ps1"), "-LaunchFile", launchFile] : args;
  return new Promise((resolve, reject) => {
    const child = spawn(command, commandArgs, { stdio: "inherit", windowsHide: true, shell: false, env });
    const timer = setTimeout(() => {
      try {
        if (windows) execute("taskkill.exe", ["/PID", String(child.pid), "/T", "/F"]);
        else child.kill("SIGKILL");
      } catch (error) { reject(new Error("Could not stop timed-out package host", { cause: error })); return; }
      reject(new Error("Installed VSIX host did not finish within its timeout"));
    }, windows ? 130000 : 120000);
    child.on("error", error => { clearTimeout(timer); reject(error); });
    child.on("exit", code => { clearTimeout(timer); if (code === 0) resolve(); else reject(new Error(`VS Code host exited ${code}`)); });
  });
}
async function main() {
  const { values } = parseArgs({ options: { target: { type: "string" }, vscode: { type: "string", default: "stable" }, executable: { type: "string" } } });
  const info = assertNative(values.target);
  const output = path.join(extensionRoot, "dist", values.target);
  const source = JSON.parse(await fs.readFile(path.join(output, "artifacts.json"), "utf8"));
  const { vsix: vsixName, standalone, ...provenance } = source;
  assert.equal(source.target, values.target);
  const vsix = path.join(output, vsixName), archive = path.join(output, standalone);
  const packageInspection = await inspectVsix(vsix, provenance);
  const checksums = await fs.readFile(path.join(output, "SHA256SUMS.txt"), "utf8");
  for (const file of [vsix, archive]) assert.ok(checksums.split("\n").includes(`${await sha256(file)}  ${path.basename(file)}`));
  const runId = crypto.randomUUID();
  const root = path.join(extensionRoot, ".test-data", `installed package with spaces ${runId}`);
  const unpacked = path.join(root, "standalone with spaces");
  const workspace = path.join(root, "workspace with spaces");
  // Recent VS Code uses a Unix socket under the profile. A deep checkout plus
  // our evidence UUID exceeds macOS's 103-byte limit; keep only that profile
  // short. Packages, executables, extensions and workspace still test spaces.
  const profile = process.platform === "darwin"
    ? await fs.mkdtemp(path.join("/tmp", "dyg profile "))
    : path.join(root, "profile with spaces");
  const extensions = path.join(root, "extensions with spaces");
  const harness = path.join(root, "extension harness");
  for (const directory of [unpacked, workspace, profile, extensions, harness]) await fs.mkdir(directory, { recursive: true });
  const archiveRoot = path.basename(archive, ".tar.gz");
  const archiveEntries = execute("tar", ["-tzf", archive]).trim().split(/\r?\n/);
  assert.ok(archiveEntries.every(entry => entry.startsWith(archiveRoot + "/") && !entry.split("/").includes("..") && !entry.includes("\\")), "Unsafe standalone archive entry");
  execute("tar", ["-xzf", archive, "-C", unpacked]);
  const unpackedRoot = path.join(unpacked, archiveRoot);
  assert.deepEqual(JSON.parse(await fs.readFile(path.join(unpackedRoot, "SOURCE.json"), "utf8")), provenance);
  assert.equal(source.license.standalone_path, "LICENSE");
  assert.equal(await sha256(path.join(unpackedRoot, source.license.standalone_path)), source.license.source_sha256);
  const binary = path.join(unpackedRoot, binaryName(source.target));
  assert.equal(await sha256(binary), source.binary_sha256);
  if (process.platform !== "win32") assert.ok((await fs.stat(binary)).mode & 0o111, "Standalone archive lost the executable permission");
  const launch = await probeBinary(binary, source.version, workspace);
  const executable = values.executable ? path.resolve(values.executable) : await downloadAndUnzipVSCode({ version: values.vscode, platform: info.vscode, cachePath: path.join(extensionRoot, ".vscode-test") });
  const cli = await resolveCli(executable, values.vscode === "stable" ? undefined : values.vscode);
  const cliArgs = [cli, "--user-data-dir", profile, "--extensions-dir", extensions];
  execute(executable, [...cliArgs, "--install-extension", vsix, "--force"], { env: { ...process.env, ELECTRON_RUN_AS_NODE: "1" } });
  const installed = execute(executable, [...cliArgs, "--list-extensions", "--show-versions"], { env: { ...process.env, ELECTRON_RUN_AS_NODE: "1" } });
  assert.ok(installed.split(/\r?\n/).includes(`${source.publisher}.dygnosis@${source.version}`), "VSIX installation did not report the matching version");
  await writeJson(path.join(harness, "package.json"), { name: "dygnosis-package-harness", publisher: "test", version: "1.0.0", engines: { vscode: "^1.102.0" } });
  await fs.mkdir(path.join(profile, "User"), { recursive: true });
  await writeJson(path.join(profile, "User", "settings.json"), { "extensions.autoUpdate": false, "extensions.autoCheckUpdates": false, "telemetry.telemetryLevel": "off" });
  // A separate harness loads the test suite; Dygnosis itself is installed by the
  // VS Code CLI, never supplied as an extensionDevelopmentPath.
  const hostResult = path.join(root, "installed-host.json");
  const env = { ...process.env, DYGNOSIS_PACKAGE_SOURCE: JSON.stringify(provenance), DYGNOSIS_PACKAGE_RUN_ID: runId, DYGNOSIS_PACKAGE_EXTENSIONS: extensions, DYGNOSIS_PACKAGE_HOST_RESULT: hostResult };
  delete env.ELECTRON_RUN_AS_NODE;
  await runHost(executable, [workspace, `--extensionDevelopmentPath=${harness}`, `--extensionTestsPath=${path.join(__dirname, "installed-host.cjs")}`, "--user-data-dir", profile, "--extensions-dir", extensions, "--skip-welcome", "--skip-release-notes", "--disable-workspace-trust", "--disable-gpu", "--no-sandbox"], env, root);
  const host = JSON.parse(await fs.readFile(hostResult, "utf8"));
  assert.equal(host.runId, runId); assert.equal(host.passed, true);
  assert.equal(host.platform, info.platform); assert.equal(host.arch, info.arch);
  if (values.vscode !== "stable") assert.equal(host.vscode, values.vscode, "VS Code version differs from the requested minimum/current host");
  const evidence = { schema_version: 1, runId, target: source.target, version: source.version, commit: source.commit, release: source.release, host: hostFacts(), package: packageInspection, standalone: { sha256: await sha256(archive), launch }, installed: host, passed: true };
  await writeJson(path.join(output, `verification-${host.vscode}.json`), evidence);
  process.stdout.write(`${JSON.stringify({ target: source.target, vscode: host.vscode, runId, passed: true })}\n`);
}
main().catch(error => { process.stderr.write(`${error.stack}\n`); process.exitCode = 1; });
