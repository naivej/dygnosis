const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const os = require("node:os");
const path = require("node:path");
const test = require("node:test");
const { execute } = require("../scripts/common.cjs");

test("standalone archives round-trip absolute paths containing spaces", async t => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), "dygnosis archive "));
  t.after(() => {
    const target = path.resolve(root);
    assert.equal(path.dirname(target), path.resolve(os.tmpdir()));
    assert.ok(path.basename(target).startsWith("dygnosis archive "));
    return fs.rm(target, { recursive: true, force: true });
  });
  const payload = path.join(root, "payload with spaces");
  const archive = path.join(root, "standalone with spaces.tar.gz");
  const unpacked = path.join(root, "unpacked with spaces");
  await fs.mkdir(payload); await fs.mkdir(unpacked);
  await fs.writeFile(path.join(payload, "LICENSE"), "exact license bytes\n");
  execute("tar", ["-czf", archive, "-C", root, path.basename(payload)]);
  assert.ok(execute("tar", ["-tzf", archive]).includes("payload with spaces/LICENSE"));
  execute("tar", ["-xzf", archive, "-C", unpacked]);
  assert.equal(await fs.readFile(path.join(unpacked, "payload with spaces/LICENSE"), "utf8"), "exact license bytes\n");
});
