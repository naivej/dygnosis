const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const os = require("node:os");
const path = require("node:path");
const test = require("node:test");
const { readVersion, syncMetadata, checkVersion } = require("../../../scripts/version.cjs");

async function fixture(t, previous = "0.11.12") {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), "dygnosis version "));
  assert.equal(path.dirname(root), path.resolve(os.tmpdir()));
  t.after(() => fs.rm(root, { recursive: true, force: true }));
  await fs.mkdir(path.join(root, "editors/vscode"), { recursive: true });
  await fs.mkdir(path.join(root, "help"));
  const files = {
    "Cargo.toml": '[dependencies]\nhelper = { version = "9.8.7" }\n\n[package]\nname = "dygnosis"\nversion = "0.12.0"\n',
    "Cargo.lock": `version = 4\n\n[[package]]\nname = "helper"\nversion = "${previous}"\nsource = "registry+https://example.test"\n\n[[package]]\nname = "dygnosis"\nversion = "${previous}"\ndependencies = [\n "helper",\n]\n\n[[package]]\nname = "other"\nversion = "${previous}"\n`,
    "editors/vscode/package.json": JSON.stringify({ name: "dygnosis", version: previous, dependencies: { helper: previous } }, null, 2) + "\n",
    "editors/vscode/package-lock.json": JSON.stringify({
      name: "dygnosis", version: previous, lockfileVersion: 3,
      packages: { "": { name: "dygnosis", version: previous }, "node_modules/helper": { version: previous, integrity: "untouched" } },
    }, null, 2) + "\n",
    "help/reference.json": JSON.stringify({ version: previous, fingerprint: "unchanged", tools: [] }) + "\n",
  };
  await Promise.all(Object.entries(files).map(([file, text]) => fs.writeFile(path.join(root, file), text)));
  return { root, files };
}

test("Cargo package version is the source; dependency versions and invalid release values cannot replace it", async t => {
  const { root } = await fixture(t);
  assert.equal(await readVersion(root), "0.12.0");
  for (const version of ["v0.12.0", "0.12", "00.12.0", "0.12.0-beta.1"]) {
    await fs.writeFile(path.join(root, "Cargo.toml"), `[package]\nversion = "${version}"\n`);
    const before = await fs.readFile(path.join(root, "Cargo.lock"), "utf8");
    await assert.rejects(syncMetadata(root), /MAJOR.MINOR.PATCH/);
    assert.equal(await fs.readFile(path.join(root, "Cargo.lock"), "utf8"), before);
  }
});

test("sync changes only product metadata, preserves dependency pins, and is idempotent", async t => {
  const { root, files } = await fixture(t);
  // A Windows checkout must retain its line endings when JSON metadata changes.
  const manifestFile = path.join(root, "editors/vscode/package.json");
  await fs.writeFile(manifestFile, files["editors/vscode/package.json"].replaceAll("\n", "\r\n"));
  assert.equal(await syncMetadata(root), "0.12.0");
  const cargoLock = await fs.readFile(path.join(root, "Cargo.lock"), "utf8");
  assert.equal(cargoLock, files["Cargo.lock"].replace('name = "dygnosis"\nversion = "0.11.12"', 'name = "dygnosis"\nversion = "0.12.0"'));
  const manifestText = await fs.readFile(manifestFile, "utf8"), manifest = JSON.parse(manifestText);
  assert.equal(manifest.version, "0.12.0");
  assert.equal(manifest.dependencies.helper, "0.11.12");
  assert.ok(manifestText.includes("\r\n"));
  assert.doesNotMatch(manifestText.replaceAll("\r\n", ""), /\n/);
  const lock = JSON.parse(await fs.readFile(path.join(root, "editors/vscode/package-lock.json"), "utf8"));
  assert.equal(lock.version, "0.12.0");
  assert.equal(lock.packages[""].version, "0.12.0");
  assert.deepEqual(lock.packages["node_modules/helper"], { version: "0.11.12", integrity: "untouched" });
  // Metadata synchronization cannot claim that stale Help came from a rebuilt engine.
  assert.equal(await fs.readFile(path.join(root, "help/reference.json"), "utf8"), files["help/reference.json"]);
  await assert.rejects(checkVersion(root), /help\/reference.json.*--sync/);
  const before = await Promise.all(Object.keys(files).map(file => fs.readFile(path.join(root, file), "utf8")));
  await syncMetadata(root);
  assert.deepEqual(await Promise.all(Object.keys(files).map(file => fs.readFile(path.join(root, file), "utf8"))), before);
});

test("check rejects every stale product version copy and does not repair it", async t => {
  const changes = [
    ["Cargo.lock", text => text.replace('name = "dygnosis"\nversion = "0.12.0"', 'name = "dygnosis"\nversion = "0.11.12"')],
    ["editors/vscode/package.json", text => JSON.stringify({ ...JSON.parse(text), version: "0.11.12" })],
    ["editors/vscode/package-lock.json", text => JSON.stringify({ ...JSON.parse(text), version: "0.11.12" })],
    ["editors/vscode/package-lock.json", text => { const lock = JSON.parse(text); lock.packages[""].version = "0.11.12"; return JSON.stringify(lock); }],
    ["help/reference.json", text => JSON.stringify({ ...JSON.parse(text), version: "0.11.12" })],
  ];
  for (const [file, change] of changes) {
    const { root, files } = await fixture(t, "0.12.0");
    assert.equal(await checkVersion(root), "0.12.0");
    const stale = change(files[file]);
    await fs.writeFile(path.join(root, file), stale);
    await assert.rejects(checkVersion(root), error => error.message.includes(file) && error.message.includes("--sync"));
    assert.equal(await fs.readFile(path.join(root, file), "utf8"), stale);
  }
});
