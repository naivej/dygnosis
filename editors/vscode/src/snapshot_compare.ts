import * as vscode from "vscode";
import * as path from "node:path";
import { randomBytes } from "node:crypto";
import { DygnosisClient } from "./client";
import { ComparisonResource, ModelSelector } from "./changes_resource";
import { GitSources, HistoricalCapture, HistoricalInput } from "./git_source";
import { DiffSide, DiffSnapshot, parseDiff, parseSnapshotDiff } from "./diff_view";
import { HistoricalSources } from "./history_sources";
import { engineSettings } from "./settings";
import { record } from "./protocol";
import { normalizeGitText } from "./git_provenance";
import { semanticCapability } from "./semantic_view";

interface WorkingInput { kind: "working"; input_id: string; root_uri: string; expected_revision: string; search_paths?: string[] }
export interface SnapshotEnvelope {
  input_id: string; kind: "working" | "git"; root_file: string; revision: string; complete: boolean;
  file_keys: string[]; dependency_candidates: string[]; search_paths: string[];
  commit?: string;
}
export interface CapturedComparison {
  snapshot: DiffSnapshot; inputs: Record<DiffSide, SnapshotEnvelope>; sourceUris: Record<DiffSide, Map<string, vscode.Uri>>;
  texts: Record<DiffSide, Record<string, string>>; working: { root: vscode.Uri; expected: string }[]; holder: object;
  legacy?: boolean;
  instance: number;
}
export class ComparisonFailure extends Error {
  constructor(readonly code: string, message: string, readonly side?: DiffSide, readonly fileKey?: string) { super(message); }
}
function assertActive(token: vscode.CancellationToken): void { if (token.isCancellationRequested) throw new ComparisonFailure("CANCELLED", "Comparison capture was cancelled."); }
export function configuredSearchPaths(context: vscode.Uri, service: DygnosisClient): string[] {
  const folder = vscode.workspace.getWorkspaceFolder(context);
  const base = folder?.uri.fsPath ?? path.dirname(context.fsPath);
  return [...new Set(engineSettings(folder?.uri, service.log).searchPaths.map(value => path.resolve(base, value)))];
}
function envelope(value: unknown, input: WorkingInput | HistoricalInput): SnapshotEnvelope {
  const strings = (value: unknown): value is string[] => Array.isArray(value) && value.every(item => typeof item === "string");
  if (!record(value) || value.input_id !== input.input_id || value.kind !== input.kind ||
      typeof value.root_file !== "string" || typeof value.revision !== "string" || value.complete !== true ||
      !strings(value.file_keys) || !strings(value.dependency_candidates) || !strings(value.search_paths)) throw new Error("The engine returned unsupported snapshot inputs. Use the bundled binary.");
  if (!value.file_keys.includes(value.root_file) || new Set(value.file_keys).size !== value.file_keys.length ||
      (input.kind === "git" ? value.root_file !== input.root_file || value.repository_uri !== input.repository_uri ||
        value.commit !== input.commit || value.requested_ref !== input.requested_ref || value.source_policy !== "git_tree" :
        value.root_uri !== input.root_uri || value.expected_revision !== input.expected_revision || value.source_policy !== "editor_buffers_and_disk"))
    throw new Error("The engine returned a different snapshot identity. Refresh with the bundled binary.");
  return value as unknown as SnapshotEnvelope;
}
function sourceTexts(value: unknown): Record<string, string> {
  if (!record(value) || Object.values(value).some(text => typeof text !== "string")) throw new Error("The engine returned invalid captured source text.");
  return value as Record<string, string>;
}

/** Hosts acquire bytes; only Rust chooses include candidates and executes macros. */
export async function captureComparison(service: DygnosisClient, git: () => Promise<GitSources>, history: HistoricalSources,
  resource: ComparisonResource, token: vscode.CancellationToken): Promise<CapturedComparison> {
  await service.ensureStarted(); assertActive(token);
  const experimental: unknown = service.client?.initializeResult?.capabilities.experimental;
  const supported = record(experimental) && record(experimental.dygnosis) && record(experimental.dygnosis.compareModelSnapshots) &&
    experimental.dygnosis.compareModelSnapshots.schema_version === 1 && experimental.dygnosis.compareModelSnapshots.navigation_schema_version === 2;
  const capability = record(experimental) && record(experimental.dygnosis) ? experimental.dygnosis.compareModelSnapshots : undefined;
  if (record(capability) && !supported) throw new Error("This engine advertises an unsupported comparison. Update dynare.serverPath or use the bundled binary.");
  const semantic = semanticCapability(capability);
  if (!supported) {
    if (resource.before.kind === "working" && resource.after.kind === "working") return captureLegacy(service, resource, token);
    throw new Error("This engine does not support history comparison. Update dynare.serverPath or use the bundled binary.");
  }
  const abort = new AbortController(), cancellation = token.onCancellationRequested(() => abort.abort());
  const captures = new Map<DiffSide, HistoricalCapture>(), working: CapturedComparison["working"] = [];
  const ids = { before: randomBytes(16).toString("hex"), after: randomBytes(16).toString("hex") };
  const selectors = { before: resource.before, after: resource.after };
  const withHistory = resource.before.kind === "git" || resource.after.kind === "git";
  const searchPaths = withHistory ? configuredSearchPaths(vscode.Uri.parse(resource.context_uri), service) : undefined;
  const instance = service.currentInstance;
  const prepare = async (side: DiffSide, selector: ModelSelector): Promise<WorkingInput | HistoricalInput> => {
    assertActive(token);
    if (selector.kind === "working") {
      const root = vscode.Uri.parse(selector.root_uri), info = await service.modelInfo(root, root, true, token);
      if (!info) throw new ComparisonFailure("ROOT_NOT_FOUND", `${side === "before" ? "Before" : "After"} model could not be captured: ${selector.root_uri}`, side);
      working.push({ root, expected: info.revision });
      return { kind: "working", input_id: ids[side], root_uri: selector.root_uri, expected_revision: info.revision, ...(searchPaths ? { search_paths: searchPaths } : {}) };
    }
    const sources = await git(), repository = await sources.repositoryFor(vscode.Uri.parse(selector.repository_uri), abort.signal);
    if (repository.rootUri.toString() !== selector.repository_uri) throw new ComparisonFailure("REPOSITORY_CHANGED", "The selected repository is unavailable. Choose its source again.", side);
    const commit = await sources.resolve(repository, selector.commit, abort.signal);
    commit.requested_ref = selector.requested_ref;
    const capture = await sources.capture(repository, commit, selector.root_file, abort.signal); captures.set(side, capture);
    const folders = (searchPaths ?? []).map(folder => {
      const relative = path.relative(repository.rootUri.fsPath, folder).split(path.sep).join("/");
      // Keep an external folder explicit; the engine must refuse it if an active include needs it.
      return relative === "" ? "." : relative.startsWith("../") || path.isAbsolute(relative) ? folder.split(path.sep).join("/") : relative;
    });
    return capture.input(ids[side], folders);
  };
  let holder: object | undefined;
  try {
    const inputs = { before: await prepare("before", resource.before), after: await prepare("after", resource.after) };
    for (let round = 0; round < 256; ++round) {
      assertActive(token);
      if (service.currentInstance !== instance) throw new ComparisonFailure("ENGINE_RESTARTED", "The engine restarted. Refresh this comparison.");
      const result = await service.execute("dynare/compareModelSnapshots", [{ schema_version: 1, ...inputs }], token); assertActive(token);
      if (!record(result)) throw new Error("The engine returned an invalid snapshot response.");
      if (result.state === "failure") {
        const side = result.side === "before" || result.side === "after" ? result.side : undefined;
        throw new ComparisonFailure(String(result.code), `${side ? `${side === "before" ? "Before" : "After"}: ` : ""}${String(result.message)}${typeof result.file_key === "string" ? ` (${result.file_key})` : ""}`, side, typeof result.file_key === "string" ? result.file_key : undefined);
      }
      if (result.state === "needs_sources") {
        if (!Array.isArray(result.requests) || !result.requests.length) throw new Error("Historical capture made no progress.");
        let progress = false;
        for (const request of result.requests) {
          if (!record(request) || (request.side !== "before" && request.side !== "after") || request.input_id !== ids[request.side] ||
              !Array.isArray(request.file_keys) || !request.file_keys.length || request.file_keys.some(key => typeof key !== "string")) throw new Error("The engine requested invalid historical sources.");
          const side = request.side, capture = captures.get(side), keys = request.file_keys as string[];
          if (!capture || keys.some(key => !Object.hasOwn(capture.manifest, key))) throw new Error("The engine requested a source outside its captured tree.");
          const count = Object.keys(capture.sources).length;
          await capture.load(keys, abort.signal);
          progress ||= Object.keys(capture.sources).length > count;
          inputs[side] = capture.input(ids[side], (inputs[side] as HistoricalInput).search_paths);
        }
        if (!progress) throw new Error("Historical capture made no progress. Refresh or choose another revision.");
        continue;
      }
      if (result.state !== "result" || !record(result.inputs) || result.inputs.schema_version !== 1 || !record(result.sources)) throw new Error("The engine returned an unsupported snapshot response.");
      const verified = await Promise.all(working.map(input => service.revalidate(input.root, input.expected, instance, input.root, token)));
      if (verified.some(info => !info) || service.currentInstance !== instance) throw new ComparisonFailure("INPUT_CHANGED", "The Working model changed during capture. Refresh the comparison.");
      const envelopes = { before: envelope(result.inputs.before, inputs.before), after: envelope(result.inputs.after, inputs.after) };
      const texts = { before: sourceTexts(result.sources.before), after: sourceTexts(result.sources.after) };
      const sourceUris = { before: new Map<string, vscode.Uri>(), after: new Map<string, vscode.Uri>() }; holder = {};
      for (const side of ["before", "after"] as const) {
        if (envelopes[side].file_keys.length !== Object.keys(texts[side]).length || envelopes[side].file_keys.some(key => !Object.hasOwn(texts[side], key))) throw new Error("The captured source list is incomplete.");
        const input = selectors[side];
        if (input.kind === "git") for (const [key, text] of Object.entries(texts[side])) {
          const source = captures.get(side)!.source(key);
          if (source.kind !== "text" || normalizeGitText(source.text) !== normalizeGitText(text)) throw new Error("The returned historical text differs from its captured Git source.");
        }
        sourceUris[side] = input.kind === "git" ? history.retain(holder, input, texts[side]) :
          new Map(Object.keys(texts[side]).map(key => [key, /^(file|untitled):/.test(key) ? vscode.Uri.parse(key) : vscode.Uri.file(key)]));
      }
      const snapshot = parseSnapshotDiff(result, ids, (side, key) => {
        const uri = sourceUris[side].get(key);
        if (!uri) throw new Error("The engine returned a source target outside its captured input.");
        return uri.toString();
      }, envelopes, semantic);
      if (!snapshot.complete) throw new ComparisonFailure("INCOMPLETE_INPUT", "The model inputs are incomplete. Fix them before comparing.");
      for (const [side, capture] of captures) service.log(`${side} historical capture: ${JSON.stringify(capture.stats)}`);
      return { snapshot, inputs: envelopes, texts, sourceUris, working, holder, instance };
    }
    throw new ComparisonFailure("CAPTURE_LIMIT", "Historical capture exceeded the source-loading limit.");
  } catch (error) { if (holder) history.release(holder); throw error; }
  finally { cancellation.dispose(); }
}

async function captureLegacy(service: DygnosisClient, resource: ComparisonResource, token: vscode.CancellationToken): Promise<CapturedComparison> {
  if (resource.before.kind !== "working" || resource.after.kind !== "working") throw new Error("History comparison requires snapshot support.");
  const roots = { before: vscode.Uri.parse(resource.before.root_uri), after: vscode.Uri.parse(resource.after.root_uri) }, instance = service.currentInstance;
  const infos = { before: await service.modelInfo(roots.before, roots.before, true, token), after: await service.modelInfo(roots.after, roots.after, true, token) };
  if (!infos.before?.complete || !infos.after?.complete) throw new ComparisonFailure("INCOMPLETE_INPUT", "Incomplete model inputs. Fix them and Refresh.");
  const experimental: unknown = service.client?.initializeResult?.capabilities.experimental;
  const capability = record(experimental) && record(experimental.dygnosis) ? experimental.dygnosis.compareModels : undefined;
  if (record(capability) && capability.navigation_schema_version !== 1) throw new Error("This engine advertises an unsupported comparison. Update dynare.serverPath or use the bundled binary.");
  const raw = await service.execute("dynare/compareModels", [roots.before.toString(), roots.after.toString()], token);
  const texts = record(raw) && record(raw.sources) ? { before: sourceTexts(raw.sources.before), after: sourceTexts(raw.sources.after) } : { before: {}, after: {} };
  const registered = { before: new Set<string>(), after: new Set<string>() };
  const currentUri = (side: DiffSide, key: string): vscode.Uri => {
    if (/^(file|untitled):/.test(key)) return vscode.Uri.parse(key);
    // compareModels is the current LSP namespace: Workspace keys are absolute
    // native paths. Snapshot and supplied-map keys never enter this conversion.
    if (!path.isAbsolute(key)) throw new Error("The engine returned an unsupported current source key.");
    const sameRoot = process.platform === "win32" ? path.normalize(key).toLowerCase() === path.normalize(roots[side].fsPath).toLowerCase() : key === roots[side].fsPath;
    return sameRoot ? roots[side] : vscode.Uri.file(key);
  };
  for (const side of ["before", "after"] as const) for (const key of Object.keys(texts[side])) registered[side].add(currentUri(side, key).toString());
  const snapshot = parseDiff(raw, roots.before.toString(), roots.after.toString(), semanticCapability(capability), (side, uri) => registered[side].has(vscode.Uri.parse(uri).toString()));
  const sourceUris = { before: new Map<string, vscode.Uri>(), after: new Map<string, vscode.Uri>() };
  const working = ["before", "after"].map(side => ({ root: roots[side as DiffSide], expected: infos[side as DiffSide]!.revision }));
  const verified = await Promise.all(working.map(input => service.revalidate(input.root, input.expected, instance, input.root, token)));
  if (!snapshot.complete || verified.some(info => !info) || snapshot.before.revision !== infos.before.revision || snapshot.after.revision !== infos.after.revision)
    throw new ComparisonFailure("INPUT_CHANGED", "The model inputs changed. Refresh the comparison.");
  const inputs = {} as Record<DiffSide, SnapshotEnvelope>;
  for (const side of ["before", "after"] as const) {
    if (snapshot.semantic) for (const key of Object.keys(texts[side])) sourceUris[side].set(key, currentUri(side, key));
    else {
      sourceUris[side].set(roots[side].toString(), roots[side]);
      for (const row of snapshot.rows) for (const target of row.navigation[side]?.written_locations ?? []) sourceUris[side].set(target.uri, vscode.Uri.parse(target.uri));
    }
    inputs[side] = { kind: "working", input_id: roots[side].toString(), root_file: roots[side].toString(), revision: infos[side]!.revision,
      complete: true, file_keys: [...sourceUris[side].keys()], dependency_candidates: infos[side]!.dependency_candidates ?? [], search_paths: [] };
  }
  return { snapshot, inputs, sourceUris, texts, working, holder: {}, legacy: !snapshot.semantic, instance };
}
