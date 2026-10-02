const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const { parseArgs } = require("node:util");
const { targets, sha256, writeJson } = require("./common.cjs");

async function main() {
  const { values } = parseArgs({ options: { directory: { type: "string" }, release: { type: "boolean" }, gates: { type: "string" } } });
  assert.ok(values.directory, "Pass the directory containing all six target artifact folders");
  const root = path.resolve(values.directory);
  const matrix = [];
  let identity;
  for (const target of Object.keys(targets)) {
    const directory = path.join(root, target);
    const source = JSON.parse(await fs.readFile(path.join(directory, "artifacts.json"), "utf8"));
    assert.equal(source.target, target);
    assert.equal(source.vsix, `dygnosis-${source.version}-${target}.vsix`);
    assert.equal(source.standalone, `dygnosis-${source.version}-${target}.tar.gz`);
    const current = { commit: source.commit, version: source.version, publisher: source.publisher, tag: source.tag, release: source.release, cargo_lock_sha256: source.cargo_lock_sha256, npm_lock_sha256: source.npm_lock_sha256, logo_sha256: source.logo_sha256, rustc: source.rustc };
    if (!identity) identity = current; else assert.deepEqual(current, identity, "All artifacts must share the same product commit, version, publisher and tag");
    if (values.release) { assert.equal(source.release, true); assert.equal(source.dirty, false); assert.equal(source.tag, `v${source.version}`); }
    const checksums = await fs.readFile(path.join(directory, "SHA256SUMS.txt"), "utf8");
    const vsixHash = await sha256(path.join(directory, source.vsix));
    const standaloneHash = await sha256(path.join(directory, source.standalone));
    assert.ok(checksums.includes(`${vsixHash}  ${source.vsix}\n`) && checksums.includes(`${standaloneHash}  ${source.standalone}\n`));
    const verificationFiles = (await fs.readdir(directory)).filter(file => /^verification-[0-9.]+\.json$/.test(file));
    const verified = await Promise.all(verificationFiles.map(async file => JSON.parse(await fs.readFile(path.join(directory, file), "utf8"))));
    assert.ok(verified.some(result => result.installed.vscode === "1.102.0"), `Missing minimum-version installed check: ${target}`);
    assert.ok(verified.some(result => result.installed.vscode !== "1.102.0"), `Missing current-version installed check: ${target}`);
    for (const result of verified) {
      assert.equal(result.passed, true); assert.equal(result.installed.passed, true);
      assert.equal(result.target, target); assert.equal(result.commit, source.commit); assert.equal(result.version, source.version);
      assert.equal(result.package.sha256, vsixHash); assert.equal(result.standalone.sha256, standaloneHash);
      assert.equal(result.host.platform, targets[target].platform); assert.equal(result.host.arch, targets[target].arch);
      assert.equal(result.standalone.launch.mcp.tool, "dynare_model_info");
    }
    matrix.push({ target, vsix: source.vsix, vsix_sha256: vsixHash, standalone: source.standalone, standalone_sha256: standaloneHash, vscode: verified.map(result => result.installed.vscode) });
  }
  let gates;
  if (values.release) {
    assert.ok(values.gates, "Publication requires the release's reviewed manual-gates JSON");
    gates = JSON.parse(await fs.readFile(path.resolve(values.gates), "utf8"));
    assert.equal(gates.commit, identity.commit); assert.equal(gates.version, identity.version);
    for (const gate of ["publisher", "native_vscode_mcp", "extension_upgrade", "windows_linux_remote", "runtime_requirements", "client_review"]) {
      assert.equal(gates[gate]?.passed, true, `Missing release gate: ${gate}`);
      assert.ok(typeof gates[gate].evidence === "string" && gates[gate].evidence.length > 0, `Missing evidence: ${gate}`);
    }
  }
  await writeJson(path.join(root, "verified-artifacts.json"), { schema_version: 1, ...identity, publication_ready: !!values.release, manual_gates: gates ?? null, matrix });
  await fs.writeFile(path.join(root, "SHA256SUMS.txt"), matrix.flatMap(item => [`${item.vsix_sha256}  ${item.vsix}`, `${item.standalone_sha256}  ${item.standalone}`]).join("\n") + "\n");
  process.stdout.write(`Verified ${matrix.length} native package targets. Publication ready: ${!!values.release}\n`);
}
main().catch(error => { process.stderr.write(`${error.stack}\n`); process.exitCode = 1; });
