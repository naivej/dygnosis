const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const os = require("node:os");
const path = require("node:path");
const test = require("node:test");
const { sha256 } = require("../../../scripts/package-common.cjs");
const { createStandalone, inspectStandalone } = require("../../../scripts/standalone-package.cjs");

// Guards the native binary pair and license bytes against extension-only
// content leaking into the archive, including when paths contain spaces.
test("standalone archives round-trip only native files and provenance", async t => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), "dygnosis archive "));
  t.after(() => {
    const target = path.resolve(root);
    assert.equal(path.dirname(target), path.resolve(os.tmpdir()));
    assert.ok(path.basename(target).startsWith("dygnosis archive "));
    return fs.rm(target, { recursive: true, force: true });
  });
  const licenseSource = path.join(root, "license source");
  const license = path.join(licenseSource, "LICENSE");
  const licenseTexts = path.join(licenseSource, "licenses", "texts");
  await fs.mkdir(licenseTexts, { recursive: true });
  await fs.writeFile(license, "exact license bytes\n");
  const nativeHash = await sha256(license);
  await fs.copyFile(license, path.join(licenseTexts, nativeHash));
  const npmLicense = path.join(licenseSource, "npm license");
  await fs.writeFile(npmLicense, "extension-only license\n");
  const npmHash = await sha256(npmLicense);
  await fs.copyFile(npmLicense, path.join(licenseTexts, npmHash));
  const nativeFile = { sha256: nativeHash, path: `licenses/texts/${nativeHash}` };
  const notices = [
    { kind: "cargo", name: "crate", files: [nativeFile] },
    { kind: "toolchain", name: "rust", files: [nativeFile] },
    { kind: "npm", name: "vscode-languageclient", files: [nativeFile, { sha256: npmHash, path: `licenses/texts/${npmHash}` }] },
  ];
  const binaries = { lsp: path.join(root, "lsp with spaces"), mcp: path.join(root, "mcp with spaces") };
  await fs.writeFile(binaries.lsp, "LSP bytes\n");
  await fs.writeFile(binaries.mcp, "MCP bytes\n");
  const provenance = {
    schema_version: 1, name: "dygnosis", publisher: "CoconutWater", version: "0.12.7",
    target: "win32-x64", rust_target: "x86_64-pc-windows-msvc", commit: "test-commit",
    tag: null, release: false, dirty: false, source: "test-source", source_archive: "test-source-archive",
    binary_sha256: await sha256(binaries.lsp), mcp_binary_name: "dygnosis-mcp.exe",
    mcp_binary_sha256: await sha256(binaries.mcp), mcp_binary_size: (await fs.stat(binaries.mcp)).size,
    license: { spdx: "GPL-3.0-or-later", source_file: "LICENSE", source_sha256: nativeHash,
      vsix_path: "extension/LICENSE.txt", standalone_path: "LICENSE" },
    rustc: "rustc test version", cargo_lock_sha256: "cargo-lock", cargo_lock_checkout_sha256: "cargo-lock",
    build: "provided candidate binary pair", rustflags: null, host: { platform: "win32", arch: "x64" },
    client: { entry: "out/extension.js" }, npm_lock_sha256: "npm-lock", logo_sha256: "logo",
  };
  const inputs = { output: root, binaries, provenance, licenseSource, notices };
  const archive = await createStandalone(inputs);
  const { lsp, mcp } = await inspectStandalone(archive, path.join(root, "unpacked with spaces"), provenance);
  const payload = path.dirname(lsp);
  const readJson = async file => JSON.parse(await fs.readFile(path.join(payload, file), "utf8"));
  assert.equal(await fs.readFile(lsp, "utf8"), "LSP bytes\n");
  assert.equal(await fs.readFile(mcp, "utf8"), "MCP bytes\n");
  assert.equal(await fs.readFile(path.join(payload, "LICENSE"), "utf8"), "exact license bytes\n");
  assert.deepEqual(await readJson("THIRD-PARTY-NOTICES.json"), notices.slice(0, 2));
  assert.deepEqual(await fs.readdir(path.join(payload, "licenses", "texts")), [nativeHash]);
  const source = await readJson("SOURCE.json");
  assert.equal(source.schema_version, 2);
  assert.equal(source.cargo_lock_sha256, provenance.cargo_lock_sha256);
  for (const field of ["publisher", "client", "npm_lock_sha256", "logo_sha256"]) {
    assert.ok(!Object.hasOwn(source, field), `Standalone provenance includes ${field}`);
  }
  assert.ok(!Object.hasOwn(source.license, "vsix_path"));

  // An altered license must prevent the new archive from passing verification.
  await fs.writeFile(path.join(licenseTexts, nativeHash), "different license bytes\n");
  await createStandalone(inputs);
  await assert.rejects(inspectStandalone(archive, path.join(root, "second unpack"), provenance), /Changed license/);
});
