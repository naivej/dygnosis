const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const os = require("node:os");
const test = require("node:test");
const { resolveCli } = require("../scripts/vscode-cli.cjs");

test("finds the matching traditional and versioned Windows CLI", async t => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), "dygnosis-cli-"));
  t.after(() => {
    const target = path.resolve(root);
    assert.equal(path.dirname(target), path.resolve(os.tmpdir()));
    assert.ok(path.basename(target).startsWith("dygnosis-cli-"));
    return fs.rm(target, { recursive: true, force: true });
  });
  const executable = path.join(root, "Code.exe");
  async function app(prefix, version) {
    const appRoot = path.join(root, prefix, "resources/app");
    await fs.mkdir(path.join(appRoot, "out"), { recursive: true });
    await fs.writeFile(path.join(appRoot, "package.json"), JSON.stringify({ version }));
    await fs.writeFile(path.join(appRoot, "out/cli.js"), "");
    return path.join(appRoot, "out/cli.js");
  }
  const minimum = await app("", "1.102.0");
  const current = await app("07f806f999", "1.140.0");
  await app("old-build", "1.139.0");
  assert.equal(await resolveCli(executable, "1.102.0", "win32"), minimum);
  assert.equal(await resolveCli(executable, "1.140.0", "win32"), current);
  await assert.rejects(resolveCli(executable, "1.141.0", "win32"), /Expected one/);
  await app("duplicate", "1.140.0");
  await assert.rejects(resolveCli(executable, "1.140.0", "win32"), /Expected one/);
  await fs.mkdir(path.join(root, "bin"));
  await fs.writeFile(path.join(root, "bin/code.cmd"), '"%~dp0..\\Code.exe" "%~dp0..\\07f806f999\\resources\\app\\out\\cli.js" %*');
  assert.equal(await resolveCli(executable, undefined, "win32"), current);
  await assert.rejects(resolveCli(executable, "1.102.0", "win32"), /1.140.0/);
});
