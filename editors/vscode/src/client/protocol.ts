/** Wire records are facts from the engine, never recovered from source text. */
export interface Position { line: number; character: number }
export interface Range { start: Position; end: Position }
export interface Location { uri: string; range: Range }
export interface SourceRecord {
  location: Location | null;
  anchor: Location | null;
  segments: Location[];
  origin_frames: { kind: string; variable: string | null; value: string | null; segments: Location[] }[];
}
export interface Statement extends SourceRecord {
  id: string; kind: string; name: string; complete: boolean; native: boolean;
  category: string | null; subtype: string | null; dimension: string | null;
  lens_anchor: Location | null; equation_count: number | null;
}
export interface Declaration extends SourceRecord {
  id: string; statement_id: string; name: string; written_kind: string;
  final_kind: string | null; long_name: string | null; tex_name: string | null;
  written_dimension: string | null; dimension: string | null;
  timing: { class: string; offsets: number[] } | null;
}
export interface Equation extends SourceRecord {
  id: string; statement_id: string; block_id: string; scope: string;
  number: number; dimension: string | null; name: string; text: string;
}
export interface Dimension {
  dimension: string; n_endogenous: number; n_exogenous: number;
  n_parameters: number; n_equations: number;
  endogenous: string[]; exogenous: string[]; parameters: string[];
  static: string[]; predetermined: string[]; forward_looking: string[]; mixed: string[];
}
export interface RelatedFile { kind: string; filename: string; resolved: boolean; path?: string }
export interface GuardedTarget { file: string | null; site_id: string; source_revision: string }
export type AnalysisOverride =
  { kind: "definition"; target: GuardedTarget; expression: string } |
  { kind: "skip_definition"; target: GuardedTarget } |
  { kind: "branch"; target: GuardedTarget; branch_id: string | null };
export interface EquationSelection { mode: "include" | "exclude"; tag?: string; values: string[] }
export interface AnalysisConfiguration { name?: string; defines?: { name: string; expression: string }[]; overrides?: AnalysisOverride[]; equation_selection?: EquationSelection }
export interface OverrideReceipt {
  operation: AnalysisOverride; name?: string | null; application: "applied" | "not_reached" | "unknown";
  branch_label?: string | null;
  source_line?: number | null;
  known_execution_count: number; execution_count: number | null; cause: string | null;
}
export interface AnalysisMetadata {
  state: "selected" | "stale";
  selection: { configuration_id: string; name: string | null; preview: boolean; invocation_scope: string };
  configuration: AnalysisConfiguration; selection_generation: number; include_paths: string[] | null;
  macro_complete: boolean | null; model_complete: boolean | null; overrides: OverrideReceipt[];
  override_names?: (string | null)[];
  override_branch_labels?: (string | null)[];
  cause?: { code: string; message: string; target?: GuardedTarget } | null;
}
export interface DirectiveSite {
  target: GuardedTarget; kind: "definition" | "function_definition" | "conditional";
  name: string | null; expected_text: string; location: Location;
  branches: { branch_id: string; expected_text: string; location: Location }[];
}
export interface PresetBinding {
  operation_index: number; file: string | null; kind: DirectiveSite["kind"]; name: string | null;
  expected_text: string; target_text: string; branch_index: number | null; branch_text: string | null;
}
export interface AnalysisContext {
  schema_version: 1; root_uri: string; document_uri: string; document_version: number | null;
  revision: string; analysis: AnalysisMetadata; sites: DirectiveSite[];
  source_coverage: { complete: boolean; cause?: string | null }; preset_bindings?: PresetBinding[];
  partial?: boolean; message?: string;
  prepared_analysis?: AnalysisConfiguration; prepared_include_paths?: string[];
}
export interface ModelInfo {
  schema_version: 1; root_uri: string; document_uri: string;
  document_version: number | null; revision: string; complete: boolean;
  owner_roots: string[]; statements: Statement[]; declarations: Declaration[];
  dependency_candidates?: string[];
  equations: Equation[]; related_files: RelatedFile[];
  block_categories: { category: string; default: string }[];
  first_model_anchor: Location | null;
  n_endogenous?: number; n_exogenous?: number; n_parameters?: number; n_equations?: number;
  endogenous?: string[]; exogenous?: string[]; parameters?: string[];
  static?: string[]; predetermined?: string[]; forward_looking?: string[]; mixed?: string[];
  heterogeneity_dimensions?: Dimension[];
  status?: string; message?: string;
  incomplete_reasons?: { code?: string; message?: string; location?: unknown }[];
  analysis?: AnalysisMetadata; source_browsing?: boolean; browsing_documents?: string[];
}
export function record(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
function natural(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}
function position(value: unknown): value is Position {
  return record(value) && natural(value.line) && natural(value.character);
}
export function location(value: unknown): value is Location {
  if (!record(value) || typeof value.uri !== "string" || !record(value.range)) return false;
  const { start, end } = value.range;
  return position(start) && position(end) &&
    (start.line < end.line || (start.line === end.line && start.character <= end.character));
}
function source(value: unknown): boolean {
  return record(value) && (value.location === null || location(value.location)) &&
    (value.anchor === null || location(value.anchor)) &&
    Array.isArray(value.segments) && value.segments.every(location) &&
    rows(value.origin_frames, frame => typeof frame.kind === "string" && nullableString(frame.variable) && nullableString(frame.value) && Array.isArray(frame.segments) && frame.segments.every(location));
}
function nullableString(value: unknown): boolean { return value === null || typeof value === "string"; }
function strings(value: unknown): value is string[] {
  return Array.isArray(value) && value.every(item => typeof item === "string");
}
function rows(value: unknown, validate: (row: Record<string, unknown>) => boolean): boolean {
  return Array.isArray(value) && value.every(row => record(row) && validate(row));
}
export function guardedTarget(value: unknown): value is GuardedTarget {
  return record(value) && nullableString(value.file) && typeof value.site_id === "string" && typeof value.source_revision === "string";
}
export function analysisOverride(value: unknown): value is AnalysisOverride {
  return record(value) && guardedTarget(value.target) && (value.kind === "definition" && typeof value.expression === "string" ||
    value.kind === "skip_definition" || value.kind === "branch" && nullableString(value.branch_id));
}
export function analysisConfiguration(value: unknown): value is AnalysisConfiguration {
  return record(value) && (value.name === undefined || typeof value.name === "string") &&
    (value.defines === undefined || rows(value.defines, row => typeof row.name === "string" && typeof row.expression === "string")) &&
    (value.overrides === undefined || Array.isArray(value.overrides) && value.overrides.every(analysisOverride)) &&
    (value.equation_selection === undefined || record(value.equation_selection) &&
      ["include", "exclude"].includes(String(value.equation_selection.mode)) &&
      (value.equation_selection.tag === undefined || typeof value.equation_selection.tag === "string" && /^[A-Za-z_]\w*$/.test(value.equation_selection.tag)) &&
      Array.isArray(value.equation_selection.values) && value.equation_selection.values.length > 0 &&
      value.equation_selection.values.every(item => typeof item === "string" && item.trim().length > 0));
}
export function parseAnalysisMetadata(value: unknown): AnalysisMetadata {
  if (!record(value) || !["selected", "stale"].includes(String(value.state)) || !record(value.selection) ||
      typeof value.selection.configuration_id !== "string" || !nullableString(value.selection.name) || typeof value.selection.preview !== "boolean" ||
      typeof value.selection.invocation_scope !== "string" || !analysisConfiguration(value.configuration) || !natural(value.selection_generation) ||
      !(value.include_paths === null || strings(value.include_paths)) || ![true, false, null].includes(value.macro_complete as boolean | null) ||
      ![true, false, null].includes(value.model_complete as boolean | null) || !rows(value.overrides, row => analysisOverride(row.operation) &&
        (row.name === undefined || nullableString(row.name)) &&
        (row.branch_label === undefined || nullableString(row.branch_label)) &&
        (row.source_line === undefined || row.source_line === null || natural(row.source_line) && row.source_line > 0) &&
        ["applied", "not_reached", "unknown"].includes(String(row.application)) && natural(row.known_execution_count) &&
        (row.execution_count === null || natural(row.execution_count)) && nullableString(row.cause)) ||
      !(value.override_names === undefined || Array.isArray(value.override_names) && value.override_names.every(nullableString) &&
        value.override_names.length === (value.configuration.overrides?.length ?? 0)) ||
      !(value.override_branch_labels === undefined || Array.isArray(value.override_branch_labels) && value.override_branch_labels.every(nullableString) &&
        value.override_branch_labels.length === (value.configuration.overrides?.length ?? 0)) ||
      value.state === "stale" && (!record(value.cause) || typeof value.cause.code !== "string" || typeof value.cause.message !== "string" ||
        value.cause.target !== undefined && !guardedTarget(value.cause.target))) {
    throw new Error("Unsupported or invalid analysis configuration. Use the bundled engine or update dynare.serverPath.");
  }
  return value as unknown as AnalysisMetadata;
}
export function parseAnalysisContext(value: unknown, root: string): AnalysisContext {
  if (record(value) && typeof value.error === "string") throw new Error(value.error);
  if (!record(value) || value.schema_version !== 1 || value.root_uri !== root || typeof value.document_uri !== "string" ||
      !(value.document_version === null || natural(value.document_version)) || typeof value.revision !== "string" ||
      !rows(value.sites, site => guardedTarget(site.target) && ["definition", "function_definition", "conditional"].includes(String(site.kind)) &&
        nullableString(site.name) && typeof site.expected_text === "string" && location(site.location) &&
        rows(site.branches, branch => typeof branch.branch_id === "string" && typeof branch.expected_text === "string" && location(branch.location))) ||
      !record(value.source_coverage) || typeof value.source_coverage.complete !== "boolean" ||
      (value.source_coverage.cause !== undefined && !nullableString(value.source_coverage.cause)) ||
      (value.partial !== undefined && typeof value.partial !== "boolean") || (value.message !== undefined && typeof value.message !== "string") ||
      (value.prepared_analysis !== undefined && !analysisConfiguration(value.prepared_analysis)) ||
      (value.prepared_include_paths !== undefined && !strings(value.prepared_include_paths)) ||
      !(value.preset_bindings === undefined || rows(value.preset_bindings, binding => natural(binding.operation_index) && nullableString(binding.file) &&
        ["definition", "function_definition", "conditional"].includes(String(binding.kind)) && nullableString(binding.name) &&
        typeof binding.expected_text === "string" && typeof binding.target_text === "string" &&
        (binding.branch_index === null || natural(binding.branch_index)) && nullableString(binding.branch_text)))) {
    throw new Error("Unsupported or invalid analysis context. Use the bundled engine or update dynare.serverPath.");
  }
  parseAnalysisMetadata(value.analysis);
  return value as unknown as AnalysisContext;
}
/** Reject an unknown schema or malformed navigation rather than guessing. */
export function parseModelInfo(value: unknown, root: string, document: string): ModelInfo {
  if (record(value) && typeof value.error === "string") throw new Error(value.error);
  if (!record(value) || value.schema_version !== 1 || value.root_uri !== root ||
      value.document_uri !== document || typeof value.revision !== "string" ||
      typeof value.complete !== "boolean" ||
      !(value.document_version === null || natural(value.document_version)) ||
      !strings(value.owner_roots) ||
      !(value.browsing_documents === undefined || strings(value.browsing_documents)) ||
      !(value.source_browsing === undefined || typeof value.source_browsing === "boolean") ||
      !(value.dependency_candidates === undefined || strings(value.dependency_candidates)) ||
      !rows(value.statements, row => source(row) && typeof row.id === "string" &&
        typeof row.kind === "string" && typeof row.name === "string" &&
        typeof row.complete === "boolean" && typeof row.native === "boolean" &&
        nullableString(row.category) && nullableString(row.subtype) && nullableString(row.dimension) &&
        (row.equation_count === null || natural(row.equation_count)) &&
        (row.lens_anchor === null || location(row.lens_anchor))) ||
      !rows(value.declarations, row => source(row) && typeof row.id === "string" &&
        typeof row.statement_id === "string" && typeof row.name === "string" &&
        typeof row.written_kind === "string" && nullableString(row.final_kind) &&
        nullableString(row.long_name) && nullableString(row.tex_name) &&
        nullableString(row.written_dimension) && nullableString(row.dimension) &&
        (row.timing === null || (record(row.timing) && typeof row.timing.class === "string" &&
          Array.isArray(row.timing.offsets) && row.timing.offsets.every(offset => typeof offset === "number" && Number.isSafeInteger(offset))))) ||
      !rows(value.equations, row => source(row) && typeof row.id === "string" &&
        typeof row.statement_id === "string" && natural(row.number) && row.number > 0 &&
        typeof row.block_id === "string" && typeof row.name === "string" && typeof row.text === "string" &&
        ["aggregate", "dimension"].includes(String(row.scope)) && nullableString(row.dimension)) ||
      !rows(value.related_files, row => typeof row.kind === "string" &&
        typeof row.filename === "string" && typeof row.resolved === "boolean" &&
        (!row.resolved || typeof row.path === "string")) ||
      !rows(value.block_categories, row => typeof row.category === "string" &&
        ["off", "subtle", "model"].includes(String(row.default))) ||
      !(value.first_model_anchor === null || location(value.first_model_anchor))) {
    throw new Error("Unsupported or invalid Dygnosis model information. Update the engine or use the bundled binary.");
  }
  if (value.analysis !== undefined) parseAnalysisMetadata(value.analysis);
  if (value.complete && (!["n_endogenous", "n_exogenous", "n_parameters", "n_equations"]
      .every(key => natural(value[key])) ||
      !["endogenous", "exogenous", "parameters", "static", "predetermined", "forward_looking", "mixed"].every(key => strings(value[key])) ||
      !rows(value.heterogeneity_dimensions, row => typeof row.dimension === "string" &&
        ["n_endogenous", "n_exogenous", "n_parameters", "n_equations"].every(key => natural(row[key])) &&
        ["endogenous", "exogenous", "parameters", "static", "predetermined", "forward_looking", "mixed"].every(key => strings(row[key]))))) {
    throw new Error("Dygnosis returned incomplete model counts.");
  }
  return value as unknown as ModelInfo;
}

export const tokenRoles = ["dynareEndogenous", "dynareExogenous", "dynareParameter", "dynareModelLocal"];
export const timingModifiers = ["forwardLooking", "predetermined"];
