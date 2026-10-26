const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const os = require("node:os");
const path = require("node:path");
const test = require("node:test");
const { execute, sha256, targets, writeJson } = require("../scripts/common.cjs");

test("release collection requires native launches for every target and installed hosts for each OS", async t => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), "dygnosis collection "));
  t.after(() => {
    const directory = path.resolve(root);
    assert.equal(path.dirname(directory), path.resolve(os.tmpdir()));
    assert.ok(path.basename(directory).startsWith("dygnosis collection "));
    return fs.rm(directory, { recursive: true, force: true });
  });
  for (const [target, info] of Object.entries(targets)) {
    const directory = path.join(root, target);
    await fs.mkdir(directory);
    const source = { target, version: "0.11.17", commit: "tested-commit", publisher: "CoconutWater", tag: "v0.11.17", release: true,
      cargo_lock_sha256: "cargo-lock", npm_lock_sha256: "npm-lock", logo_sha256: "logo", rustc: "rustc 1.98.0",
      binary_sha256: "a".repeat(64), mcp_binary_name: info.platform === "win32" ? "dygnosis-mcp.exe" : "dygnosis-mcp",
      mcp_binary_sha256: "b".repeat(64), mcp_binary_size: 4096,
      vsix: `dygnosis-0.11.17-${target}.vsix`, standalone: `dygnosis-0.11.17-${target}.tar.gz` };
    await writeJson(path.join(directory, "artifacts.json"), source);
    for (const name of [source.vsix, source.standalone]) await fs.writeFile(path.join(directory, name), `${target}/${name}\n`);
    const vsixHash = await sha256(path.join(directory, source.vsix));
    const standaloneHash = await sha256(path.join(directory, source.standalone));
    await fs.writeFile(path.join(directory, "SHA256SUMS.txt"), `${vsixHash}  ${source.vsix}\n${standaloneHash}  ${source.standalone}\n`);
    const native = { target, version: source.version, commit: source.commit, release: true, passed: true, host: { platform: info.platform, arch: info.arch },
      package: { sha256: vsixHash, binary_sha256: source.binary_sha256, mcp_binary_name: source.mcp_binary_name,
        mcp_binary_sha256: source.mcp_binary_sha256, mcp_binary_size: source.mcp_binary_size },
      standalone: { sha256: standaloneHash, launch: { lsp: { initialize: true, symbols: true }, mcp: { tool: "dynare_model_info" } } } };
    await writeJson(path.join(directory, "verification-native.json"), native);
    for (const vscode of info.arch === "x64" ? (target === "linux-x64" ? ["1.102.0", "1.141.0"] : ["1.141.0"]) : []) {
      await writeJson(path.join(directory, `verification-${vscode}.json`), { ...native, requested_vscode: vscode === "1.102.0" ? vscode : "stable",
        installed: { vscode, platform: info.platform, arch: info.arch, passed: true } });
    }
  }
  const collect = () => execute(process.execPath, [path.resolve(__dirname, "../scripts/collect-artifacts.cjs"), "--directory", root], { stdio: "pipe" });
  assert.match(collect(), /Verified 6 native package targets/);
  const matrix = JSON.parse(await fs.readFile(path.join(root, "verified-artifacts.json"), "utf8")).matrix;
  assert.ok(matrix.every(result => result.native));
  assert.deepEqual(matrix.find(result => result.target === "linux-arm64").vscode, []);

  // These failures guard the release gate when ARM skips VS Code, when evidence
  // names the wrong build/host, and when an immutable artifact is changed.
  for (const relative of ["linux-arm64/verification-native.json", "darwin-x64/verification-1.141.0.json", "linux-x64/verification-1.102.0.json"]) {
    const file = path.join(root, relative);
    const content = await fs.readFile(file);
    await fs.rm(file);
    assert.throws(collect);
    await fs.writeFile(file, content);
  }
  const nativeFile = path.join(root, "win32-arm64/verification-native.json");
  const native = JSON.parse(await fs.readFile(nativeFile, "utf8"));
  for (const change of [{ passed: false }, { commit: "other-commit" }, { host: { platform: "win32", arch: "x64" } }]) {
    await writeJson(nativeFile, { ...native, ...change });
    assert.throws(collect);
  }
  await writeJson(nativeFile, native);
  const artifactFile = path.join(root, "win32-arm64/artifacts.json");
  const artifact = JSON.parse(await fs.readFile(artifactFile, "utf8"));
  for (const change of [{ mcp_binary_sha256: undefined }, { mcp_binary_size: 0 }]) {
    await writeJson(artifactFile, { ...artifact, ...change });
    assert.throws(collect);
  }
  await writeJson(artifactFile, artifact);
  await fs.appendFile(path.join(root, "linux-arm64/dygnosis-0.11.17-linux-arm64.vsix"), "changed");
  assert.throws(collect);
});
