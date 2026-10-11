// Build the single VSIX client and identify the third-party code it contains.
const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const { builtinModules } = require("node:module");
const esbuild = require("esbuild");
const { extensionRoot, sha256, writeJson } = require("./common.cjs");

async function bundle() {
  const entry = "out/extension.js";
  const result = await esbuild.build({
    absWorkingDir: extensionRoot, entryPoints: ["src/extension.ts"], outfile: entry,
    bundle: true, platform: "node", format: "cjs", target: "es2022", minify: true,
    external: ["vscode"], metafile: true, legalComments: "none",
  });
  const builtins = new Set(builtinModules.map(name => name.replace(/^node:/, "")));
  for (const output of Object.values(result.metafile.outputs)) for (const dependency of output.imports) {
    assert.ok(dependency.external && (dependency.path === "vscode" || builtins.has(dependency.path.replace(/^node:/, ""))),
      `Unbundled runtime dependency: ${dependency.path}`);
  }
  const packagePaths = new Set();
  for (const input of Object.keys(result.metafile.inputs)) {
    const packages = [...input.matchAll(/(?:^|\/)node_modules\/(?:@[^/]+\/)?[^/]+/g)];
    const last = packages.at(-1);
    if (last) packagePaths.add(`${input.slice(0, last.index + last[0].length)}/package.json`);
  }
  const dependencies = [];
  for (const packagePath of [...packagePaths].sort()) {
    const pkg = JSON.parse(await fs.readFile(path.join(extensionRoot, packagePath), "utf8"));
    dependencies.push({ name: pkg.name, version: pkg.version, packagePath });
  }
  const client = { entry, sha256: await sha256(path.join(extensionRoot, entry)), dependencies };
  await writeJson(path.join(extensionRoot, "out/bundle.json"), client);
  return client;
}
if (require.main === module) bundle().catch(error => { process.stderr.write(`${error.stack}\n`); process.exitCode = 1; });
module.exports = { bundle };
