const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const { execute, productRoot, writeJson } = require("./common.cjs");

async function main() {
  const { RELEASE_TAG: tag, VERIFIED_RUN_ID: runId, GITHUB_REPOSITORY: repository, ARTIFACT_DIRECTORY: directory, RELEASE_GATES_FILE: gates } = process.env;
  assert.ok(/^v\d+\.\d+\.\d+$/.test(tag ?? ""), "Set a version tag");
  assert.ok(/^\d+$/.test(runId ?? ""), "Set the successful verification workflow run ID");
  assert.equal(repository, "naivej/dygnosis");
  const commit = execute("git", ["rev-parse", `${tag}^{commit}`], { cwd: productRoot }).trim();
  assert.equal(execute("git", ["rev-parse", "HEAD"], { cwd: productRoot }).trim(), commit);
  const run = JSON.parse(execute("gh", ["api", `repos/${repository}/actions/runs/${runId}`]));
  assert.equal(run.conclusion, "success"); assert.equal(run.head_sha, commit);
  assert.equal(run.repository.full_name, repository);
  assert.ok([".github/workflows/extension-build.yml", ".github/workflows/release.yml"].includes(run.path.split("@")[0]), "Artifacts must come from the reviewed package verification workflow");
  execute(process.execPath, [path.join(__dirname, "collect-artifacts.cjs"), "--directory", directory, "--release", "--gates", gates], { stdio: "inherit" });
  const verified = JSON.parse(await fs.readFile(path.join(directory, "verified-artifacts.json"), "utf8"));
  assert.equal(verified.commit, commit); assert.equal(verified.tag, tag); assert.equal(verified.publication_ready, true);
  const changelog = await fs.readFile(path.join(productRoot, "CHANGELOG.md"), "utf8");
  const lines = changelog.split(/\r?\n/);
  const first = lines.indexOf(`## ${tag}`);
  assert.ok(first >= 0, "Missing matching release notes");
  const next = lines.findIndex((line, index) => index > first && /^## /.test(line));
  const notes = lines.slice(first + 1, next < 0 ? undefined : next).join("\n").trim();
  assert.ok(notes);
  await fs.writeFile(path.join(directory, "release-notes.md"), notes + "\n");
  await writeJson(path.join(directory, "publication-source.json"), { tag, commit, verified_run_id: runId, workflow: run.path, publisher: verified.publisher });
}
main().catch(error => { process.stderr.write(`${error.stack}\n`); process.exitCode = 1; });
