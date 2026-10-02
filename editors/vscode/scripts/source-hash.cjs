const crypto = require("node:crypto");
const fs = require("node:fs/promises");
const path = require("node:path");
const { execute } = require("./common.cjs");

async function sourceFileHashes(repository, commit, relative) {
  const committed = execute("git", ["show", `${commit}:${relative}`], { cwd: repository, encoding: null });
  const checkout = await fs.readFile(path.join(repository, relative));
  const hash = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
  return { committed: hash(committed), checkout: hash(checkout) };
}
module.exports = { sourceFileHashes };
