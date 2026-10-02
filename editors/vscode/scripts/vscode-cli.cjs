const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");

async function resolveCli(executable, version, platform = process.platform) {
  const root = path.dirname(executable);
  if (platform === "win32") {
    try {
      const wrapper = await fs.readFile(path.join(root, "bin/code.cmd"), "utf8");
      const relative = /"%~dp0\.\.\\([^"\r\n]*resources\\app\\out\\cli\.js)"/i.exec(wrapper)?.[1];
      if (relative) {
        const cli = path.resolve(root, relative.replaceAll("\\", path.sep));
        assert.ok(cli.startsWith(path.resolve(root) + path.sep), "VS Code CLI wrapper escaped its application directory");
        const manifest = JSON.parse(await fs.readFile(path.resolve(path.dirname(cli), "../package.json"), "utf8"));
        if (version) assert.equal(manifest.version, version);
        await fs.access(cli);
        return cli;
      }
    } catch (error) {
      if (error.code !== "ENOENT" && error.code !== "ENOTDIR") throw error;
    }
  }
  const roots = platform === "darwin" ? [path.resolve(root, "../Resources/app")] : [path.join(root, "resources/app")];
  if (platform === "win32") {
    for (const entry of await fs.readdir(root, { withFileTypes: true })) {
      if (entry.isDirectory()) roots.push(path.join(root, entry.name, "resources/app"));
    }
  }
  const matches = [];
  for (const application of roots) {
    try {
      const manifest = JSON.parse(await fs.readFile(path.join(application, "package.json"), "utf8"));
      if (version && manifest.version !== version) continue;
      const cli = path.join(application, "out/cli.js");
      await fs.access(cli);
      matches.push(cli);
    } catch (error) {
      if (error.code !== "ENOENT" && error.code !== "ENOTDIR") throw error;
    }
  }
  assert.equal(matches.length, 1, `Expected one VS Code ${version} CLI beside ${executable}`);
  return matches[0];
}
module.exports = { resolveCli };
