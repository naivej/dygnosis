const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const os = require("node:os");
const path = require("node:path");
const test = require("node:test");
const { copyHelpAssets } = require("../scripts/help.cjs");
const { collectNotice } = require("../scripts/licenses.cjs");

async function fixture(t) {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), "dygnosis package contents "));
  t.after(() => {
    assert.equal(path.dirname(path.resolve(root)), path.resolve(os.tmpdir()));
    assert.ok(path.basename(root).startsWith("dygnosis package contents "));
    return fs.rm(root, { recursive: true, force: true });
  });
  return root;
}

// Guard against omitting a used image or retaining unused/stale screenshots.
test("Help copies only referenced images and removes assets from the previous build", async t => {
  const root = await fixture(t), source = path.join(root, "source"), output = path.join(root, "output");
  await fs.mkdir(path.join(source, "assets/nested"), { recursive: true });
  await fs.writeFile(path.join(source, "assets/nested/used.svg"), "<svg>used</svg>");
  await fs.writeFile(path.join(source, "assets/unused.png"), "unused");
  await copyHelpAssets([{ markdown: "![Image](assets/nested/used.svg)" }], source, output);
  assert.equal(await fs.readFile(path.join(output, "assets/nested/used.svg"), "utf8"), "<svg>used</svg>");
  await assert.rejects(fs.stat(path.join(output, "assets/unused.png")), { code: "ENOENT" });
  await copyHelpAssets([{ markdown: "No images" }], source, output);
  await assert.rejects(fs.stat(path.join(output, "assets")), { code: "ENOENT" });
  await assert.rejects(copyHelpAssets([{ markdown: "![Image](assets/missing.svg)" }], source, output), { code: "ENOENT" });
});

// Guard against losing attribution or changing bytes when two packages share a license.
test("license notices retain each component and source filename while identical texts share one file", async t => {
  const root = await fixture(t), licenses = path.join(root, "licenses");
  const components = [];
  for (const [name, filename, contents] of [["first", "LICENSE", "shared\r\n"], ["second", "COPYING", "shared\r\n"], ["third", "NOTICE", "different\n"]]) {
    const directory = path.join(root, name);
    await fs.mkdir(directory);
    await fs.writeFile(path.join(directory, filename), contents);
    components.push(await collectNotice(licenses, "npm", { name, version: "1.0.0", license: "MIT", authors: [name] }, directory));
  }
  assert.deepEqual(components.map(notice => notice.name), ["first", "second", "third"]);
  assert.deepEqual(components.map(notice => notice.authors), [["first"], ["second"], ["third"]]);
  assert.deepEqual(components.map(notice => notice.files[0].original_path), ["LICENSE", "COPYING", "NOTICE"]);
  assert.equal(components[0].files[0].path, components[1].files[0].path);
  assert.notEqual(components[0].files[0].path, components[2].files[0].path);
  assert.equal(await fs.readFile(path.join(root, components[0].files[0].path), "utf8"), "shared\r\n");
  assert.deepEqual((await fs.readdir(path.join(licenses, "texts"))).sort(), [...new Set(components.map(notice => notice.files[0].sha256))].sort());
});
