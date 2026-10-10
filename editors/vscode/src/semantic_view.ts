import { record } from "./protocol";

export const semanticFamilies = ["symbols", "parameters", "equations", "shocks", "steady_state", "priors", "commands", "observables", "data", "occbin", "policy", "semi_structural", "moments", "ms_sbvar", "heterogeneity", "external_functions", "trends", "operations", "macro_context"] as const;
export const facets = ["expression", "parameter_value", "symbol_kind", "label", "tags", "scope", "log_transform", "predetermined_convention", "timing", "shock_setup", "target", "role", "options", "order", "assignment", "prior", "operation", "complementarity"] as const;
export type Availability = "complete" | "partial" | "not_available" | "limit_exceeded";
export type SemanticFamily = typeof semanticFamilies[number];
export interface Limit { code: string; reason: string; owner: string; omitted: number | null }
export interface Scope { domain: string; dimension: string | null; block: string | null }
export interface RowSide { name: string; scope: Scope; occurrence: number | null; equation_index: number | null; context: { kind: string; name: string; execution_order: number; scope: Scope; pointer: string | null } | null }
export type FieldValue = { kind: "text"; value: string } | { kind: "number" | "integer"; value: number } | { kind: "boolean"; value: boolean } | { kind: "list"; value: FieldValue[] } | { kind: "record"; value: Record<string, FieldValue> };
export interface FieldState { state: "absent" | "empty" | "unknown" | "present"; value: FieldValue | null }
export interface FieldChange { name: string; label: string; before: FieldState; after: FieldState; changed: boolean; comparison_availability: Availability; numeric_difference: number | null }
export interface TokenRun { text: string; role: "unchanged" | "added" | "removed" }
export interface ExpressionSide { text: string; runs: TokenRun[] }
export interface Expression { field: string; before: ExpressionSide | null; after: ExpressionSide | null; highlight_basis: "paired_expression" | "unpaired_text_only" | "none"; availability: Availability; reason: string | null }
export interface TimingSide { name: string; class: string; written_offset: number; converted_offset: number; occurrence: number }
export interface Reference { pointer: string; symbol: string; side: "before" | "after"; equation_pointer: string; equation_index: number; label: string; scope: Scope; occurrence: number; timing: TimingSide }
export interface SemanticRow { pointer: string; family: SemanticFamily; change: "added" | "removed" | "changed"; name: string; count_unit: "final_fact" | "accepted_occurrence" | "operation"; facets: typeof facets[number][]; before: RowSide | null; after: RowSide | null; fields: FieldChange[]; expressions: Expression[]; timing: { before: TimingSide | null; after: TimingSide | null }[]; references: string[]; limits: Limit[] }
export interface Semantic { schema_version: 1; availability: Availability; budgets: Record<string, number>; rows: SemanticRow[]; references: Reference[]; limits: Limit[] }
export interface SourceSide { input_id: string | null; file_key: string; exact_text_available: boolean }
export interface SourceHunk { before_start: number; before_lines: number; after_start: number; after_lines: number; lines: TokenRun[] }
export interface SourceFile { pointer: string; change: SemanticRow["change"]; correspondence: "selected_roots" | "proven_file_identity" | "unpaired"; before: SourceSide | null; after: SourceSide | null; availability: Availability; hunks: SourceHunk[]; omitted_hunks: number | null; limits: Limit[] }
export interface SourceChanges { schema_version: 1; availability: Availability; files: SourceFile[]; limits: Limit[] }
export interface Coverage { schema_version: 1; availability: Availability; source_boundary: string; families: { family: SemanticFamily; availability: Availability; fields: string[]; limits: Limit[] }[]; limits: Limit[] }
export interface SemanticPayload { semantic: Semantic; sourceChanges: SourceChanges; coverage: Coverage }
export type SourceRegistry = Record<"before" | "after", Record<string, string>>;
const availability = ["complete", "partial", "not_available", "limit_exceeded"] as const;
const changes = ["added", "removed", "changed"] as const;
const roles = ["unchanged", "added", "removed"] as const;
function invalid(): never { throw new Error("This engine returned an unsupported comparison detail. Update dynare.serverPath or use the bundled binary."); }
function object(value: unknown, keys: string[]): Record<string, unknown> {
  if (!record(value) || Object.keys(value).some(key => !keys.includes(key)) || keys.some(key => !Object.hasOwn(value, key))) invalid();
  return value;
}
function array(value: unknown): unknown[] { return Array.isArray(value) ? value : invalid(); }
function text(value: unknown): string { return typeof value === "string" ? value : invalid(); }
function bool(value: unknown): boolean { return typeof value === "boolean" ? value : invalid(); }
function integer(value: unknown, min = 0): number { return typeof value === "number" && Number.isSafeInteger(value) && value >= min ? value : invalid(); }
function number(value: unknown): number { return typeof value === "number" && Number.isFinite(value) ? value : invalid(); }
function one<T extends string>(value: unknown, values: readonly T[]): T { return typeof value === "string" && values.includes(value as T) ? value as T : invalid(); }
function nullable<T>(value: unknown, parse: (value: unknown) => T): T | null { return value === null ? null : parse(value); }
function unique<T>(values: T[]): T[] { return new Set(values).size === values.length ? values : invalid(); }
function pointer(value: unknown): string { const result = text(value); return /^\/(?:[^~\s]|~[01])+$/.test(result) ? result : invalid(); }
function limit(value: unknown): Limit { const r = object(value, ["code", "reason", "owner", "omitted"]); return { code: text(r.code), reason: text(r.reason), owner: text(r.owner), omitted: nullable(r.omitted, v => integer(v, 1)) }; }
function scope(value: unknown): Scope { const r = object(value, ["domain", "dimension", "block"]); return { domain: text(r.domain), dimension: nullable(r.dimension, text), block: nullable(r.block, text) }; }
function rowSide(value: unknown): RowSide {
  const r = object(value, ["name", "scope", "occurrence", "equation_index", "context"]);
  const context = nullable(r.context, value => { const c = object(value, ["kind", "name", "execution_order", "scope", "pointer"]); return { kind: text(c.kind), name: text(c.name), execution_order: integer(c.execution_order), scope: scope(c.scope), pointer: nullable(c.pointer, pointer) }; });
  return { name: text(r.name), scope: scope(r.scope), occurrence: nullable(r.occurrence, integer), equation_index: nullable(r.equation_index, integer), context };
}
function fieldValue(value: unknown, depth = 0): FieldValue {
  if (depth > 64) invalid();
  const r = object(value, ["kind", "value"]), kind = one(r.kind, ["text", "number", "integer", "boolean", "list", "record"]);
  switch (kind) {
    case "text": return { kind, value: text(r.value) };
    case "number": return { kind, value: number(r.value) };
    case "integer": return { kind, value: integer(r.value, Number.MIN_SAFE_INTEGER) };
    case "boolean": return { kind, value: bool(r.value) };
    case "list": return { kind, value: array(r.value).map(v => fieldValue(v, depth + 1)) };
    case "record": if (!record(r.value)) invalid(); return { kind, value: Object.fromEntries(Object.entries(r.value).map(([key, v]) => [key, fieldValue(v, depth + 1)])) };
  }
}
function state(value: unknown): FieldState {
  const r = object(value, ["state", "value"]), s = one(r.state, ["absent", "empty", "unknown", "present"]), v = nullable(r.value, fieldValue);
  if ((s === "absent" || s === "unknown") ? v !== null : v === null || s === "empty" && (v.kind !== "text" || v.value !== "")) invalid();
  return { state: s, value: v };
}
function field(value: unknown): FieldChange {
  const r = object(value, ["name", "label", "before", "after", "changed", "comparison_availability", "numeric_difference"]);
  const result = { name: text(r.name), label: text(r.label), before: state(r.before), after: state(r.after), changed: bool(r.changed), comparison_availability: one(r.comparison_availability, availability), numeric_difference: nullable(r.numeric_difference, number) };
  if (result.comparison_availability !== "complete" && (result.changed || result.numeric_difference !== null)) invalid();
  if (result.numeric_difference !== null && [result.before, result.after].some(side => side.state !== "present" || !side.value || side.value.kind !== "number" && side.value.kind !== "integer")) invalid();
  return result;
}
function runs(value: unknown, side?: "before" | "after"): TokenRun[] { return array(value).map(value => { const r = object(value, ["text", "role"]), role = one(r.role, roles); if (side === "before" && role === "added" || side === "after" && role === "removed") invalid(); return { text: text(r.text), role }; }); }
function expressionSide(value: unknown, side: "before" | "after"): ExpressionSide { const r = object(value, ["text", "runs"]), result = { text: text(r.text), runs: runs(r.runs, side) }; if (result.runs.map(run => run.text).join("") !== result.text) invalid(); return result; }
function expression(value: unknown): Expression {
  const r = object(value, ["field", "before", "after", "highlight_basis", "availability", "reason"]);
  const result = { field: text(r.field), before: nullable(r.before, v => expressionSide(v, "before")), after: nullable(r.after, v => expressionSide(v, "after")), highlight_basis: one(r.highlight_basis, ["paired_expression", "unpaired_text_only", "none"]), availability: one(r.availability, availability), reason: nullable(r.reason, text) };
  if (result.availability !== "complete" && (result.highlight_basis !== "none" || !result.reason)) invalid();
  if (result.highlight_basis === "none" && [result.before, result.after].some(side => side?.runs.some(run => run.role !== "unchanged"))) invalid();
  return result;
}
function timing(value: unknown): TimingSide { const r = object(value, ["name", "class", "written_offset", "converted_offset", "occurrence"]); return { name: text(r.name), class: text(r.class), written_offset: integer(r.written_offset, -2147483648), converted_offset: integer(r.converted_offset, -2147483648), occurrence: integer(r.occurrence) }; }
function semanticRow(value: unknown): SemanticRow {
  const r = object(value, ["pointer", "family", "change", "name", "count_unit", "facets", "before", "after", "fields", "expressions", "timing", "references", "limits"]);
  const result: SemanticRow = { pointer: pointer(r.pointer), family: one(r.family, semanticFamilies), change: one(r.change, changes), name: text(r.name), count_unit: one(r.count_unit, ["final_fact", "accepted_occurrence", "operation"]), facets: unique(array(r.facets).map(v => one(v, facets))), before: nullable(r.before, rowSide), after: nullable(r.after, rowSide), fields: array(r.fields).map(field), expressions: array(r.expressions).map(expression), timing: array(r.timing).map(v => { const t = object(v, ["before", "after"]); const before = nullable(t.before, timing), after = nullable(t.after, timing); if (!before && !after) invalid(); return { before, after }; }), references: unique(array(r.references).map(pointer)), limits: array(r.limits).map(limit) };
  if (!result.before && !result.after || result.change === "added" && result.before || result.change === "removed" && result.after || result.change === "changed" && (!result.before || !result.after)) invalid();
  unique(result.fields.map(f => f.name)); unique(result.expressions.map(e => e.field));
  return result;
}
function reference(value: unknown): Reference { const r = object(value, ["pointer", "symbol", "side", "equation_pointer", "equation_index", "label", "scope", "occurrence", "timing"]); return { pointer: pointer(r.pointer), symbol: text(r.symbol), side: one(r.side, ["before", "after"]), equation_pointer: pointer(r.equation_pointer), equation_index: integer(r.equation_index), label: text(r.label), scope: scope(r.scope), occurrence: integer(r.occurrence), timing: timing(r.timing) }; }
/** Missing additive advertisements mean legacy support; partial/unknown advertisements fail. */
export function semanticCapability(value: unknown): boolean {
  if (!record(value)) return false;
  const keys = ["semantic_schema_version", "source_changes_schema_version", "coverage_schema_version"];
  if (keys.every(key => !Object.hasOwn(value, key))) return false;
  if (keys.some(key => value[key] !== 1)) invalid();
  return true;
}
/** Validate closed v1 data before it can enter the webview or source-action registry. */
export function parseSemantic(value: Record<string, unknown>, ids: Record<"before" | "after", string | null>, registry: SourceRegistry, expected?: boolean): SemanticPayload | undefined {
  const present = ["comparison_versions", "semantic", "source_changes", "coverage"].some(key => Object.hasOwn(value, key));
  if (!present) { if (expected) invalid(); return undefined; }
  if (expected === false) invalid();
  const versions = object(value.comparison_versions, ["semantic", "source_changes", "coverage"]);
  if (Object.values(versions).some(v => v !== 1)) invalid();
  const s = object(value.semantic, ["schema_version", "availability", "budgets", "rows", "references", "limits"]);
  const budgetKeys = ["token_alignment_cells", "source_alignment_cells", "references_per_side", "source_hunks", "serialized_output_bytes"];
  const budget = object(s.budgets, budgetKeys);
  const semantic: Semantic = { schema_version: 1, availability: one(s.availability, availability), budgets: Object.fromEntries(Object.entries(budget).map(([k, v]) => [k, integer(v)])), rows: array(s.rows).map(semanticRow), references: array(s.references).map(reference), limits: array(s.limits).map(limit) };
  const c = object(value.coverage, ["schema_version", "availability", "source_boundary", "families", "limits"]);
  const coverage: Coverage = { schema_version: 1, availability: one(c.availability, availability), source_boundary: text(c.source_boundary), families: array(c.families).map(v => { const f = object(v, ["family", "availability", "fields", "limits"]); return { family: one(f.family, semanticFamilies), availability: one(f.availability, availability), fields: unique(array(f.fields).map(text)), limits: array(f.limits).map(limit) }; }), limits: array(c.limits).map(limit) };
  const source = object(value.source_changes, ["schema_version", "availability", "files", "limits"]);
  if (s.schema_version !== 1 || c.schema_version !== 1 || source.schema_version !== 1) invalid();
  const sourceSide = (value: unknown, side: "before" | "after"): SourceSide => { const r = object(value, ["input_id", "file_key", "exact_text_available"]), input_id = nullable(r.input_id, text), file_key = text(r.file_key), exact_text_available = bool(r.exact_text_available); if (input_id !== ids[side] || !Object.hasOwn(registry[side], file_key)) invalid(); return { input_id, file_key, exact_text_available }; };
  const sourceChanges: SourceChanges = { schema_version: 1, availability: one(source.availability, availability), files: array(source.files).map(v => {
    const f = object(v, ["pointer", "change", "correspondence", "before", "after", "availability", "hunks", "omitted_hunks", "limits"]);
    const before = nullable(f.before, v => sourceSide(v, "before")), after = nullable(f.after, v => sourceSide(v, "after")), change = one(f.change, changes), correspondence = one(f.correspondence, ["selected_roots", "proven_file_identity", "unpaired"]);
    if (!before && !after || change === "added" && before || change === "removed" && after || change === "changed" && (!before || !after) || correspondence === "unpaired" && before && after) invalid();
    return { pointer: pointer(f.pointer), change, correspondence, before, after, availability: one(f.availability, availability), omitted_hunks: nullable(f.omitted_hunks, v => integer(v, 1)), limits: array(f.limits).map(limit), hunks: array(f.hunks).map(v => { const h = object(v, ["before_start", "before_lines", "after_start", "after_lines", "lines"]), lines = runs(h.lines), before_lines = integer(h.before_lines), after_lines = integer(h.after_lines); if (lines.filter(l => l.role !== "added").length !== before_lines || lines.filter(l => l.role !== "removed").length !== after_lines) invalid(); return { before_start: integer(h.before_start, 1), before_lines, after_start: integer(h.after_start, 1), after_lines, lines }; }) };
  }), limits: array(source.limits).map(limit) };
  unique(semantic.rows.map(r => r.pointer)); unique(semantic.references.map(r => r.pointer)); unique(coverage.families.map(f => f.family)); unique(sourceChanges.files.map(f => f.pointer));
  semantic.rows.forEach((row, index) => { if (row.pointer.startsWith("/semantic/") && row.pointer !== `/semantic/rows/${index}`) invalid(); });
  semantic.references.forEach((row, index) => { if (row.pointer !== `/semantic/references/${index}`) invalid(); });
  sourceChanges.files.forEach((file, index) => { if (file.pointer !== `/source_changes/files/${index}`) invalid(); });
  const references = new Map(semantic.references.map(r => [r.pointer, r]));
  for (const row of semantic.rows) for (const p of row.references) if (!references.has(p) || !row[references.get(p)!.side]) invalid();
  return { semantic, sourceChanges, coverage };
}
