// Create and inspect the native Dygnosis archive used by LSP and MCP clients.
const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const { binaryName, execute, mcpBinaryName, sha256, targetInfo, writeJson } = require("./package-common.cjs");

const noticesFile = "THIRD-PARTY-NOTICES.json";

function standaloneSource(provenance) {
  const { name, version, target, rust_target, commit, tag, release, dirty, source, source_archive,
    binary_sha256, mcp_binary_name, mcp_binary_sha256, mcp_binary_size, license, rustc,
    cargo_lock_sha256, cargo_lock_checkout_sha256, build, rustflags, host } = provenance;
  return {
    schema_version: 2, name, version, target, rust_target, commit, tag, release, dirty, source, source_archive,
    binary_sha256, mcp_binary_name, mcp_binary_sha256, mcp_binary_size,
    license: { spdx: license.spdx, source_file: license.source_file, source_sha256: license.source_sha256, standalone_path: "LICENSE" },
    rustc, cargo_lock_sha256, cargo_lock_checkout_sha256, build, rustflags, host,
  };
}

function nativeLicenseFiles(notices) {
  assert.ok(notices.some(notice => notice.kind === "cargo"), "Standalone archive lacks Cargo notices");
  assert.ok(notices.some(notice => notice.kind === "toolchain"), "Standalone archive lacks a Rust toolchain notice");
  const files = new Map();
  for (const notice of notices) {
    assert.ok(["cargo", "toolchain"].includes(notice.kind), `Unexpected standalone notice kind: ${notice.kind}`);
    assert.ok(notice.files.length > 0, `No license files for ${notice.name}`);
    for (const file of notice.files) {
      assert.match(file.sha256, /^[a-f0-9]{64}$/);
      assert.equal(file.path, `licenses/texts/${file.sha256}`);
      files.set(file.path, file.sha256);
    }
  }
  return files;
}

async function createStandalone({ output, binaries, provenance, licenseSource, notices }) {
  const { version, target, commit, source } = provenance;
  const rootName = `dygnosis-${version}-${target}`;
  assert.match(version, /^\d+\.\d+\.\d+$/);
  const { platform } = targetInfo(target);
  const lspName = binaryName(target);
  const mcpName = mcpBinaryName(target);
  const nativeNotices = notices.filter(notice => notice.kind === "cargo" || notice.kind === "toolchain");
  const licenses = nativeLicenseFiles(nativeNotices);
  const archive = path.join(output, `${rootName}.tar.gz`);
  const outputRoot = path.resolve(output);
  const staging = await fs.mkdtemp(path.join(outputRoot, "standalone-staging-"));
  assert.equal(path.dirname(staging), outputRoot);
  try {
    const payload = path.join(staging, rootName);
    await fs.mkdir(path.join(payload, "licenses", "texts"), { recursive: true });
    await fs.copyFile(path.join(licenseSource, "LICENSE"), path.join(payload, "LICENSE"));
    await writeJson(path.join(payload, "SOURCE.json"), standaloneSource(provenance));
    await writeJson(path.join(payload, noticesFile), nativeNotices);
    for (const file of licenses.keys()) {
      await fs.copyFile(path.join(licenseSource, file), path.join(payload, file));
    }
    for (const [name, binary] of [[lspName, binaries.lsp], [mcpName, binaries.mcp]]) {
      const destination = path.join(payload, name);
      await fs.copyFile(binary, destination);
      if (platform !== "win32") await fs.chmod(destination, 0o755);
    }
    const instructions = [
      `Dygnosis ${version} (${target})`,
      `Source: ${source}`,
      "License: GPL-3.0-or-later; see LICENSE and THIRD-PARTY-NOTICES.json.",
      "",
      "Extract the archive to a persistent directory. No Rust, Node.js, VS Code, MATLAB or Dynare install is needed.",
      `Configure your MCP client's executable field with the absolute path to ${mcpName}. No arguments are required. Each tool describes its inputs.`,
      "Use --version to identify each executable. Reconnect your MCP client after replacing it.",
      `An LSP client starts ${lspName} without arguments.`,
      `About and support: https://github.com/naivej/dygnosis/blob/${commit}/help/about.md`,
      "",
    ];
    await fs.writeFile(path.join(payload, "README.txt"), instructions.join("\n"));
    const stagedArchive = path.join(staging, path.basename(archive));
    execute("tar", ["-czf", stagedArchive, "-C", staging, rootName]);
    await fs.rename(stagedArchive, archive);
    return archive;
  } finally {
    await fs.rm(staging, { recursive: true, force: true });
  }
}

async function inspectStandalone(archive, unpacked, provenance) {
  const archiveRoot = path.basename(archive, ".tar.gz");
  const archiveEntries = execute("tar", ["-tzf", archive]).trim().split(/\r?\n/);
  assert.ok(archiveEntries.every(entry => entry.startsWith(archiveRoot + "/") && !entry.split("/").includes("..") && !entry.includes("\\")), "Unsafe standalone archive entry");
  await fs.mkdir(unpacked, { recursive: true });
  execute("tar", ["-xzf", archive, "-C", unpacked]);
  const root = path.join(unpacked, archiveRoot);
  const source = JSON.parse(await fs.readFile(path.join(root, "SOURCE.json"), "utf8"));
  // Candidates produced before the split carry the same provenance as their VSIX.
  assert.deepEqual(source, source.schema_version === 1 ? provenance : standaloneSource(provenance));
  assert.equal(source.license.standalone_path, "LICENSE");
  assert.equal(await sha256(path.join(root, "LICENSE")), provenance.license.source_sha256);
  if (source.schema_version === 2) {
    const notices = JSON.parse(await fs.readFile(path.join(root, noticesFile), "utf8"));
    const licenses = nativeLicenseFiles(notices);
    const prefix = `${archiveRoot}/licenses/`;
    const licenseEntries = archiveEntries.filter(entry => entry.startsWith(prefix) && !entry.endsWith("/"));
    assert.deepEqual(licenseEntries.map(entry => entry.slice(archiveRoot.length + 1)).sort(),
      [...licenses.keys()].sort(), "Standalone archive license files differ from its notices");
    for (const [file, hash] of licenses) {
      assert.equal(await sha256(path.join(root, file)), hash, `Changed license: ${file}`);
    }
  }
  const lsp = path.join(root, binaryName(provenance.target));
  const mcp = path.join(root, mcpBinaryName(provenance.target));
  assert.equal(await sha256(lsp), provenance.binary_sha256);
  assert.equal(await sha256(mcp), provenance.mcp_binary_sha256);
  assert.equal((await fs.stat(mcp)).size, provenance.mcp_binary_size);
  if (targetInfo(provenance.target).platform !== "win32") {
    for (const executable of [lsp, mcp]) {
      assert.ok((await fs.stat(executable)).mode & 0o111, "Standalone archive lost an executable permission");
    }
  }
  return { lsp, mcp };
}

module.exports = { createStandalone, inspectStandalone };
