const assert = require("node:assert/strict");
const crypto = require("node:crypto");
const fs = require("node:fs/promises");
const path = require("node:path");
const test = require("node:test");
const { execute, productRoot } = require("../scripts/common.cjs");
const { sourceFileHashes } = require("../scripts/source-hash.cjs");

test("source provenance distinguishes committed bytes from checkout line endings", async () => {
  const commit = execute("git", ["rev-parse", "HEAD"], { cwd: productRoot }).trim();
  const hash = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
  for (const relative of ["Cargo.lock", "editors/vscode/package-lock.json"]) {
    const committed = execute("git", ["show", `${commit}:${relative}`], { cwd: productRoot, encoding: null });
    const text = committed.toString("utf8");
    const hashes = await sourceFileHashes(productRoot, commit, relative);
    assert.equal(hashes.committed, hash(committed));
    assert.equal(hashes.checkout, hash(await fs.readFile(path.join(productRoot, relative))));
    assert.equal(hashes.committed, hash(text.replaceAll("\r\n", "\n")));
    assert.notEqual(hashes.committed, hash(text.replaceAll("\r\n", "\n").replaceAll("\n", "\r\n")));
  }
});
