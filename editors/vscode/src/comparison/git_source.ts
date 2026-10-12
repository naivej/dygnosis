import { spawn } from "node:child_process";
import { stat } from "node:fs/promises";
import * as path from "node:path";
import type * as vscode from "vscode";
import { decodeGitDocument, decodeGitText, normalizeGitText } from "./git_provenance";

// This is the read-only subset of vscode.git API version 1, available since VS Code 1.102.
export interface GitCommit { hash: string; message: string; parents: string[]; authorDate?: Date; commitDate?: Date }
export interface GitRef { type: number; name?: string; commit?: string; remote?: string }
export interface GitChange { uri: vscode.Uri; originalUri: vscode.Uri; renameUri?: vscode.Uri }
export interface GitRepository {
  rootUri: vscode.Uri;
  state: { refs: GitRef[]; indexChanges: GitChange[]; workingTreeChanges: GitChange[] };
  getCommit(ref: string): Promise<GitCommit>;
  getRefs(query: { count?: number; sort?: "alphabetically" }): Promise<GitRef[]>;
}
export interface GitAPI {
  git: { path: string };
  repositories: GitRepository[];
  getRepository(uri: vscode.Uri): GitRepository | null;
  openRepository(root: vscode.Uri): Promise<GitRepository | null>;
}
export interface PinnedCommit extends GitCommit { requested_ref: string }
export interface HistoryCursor { commits: string[]; offset: number }
export interface HistoryPage { commits: PinnedCommit[]; next?: HistoryCursor }
export interface RevisionRef { kind: "branch" | "remote" | "tag"; name: string; revision: PinnedCommit }
export interface PreviousRevision { revision: PinnedCommit; description: string }
export interface TreeEntry { mode: "100644" | "100755" | "120000" | "160000"; object_id: string }
export type HistoricalSource = { kind: "text"; text: string } | { kind: "failure"; code: string; message: string };
export interface HistoricalInput {
  kind: "git"; input_id: string; root_file: string; repository_uri: string; commit: string; requested_ref: string;
  search_paths: string[]; manifest: Record<string, TreeEntry>; sources: Record<string, HistoricalSource>;
}
export interface CaptureStats { manifestEntries: number; blobReads: number; blobBytes: number; cacheHits: number; durationMs: number }
export interface GitLimits {
  historyPage: number; refs: number; manifestEntries: number; manifestBytes: number; blobBytes: number;
  captureBytes: number; captureSources: number; cacheBytes: number; cacheManifests: number; timeoutMs: number;
}
const defaults: GitLimits = {
  historyPage: 32, refs: 1024, manifestEntries: 100_000, manifestBytes: 32 * 1024 * 1024,
  blobBytes: 8 * 1024 * 1024, captureBytes: 32 * 1024 * 1024, captureSources: 4096,
  cacheBytes: 32 * 1024 * 1024, cacheManifests: 4, timeoutMs: 30_000,
};

export class GitSourceError extends Error {
  constructor(public readonly code: string, message: string, public readonly file_key?: string) { super(message); this.name = "GitSourceError"; }
}
function cancelled(signal?: AbortSignal): void {
  if (signal?.aborted) throw new GitSourceError("cancelled", "Source capture was cancelled.");
}
function fullCommit(value: string): boolean { return /^(?:[a-f0-9]{40}|[a-f0-9]{64})$/.test(value); }
function revisionExpression(value: string): string {
  if (!value || value.length > 1024 || value.startsWith("-") || /[\0\r\n]/.test(value)) throw new GitSourceError("invalid_revision", "Enter a valid Git revision.");
  return value;
}
function exactFileKey(value: string): boolean {
  return value.length > 0 && !value.startsWith("/") && !/[\0\\]/.test(value) && !value.split("/").some(part => part === "" || part === "." || part === "..");
}
function failure(error: unknown, fileKey: string): HistoricalSource {
  if (error instanceof GitSourceError) return { kind: "failure", code: error.code, message: error.message };
  return { kind: "failure", code: "source_read_failure", message: `Cannot read historical source ${fileKey}.` };
}
function requireCommit(value: string): void {
  if (!fullCommit(value)) throw new GitSourceError("invalid_revision", "A historical source requires a full, fixed commit ID.");
}

/** NUL records preserve paths containing spaces, tabs, newlines and non-ASCII characters. */
export function parseTreeManifest(bytes: Buffer, entryLimit = defaults.manifestEntries): Record<string, TreeEntry> {
  const manifest: Record<string, TreeEntry> = Object.create(null) as Record<string, TreeEntry>;
  if (bytes.length > 0 && bytes.at(-1) !== 0) throw new GitSourceError("invalid_manifest", "Git returned an incomplete tree manifest.");
  const decoder = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true });
  let start = 0, entries = 0;
  for (let end = bytes.indexOf(0); end >= 0; end = bytes.indexOf(0, start)) {
    const record = bytes.subarray(start, end), tab = record.indexOf(9);
    const header = record.subarray(0, tab).toString("ascii");
    const match = /^(100644|100755|120000|160000) (blob|commit) ([a-f0-9]{40}|[a-f0-9]{64})$/.exec(header);
    let key: string;
    try { key = decoder.decode(record.subarray(tab + 1)); } catch { throw new GitSourceError("invalid_manifest", "A Git tree path is not valid UTF-8."); }
    if (!match || tab < 0 || !exactFileKey(key) || Object.hasOwn(manifest, key) || (match[1] === "160000") !== (match[2] === "commit")) throw new GitSourceError("invalid_manifest", "Git returned an invalid tree manifest.");
    if (++entries > entryLimit) throw new GitSourceError("capture_limit", `The Git tree exceeds ${entryLimit} sources.`);
    manifest[key] = { mode: match[1] as TreeEntry["mode"], object_id: match[3] };
    start = end + 1;
  }
  return manifest;
}

/** Repository reads are local-only. This class does not interpret model source text. */
export class GitSources {
  readonly limits: GitLimits;
  private readonly manifests = new Map<string, { value: Record<string, TreeEntry>; bytes: number }>();
  private readonly blobs = new Map<string, { text: string; bytes: number }>();
  private cacheSize = 0;
  private manifestSize = 0;

  constructor(private readonly api: GitAPI, private readonly fileUri: (file: string) => vscode.Uri, limits: Partial<GitLimits> = {}) {
    this.limits = { ...defaults, ...limits };
    for (const limit of Object.values(this.limits)) if (!Number.isSafeInteger(limit) || limit < 1) throw new Error("Git source limits must be positive integers.");
    if (!api.git?.path) throw new GitSourceError("git_unavailable", "The built-in Git extension has no Git executable.");
  }

  private run(folder: string, args: string[], maxBytes: number, signal?: AbortSignal): Promise<Buffer> {
    cancelled(signal);
    return new Promise((resolve, reject) => {
      const child = spawn(this.api.git.path, ["-C", folder, ...args], {
        shell: false, windowsHide: true, stdio: ["ignore", "pipe", "pipe"],
        env: { ...process.env, GIT_NO_LAZY_FETCH: "1", GIT_TERMINAL_PROMPT: "0", GIT_OPTIONAL_LOCKS: "0" },
      });
      const chunks: Buffer[] = [];
      let bytes = 0, stderr = "", done = false, pendingError: Error | undefined;
      const finish = (error?: Error, output?: Buffer): void => {
        if (done) return;
        done = true; clearTimeout(timer); signal?.removeEventListener("abort", abort);
        if (error) reject(error); else resolve(output ?? Buffer.alloc(0));
      };
      const fail = (error: Error): void => { if (!pendingError && !done) { pendingError = error; child.kill(); } };
      const abort = (): void => fail(new GitSourceError("cancelled", "Source capture was cancelled."));
      const timer = setTimeout(() => fail(new GitSourceError("source_timeout", "The local Git read timed out.")), this.limits.timeoutMs);
      signal?.addEventListener("abort", abort, { once: true });
      child.on("error", () => fail(new GitSourceError("git_unavailable", "Cannot start the Git executable from the built-in Git extension.")));
      child.stdout.on("data", (data: Buffer) => {
        if (done || pendingError) return;
        bytes += data.length;
        if (bytes > maxBytes) fail(new GitSourceError("capture_limit", `The local Git read exceeds ${maxBytes} bytes.`)); else chunks.push(data);
      });
      child.stderr.on("data", (data: Buffer) => { if (stderr.length < 4096) stderr += data.toString("utf8").slice(0, 4096 - stderr.length); });
      child.on("close", code => {
        if (done) return;
        if (pendingError) finish(pendingError);
        else if (code !== 0) finish(new GitSourceError("missing_object", `The requested Git revision or object is unavailable locally.${stderr.trim() ? ` ${stderr.trim()}` : ""}`));
        else finish(undefined, Buffer.concat(chunks, bytes));
      });
    });
  }

  async repositoryFor(uri: vscode.Uri, signal?: AbortSignal): Promise<GitRepository> {
    if (uri.scheme !== "file") throw new GitSourceError("repository_unavailable", "This source has no local repository history.");
    let folder = uri.fsPath;
    for (;;) {
      try { if (!(await stat(folder)).isDirectory()) folder = path.dirname(folder); else break; }
      catch { const parent = path.dirname(folder); if (parent === folder) throw new GitSourceError("repository_unavailable", "No repository contains this model source."); folder = parent; }
    }
    let root: string;
    try { root = (await this.run(folder, ["rev-parse", "--show-toplevel"], 32 * 1024, signal)).toString("utf8").trim(); }
    catch (error) { if (error instanceof GitSourceError && error.code === "cancelled") throw error; throw new GitSourceError("repository_unavailable", "No locally available repository contains this model source."); }
    const rootUri = this.fileUri(root);
    let repository = this.api.repositories.find(item => path.resolve(item.rootUri.fsPath) === path.resolve(root));
    repository ??= (await this.api.openRepository(rootUri)) ?? undefined;
    cancelled(signal);
    if (!repository || repository.rootUri.scheme !== "file") throw new GitSourceError("repository_unavailable", "The built-in Git extension cannot open this repository.");
    return repository;
  }

  fileKey(repository: GitRepository, uri: vscode.Uri): string {
    if (uri.scheme !== "file") throw new GitSourceError("unsupported_source", "The source path is not a repository file.");
    const relative = path.relative(repository.rootUri.fsPath, uri.fsPath).split(path.sep).join("/");
    if (!exactFileKey(relative)) throw new GitSourceError("unsupported_source", "The source path leaves the selected repository.", relative);
    return relative;
  }

  async resolve(repository: GitRepository, requestedRef: string, signal?: AbortSignal): Promise<PinnedCommit> {
    const ref = revisionExpression(requestedRef);
    const hash = (await this.run(repository.rootUri.fsPath, ["rev-parse", "--verify", "--end-of-options", `${ref}^{commit}`], 1024, signal)).toString("ascii").trim();
    requireCommit(hash);
    // The API's metadata read can only access the already verified local commit object.
    await this.run(repository.rootUri.fsPath, ["cat-file", "-e", `${hash}^{commit}`], 1024, signal);
    const value = await repository.getCommit(hash);
    cancelled(signal);
    if (value.hash.toLowerCase() !== hash || !Array.isArray(value.parents) || value.parents.some(parent => !fullCommit(parent.toLowerCase()))) throw new GitSourceError("invalid_revision", "The Git extension returned an invalid commit identity.");
    return { ...value, hash, parents: value.parents.map(parent => parent.toLowerCase()), requested_ref: requestedRef };
  }

  async previous(repository: GitRepository, anchorCommit?: string, signal?: AbortSignal): Promise<PreviousRevision> {
    if (!anchorCommit) {
      const revision = await this.resolve(repository, "HEAD", signal);
      return { revision, description: `Last committed model · ${revision.hash.slice(0, 7)}` };
    }
    requireCommit(anchorCommit);
    const anchor = await this.resolve(repository, anchorCommit, signal);
    if (anchor.parents.length === 0) throw new GitSourceError("no_previous_revision", "The opened revision is an initial commit and has no parent.");
    const revision = await this.resolve(repository, anchor.parents[0], signal);
    return { revision, description: `${anchor.parents.length > 1 ? "First parent" : "Parent"} of ${anchor.hash.slice(0, 7)} · ${revision.hash.slice(0, 7)}` };
  }

  async history(repository: GitRepository, cursor?: HistoryCursor, signal?: AbortSignal): Promise<HistoryPage> {
    cancelled(signal);
    // Pin each page to the initial tips so new commits cannot shift offsets between pages.
    const commits = cursor?.commits ?? (await this.refTips(repository, signal));
    if (commits.length === 0) return { commits: [] };
    const offset = cursor?.offset ?? 0;
    if (!Number.isSafeInteger(offset) || offset < 0 || offset > 100_000 || commits.some(commit => !fullCommit(commit))) throw new GitSourceError("invalid_revision", "The history cursor is invalid.");
    const output = await this.run(repository.rootUri.fsPath, ["log", "--date-order", "--format=%H", `--max-count=${this.limits.historyPage + 1}`, `--skip=${offset}`, ...commits, "--"], 16 * 1024, signal);
    const hashes = output.toString("ascii").trim().split("\n").filter(Boolean);
    const page: PinnedCommit[] = [];
    for (const hash of hashes.slice(0, this.limits.historyPage)) page.push(await this.resolve(repository, hash, signal));
    return { commits: page, ...(hashes.length > this.limits.historyPage ? { next: { commits, offset: offset + page.length } } : {}) };
  }

  private async refTips(repository: GitRepository, signal?: AbortSignal): Promise<string[]> {
    const values = await repository.getRefs({ count: this.limits.refs, sort: "alphabetically" });
    cancelled(signal);
    const commits = new Set<string>();
    try { commits.add((await this.resolve(repository, "HEAD", signal)).hash); }
    catch (error) { if (error instanceof GitSourceError && error.code === "cancelled") throw error; }
    for (const value of values.slice(0, this.limits.refs)) {
      if (!value.name || ![0, 1, 2].includes(value.type)) continue;
      // API refs already carry object IDs; an annotated tag tip also pins its target ancestry.
      if (value.commit && fullCommit(value.commit)) { commits.add(value.commit); continue; }
      try { commits.add((await this.resolve(repository, refName(value), signal)).hash); }
      catch (error) { if (error instanceof GitSourceError && error.code === "cancelled") throw error; }
    }
    return [...commits];
  }

  async refs(repository: GitRepository, signal?: AbortSignal): Promise<RevisionRef[]> {
    const refs = await repository.getRefs({ count: this.limits.refs, sort: "alphabetically" });
    cancelled(signal);
    const result: RevisionRef[] = [];
    for (const ref of refs.slice(0, this.limits.refs)) {
      if (!ref.name || ![0, 1, 2].includes(ref.type)) continue;
      try { result.push({ kind: ref.type === 2 ? "tag" : ref.type === 1 ? "remote" : "branch", name: ref.name, revision: await this.resolve(repository, refName(ref), signal) }); }
      catch (error) { if (error instanceof GitSourceError && error.code === "cancelled") throw error; }
    }
    return result.sort((a, b) => ["branch", "remote", "tag"].indexOf(a.kind) - ["branch", "remote", "tag"].indexOf(b.kind) || a.name.localeCompare(b.name));
  }

  async capture(repository: GitRepository, commit: PinnedCommit, rootFile: string, signal?: AbortSignal): Promise<HistoricalCapture> {
    requireCommit(commit.hash);
    if (!exactFileKey(rootFile)) throw new GitSourceError("unsupported_source", "The root path leaves the selected repository.", rootFile);
    const started = Date.now(), key = `${repository.rootUri.toString()}\0${commit.hash}`;
    let cached = this.manifests.get(key);
    if (cached) { this.manifests.delete(key); this.manifests.set(key, cached); }
    else {
      const bytes = await this.run(repository.rootUri.fsPath, ["ls-tree", "-r", "-z", "--full-tree", commit.hash], this.limits.manifestBytes, signal);
      cached = { value: parseTreeManifest(bytes, this.limits.manifestEntries), bytes: bytes.length };
      while (this.manifests.size >= this.limits.cacheManifests || this.manifestSize + cached.bytes > this.limits.cacheBytes) {
        const first = this.manifests.keys().next().value;
        if (first === undefined) break;
        this.manifestSize -= this.manifests.get(first)?.bytes ?? 0; this.manifests.delete(first);
      }
      if (cached.bytes <= this.limits.cacheBytes) { this.manifests.set(key, cached); this.manifestSize += cached.bytes; }
    }
    cancelled(signal);
    const capture = new HistoricalCapture(this, repository, commit, rootFile, cached.value, started);
    await capture.load([rootFile], signal);
    return capture;
  }

  async readBlob(repository: GitRepository, entry: TreeEntry, stats: CaptureStats, signal?: AbortSignal): Promise<{ text: string; bytes: number }> {
    if (!["100644", "100755"].includes(entry.mode) || !fullCommit(entry.object_id)) throw new GitSourceError("unsupported_source", "Only regular-file Git blobs can supply historical source text.");
    const key = `${repository.rootUri.toString()}\0${entry.object_id}`;
    const cached = this.blobs.get(key);
    if (cached) { cancelled(signal); this.blobs.delete(key); this.blobs.set(key, cached); stats.cacheHits++; return cached; }
    const output = await this.run(repository.rootUri.fsPath, ["cat-file", "blob", entry.object_id], this.limits.blobBytes, signal);
    stats.blobReads++; stats.blobBytes += output.length;
    const text = decodeGitText(output);
    const value = { text, bytes: output.length };
    while (this.blobs.size >= this.limits.captureSources || this.cacheSize + value.bytes > this.limits.cacheBytes) {
      const first = this.blobs.keys().next().value;
      if (first === undefined) break;
      this.cacheSize -= this.blobs.get(first)?.bytes ?? 0; this.blobs.delete(first);
    }
    if (value.bytes <= this.limits.cacheBytes) { this.blobs.set(key, value); this.cacheSize += value.bytes; }
    return value;
  }

  cacheStats(): { blobEntries: number; blobBytes: number; manifestEntries: number; manifestBytes: number } {
    return { blobEntries: this.blobs.size, blobBytes: this.cacheSize, manifestEntries: this.manifests.size, manifestBytes: this.manifestSize };
  }
  clear(): void { this.blobs.clear(); this.manifests.clear(); this.cacheSize = 0; this.manifestSize = 0; }

  async provenance(document: Pick<vscode.TextDocument, "uri" | "getText">, signal?: AbortSignal): Promise<{ repository: GitRepository; commit: PinnedCommit; file_key: string; capture: HistoricalCapture }> {
    const decoded = decodeGitDocument(document.uri);
    if (decoded.kind === "unavailable") throw new GitSourceError(decoded.code, decoded.message);
    const uri = this.fileUri(decoded.path), repository = await this.repositoryFor(uri, signal);
    const commit = await this.resolve(repository, decoded.ref, signal), file_key = this.fileKey(repository, uri);
    const capture = await this.capture(repository, commit, file_key, signal), source = capture.source(file_key);
    if (source.kind === "failure") throw new GitSourceError(source.code, source.message, file_key);
    if (normalizeGitText(document.getText()) !== source.text) throw new GitSourceError("source_revision_required", "The displayed document differs from the resolved commit. Select its source revision.", file_key);
    return { repository, commit, file_key, capture };
  }

  /** Return only a Git-established rename. The caller must ask before accepting it. */
  async rename(repository: GitRepository, beforeCommit: string, afterCommit: string | undefined, afterFile: string, signal?: AbortSignal): Promise<{ before: string; after: string } | undefined> {
    requireCommit(beforeCommit);
    if (!exactFileKey(afterFile)) return undefined;
    if (afterCommit) requireCommit(afterCommit);
    if (!afterCommit) {
      for (const change of [...repository.state.indexChanges, ...repository.state.workingTreeChanges]) {
        if (change.renameUri && this.fileKey(repository, change.renameUri) === afterFile) return { before: this.fileKey(repository, change.originalUri), after: afterFile };
      }
    }
    const args = afterCommit ? ["diff-tree", "-r", "--no-commit-id", "--name-status", "-z", "--no-ext-diff", "--no-textconv", "-M", beforeCommit, afterCommit, "--"] : ["diff", "--name-status", "-z", "--no-ext-diff", "--no-textconv", "-M", beforeCommit, "--"];
    const records = (await this.run(repository.rootUri.fsPath, args, this.limits.manifestBytes, signal)).toString("utf8").split("\0");
    for (let index = 0; index < records.length;) {
      const status = records[index++];
      if (!status) break;
      const oldPath = records[index++];
      if (/^[RC]\d+$/.test(status)) {
        const newPath = records[index++];
        if (status.startsWith("R") && newPath === afterFile && exactFileKey(oldPath)) return { before: oldPath, after: newPath };
      }
    }
    return undefined;
  }
}

function refName(ref: GitRef): string {
  const name = ref.name ?? "";
  if (name.startsWith("refs/")) return name;
  return `${ref.type === 2 ? "refs/tags/" : ref.type === 1 ? "refs/remotes/" : "refs/heads/"}${name}`;
}

export class HistoricalCapture {
  readonly sources: Record<string, HistoricalSource> = Object.create(null) as Record<string, HistoricalSource>;
  readonly stats: CaptureStats;
  private bytes = 0;
  constructor(private readonly owner: GitSources, readonly repository: GitRepository, readonly commit: PinnedCommit, readonly rootFile: string, readonly manifest: Record<string, TreeEntry>, private readonly started: number) {
    this.stats = { manifestEntries: Object.keys(manifest).length, blobReads: 0, blobBytes: 0, cacheHits: 0, durationMs: Date.now() - started };
  }
  input(inputId: string, searchPaths: string[]): HistoricalInput {
    return { kind: "git", input_id: inputId, root_file: this.rootFile, repository_uri: this.repository.rootUri.toString(), commit: this.commit.hash, requested_ref: this.commit.requested_ref, search_paths: [...searchPaths], manifest: this.manifest, sources: { ...this.sources } };
  }
  source(key: string): HistoricalSource {
    return this.sources[key] ?? { kind: "failure", code: "source_not_loaded", message: `Historical source ${key} has not been captured.` };
  }
  modelPaths(): string[] { return Object.keys(this.manifest).filter(key => /\.(mod|dyn)$/i.test(key) && ["100644", "100755"].includes(this.manifest[key].mode)); }

  async load(keys: string[], signal?: AbortSignal): Promise<void> {
    for (const key of new Set(keys)) {
      cancelled(signal);
      if (Object.hasOwn(this.sources, key)) continue;
      if (Object.keys(this.sources).length >= this.owner.limits.captureSources) throw new GitSourceError("capture_limit", `Historical capture exceeds ${this.owner.limits.captureSources} sources.`, key);
      let source: HistoricalSource;
      if (!exactFileKey(key)) source = { kind: "failure", code: "unsupported_source", message: `Historical source ${key} leaves the selected repository.` };
      else {
        const entry = this.manifest[key];
        const ancestor = key.split("/").slice(0, -1).map((_, index, parts) => parts.slice(0, index + 1).join("/")).find(candidate => ["120000", "160000"].includes(this.manifest[candidate]?.mode));
        if (ancestor || entry?.mode === "120000" || entry?.mode === "160000") {
          const mode = this.manifest[ancestor ?? key].mode;
          source = { kind: "failure", code: "unsupported_source", message: `Historical source ${key} ${mode === "160000" ? "crosses a submodule" : "uses a symbolic link"}.` };
        } else if (!entry) source = { kind: "failure", code: "missing_source", message: `Historical source ${key} is absent at ${this.commit.hash.slice(0, 7)}.` };
        else {
          try {
            const blob = await this.owner.readBlob(this.repository, entry, this.stats, signal);
            if (this.bytes + blob.bytes > this.owner.limits.captureBytes) throw new GitSourceError("capture_limit", `Historical capture exceeds ${this.owner.limits.captureBytes} bytes.`, key);
            this.bytes += blob.bytes; source = { kind: "text", text: blob.text };
          } catch (error) {
            if (error instanceof GitSourceError && error.code === "cancelled") throw error;
            source = failure(error, key);
          }
        }
      }
      this.sources[key] = source;
      this.stats.durationMs = Date.now() - this.started;
    }
  }
}
