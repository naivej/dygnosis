const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const { parseArgs } = require("node:util");
const vsce = require("@vscode/vsce");
const { collectLicenses } = require("./licenses.cjs");
const { sourceFileHashes } = require("./source-hash.cjs");
const { assertNative, execute, extensionRoot, productRoot, binaryName, hostFacts, sha256, writeJson } = require("./common.cjs");
const { checkVersion } = require("../../../scripts/version.cjs");

async function main() {
  const { values } = parseArgs({ options: { target: { type: "string" }, tag: { type: "string" }, candidate: { type: "boolean" }, binary: { type: "string" } } });
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
    assert.ok(!values.binary, "Release builds must build their own engine from the tagged checkout");
  }
  let binary;
  const rustFlags = [process.env.RUSTFLAGS, info.platform === "win32" ? "-C target-feature=+crt-static" : ""].filter(Boolean).join(" ");
  if (values.binary) { assert.ok(values.candidate); binary = path.resolve(values.binary); }
  else {
    execute("cargo", ["build", "--locked", "--release", "--target", info.rust], { cwd: productRoot, stdio: "inherit", timeout: 20 * 60 * 1000, env: { ...process.env, RUSTFLAGS: rustFlags } });
    binary = path.join(process.env.CARGO_TARGET_DIR ? path.resolve(productRoot, process.env.CARGO_TARGET_DIR) : path.join(productRoot, "target"), info.rust, "release", binaryName(target));
  }
  assert.equal(execute(binary, ["--version"]).trim(), `dygnosis ${version}`);
  // Package Help references from the exact engine selected for this artifact.
  await require("./help.cjs").generate(binary);
  await require("./help.cjs").build();
  assert.equal(await sha256(path.join(extensionRoot, manifest.icon)), await sha256(path.join(productRoot, "media/logo_s.png")), "Extension logo differs from the product source");
  const logo = await fs.readFile(path.join(productRoot, "media/logo_s.png"));
  assert.equal(logo.subarray(0, 8).toString("hex"), "89504e470d0a1a0a");
  assert.ok(logo.readUInt32BE(16) >= 128 && logo.readUInt32BE(20) >= 128, "Marketplace icon must be at least 128px");
  await fs.mkdir(path.join(extensionRoot, "bin"), { recursive: true });
  await fs.copyFile(binary, path.join(extensionRoot, "bin", binaryName(target)));
  if (info.platform !== "win32") await fs.chmod(path.join(extensionRoot, "bin", binaryName(target)), 0o755);
  for (const file of ["LICENSE", "CHANGELOG.md"]) await fs.copyFile(path.join(productRoot, file), path.join(extensionRoot, file));
  // CHANGELOG originates at the product root. vsce joins relative links onto editors/vscode.
  // A help/ link must be absolute, or the listing resolves it under editors/vscode/help/.
  // Remaining docs/ links stay on the v0.11.6 tree.
  const changelog = path.join(extensionRoot, "CHANGELOG.md");
  await fs.writeFile(changelog, (await fs.readFile(changelog, "utf8"))
    .replace(/\]\((docs\/[^)]+)\)/g, '](https://github.com/naivej/dygnosis/blob/v0.11.6/$1)')
    .replace(/\]\((help\/[^)]+)\)/g, `](https://github.com/naivej/dygnosis/blob/${commit}/$1)`));
  const runtimeFiles = await vsce.listFiles({ cwd: extensionRoot, packageManager: vsce.PackageManager.Npm });
  const notices = await collectLicenses(info.rust, runtimeFiles);
  const cargoLock = await sourceFileHashes(productRoot, commit, "Cargo.lock");
  const npmLock = await sourceFileHashes(productRoot, commit, "editors/vscode/package-lock.json");
  const provenance = {
    schema_version: 1, name: manifest.name, publisher: manifest.publisher, version, target, rust_target: info.rust,
    commit, tag: values.tag ?? null, release: !values.candidate, dirty,
    source: `https://github.com/naivej/dygnosis/tree/${commit}`,
    source_archive: `https://github.com/naivej/dygnosis/archive/${commit}.tar.gz`,
    binary_sha256: await sha256(binary), logo_sha256: await sha256(path.join(productRoot, "media/logo_s.png")),
    license: { spdx: "GPL-3.0-or-later", source_file: "LICENSE", source_sha256: await sha256(path.join(productRoot, "LICENSE")), vsix_path: "extension/LICENSE.txt", standalone_path: "LICENSE" },
    rustc: execute("rustc", ["--version"]).trim(), cargo_lock_sha256: cargoLock.committed, cargo_lock_checkout_sha256: cargoLock.checkout,
    build: values.binary ? "provided candidate binary" : "native cargo --locked --release", rustflags: values.binary ? null : rustFlags,
    npm_lock_sha256: npmLock.committed, npm_lock_checkout_sha256: npmLock.checkout, host: hostFacts(),
    runtime_dependencies: notices.filter(notice => notice.kind === "npm").map(notice => notice.packagePath),
  };
  await writeJson(path.join(extensionRoot, "SOURCE.json"), provenance);
  const output = path.join(extensionRoot, "dist", target);
  await fs.mkdir(output, { recursive: true });
  const vsix = path.join(output, `dygnosis-${version}-${target}.vsix`);
  await vsce.createVSIX({ cwd: extensionRoot, target, packagePath: vsix, dependencies: true, useYarn: false, githubBranch: commit,
    baseContentUrl: `https://github.com/naivej/dygnosis/blob/${commit}/editors/vscode`,
    baseImagesUrl: `https://github.com/naivej/dygnosis/raw/${commit}/editors/vscode` });
  const { inspectVsix } = require("./vsix.cjs");
  await inspectVsix(vsix, provenance);
  const standalone = path.join(output, `dygnosis-${version}-${target}`);
  await fs.mkdir(standalone, { recursive: true });
  for (const file of ["LICENSE", "SOURCE.json", "THIRD-PARTY-NOTICES.json"]) await fs.copyFile(path.join(extensionRoot, file), path.join(standalone, file));
  await fs.cp(path.join(extensionRoot, "licenses"), path.join(standalone, "licenses"), { recursive: true });
  await fs.copyFile(binary, path.join(standalone, binaryName(target)));
  if (info.platform !== "win32") await fs.chmod(path.join(standalone, binaryName(target)), 0o755);
  await fs.writeFile(path.join(standalone, "README.txt"), `Dygnosis ${version} (${target})\nSource: ${provenance.source}\nLicense: GPL-3.0-or-later; see LICENSE and THIRD-PARTY-NOTICES.json.\n\nExtract the archive to a persistent directory. No Rust, Node.js, VS Code, MATLAB or Dynare install is needed.\nConfigure your MCP client's executable field with the absolute path to ${binaryName(target)} and its argument list with ["mcp"]. Each tool describes its inputs.\nUse --version to identify the executable. Reconnect your MCP client after replacing it.\nAn LSP client starts the executable without arguments.\nInstallation and runtime requirements: https://github.com/naivej/dygnosis/blob/${commit}/help/get-started.md\n`);
  const archive = `${standalone}.tar.gz`;
  execute("tar", ["-czf", archive, "-C", output, path.basename(standalone)]);
  const files = [vsix, archive];
  await fs.writeFile(path.join(output, "SHA256SUMS.txt"), (await Promise.all(files.map(async file => `${await sha256(file)}  ${path.basename(file)}`))).join("\n") + "\n");
  await writeJson(path.join(output, "artifacts.json"), { ...provenance, vsix: path.basename(vsix), standalone: path.basename(archive) });
  process.stdout.write(`${JSON.stringify({ output, vsix, archive, release: provenance.release })}\n`);
}
main().catch(error => { process.stderr.write(`${error.stack}\n`); process.exitCode = 1; });
