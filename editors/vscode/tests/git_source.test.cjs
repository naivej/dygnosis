const assert = require("node:assert/strict");
const test = require("node:test");
const fs = require("node:fs/promises");
const os = require("node:os");
const path = require("node:path");
const childProcess = require("node:child_process");
const { Buffer } = require("node:buffer");
const { pathToFileURL } = require("node:url");
const { GitSources, GitSourceError, parseTreeManifest } = require("../out/git_source");
const { decodeGitDocument, decodeGitText, normalizeGitText } = require("../out/git_provenance");

function fileUri(file) {
  const absolute = path.resolve(file);
  return { scheme: "file", authority: "", query: "", fsPath: absolute, path: pathToFileURL(absolute).pathname, toString: () => pathToFileURL(absolute).href };
}
function git(folder, ...args) {
  return childProcess.execFileSync("git", ["-C", folder, ...args], { encoding: "utf8", windowsHide: true, env: { ...process.env, GIT_NO_LAZY_FETCH: "1", GIT_TERMINAL_PROMPT: "0" } }).trim();
}
function repository(folder) {
  return {
    rootUri: fileUri(folder), state: { refs: [], indexChanges: [], workingTreeChanges: [] },
    async getCommit(ref) {
      const [hash, parents, timestamp, message] = git(folder, "show", "-s", "--format=%H%x00%P%x00%ct%x00%B", ref).split("\0");
      return { hash, parents: parents ? parents.split(" ") : [], message, commitDate: new Date(Number(timestamp) * 1000) };
    },
    async getRefs(query) {
      return git(folder, "for-each-ref", "--format=%(refname)%00%(objectname)", `--count=${query.count}`, "refs/heads", "refs/remotes", "refs/tags").split("\n").filter(Boolean).map(line => {
        const [name, commit] = line.split("\0");
        return { type: name.startsWith("refs/tags/") ? 2 : name.startsWith("refs/remotes/") ? 1 : 0, name, commit };
      });
    },
  };
}
function service(folder, limits) {
  const repositories = [repository(folder)];
  const api = {
    git: { path: "git" }, repositories,
    getRepository(uri) { return repositories.find(value => uri.fsPath.startsWith(value.rootUri.fsPath)) ?? null; },
    async openRepository(uri) { const found = repository(uri.fsPath); repositories.push(found); return found; },
  };
  return { sources: new GitSources(api, fileUri, limits), repository: repositories[0] };
}
async function fixture(t) {
  const folder = await fs.mkdtemp(path.join(os.tmpdir(), "dygnosis-git-source-"));
  t.after(async () => {
    assert.equal(path.dirname(folder), path.resolve(os.tmpdir()));
    assert.match(path.basename(folder), /^dygnosis-git-source-/);
    await fs.rm(folder, { recursive: true, force: true });
  });
  git(folder, "init", "-b", "main");
  git(folder, "config", "user.name", "Dygnosis source tests");
  git(folder, "config", "user.email", "test@dygnosis.invalid");
  git(folder, "config", "core.autocrlf", "false");
  await fs.mkdir(path.join(folder, "includes"));
  await fs.writeFile(path.join(folder, "main.mod"), '@#include "includes/part.data"\r\nvar y;\r\n// 😀 café\r\n');
  await fs.writeFile(path.join(folder, "includes/part.data"), "model; y = 1; end;\n");
  await fs.writeFile(path.join(folder, "includes/Unused.data"), "unused body\n");
  await fs.writeFile(path.join(folder, "__proto__"), "a valid Git filename\n");
  await fs.writeFile(path.join(folder, "model with spaces.dyn"), "var x; model; x=1; end;\n");
  git(folder, "add", "."); git(folder, "commit", "-m", "initial models");
  const initial = git(folder, "rev-parse", "HEAD");
  git(folder, "tag", "-a", "release-one", "-m", "initial tag");
  await fs.writeFile(path.join(folder, "includes/part.data"), "model; y = 2; end;\n");
  git(folder, "add", "."); git(folder, "commit", "-m", "include-only change");
  const second = git(folder, "rev-parse", "HEAD");
  git(folder, "mv", "main.mod", "renamed.mod"); git(folder, "commit", "-m", "rename root");
  const renamed = git(folder, "rev-parse", "HEAD");
  return { folder, initial, second, renamed, ...service(folder) };
}

test("captures a pinned tree and only requested regular blobs, without current-file fallback", async t => {
  const { folder, initial, second, sources, repository: repo } = await fixture(t);
  await fs.writeFile(path.join(folder, "includes/part.data"), "current disk must never enter history\n");
  await fs.writeFile(path.join(folder, "current-only.data"), "absent at the selected revision\n");
  const status = git(folder, "status", "--porcelain=v1");
  const first = await sources.capture(repo, await sources.resolve(repo, "release-one"), "main.mod");
  assert.equal(first.commit.hash, initial); assert.equal(first.commit.requested_ref, "release-one");
  assert.equal(first.stats.blobReads, 1); assert.deepEqual(Object.keys(first.sources), ["main.mod"]);
  assert.equal(first.manifest["__proto__"].mode, "100644");
  assert.ok(first.manifest["includes/Unused.data"]); assert.equal(first.source("includes/Unused.data").code, "source_not_loaded");
  await first.load(["includes/part.data", "current-only.data", "includes/unused.data"]);
  assert.equal(first.stats.blobReads, 2); assert.equal(first.source("includes/part.data").text, "model; y = 1; end;\n");
  assert.equal(first.source("current-only.data").code, "missing_source"); assert.equal(first.source("includes/unused.data").code, "missing_source");
  const next = await sources.capture(repo, await sources.resolve(repo, second), "main.mod");
  await next.load(["includes/part.data"]);
  assert.equal(next.stats.cacheHits, 1); assert.equal(next.stats.blobReads, 1); assert.equal(next.source("includes/part.data").text, "model; y = 2; end;\n");
  assert.match(next.source("main.mod").text, /\r\n/); assert.match(next.source("main.mod").text, /😀 café/);
  const input = first.input("before:1", [".", "includes"]);
  assert.equal(input.kind, "git"); assert.equal(input.commit, initial); assert.equal(input.root_file, "main.mod");
  assert.deepEqual(input.search_paths, [".", "includes"]); assert.equal(input.sources["includes/part.data"].kind, "text");
  assert.equal(git(folder, "status", "--porcelain=v1"), status);
});

test("history pages retain fixed tips, include repository changes and resolve tags and revision expressions", async t => {
  const { folder, initial, second, renamed, repository: repo } = await fixture(t), { sources } = service(folder, { historyPage: 2 });
  const page = await sources.history(repo);
  assert.deepEqual(page.commits.map(commit => commit.hash), [renamed, second]); assert.ok(page.next);
  await fs.writeFile(path.join(folder, "new.data"), "new commit\n");
  git(folder, "add", "."); git(folder, "commit", "-m", "moved after first page");
  const remaining = await sources.history(repo, page.next);
  assert.deepEqual(remaining.commits.map(commit => commit.hash), [initial]); assert.equal(remaining.next, undefined);
  git(folder, "update-ref", "refs/remotes/origin/main", second);
  const refs = await sources.refs(repo);
  assert.deepEqual(refs.map(ref => ref.kind), ["branch", "remote", "tag"]);
  assert.equal(refs.find(ref => ref.kind === "tag").revision.hash, initial);
  assert.equal((await sources.resolve(repo, "HEAD~2")).hash, second);
  assert.equal((await sources.resolve(repo, "release-one^0")).hash, initial);
  await assert.rejects(sources.resolve(repo, "--help"), error => error.code === "invalid_revision");
  await assert.rejects(sources.resolve(repo, "no-local-revision"), error => error.code === "missing_object");
});

test("previous means HEAD for Working and first parent for a historical merge, including initial and detached states", async t => {
  const { folder, initial, second, renamed, sources, repository: repo } = await fixture(t);
  assert.equal((await sources.previous(repo)).revision.hash, renamed);
  assert.equal((await sources.previous(repo, second)).revision.hash, initial);
  await assert.rejects(sources.previous(repo, initial), error => error.code === "no_previous_revision");
  git(folder, "checkout", "-b", "side", initial);
  await fs.writeFile(path.join(folder, "side.data"), "side tree\n"); git(folder, "add", "."); git(folder, "commit", "-m", "side");
  const side = git(folder, "rev-parse", "HEAD");
  git(folder, "checkout", "main"); git(folder, "merge", "--no-ff", "side", "-m", "merge side");
  const merge = git(folder, "rev-parse", "HEAD"), previous = await sources.previous(repo, merge);
  assert.equal(previous.revision.hash, renamed); assert.match(previous.description, /First parent/);
  assert.equal((await sources.resolve(repo, `${merge}^2`)).hash, side);
  git(folder, "checkout", "--detach", second);
  assert.equal((await sources.previous(repo)).revision.hash, second);
  assert.equal((await sources.previous(repo, merge)).revision.hash, renamed);
});

test("root-path recovery proposes established rename without a filename heuristic", async t => {
  const { folder, second, renamed, sources, repository: repo } = await fixture(t);
  git(folder, "config", "diff.external", "dygnosis-this-command-must-never-run");
  assert.deepEqual(await sources.rename(repo, second, renamed, "renamed.mod"), { before: "main.mod", after: "renamed.mod" });
  assert.equal(await sources.rename(repo, second, renamed, "model with spaces.dyn"), undefined);
  const missing = await sources.capture(repo, await sources.resolve(repo, second), "renamed.mod");
  assert.equal(missing.source("renamed.mod").code, "missing_source");
  assert.deepEqual(missing.modelPaths(), ["main.mod", "model with spaces.dyn"]);
  assert.throws(() => sources.fileKey(repo, fileUri(path.join(folder, "..", "outside.mod"))), error => error.code === "unsupported_source");
});

test("historical provenance verifies old source at its commit with replaced-extension URIs and explicit unsupported forms", async t => {
  const { folder, initial, sources } = await fixture(t), modelPath = path.join(folder, "main.mod");
  const contents = childProcess.execFileSync("git", ["-C", folder, "show", `${initial}:main.mod`], { encoding: "utf8", windowsHide: true });
  for (const suffix of ["", ".git"]) {
    const uri = { scheme: "git", authority: "", path: `${fileUri(modelPath).path}${suffix}`, query: JSON.stringify({ path: modelPath, ref: initial }) };
    const result = await sources.provenance({ uri, getText: () => contents });
    assert.equal(result.commit.hash, initial); assert.equal(result.file_key, "main.mod");
    await assert.rejects(sources.provenance({ uri, getText: () => `${contents}// wrong\n` }), error => error.code === "source_revision_required");
  }
  for (const ref of ["", "~", ":0", ":1", ":2", ":3"]) assert.equal(decodeGitDocument({ scheme: "git", query: JSON.stringify({ path: modelPath, ref }) }).code, "unsupported_source");
  for (const ref of ["HEAD", "main", "release-one", "refs/heads/main"]) assert.equal(decodeGitDocument({ scheme: "git", query: JSON.stringify({ path: modelPath, ref }) }).code, "unknown_provenance");
  for (const query of ["broken", "[]", "null", JSON.stringify({ path: "relative.mod", ref: initial }), JSON.stringify({ path: modelPath, ref: initial, submoduleOf: folder })]) assert.equal(decodeGitDocument({ scheme: "git", query }).kind, "unavailable");
  assert.equal(decodeGitDocument({ scheme: "third-party", query: JSON.stringify({ path: modelPath, ref: initial }) }).kind, "unavailable");
  assert.equal(decodeGitDocument({ scheme: "git", query: JSON.stringify({ path: modelPath, ref: `${initial}^2~1` }) }).kind, "commit");
});

test("nested repositories and linked worktrees supply their own repository identity", async t => {
  const { folder, sources, second } = await fixture(t), nested = path.join(folder, "nested");
  await fs.mkdir(nested); git(nested, "init", "-b", "main");
  await fs.writeFile(path.join(nested, "main.mod"), "nested model\n");
  const selected = await sources.repositoryFor(fileUri(path.join(nested, "main.mod")));
  assert.equal(selected.rootUri.fsPath, nested);
  await assert.rejects(sources.previous(selected), error => error.code === "missing_object");
  const worktree = path.join(folder, "worktree");
  git(folder, "worktree", "add", "--detach", worktree, second);
  const linked = await sources.repositoryFor(fileUri(path.join(worktree, "main.mod")));
  assert.equal(linked.rootUri.fsPath, worktree); assert.equal((await sources.previous(linked)).revision.hash, second);
});

test("symlinks, gitlinks, missing local objects, and outside-tree sources remain failures", async t => {
  const { folder, initial, sources, repository: repo } = await fixture(t);
  const blob = git(folder, "rev-parse", `${initial}:includes/Unused.data`);
  git(folder, "update-index", "--add", "--cacheinfo", `120000,${blob},link`);
  git(folder, "update-index", "--add", "--cacheinfo", `160000,${initial},submodule`);
  git(folder, "commit", "-m", "unsupported dependencies");
  const capture = await sources.capture(repo, await sources.resolve(repo, "HEAD"), "renamed.mod");
  await capture.load(["link", "link/part.data", "submodule/part.data", "../outside.mod"]);
  for (const key of ["link", "link/part.data", "submodule/part.data", "../outside.mod"]) assert.equal(capture.source(key).code, "unsupported_source");
  assert.equal(capture.stats.blobReads, 1);
  const objectPath = path.join(folder, ".git", "objects", blob.slice(0, 2), blob.slice(2));
  await fs.unlink(objectPath);
  git(folder, "config", "remote.origin.promisor", "true");
  git(folder, "config", "remote.origin.url", path.join(folder, "never-fetch.git"));
  git(folder, "config", "extensions.partialClone", "origin");
  const nativeSpawn = childProcess.spawn, trace = path.join(folder, ".git", "no-fetch.trace");
  childProcess.spawn = (command, args, options) => nativeSpawn(command, args, { ...options, env: { ...options.env, GIT_TRACE: trace } });
  t.after(() => { childProcess.spawn = nativeSpawn; });
  await capture.load(["includes/Unused.data"]);
  assert.equal(capture.source("includes/Unused.data").code, "missing_object");
  assert.doesNotMatch(await fs.readFile(trace, "utf8"), /run_command:.*fetch/);
});

test("stash revision uses only its selected commit tree and cannot recover separate untracked files", async t => {
  const { folder, sources, repository: repo } = await fixture(t);
  await fs.writeFile(path.join(folder, "includes/part.data"), "stash tracked source\n");
  await fs.writeFile(path.join(folder, "untracked.mod"), "separate stash tree\n");
  git(folder, "stash", "push", "--include-untracked", "-m", "temporary source fixture");
  const capture = await sources.capture(repo, await sources.resolve(repo, "stash@{0}"), "renamed.mod");
  await capture.load(["includes/part.data", "untracked.mod"]);
  assert.equal(capture.source("includes/part.data").text, "stash tracked source\n");
  assert.equal(capture.source("untracked.mod").code, "missing_source");
});

test("cache retention, read bounds, and cancellation stop acquisition with explicit outcomes", async t => {
  const { folder, initial, second, repository: repo } = await fixture(t), { sources } = service(folder, { cacheBytes: 80, cacheManifests: 1, blobBytes: 80, captureBytes: 80 });
  const first = await sources.capture(repo, await sources.resolve(repo, initial), "main.mod");
  await first.load(["includes/part.data", "includes/Unused.data"]);
  assert.equal(first.source("includes/Unused.data").code, "capture_limit");
  const next = await sources.capture(repo, await sources.resolve(repo, second), "main.mod");
  assert.ok(sources.cacheStats().blobBytes <= 80); assert.ok(sources.cacheStats().manifestEntries <= 1);
  const abort = new globalThis.AbortController(); abort.abort();
  await assert.rejects(next.load(["includes/part.data"], abort.signal), error => error.code === "cancelled");
  assert.equal(next.source("includes/part.data").code, "source_not_loaded");
  const duringRead = new globalThis.AbortController();
  const pending = sources.resolve(repo, initial, duringRead.signal);
  duringRead.abort();
  await assert.rejects(pending, error => error.code === "cancelled");
  const small = service(folder, { blobBytes: 8 }).sources;
  const bounded = await small.capture(repo, await small.resolve(repo, initial), "main.mod");
  assert.equal(bounded.source("main.mod").code, "capture_limit");
  sources.clear(); assert.deepEqual(sources.cacheStats(), { blobEntries: 0, blobBytes: 0, manifestEntries: 0, manifestBytes: 0 });
});

test("an include-heavy tree loads requested bodies while retaining a bounded manifest", async t => {
  const { folder, sources, repository: repo } = await fixture(t);
  await fs.mkdir(path.join(folder, "large"));
  await Promise.all(Array.from({ length: 256 }, (_, index) => fs.writeFile(path.join(folder, "large", `source-${index}.data`), `var q${index};\n`)));
  git(folder, "add", "."); git(folder, "commit", "-m", "many potential source files");
  const capture = await sources.capture(repo, await sources.resolve(repo, "HEAD"), "renamed.mod");
  await capture.load(["includes/part.data", "large/source-200.data"]);
  assert.equal(capture.stats.manifestEntries, 261); assert.equal(capture.stats.blobReads, 3);
  assert.deepEqual(Object.keys(capture.sources), ["renamed.mod", "includes/part.data", "large/source-200.data"]);
  t.diagnostic(`Git capture: ${JSON.stringify(capture.stats)}`);
});

test("Git object commands disable lazy fetch and leave index, working tree and refs intact", async t => {
  const { folder, initial, sources, repository: repo } = await fixture(t), calls = [], nativeSpawn = childProcess.spawn;
  childProcess.spawn = (command, args, options) => { calls.push({ command, args, options }); return nativeSpawn(command, args, options); };
  t.after(() => { childProcess.spawn = nativeSpawn; });
  const status = git(folder, "status", "--porcelain=v1"), refs = git(folder, "show-ref"), index = await fs.readFile(path.join(folder, ".git", "index"));
  const capture = await sources.capture(repo, await sources.resolve(repo, initial), "main.mod"); await capture.load(["includes/part.data"]);
  assert.ok(calls.some(call => call.args.includes("ls-tree"))); assert.ok(calls.some(call => call.args.includes("cat-file")));
  for (const call of calls) {
    assert.equal(call.options.env.GIT_NO_LAZY_FETCH, "1"); assert.equal(call.options.env.GIT_TERMINAL_PROMPT, "0"); assert.equal(call.options.shell, false);
    assert.equal(call.args.some(arg => ["checkout", "restore", "stash", "fetch"].includes(arg)), false);
  }
  assert.equal(git(folder, "status", "--porcelain=v1"), status); assert.equal(git(folder, "show-ref"), refs); assert.deepEqual(await fs.readFile(path.join(folder, ".git", "index")), index);
});

test("NUL manifests and source decoding preserve exact Git keys and Rust text rules", () => {
  const oid = "1".repeat(40), key = "Folder/space\ttab\nline😀.data";
  const manifest = parseTreeManifest(Buffer.from(`100644 blob ${oid}\t${key}\0`));
  assert.equal(manifest[key].object_id, oid);
  assert.throws(() => parseTreeManifest(Buffer.from(`100644 blob ${oid}\t${key}`)), GitSourceError);
  assert.throws(() => parseTreeManifest(Buffer.from(`100644 blob ${oid}\t../outside.mod\0`)), GitSourceError);
  assert.throws(() => parseTreeManifest(Buffer.from(`100644 blob ${oid}\ta\0`), 0), error => error.code === "capture_limit");
  assert.equal(normalizeGitText("a\r\nb\rc"), "a\r\nb\nc");
  assert.equal(decodeGitText(Buffer.from("😀 café\r\n")), "😀 café\r\n");
  assert.equal(decodeGitText(Buffer.from([0xe9, 0x0d, 0x41])), "é\nA");
});
