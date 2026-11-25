const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const { parseArgs } = require("node:util");
const vsce = require("@vscode/vsce");
const { collectLicenses } = require("./licenses.cjs");
const { bundle } = require("./bundle.cjs");
const { sourceFileHashes } = require("./source-hash.cjs");
const { assertNative, execute, extensionRoot, productRoot, binaryName, mcpBinaryName, hostFacts, sha256, writeJson } = require("./common.cjs");
const { buildEngine } = require("../../../scripts/build-engine.cjs");
const { createStandalone } = require("../../../scripts/standalone-package.cjs");
const { checkVersion } = require("../../../scripts/version.cjs");

async function main() {
  const { values } = parseArgs({ options: { target: { type: "string" }, tag: { type: "string" }, candidate: { type: "boolean" }, binary: { type: "string" }, "mcp-binary": { type: "string" } } });
  const target = values.target ?? (values.candidate ? `${process.platform}-${process.arch}` : undefined);
  const info = assertNative(target);
  const manifest = JSON.parse(await fs.readFile(path.join(extensionRoot, "package.json"), "utf8"));
  const version = await checkVersion();
  assert.equal(manifest.name, "dygnosis"); assert.equal(manifest.displayName, "Dygnosis");
  assert.equal(manifest.icon, "media/logo_s.png");
  assert.equal(manifest.license, "GPL-3.0-or-later");
  assert.deepEqual(manifest.extensionKind, ["workspace"]);
  const commit = execute("git", ["rev-parse", "HEAD"], { cwd: productRoot }).trim();
  const dirty = !!execute("git", ["status", "--porcelain", "--untracked-files=normal"], { cwd: productRoot }).trim();
  if (!values.candidate) {
    assert.equal(dirty, false, "Release packaging requires a clean tagged product checkout");
    assert.equal(values.tag, `v${version}`, "A matching product version tag is required");
    assert.equal(execute("git", ["rev-parse", `${values.tag}^{commit}`], { cwd: productRoot }).trim(), commit);
    assert.ok((await fs.readFile(path.join(productRoot, "CHANGELOG.md"), "utf8")).split(/\r?\n/).includes(`## ${values.tag}`), "Missing release notes for the tag");
    assert.ok(!values.binary && !values["mcp-binary"], "Release builds must build their own engines from the tagged checkout");
  }
  assert.equal(Boolean(values.binary), Boolean(values["mcp-binary"]), "Candidate artifacts require both --binary and --mcp-binary from the same build.");
  let binaries;
  const rustFlags = [process.env.RUSTFLAGS, info.platform === "win32" ? "-C target-feature=+crt-static" : ""].filter(Boolean).join(" ");
  if (values.binary) {
    assert.ok(values.candidate);
    binaries = { lsp: path.resolve(values.binary), mcp: path.resolve(values["mcp-binary"]) };
    assert.notEqual(binaries.lsp, binaries.mcp, "Candidate LSP and MCP executables must be separate files.");
  }
  else {
    binaries = buildEngine(productRoot, { release: true, target: info.rust, env: { ...process.env, RUSTFLAGS: rustFlags } });
  }
  assert.equal(execute(binaries.lsp, ["--version"]).trim(), `dygnosis ${version}`);
  assert.equal(execute(binaries.mcp, ["--version"]).trim(), `dygnosis-mcp ${version}`);
  // Package Help references from the exact engine selected for this artifact.
  await require("./help.cjs").generate(binaries.mcp);
  await require("./help.cjs").build();
  assert.equal(await sha256(path.join(extensionRoot, manifest.icon)), await sha256(path.join(productRoot, "media/logo_s.png")), "Extension logo differs from the product source");
  const logo = await fs.readFile(path.join(productRoot, "media/logo_s.png"));
  assert.equal(logo.subarray(0, 8).toString("hex"), "89504e470d0a1a0a");
  assert.ok(logo.readUInt32BE(16) >= 128 && logo.readUInt32BE(20) >= 128, "Marketplace icon must be at least 128px");
  await fs.mkdir(path.join(extensionRoot, "bin"), { recursive: true });
  for (const [kind, name] of [["lsp", binaryName(target)], ["mcp", mcpBinaryName(target)]]) {
    const destination = path.join(extensionRoot, "bin", name);
    await stageEngine(binaries[kind], destination, values.candidate && info.platform === "win32");
    if (info.platform !== "win32") await fs.chmod(destination, 0o755);
  }
  for (const file of ["LICENSE", "CHANGELOG.md"]) await fs.copyFile(path.join(productRoot, file), path.join(extensionRoot, file));
  // CHANGELOG originates at the product root. vsce joins relative links onto editors/vscode.
  // A help/ link must be absolute, or the listing resolves it under editors/vscode/help/.
  // Remaining docs/ links stay on the v0.11.6 tree.
  const changelog = path.join(extensionRoot, "CHANGELOG.md");
  await fs.writeFile(changelog, (await fs.readFile(changelog, "utf8"))
    .replace(/\]\((docs\/[^)]+)\)/g, '](https://github.com/naivej/dygnosis/blob/v0.11.6/$1)')
    .replace(/\]\((help\/[^)]+)\)/g, `](https://github.com/naivej/dygnosis/blob/${commit}/$1)`));
  const client = await bundle();
  const notices = await collectLicenses(info.rust, client.dependencies.map(dependency => dependency.packagePath));
  const cargoLock = await sourceFileHashes(productRoot, commit, "Cargo.lock");
  const npmLock = await sourceFileHashes(productRoot, commit, "editors/vscode/package-lock.json");
  const provenance = {
    schema_version: 1, name: manifest.name, publisher: manifest.publisher, version, target, rust_target: info.rust,
    commit, tag: values.tag ?? null, release: !values.candidate, dirty,
    source: `https://github.com/naivej/dygnosis/tree/${commit}`,
    source_archive: `https://github.com/naivej/dygnosis/archive/${commit}.tar.gz`,
    binary_sha256: await sha256(binaries.lsp), mcp_binary_name: mcpBinaryName(target),
    mcp_binary_sha256: await sha256(binaries.mcp),
    mcp_binary_size: (await fs.stat(binaries.mcp)).size, logo_sha256: await sha256(path.join(productRoot, "media/logo_s.png")),
    license: { spdx: "GPL-3.0-or-later", source_file: "LICENSE", source_sha256: await sha256(path.join(productRoot, "LICENSE")), vsix_path: "extension/LICENSE.txt", standalone_path: "LICENSE" },
    rustc: execute("rustc", ["--version"]).trim(), cargo_lock_sha256: cargoLock.committed, cargo_lock_checkout_sha256: cargoLock.checkout,
    build: values.binary ? "provided candidate binary pair" : "native cargo --locked --release", rustflags: values.binary ? null : rustFlags,
    npm_lock_sha256: npmLock.committed, npm_lock_checkout_sha256: npmLock.checkout, host: hostFacts(),
    client, runtime_dependencies: [],
  };
  await writeJson(path.join(extensionRoot, "SOURCE.json"), provenance);
  const output = path.join(extensionRoot, "dist", target);
  await fs.mkdir(output, { recursive: true });
  const vsix = path.join(output, `dygnosis-${version}-${target}.vsix`);
  await vsce.createVSIX({ cwd: extensionRoot, target, packagePath: vsix, dependencies: false, useYarn: false, githubBranch: commit,
    baseContentUrl: `https://github.com/naivej/dygnosis/blob/${commit}/editors/vscode`,
    baseImagesUrl: `https://github.com/naivej/dygnosis/raw/${commit}/editors/vscode` });
  const { inspectVsix } = require("./vsix.cjs");
  await inspectVsix(vsix, provenance);
  const archive = await createStandalone({ output, binaries, provenance, licenseSource: extensionRoot, notices });
  const files = [vsix, archive];
  await fs.writeFile(path.join(output, "SHA256SUMS.txt"), (await Promise.all(files.map(async file => `${await sha256(file)}  ${path.basename(file)}`))).join("\n") + "\n");
  await writeJson(path.join(output, "artifacts.json"), { ...provenance, vsix: path.basename(vsix), standalone: path.basename(archive) });
  process.stdout.write(`${JSON.stringify({ output, vsix, archive, release: provenance.release })}\n`);
}

async function stageEngine(source, destination, reuseLocked) {
  try {
    await fs.copyFile(source, destination);
  } catch (error) {
    if (!reuseLocked || !["EBUSY", "EPERM", "EACCES"].includes(error.code)) throw error;
    const existing = await sha256(destination).catch(() => undefined);
    if (existing && existing === await sha256(source)) return;
    throw new Error(`Cannot replace staged engine ${destination}. Close the editor or MCP client using it and rebuild the candidate.`, { cause: error });
  }
}

if (require.main === module) main().catch(error => { process.stderr.write(`${error.stack}\n`); process.exitCode = 1; });
module.exports = { stageEngine };
