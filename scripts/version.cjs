// Read the product version from Cargo.toml; synchronize or check required copies.
const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");

const productRoot = path.resolve(__dirname, "..");
const syncHint = "Run node scripts/version.cjs --sync from the product root.";

async function readVersion(root = productRoot) {
  const cargo = await fs.readFile(path.join(root, "Cargo.toml"), "utf8");
  const section = /^\[package\][ \t]*\r?\n([\s\S]*?)(?=^\[|(?![\s\S]))/m.exec(cargo)?.[1];
  const version = /^version\s*=\s*"([^"]+)"/m.exec(section ?? "")?.[1];
  assert.match(version ?? "", /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/, "Cargo.toml [package].version must be MAJOR.MINOR.PATCH");
  return version;
}

async function metadata(root) {
  const version = await readVersion(root);
  const files = ["Cargo.lock", "editors/vscode/package.json", "editors/vscode/package-lock.json"];
  const [cargoLock, manifestText, npmLockText] = await Promise.all(files.map(file => fs.readFile(path.join(root, file), "utf8")));
  const blocks = cargoLock.split(/(?=^\[\[package\]\]\r?$)/m).filter(block => /^name\s*=\s*"dygnosis"\s*\r?$/m.test(block));
  assert.equal(blocks.length, 1, "Cargo.lock must contain one dygnosis package");
  const lockedVersion = /^version\s*=\s*"([^"]+)"/m.exec(blocks[0])?.[1];
  assert.ok(lockedVersion, "Cargo.lock dygnosis package must contain a version");
  const manifest = JSON.parse(manifestText), npmLock = JSON.parse(npmLockText);
  assert.equal(manifest.name, "dygnosis");
  assert.equal(npmLock.name, "dygnosis");
  assert.equal(npmLock.packages?.[""]?.name, "dygnosis");
  const json = (value, text) => (JSON.stringify(value, null, 2) + "\n").replaceAll("\n", text.includes("\r\n") ? "\r\n" : "\n");
  return {
    version,
    files: [
      { file: files[0], versions: [lockedVersion], updated: cargoLock.replace(blocks[0], blocks[0].replace(/^(version\s*=\s*)"[^"]+"/m, `$1"${version}"`)) },
      { file: files[1], versions: [manifest.version], updated: json({ ...manifest, version }, manifestText) },
      { file: files[2], versions: [npmLock.version, npmLock.packages[""].version], updated: json({ ...npmLock, version, packages: { ...npmLock.packages, "": { ...npmLock.packages[""], version } } }, npmLockText) },
    ],
  };
}

async function syncMetadata(root = productRoot) {
  const current = await metadata(root);
  for (const entry of current.files) {
    if (entry.versions.some(version => version !== current.version)) await fs.writeFile(path.join(root, entry.file), entry.updated);
  }
  return current.version;
}

async function checkVersion(root = productRoot) {
  const current = await metadata(root);
  const reference = JSON.parse(await fs.readFile(path.join(root, "help/reference.json"), "utf8"));
  const files = [...current.files, { file: "help/reference.json", versions: [reference.version] }];
  for (const entry of files) {
    for (const version of entry.versions) assert.equal(version, current.version, `${entry.file} differs from Cargo.toml (${current.version}). ${syncHint}`);
  }
  return current.version;
}

async function syncVersion(root = productRoot) {
  await syncMetadata(root);
  const help = require(path.join(root, "editors/vscode/scripts/help.cjs"));
  await help.ensureReference({ root });
  return checkVersion(root);
}

if (require.main === module) (async () => {
  const [mode, ...extra] = process.argv.slice(2);
  assert.ok(["--sync", "--check"].includes(mode) && !extra.length, "Usage: node scripts/version.cjs --sync|--check");
  const version = await (mode === "--sync" ? syncVersion() : checkVersion());
  process.stdout.write(`Product version ${version}: ${mode === "--sync" ? "synchronized" : "all copies match Cargo.toml"}.\n`);
})().catch(error => { process.stderr.write(`${error.message}\n`); process.exitCode = 1; });

module.exports = { readVersion, syncMetadata, checkVersion, syncVersion };
