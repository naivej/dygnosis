// Stage immutable, CI-verified VSIX files and complete standalone bundles for a release.
const assert = require("node:assert/strict");
const { createHash } = require("node:crypto");
const { createReadStream, createWriteStream } = require("node:fs");
const fs = require("node:fs/promises");
const path = require("node:path");
const { pipeline } = require("node:stream/promises");
const { parseArgs } = require("node:util");
const { targets, mcpBinaryName } = require("../editors/vscode/scripts/common.cjs");

function contains(parent, child) {
  const relative = path.relative(parent, child);
  return relative === "" || !relative.startsWith(`..${path.sep}`) && relative !== ".." && !path.isAbsolute(relative);
}

async function copyVerified(source, destination, expected) {
  const hash = createHash("sha256");
  await pipeline(createReadStream(source), async function* (chunks) {
    for await (const chunk of chunks) { hash.update(chunk); yield chunk; }
  }, createWriteStream(destination, { flags: "wx" }));
  assert.equal(hash.digest("hex"), expected, `Release asset checksum mismatch: ${path.basename(source)}`);
}

async function stageRelease({ directory, output, tag, commit }) {
  assert.ok(directory && output && tag && commit, "Pass --directory, --output, --tag and --commit");
  const root = path.resolve(directory), destination = path.resolve(output);
  assert.ok(!contains(root, destination) && !contains(destination, root), "Release input and output must not overlap");
  const existing = await fs.lstat(destination).catch(error => {
    if (error.code === "ENOENT") return null;
    throw error;
  });
  if (existing) {
    assert.ok(existing.isDirectory() && !existing.isSymbolicLink(), "Release output must be a directory");
    assert.equal((await fs.readdir(destination)).length, 0, "Release output directory must be empty");
  }
  const bytes = await fs.readFile(path.join(root, "verified-artifacts.json"));
  const manifest = JSON.parse(bytes.toString("utf8"));
  assert.equal(manifest.schema_version, 1, "Unsupported verified artifact manifest");
  assert.equal(manifest.release, true, "Candidate artifacts cannot be published as a release");
  assert.match(tag, /^v[0-9]+\.[0-9]+\.[0-9]+$/);
  assert.equal(manifest.tag, tag, "Verified artifacts do not match the release tag");
  assert.equal(`v${manifest.version}`, tag, "Verified artifact version does not match the release tag");
  assert.equal(manifest.commit, commit, "Verified artifacts do not match the checked-out commit");
  assert.ok(Array.isArray(manifest.matrix), "Missing verified artifact matrix");
  assert.deepEqual(manifest.matrix.map(row => row.target).sort(), Object.keys(targets).sort(), "Release requires each verified native target exactly once");
  const assets = [];
  for (const row of manifest.matrix) {
    assert.equal(row.native, true, `Missing native verification: ${row.target}`);
    assert.match(row.binary_sha256, /^[a-f0-9]{64}$/);
    assert.match(row.mcp_binary_sha256, /^[a-f0-9]{64}$/);
    assert.equal(row.mcp_binary_name, mcpBinaryName(row.target));
    assert.ok(Number.isSafeInteger(row.mcp_binary_size) && row.mcp_binary_size > 0, "Missing MCP binary size");
    for (const [field, suffix] of [["vsix", "vsix"], ["standalone", "tar.gz"]]) {
      const name = `dygnosis-${manifest.version}-${row.target}.${suffix}`;
      assert.equal(row[field], name, "Release assets must use the verified canonical filename");
      const sha256 = row[`${field}_sha256`];
      assert.match(sha256, /^[a-f0-9]{64}$/);
      const source = path.join(root, row.target, name);
      const info = await fs.lstat(source);
      assert.ok(info.isFile() && !info.isSymbolicLink(), `Release asset is not a regular file: ${source}`);
      assets.push({ name, source, sha256 });
    }
  }
  // Hash each downloaded package while copying it once. The verified whole-file
  // identity carries the native pair checks; no second extraction/build is needed.
  const parent = path.dirname(destination), prefix = `${path.basename(destination)}-staging-`;
  await fs.mkdir(parent, { recursive: true });
  const staging = await fs.mkdtemp(path.join(parent, prefix));
  let committed = false;
  try {
    for (const asset of assets) await copyVerified(asset.source, path.join(staging, asset.name), asset.sha256);
    await fs.writeFile(path.join(staging, "verified-artifacts.json"), bytes, { flag: "wx" });
    const manifestHash = createHash("sha256").update(bytes).digest("hex");
    await fs.writeFile(path.join(staging, "SHA256SUMS"), [...assets.map(asset => `${asset.sha256}  ${asset.name}`), `${manifestHash}  verified-artifacts.json`].join("\n") + "\n", { flag: "wx" });
    if (existing) await fs.rmdir(destination); // Non-recursive: a concurrent new file prevents replacement.
    await fs.rename(staging, destination);
    committed = true;
  } finally {
    if (!committed) {
      assert.equal(path.dirname(path.resolve(staging)), parent);
      assert.ok(path.basename(staging).startsWith(prefix));
      await fs.rm(staging, { recursive: true, force: true });
    }
  }
  return { packages: assets.length, output: destination };
}

if (require.main === module) {
  const { values } = parseArgs({ options: { directory: { type: "string" }, output: { type: "string" }, tag: { type: "string" }, commit: { type: "string" } } });
  stageRelease(values).then(result => process.stdout.write(`Staged ${result.packages} verified packages.\n`)).catch(error => {
    process.stderr.write(`${error.stack}\n`); process.exitCode = 1;
  });
}
module.exports = { stageRelease };
