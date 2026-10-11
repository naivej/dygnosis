const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const { createHash } = require("node:crypto");
const { execute, productRoot, extensionRoot, writeJson } = require("./common.cjs");
const { fetch, AbortSignal } = globalThis;

async function licenseFiles(directory) {
  const entries = await fs.readdir(directory, { withFileTypes: true });
  const files = entries.filter(entry => entry.isFile() && /^(licen[cs]e|copying|notice|copyright)(?:$|[._-])/i.test(entry.name)).map(entry => entry.name);
  for (const entry of entries.filter(entry => entry.isDirectory() && /^licen[cs]es?$/i.test(entry.name))) {
    for (const file of await fs.readdir(path.join(directory, entry.name), { withFileTypes: true })) {
      if (file.isFile()) files.push(`${entry.name}/${file.name}`);
    }
  }
  return files.sort();
}
async function collectNotice(root, kind, metadata, directory, fallback) {
  assert.ok(metadata.license, `${kind} ${metadata.name} has no declared license`);
  let files = await licenseFiles(directory);
  if (!files.length && fallback?.directory) {
    directory = fallback.directory;
    files = await licenseFiles(directory);
  }
  const copied = [];
  async function store(bytes, originalPath, source) {
    const hash = createHash("sha256").update(bytes).digest("hex");
    const destination = path.join(root, "texts", hash);
    await fs.mkdir(path.dirname(destination), { recursive: true });
    await fs.writeFile(destination, bytes);
    copied.push({ path: `licenses/texts/${hash}`, sha256: hash, original_path: originalPath, ...(source ? { source } : {}) });
  }
  for (const file of files) {
    await store(await fs.readFile(path.join(directory, file)), file);
  }
  if (!copied.length && fallback?.url) {
    const response = await fetch(fallback.url, { signal: AbortSignal.timeout(30000) });
    assert.ok(response.ok, `Cannot retrieve pinned license: ${fallback.url}`);
    const text = await response.text();
    assert.ok(text.includes("Apache License") && text.length > 1000 && text.length < 100000, "Invalid upstream license response");
    await store(text, "LICENSE", fallback.url);
  }
  assert.ok(copied.length, `No license text for ${kind} ${metadata.name} ${metadata.version}; add a checked pinned source`);
  return { kind, ...metadata, files: copied };
}
async function collectLicenses(target, runtimeFiles) {
  const root = path.resolve(extensionRoot, "licenses");
  assert.equal(path.dirname(root), extensionRoot);
  await fs.rm(root, { recursive: true, force: true });
  const notices = [];
  const collect = async (...args) => { notices.push(await collectNotice(root, ...args)); };
  const cargo = JSON.parse(execute("cargo", ["metadata", "--locked", "--filter-platform", target, "--format-version", "1"], { cwd: productRoot }));
  const packages = cargo.packages.filter(pkg => pkg.id !== cargo.resolve.root);
  for (const pkg of packages.sort((a, b) => a.name.localeCompare(b.name))) {
    let fallback;
    if (["rmcp", "rmcp-macros"].includes(pkg.name) && pkg.version === "3.2.0") {
      const vcs = JSON.parse(await fs.readFile(path.join(path.dirname(pkg.manifest_path), ".cargo_vcs_info.json"), "utf8"));
      assert.equal(vcs.git.sha1, "51ccb42993d6eb5075399672ce7a0c21a0e55eea");
      fallback = { url: "https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/51ccb42993d6eb5075399672ce7a0c21a0e55eea/LICENSE" };
    }
    if (pkg.name === "tower-lsp-macros" && pkg.version === "0.9.0") {
      const owner = packages.find(item => item.name === "tower-lsp" && item.version === "0.20.0");
      assert.ok(owner, "tower-lsp-macros needs the matching parent license texts");
      const vcs = JSON.parse(await fs.readFile(path.join(path.dirname(owner.manifest_path), ".cargo_vcs_info.json"), "utf8"));
      const childVcs = JSON.parse(await fs.readFile(path.join(path.dirname(pkg.manifest_path), ".cargo_vcs_info.json"), "utf8"));
      assert.equal(vcs.git.sha1, childVcs.git.sha1);
      fallback = { directory: path.dirname(owner.manifest_path) };
    }
    await collect("cargo", { name: pkg.name, version: pkg.version, license: pkg.license, repository: pkg.repository, authors: pkg.authors }, path.dirname(pkg.manifest_path), fallback);
  }
  // The client bundler supplies the packages whose code enters the VSIX.
  for (const file of runtimeFiles.filter(file => /(?:^|\/)node_modules\/(?:@[^/]+\/)?[^/]+\/package.json$/.test(file)).sort()) {
    const absolute = path.join(extensionRoot, file);
    const pkg = JSON.parse(await fs.readFile(absolute, "utf8"));
    await collect("npm", { name: pkg.name, version: pkg.version, license: typeof pkg.license === "string" ? pkg.license : pkg.license?.type, repository: pkg.repository, packagePath: file }, path.dirname(absolute));
  }
  const sysroot = execute("rustc", ["--print", "sysroot"]).trim();
  await collect("toolchain", { name: "rust", version: execute("rustc", ["--version"]).trim(), license: "MIT OR Apache-2.0", repository: "https://github.com/rust-lang/rust" }, path.join(sysroot, "share/doc/rust"));
  await writeJson(path.join(extensionRoot, "THIRD-PARTY-NOTICES.json"), notices);
  return notices;
}
module.exports = { collectLicenses, collectNotice };
