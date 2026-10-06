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
/** Reject an unknown schema or malformed navigation rather than guessing. */
export function parseModelInfo(value: unknown, root: string, document: string): ModelInfo {
  if (record(value) && typeof value.error === "string") throw new Error(value.error);
  if (!record(value) || value.schema_version !== 1 || value.root_uri !== root ||
      value.document_uri !== document || typeof value.revision !== "string" ||
      typeof value.complete !== "boolean" ||
      !(value.document_version === null || natural(value.document_version)) ||
      !strings(value.owner_roots) ||
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
