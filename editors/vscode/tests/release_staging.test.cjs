const assert = require("node:assert/strict");
const { Buffer } = require("node:buffer");
const crypto = require("node:crypto");
const fs = require("node:fs/promises");
const os = require("node:os");
const path = require("node:path");
const test = require("node:test");
const { execute, targets, writeJson } = require("../scripts/common.cjs");
const { stageRelease } = require("../../../scripts/stage-release.cjs");

const tag = "v0.11.37";
const version = tag.slice(1);
const commit = "fixture-commit";
const digest = async file => crypto.createHash("sha256").update(await fs.readFile(file)).digest("hex");
async function cleanupTemp(directory, prefix) {
  const resolved = path.resolve(directory);
  assert.equal(path.dirname(resolved), path.resolve(os.tmpdir()));
  assert.ok(path.basename(resolved).startsWith(prefix));
  await fs.rm(resolved, { recursive: true, force: true });
}

async function fixture(parent, label = "valid") {
  const directory = path.join(parent, `${label}-input`);
  const output = path.join(parent, `${label}-release`);
  await fs.mkdir(directory);
  const matrix = [];
  for (const [target, info] of Object.entries(targets)) {
    const targetDirectory = path.join(directory, target);
    await fs.mkdir(targetDirectory);
    const vsix = `dygnosis-${version}-${target}.vsix`;
    const standalone = `dygnosis-${version}-${target}.tar.gz`;
    const vsixPath = path.join(targetDirectory, vsix);
    await fs.writeFile(vsixPath, `VSIX fixture ${target}\n`);
    const payloadRoot = path.join(directory, `${target}-payload`, standalone.slice(0, -7));
    await fs.mkdir(payloadRoot, { recursive: true });
    const lspName = info.platform === "win32" ? "dygnosis.exe" : "dygnosis";
    const mcpName = info.platform === "win32" ? "dygnosis-mcp.exe" : "dygnosis-mcp";
    await fs.writeFile(path.join(payloadRoot, lspName), `LSP ${target}\n`);
    const mcp = Buffer.from(`MCP ${target}\n`);
    await fs.writeFile(path.join(payloadRoot, mcpName), mcp);
    const archivePath = path.join(targetDirectory, standalone);
    execute("tar", ["-czf", archivePath, "-C", path.dirname(payloadRoot), path.basename(payloadRoot)]);
    matrix.push({
      target, vsix, vsix_sha256: await digest(vsixPath), standalone,
      standalone_sha256: await digest(archivePath), binary_sha256: await digest(path.join(payloadRoot, lspName)),
      mcp_binary_name: mcpName, mcp_binary_sha256: crypto.createHash("sha256").update(mcp).digest("hex"),
      mcp_binary_size: mcp.length, native: true, vscode: [],
    });
  }
  const manifest = { schema_version: 1, commit, version, tag, release: true, publisher: "CoconutWater", matrix };
  await writeJson(path.join(directory, "verified-artifacts.json"), manifest);
  return { directory, output, manifest };
}

async function replaceArchive(fx, target, names) {
  const row = fx.manifest.matrix.find(item => item.target === target);
  const archivePath = path.join(fx.directory, target, row.standalone);
  const payloadParent = path.join(fx.directory, `replacement-${target}`);
  const archiveRoot = row.standalone.slice(0, -7);
  const rooted = path.join(payloadParent, archiveRoot);
  await fs.mkdir(rooted, { recursive: true });
  for (const [name, bytes] of Object.entries(names)) await fs.writeFile(path.join(rooted, name), bytes);
  execute("tar", ["-czf", archivePath, "-C", payloadParent, archiveRoot]);
}

test("release staging publishes the verified VSIX and complete two-executable archives with hashes", async t => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), "dygnosis release staging "));
  t.after(async () => cleanupTemp(root, "dygnosis release staging "));
  const fx = await fixture(root);
  await stageRelease({ directory: fx.directory, output: fx.output, tag, commit });

  const names = (await fs.readdir(fx.output)).sort();
  const expectedPackages = fx.manifest.matrix.flatMap(row => [row.vsix, row.standalone]);
  assert.deepEqual(names, [...expectedPackages, "SHA256SUMS", "verified-artifacts.json"].sort());
  assert.deepEqual(JSON.parse(await fs.readFile(path.join(fx.output, "verified-artifacts.json"), "utf8")), fx.manifest);
  const sums = (await fs.readFile(path.join(fx.output, "SHA256SUMS"), "utf8")).trim().split(/\r?\n/);
  assert.equal(sums.length, expectedPackages.length + 1, "checksums cover all packages and the manifest");
  for (const filename of [...expectedPackages, "verified-artifacts.json"]) {
    const asset = path.join(fx.output, filename);
    const line = sums.find(item => item.endsWith(`  ${filename}`));
    assert.equal(line, `${await digest(asset)}  ${filename}`);
  }
  for (const row of fx.manifest.matrix) {
    assert.deepEqual(await fs.readFile(path.join(fx.directory, row.target, row.vsix)), await fs.readFile(path.join(fx.output, row.vsix)));
    assert.deepEqual(await fs.readFile(path.join(fx.directory, row.target, row.standalone)), await fs.readFile(path.join(fx.output, row.standalone)));
    const listing = execute("tar", ["-tzf", path.join(fx.output, row.standalone)]);
    assert.match(listing, new RegExp(`/${row.mcp_binary_name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\\r?\\n`));
    assert.match(listing, /\/dygnosis(?:\.exe)?\r?\n/);
  }
});

test("release staging rejects invalid matrix identity, paths, and changed package archives", async t => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), "dygnosis release rejects "));
  t.after(async () => cleanupTemp(root, "dygnosis release rejects "));
  const invalid = async (label, change, requestedTag = tag) => {
    const fx = await fixture(root, label);
    await change(fx);
    await assert.rejects(stageRelease({ directory: fx.directory, output: fx.output, tag: requestedTag, commit }));
    assert.equal(await fs.stat(fx.output).then(() => true, () => false), false, "invalid input must not create publish output");
  };

  await invalid("missing-mcp", async fx => {
    const row = fx.manifest.matrix.find(item => item.target === "linux-arm64");
    await replaceArchive(fx, row.target, { dygnosis: "LSP linux-arm64\n" });
  });
  await invalid("changed-mcp", async fx => {
    const row = fx.manifest.matrix.find(item => item.target === "darwin-x64");
    await replaceArchive(fx, row.target, { dygnosis: "LSP darwin-x64\n", "dygnosis-mcp": "tampered MCP\n" });
  });
  await invalid("missing-provenance", async fx => {
    delete fx.manifest.matrix[0].mcp_binary_sha256;
    await writeJson(path.join(fx.directory, "verified-artifacts.json"), fx.manifest);
  });
  await invalid("wrong-tag", async () => {}, "v9.9.9");
  await invalid("wrong-commit", async fx => {
    fx.manifest.commit = "different-commit";
    await writeJson(path.join(fx.directory, "verified-artifacts.json"), fx.manifest);
  });
  await invalid("candidate", async fx => {
    fx.manifest.release = false;
    await writeJson(path.join(fx.directory, "verified-artifacts.json"), fx.manifest);
  });
  await invalid("duplicate-target", async fx => {
    fx.manifest.matrix.push({ ...fx.manifest.matrix[0] });
    await writeJson(path.join(fx.directory, "verified-artifacts.json"), fx.manifest);
  });
  await invalid("traversal-name", async fx => {
    fx.manifest.matrix[0].vsix = "../outside.vsix";
    await writeJson(path.join(fx.directory, "verified-artifacts.json"), fx.manifest);
  });
});

test("release staging preserves a nonempty destination and rejects input-output overlap", async t => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), "dygnosis release protected "));
  t.after(async () => cleanupTemp(root, "dygnosis release protected "));
  const fx = await fixture(root);
  await fs.mkdir(fx.output);
  const sentinel = path.join(fx.output, "keep.txt");
  await fs.writeFile(sentinel, "keep\n");
  await assert.rejects(stageRelease({ directory: fx.directory, output: fx.output, tag, commit }));
  assert.equal(await fs.readFile(sentinel, "utf8"), "keep\n");
  await assert.rejects(stageRelease({ directory: fx.directory, output: fx.directory, tag, commit }));
});
