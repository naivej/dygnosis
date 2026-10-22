import * as vscode from "vscode";
import * as path from "node:path";
import { randomBytes } from "node:crypto";
import { DygnosisClient } from "../client/client";
import type { ModelSnapshot } from "../client/client";
import { ComparisonResource, ModelSelector } from "./changes_resource";
import { GitSources, HistoricalCapture, HistoricalInput } from "./git_source";
import { DiffSide, DiffSnapshot, parseDiff, parseSnapshotDiff } from "./diff_view";
import { HistoricalSources } from "./history_sources";
import { engineSettings } from "../client/settings";
import { analysisConfiguration, parseAnalysisContext, parseAnalysisMetadata, record } from "../client/protocol";
import type { AnalysisConfiguration, AnalysisMetadata } from "../client/protocol";
import type { AnalysisSide } from "../editor/analysis_configuration";
import { normalizeGitText } from "./git_provenance";
import { semanticCapability } from "./semantic_view";

interface WorkingInput { kind: "working"; input_id: string; root_uri: string; expected_revision: string; search_paths?: string[]; analysis?: AnalysisConfiguration }
type ConfiguredHistoricalInput = HistoricalInput & { analysis?: AnalysisConfiguration };
export type CapturedAnalysis = Pick<AnalysisMetadata, "selection" | "configuration" | "macro_complete" | "model_complete" | "overrides">;
export interface ComparisonCounts { n_endogenous: number; n_equations: number; heterogeneity_dimensions: { dimension: string; n_endogenous: number; n_equations: number }[] }
export interface AnalysisSummary {
  analysis?: CapturedAnalysis; requested_analysis?: AnalysisConfiguration | null; complete: boolean | null;
  counts: ComparisonCounts | null; counts_cause?: string; cause?: { code: string; message: string };
}
export interface SnapshotEnvelope {
  input_id: string; kind: "working" | "git"; root_file: string; revision: string; complete: boolean;
  file_keys: string[]; dependency_candidates: string[]; search_paths: string[];
  commit?: string;
  analysis?: CapturedAnalysis; counts?: ComparisonCounts; guards?: unknown[];
}

/** Freeze executable choices without changing the live root selection. */
export async function prepareAnalysisSide(service: DygnosisClient, root: vscode.Uri, side: AnalysisSide, token: vscode.CancellationToken): Promise<ModelSelector> {
  const proof = await service.analysisContext(root, root, token);
  assertActive(token);
  if (!proof) throw new ComparisonFailure("INPUT_CHANGED", "The source changed while preparing the comparison. Select its configurations again.");
  const raw = await service.execute("dynare/analysisContext", [{ root_uri: root.toString(), document_uri: proof.document_uri, document_version: proof.document_version,
    expected_revision: proof.revision, expected_selection_generation: proof.analysis.selection_generation, prepare_analysis: side.analysis,
    include_paths: side.include_paths, ...(side.preset_bindings ? { preset_bindings: side.preset_bindings } : {}) }], token);
  assertActive(token);
  const prepared = parseAnalysisContext(raw, root.toString());
  if (!prepared.prepared_analysis || !prepared.prepared_include_paths)
    throw new Error("The engine returned unsupported comparison choices. Use the bundled binary.");
  return { kind: "working", root_uri: root.toString(), analysis: structuredClone(prepared.prepared_analysis), search_paths: [...prepared.prepared_include_paths] };
}

export async function freezeWorkingSelector(service: DygnosisClient, input: ModelSelector, token: vscode.CancellationToken): Promise<ModelSelector> {
  if (input.kind !== "working" || input.analysis !== undefined || !service.supportsAnalysisConfiguration) return input;
  const root = vscode.Uri.parse(input.root_uri), context = await service.analysisContext(root, root, token);
  assertActive(token);
  if (!context) throw new ComparisonFailure("INPUT_CHANGED", "The model source changed while capturing its analysis configuration. Open the comparison again.");
  return { ...input, analysis: structuredClone(context.analysis.configuration), search_paths: input.search_paths ?? context.analysis.include_paths ?? configuredSearchPaths(root, service) };
}
export interface CapturedComparison {
  snapshot: DiffSnapshot; inputs: Record<DiffSide, SnapshotEnvelope>; sourceUris: Record<DiffSide, Map<string, vscode.Uri>>;
  texts: Record<DiffSide, Record<string, string>>; working: { root: vscode.Uri; expected: string }[]; holder: object;
  legacy?: boolean;
  instance: number;
}
export class ComparisonFailure extends Error {
  constructor(readonly code: string, message: string, readonly side?: DiffSide, readonly fileKey?: string, readonly summaries?: Record<DiffSide, AnalysisSummary>) { super(message); }
}
function configurationKey(value: AnalysisConfiguration): string {
  return JSON.stringify({ name: value.name ?? null, defines: (value.defines ?? []).map(input => [input.name, input.expression]),
    overrides: (value.overrides ?? []).map(operation => [operation.kind, operation.target.file, operation.target.site_id, operation.target.source_revision,
      operation.kind === "definition" ? operation.expression : operation.kind === "branch" ? operation.branch_id : null]) });
}
function comparisonCounts(value: unknown): value is ComparisonCounts {
  const count = (value: unknown): value is number => Number.isSafeInteger(value) && (value as number) >= 0;
  return record(value) && count(value.n_endogenous) && count(value.n_equations) && Array.isArray(value.heterogeneity_dimensions) &&
    value.heterogeneity_dimensions.every(row => record(row) && typeof row.dimension === "string" && count(row.n_endogenous) && count(row.n_equations));
}
function failureSummaries(value: unknown, inputs: Record<DiffSide, WorkingInput | ConfiguredHistoricalInput>): Record<DiffSide, AnalysisSummary> | undefined {
  if (value === undefined) return undefined;
  if (!record(value)) throw new Error("The engine returned invalid comparison summaries.");
  const summaries = {} as Record<DiffSide, AnalysisSummary>;
  for (const side of ["before", "after"] as const) {
    const raw = value[side];
    if (!record(raw) || !(typeof raw.complete === "boolean" || raw.complete === null) ||
      !(raw.counts === null || comparisonCounts(raw.counts)) ||
      (raw.counts_cause !== undefined && raw.counts_cause !== null && typeof raw.counts_cause !== "string") ||
      (raw.cause !== undefined && (!record(raw.cause) || typeof raw.cause.code !== "string" || typeof raw.cause.message !== "string")) ||
      (raw.requested_analysis !== undefined && raw.requested_analysis !== null && !analysisConfiguration(raw.requested_analysis)))
      throw new Error("The engine returned invalid comparison summaries.");
    let analysis: CapturedAnalysis | undefined;
    if (raw.analysis !== undefined) {
      if (!record(raw.analysis)) throw new Error("The engine returned invalid comparison analysis.");
      analysis = parseAnalysisMetadata({ ...raw.analysis, state: "selected", selection_generation: 0, include_paths: null });
    }
    const configuration = analysis?.configuration ?? raw.requested_analysis, requested = inputs[side].analysis;
    if (requested && (!analysisConfiguration(configuration) || configurationKey(configuration) !== configurationKey(requested)))
      throw new Error("The engine returned different analysis choices for this comparison summary.");
    summaries[side] = { ...(analysis ? { analysis } : {}), ...(raw.requested_analysis !== undefined ? { requested_analysis: raw.requested_analysis } : {}),
      complete: raw.complete, counts: raw.counts,
      ...(typeof raw.counts_cause === "string" ? { counts_cause: raw.counts_cause } : {}),
      ...(record(raw.cause) ? { cause: { code: raw.cause.code as string, message: raw.cause.message as string } } : {}) };
  }
  return summaries;
}

export async function validateCapturedInputs(service: DygnosisClient, result: Pick<CapturedComparison, "inputs" | "working" | "instance">, token?: vscode.CancellationToken): Promise<boolean> {
  if (service.currentInstance !== result.instance || token?.isCancellationRequested) return false;
  if (result.working.length && (["before", "after"] as const).filter(side => result.inputs[side].kind === "working").every(side => Array.isArray(result.inputs[side].guards))) {
    return await service.validateComparisonInputs(result.inputs, token) && service.currentInstance === result.instance;
  }
  const verified = await Promise.all(result.working.map(input => service.revalidate(input.root, input.expected, result.instance, input.root, token)));
  return verified.every(Boolean) && service.currentInstance === result.instance;
}
function assertActive(token: vscode.CancellationToken): void { if (token.isCancellationRequested) throw new ComparisonFailure("CANCELLED", "Comparison capture was cancelled."); }
function requireCompleteModel(info: ModelSnapshot | undefined, side: DiffSide): asserts info is ModelSnapshot {
  const label = side === "before" ? "Before" : "After";
  if (!info) throw new ComparisonFailure("ROOT_NOT_FOUND", `${label} model information is unavailable.`, side);
  if (!info.complete) {
    const reasons = info.incomplete_reasons?.map(reason => reason.message).join("; ") || info.message || "Model parsing or macro expansion is incomplete.";
    throw new ComparisonFailure("INCOMPLETE_INPUT", `${label}: ${reasons} Comparison changes were withheld.`, side);
  }
}
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
  if (value.analysis !== undefined) {
    // The shared capture omits the native session's mutable selection fields.
    const parsed = parseAnalysisMetadata({ ...value.analysis as object, state: "selected", selection_generation: 0, include_paths: null });
    if (!record(value.analysis) || value.analysis.revision !== value.revision) throw new Error("The engine returned a different analysis revision for this comparison input.");
    const requested = (input as WorkingInput | ConfiguredHistoricalInput).analysis;
    if (requested && configurationKey(parsed.configuration) !== configurationKey(requested)) throw new Error("The engine returned different analysis choices for this comparison input.");
  } else if ((input as WorkingInput | ConfiguredHistoricalInput).analysis !== undefined) throw new Error("The engine omitted the selected analysis configuration from this comparison input.");
  if (value.counts !== undefined) {
    const count = (value: unknown): value is number => Number.isSafeInteger(value) && (value as number) >= 0;
    if (!record(value.counts) || !count(value.counts.n_endogenous) || !count(value.counts.n_equations) || !Array.isArray(value.counts.heterogeneity_dimensions) || value.counts.heterogeneity_dimensions.some(row => !record(row) || typeof row.dimension !== "string" || !count(row.n_endogenous) || !count(row.n_equations))) throw new Error("The engine returned invalid comparison counts.");
  }
  if (value.guards !== undefined && !Array.isArray(value.guards)) throw new Error("The engine returned invalid comparison source guards.");
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
    if (resource.before.analysis !== undefined || resource.after.analysis !== undefined) throw new ComparisonFailure("UNSUPPORTED_ANALYSIS", "This engine cannot compare explicit analysis configurations. Use the bundled binary or update dynare.serverPath.");
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
  const prepare = async (side: DiffSide, selector: ModelSelector): Promise<WorkingInput | ConfiguredHistoricalInput> => {
    assertActive(token);
    if (selector.kind === "working") {
      selector = await freezeWorkingSelector(service, selector, token);
      selectors[side] = selector;
      if (selector.kind !== "working") throw new Error("The comparison root changed during capture.");
      const root = vscode.Uri.parse(selector.root_uri), info = await service.modelInfo(root, root, true, token);
      if (selector.analysis === undefined) requireCompleteModel(info, side);
      const expected = info?.revision || (await service.analysisContext(root, root, token))?.revision;
      if (!expected) throw new ComparisonFailure("ROOT_NOT_FOUND", "The Working model source is unavailable.", side);
      working.push({ root, expected });
      const paths = selector.search_paths ?? searchPaths;
      return { kind: "working", input_id: ids[side], root_uri: selector.root_uri, expected_revision: expected, ...(paths ? { search_paths: paths } : {}), ...(selector.analysis ? { analysis: selector.analysis } : {}) };
    }
    const sources = await git(), repository = await sources.repositoryFor(vscode.Uri.parse(selector.repository_uri), abort.signal);
    if (repository.rootUri.toString() !== selector.repository_uri) throw new ComparisonFailure("REPOSITORY_CHANGED", "The selected repository is unavailable. Choose its source again.", side);
    const commit = await sources.resolve(repository, selector.commit, abort.signal);
    commit.requested_ref = selector.requested_ref;
    const capture = await sources.capture(repository, commit, selector.root_file, abort.signal); captures.set(side, capture);
    const folders = (selector.search_paths ?? searchPaths ?? []).map(folder => {
      const relative = path.relative(repository.rootUri.fsPath, folder).split(path.sep).join("/");
      // Keep an external folder explicit; the engine must refuse it if an active include needs it.
      return relative === "" ? "." : relative.startsWith("../") || path.isAbsolute(relative) ? folder.split(path.sep).join("/") : relative;
    });
    return { ...capture.input(ids[side], folders), ...(selector.analysis ? { analysis: selector.analysis } : {}) };
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
        throw new ComparisonFailure(String(result.code), `${side ? `${side === "before" ? "Before" : "After"}: ` : ""}${String(result.message)}${typeof result.file_key === "string" ? ` (${result.file_key})` : ""}`, side, typeof result.file_key === "string" ? result.file_key : undefined, failureSummaries(result.analysis_summaries, inputs));
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
          const previous = inputs[side];
          inputs[side] = { ...capture.input(ids[side], (previous as HistoricalInput).search_paths), ...(previous.analysis ? { analysis: previous.analysis } : {}) };
        }
        if (!progress) throw new Error("Historical capture made no progress. Refresh or choose another revision.");
        continue;
      }
      if (result.state !== "result" || !record(result.inputs) || result.inputs.schema_version !== 1 || !record(result.sources)) throw new Error("The engine returned an unsupported snapshot response.");
      const envelopes = { before: envelope(result.inputs.before, inputs.before), after: envelope(result.inputs.after, inputs.after) };
      if (!await validateCapturedInputs(service, { inputs: envelopes, working, instance }, token)) throw new ComparisonFailure("INPUT_CHANGED", "The Working source changed during capture. Refresh the comparison.");
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
  requireCompleteModel(infos.before, "before");
  requireCompleteModel(infos.after, "after");
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
