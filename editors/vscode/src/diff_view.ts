import { Location, location, record } from "./protocol";
import { Coverage, parseSemantic, Reference, Semantic, SemanticRow, SourceChanges, SourceRegistry } from "./semantic_view";

export const diffSections = ["symbols", "parameters", "equations", "shockSetup", "shockAnalysisSetup"] as const;
export const changeKinds = ["added", "removed", "changed", "unpaired"] as const;
export const familySections = ["steadyState", "priors", "commands", "observables", "data", "occbin", "policy", "semiStructural", "moments", "msSbvar", "heterogeneity", "externalFunctions", "trends", "operations", "macroContext"] as const;
export const allDiffSections = [...diffSections, ...familySections] as const;
export type DiffSection = typeof allDiffSections[number];
export type ChangeKind = typeof changeKinds[number];
export type DiffSide = "before" | "after";
export interface DiffPreferences {
  changeKinds: ChangeKind[];
}
export interface DiffChoices { changeKinds: ChangeKind[]; customChangeKinds: ChangeKind[]; search: string; group: string | null }
export interface DiffTarget {
  occurrence_id: string; domain: string; dimension: string | null; written_locations: Location[];
}
export interface DiffNavigationRow { id: string; kind: string; dimension?: string | null; family?: string; name?: string; equation_pointer?: string; before: DiffTarget | null; after: DiffTarget | null }
export interface DiffEnvelope { root_uri: string; revision: string | null; complete: boolean }
export interface DiffRow {
  id: string; section: DiffSection; group: string; kind: ChangeKind; label: string;
  before: string | null; after: string | null; scopes: string[]; sideScopes: { before: string | null; after: string | null }; navigation: DiffNavigationRow;
  semantic?: SemanticRow;
}
export interface DiffReference extends Reference { navigation: DiffNavigationRow }
export interface DiffSnapshot { before: DiffEnvelope; after: DiffEnvelope; rows: DiffRow[]; complete: boolean; semantic?: Semantic; sourceChanges?: SourceChanges; coverage?: Coverage; references?: DiffReference[]; semanticMessage?: string }
function malformed(): never { throw new Error("This engine returned an unsupported comparison. Update dynare.serverPath or use the bundled binary."); }
const string = (value: unknown): string => typeof value === "string" ? value : malformed();
const object = (value: unknown): Record<string, unknown> => record(value) ? value : malformed();
const array = (value: unknown): unknown[] => Array.isArray(value) ? value : malformed();
const nullableString = (value: unknown): string | null => value === null ? null : string(value);
function target(value: unknown, snapshots = false): DiffTarget | null {
  if (value === null) return null;
  const row = object(value), locations = array(row.written_locations);
  if (!locations.every(item => location(item) && (snapshots ? /^(file|untitled|dygnosis-history):/ : /^(file|untitled):/).test(item.uri))) malformed();
  return { occurrence_id: string(row.occurrence_id), domain: string(row.domain), dimension: nullableString(row.dimension),
    written_locations: (locations as Location[]).map(item => ({ uri: item.uri, range: { start: { ...item.range.start }, end: { ...item.range.end } } })) };
}
function envelope(value: unknown, root: string): DiffEnvelope {
  const row = object(value);
  if (row.root_uri !== root || typeof row.complete !== "boolean") malformed();
  return { root_uri: root, revision: nullableString(row.revision), complete: row.complete };
}
/** Project legacy rows for display; their exact pointers carry the engine's pairing. */
export function parseDiff(value: unknown, beforeRoot: string, afterRoot: string, expectedSemantic?: boolean, registeredSource?: (side: DiffSide, uri: string) => boolean): DiffSnapshot {
  return projectDiff(value, beforeRoot, afterRoot, false, { before: null, after: null }, expectedSemantic, registeredSource);
}
function projectDiff(value: unknown, beforeRoot: string, afterRoot: string, snapshots = false, ids: Record<DiffSide, string | null> = { before: null, after: null }, expectedSemantic?: boolean, registeredSource?: (side: DiffSide, uri: string) => boolean): DiffSnapshot {
  const result = object(value);
  if (typeof result.error === "string") throw new Error(result.error);
  if (result.status === "incomplete") return {
    before: { root_uri: beforeRoot, revision: null, complete: false }, after: { root_uri: afterRoot, revision: null, complete: false }, rows: [], complete: false,
  };
  const nav = object(result.navigation);
  if (nav.schema_version !== 1) malformed();
  const before = envelope(nav.before, beforeRoot), after = envelope(nav.after, afterRoot);
  const navigation = new Map<string, DiffNavigationRow>();
  for (const raw of array(nav.rows)) {
    const row = object(raw), id = string(row.id);
    if (navigation.has(id)) malformed();
    navigation.set(id, { id, kind: string(row.kind), dimension: row.dimension === undefined ? undefined : nullableString(row.dimension), family: row.family === undefined ? undefined : string(row.family), name: row.name === undefined ? undefined : string(row.name), equation_pointer: row.equation_pointer === undefined ? undefined : string(row.equation_pointer), before: target(row.before, snapshots), after: target(row.after, snapshots) });
  }
  const rows: DiffRow[] = [];
  const add = (id: string, section: DiffSection, group: string, kind: ChangeKind, label: string, old: string | null, next: string | null, dimension: string | null = null, sideDimensions?: { before: string | null | undefined; after: string | null | undefined }): void => {
    const source = navigation.get(id);
    if (!source) malformed();
    const expected = section === "parameters" ? "parameter" : section === "equations" ? "equation" : section === "symbols" ? "symbol" : "shock";
    if (source.kind !== expected || (old === null && source.before !== null) || (next === null && source.after !== null)) malformed();
    const scope = (side: DiffSide): string => (source[side] ? source[side].dimension : sideDimensions?.[side] ?? dimension ?? source.dimension) ?? "aggregate";
    const sideScopes = { before: old === null ? null : scope("before"), after: next === null ? null : scope("after") };
    const scopes = [...new Set([sideScopes.before, sideScopes.after].filter((scope): scope is string => scope !== null))];
    rows.push({ id, section, group, kind, label, before: old, after: next, scopes, sideScopes, navigation: source });
  };
  for (const category of ["endogenous", "exogenous", "parameters"]) {
    for (const kind of ["added", "removed", "common"] as const) {
      array(result[`${kind}_${category}`]).forEach((raw, index) => {
        const name = string(raw);
        if (kind !== "common") add(`/${kind}_${category}/${index}`, "symbols", "Symbols", kind, `${name} · ${category}`, kind === "removed" ? name : null, kind === "added" ? name : null);
      });
    }
  }
  array(result.symbols_changed).forEach((raw, index) => {
    const row = object(raw);
    const metadata = (rawSide: unknown): string => {
      const side = object(rawSide);
      return [string(side.kind), nullableString(side.long_name) === null ? "" : `long_name: ${string(side.long_name)}`, nullableString(side.tex_name) === null ? "" : `TeX: ${string(side.tex_name)}`].filter(Boolean).join("\n");
    };
    add(`/symbols_changed/${index}`, "symbols", "Symbols", "changed", string(row.name), metadata(row.before), metadata(row.after));
  });
  array(result.changed_parameter_values).forEach((raw, index) => {
    const row = object(raw);
    const valueText = (raw: unknown, numeric: unknown): string => {
      if (numeric !== null && (typeof numeric !== "number" || !Number.isFinite(numeric))) malformed();
      const text = string(raw);
      return numeric === null ? text : `${text}\nValue: ${String(numeric)}`;
    };
    add(`/changed_parameter_values/${index}`, "parameters", "Parameter values", "changed", string(row.name), valueText(row.old_raw, row.old_value), valueText(row.new_raw, row.new_value));
  });
  function equations(owner: Record<string, unknown>, prefix: string, dimension: string | null): void {
    const section = "equations", group = "Equations";
    const suffix = prefix ? "" : "_equations";
    const text = (raw: unknown): string => {
      const row = object(raw);
      if (!Number.isSafeInteger(row.index) || (row.index as number) < 0) malformed();
      const name = nullableString(row.name);
      return `[${String(row.index)}]${name ? ` ${name}` : ""}\n${string(row.text)}\nTags: ${JSON.stringify(object(row.tags))}`;
    };
    for (const kind of ["added", "removed"] as const) array(owner[`${kind}${suffix}`]).forEach((raw, index) => {
      const row = object(raw);
      add(`${prefix}/${kind}${suffix}/${index}`, section, group, kind, nullableString(row.name) ?? `Equation ${String(row.index)}`, kind === "removed" ? text(row) : null, kind === "added" ? text(row) : null, dimension);
    });
    array(owner[`changed${suffix}`]).forEach((raw, index) => {
      const row = object(raw);
      const side = (key: string): string => text({ index: row[`index_${key}`], name: row[`name_${key}`], tags: row[`tags_${key}`], text: row[`text_${key}`] });
      add(`${prefix}/changed${suffix}/${index}`, section, group, "changed", nullableString(row.name_old) ?? nullableString(row.name_new) ?? "Equation", side("old"), side("new"), dimension);
    });
    array(owner.unmatched_same_name).forEach((raw, groupIndex) => {
      const unmatched = object(raw), name = string(unmatched.name);
      for (const kind of ["removed", "added"] as const) array(unmatched[kind]).forEach((row, index) => {
        add(`${prefix}/unmatched_same_name/${groupIndex}/${kind}/${index}`, section, group, "unpaired", `${name} · unpaired ${kind === "added" ? "After" : "Before"}`, kind === "removed" ? text(row) : null, kind === "added" ? text(row) : null, dimension);
      });
    });
  }
  equations(result, "", null);
  array(result.heterogeneous_equations).forEach((raw, index) => {
    const row = object(raw); equations(row, `/heterogeneous_equations/${index}`, string(row.dimension));
  });
  array(result.shock_setup_changes).forEach((raw, index) => {
    const row = object(raw), kind = string(row.change);
    if (!changeKinds.includes(kind as ChangeKind)) malformed();
    const section = ["shock_group", "init2shocks"].includes(string(row.form)) ? "shockAnalysisSetup" : "shockSetup";
    const side = (raw: unknown): string | null => {
      if (raw === undefined || raw === null) return null;
      const fields = Object.entries(object(raw)).filter(([key]) => !["location", "group_location", "origin_uri"].includes(key));
      return JSON.stringify(Object.fromEntries(fields), null, 2);
    };
    const dimension = (raw: unknown): string | null | undefined => {
      if (raw === undefined || raw === null) return undefined;
      const value = object(raw).heterogeneity;
      return value === undefined ? undefined : nullableString(value);
    };
    add(`/shock_setup_changes/${index}`, section, section === "shockSetup" ? "Shock setup" : "Shock analysis setup", kind as ChangeKind, `${string(row.target)} · ${string(row.form)} · ${string(row.role)}`, side(row.before), side(row.after), null, { before: dimension(row.before), after: dimension(row.after) });
  });
  const complete = before.complete && after.complete && !!before.revision && !!after.revision;
  const registry: SourceRegistry = { before: {}, after: {} };
  if (result.sources !== undefined) {
    const sources = object(result.sources);
    for (const side of ["before", "after"] as const) registry[side] = Object.fromEntries(Object.entries(object(sources[side])).map(([key, value]) => [key, string(value)]));
  }
  const details = parseSemantic(result, ids, registry, expectedSemantic);
  if (!details) return { before, after, rows: complete ? rows : [], complete, semanticMessage: "Semantic detail is unavailable with this engine. Showing structural changes; use Text diff for written edits." };
  const rowMap = new Map(rows.map(row => [row.id, row]));
  const isLocal = (row: SemanticRow): boolean => row.family === "symbols" && row.fields.some(field => field.name === "role" && [field.before, field.after].some(side => side.value?.kind === "text" && ["model_local_definition", "model_local_declaration"].includes(side.value.value)));
  const section = (row: SemanticRow): DiffSection => {
    const mapped: Record<string, DiffSection> = { symbols: "symbols", parameters: "parameters", shocks: "shockSetup", steady_state: "steadyState", priors: "priors", commands: "commands", observables: "observables", data: "data", occbin: "occbin", policy: "policy", semi_structural: "semiStructural", moments: "moments", ms_sbvar: "msSbvar", heterogeneity: "heterogeneity", external_functions: "externalFunctions", trends: "trends", operations: "operations", macro_context: "macroContext" };
    return row.family === "equations" || isLocal(row) ? "equations" : mapped[row.family];
  };
  for (const [index, row] of details.semantic.rows.entries()) {
    const source = navigation.get(row.pointer), legacy = rowMap.get(row.pointer);
    if (!legacy && row.pointer !== `/semantic/rows/${index}`) malformed();
    // The shared view group must not let typed detail change a legacy equation's domain.
    if (legacy && row.family === "equations" && legacy.id.startsWith("/heterogeneous_equations/") !== Boolean(row.before?.scope.dimension || row.after?.scope.dimension)) malformed();
    // Duplicate-name equations retain legacy add/remove pointers. Their typed
    // detail refines those same one-sided occurrences to Unpaired.
    const unpairedEquation = legacy && row.family === "equations" && row.change === "unpaired" &&
      (legacy.kind === "added" || legacy.kind === "removed") &&
      (legacy.before === null) === (row.before === null) && (legacy.after === null) === (row.after === null);
    if (!source || legacy && (legacy.kind !== row.change && !unpairedEquation || section(row) !== legacy.section && row.family !== "shocks") || !legacy && (source.kind !== "semantic" || source.family !== row.family || source.name !== row.name)) malformed();
    for (const side of ["before", "after"] as const) {
      if (!row[side] && source[side] || row[side] && source[side] && (source[side].domain !== (row[side].scope.dimension === null ? "aggregate" : "heterogeneous") || source[side].dimension !== row[side].scope.dimension)) malformed();
      for (const location of source[side]?.written_locations ?? []) {
        if (!snapshots && !(registeredSource ? registeredSource(side, location.uri) : Object.hasOwn(registry[side], location.uri))) malformed();
      }
    }
    const sideScopes = { before: row.before?.scope.dimension ?? (row.before ? row.before.scope.domain : null), after: row.after?.scope.dimension ?? (row.after ? row.after.scope.domain : null) };
    const projected: DiffRow = { id: row.pointer, section: legacy?.section ?? section(row), group: isLocal(row) ? "Model-local variables" : row.family === "parameters" ? "Parameters" : legacy?.group ?? semanticGroup(section(row)), kind: row.change, label: row.name, before: row.before?.name ?? null, after: row.after?.name ?? null, scopes: [...new Set(Object.values(sideScopes).filter((s): s is string => s !== null))], sideScopes, navigation: source, semantic: row };
    if (legacy) Object.assign(legacy, projected); else rows.push(projected);
  }
  const equationOwners = new Map(rows.filter(row => row.section === "equations").map(row => [row.id, row]));
  const references = details.semantic.references.map(ref => {
    const source = navigation.get(ref.pointer);
    if (!source || source.kind !== "reference" || source.name !== ref.symbol || source.equation_pointer !== ref.equation_pointer || source[ref.side === "before" ? "after" : "before"] !== null || source[ref.side] && (source[ref.side]!.domain !== (ref.scope.dimension === null ? "aggregate" : "heterogeneous") || source[ref.side]!.dimension !== ref.scope.dimension)) malformed();
    const equationTarget = navigation.get(ref.equation_pointer);
    const equationOwner = equationOwners.get(ref.equation_pointer);
    if (ref.equation_pointer !== ref.pointer && (!equationOwner || equationOwner[ref.side] === null)) malformed();
    if (ref.equation_pointer !== ref.pointer && !(equationTarget?.kind === "equation" || equationTarget?.kind === "semantic" && equationTarget.family === "equations")) malformed();
    for (const location of source[ref.side]?.written_locations ?? []) if (!snapshots && !(registeredSource ? registeredSource(ref.side, location.uri) : Object.hasOwn(registry[ref.side], location.uri))) malformed();
    return { ...ref, navigation: source };
  });
  return { before, after, rows: complete ? rows : [], complete, ...details, references: complete ? references : [] };
}

export function semanticGroup(section: DiffSection): string {
  const labels: Record<DiffSection, string> = { symbols: "Symbols", parameters: "Parameters", equations: "Equations", shockSetup: "Shock setup", shockAnalysisSetup: "Shock analysis setup", steadyState: "Steady state", priors: "Priors", commands: "Commands", observables: "Observables", data: "Data", occbin: "OccBin", policy: "Policy", semiStructural: "Semi-structural", moments: "Moments and IRFs", msSbvar: "MS-SBVAR", heterogeneity: "Heterogeneity", externalFunctions: "External functions", trends: "Trends", operations: "Operations", macroContext: "Macro context" };
  return labels[section];
}

/** Schema 2 keys are mapped only through the host's retained captured source registry. */
export function parseSnapshotDiff(value: unknown, ids: Record<DiffSide, string>, source: (side: DiffSide, key: string) => string,
  inputs: Record<DiffSide, { root_file: string; revision: string; commit?: string }>, expectedSemantic?: boolean): DiffSnapshot {
  const response = object(value), nav = object(response.navigation);
  if (response.state !== "result" || nav.schema_version !== 2) malformed();
  const envelope = (side: DiffSide) => {
    const input = object(nav[side]);
    if (input.input_id !== ids[side] || input.root_file !== inputs[side].root_file || input.revision !== inputs[side].revision || input.complete !== true) malformed();
    return { root_uri: ids[side], revision: nullableString(input.revision), complete: input.complete };
  };
  const mapped = (side: DiffSide, value: unknown): unknown => {
    if (value === null) return null;
    const item = object(value);
    const written_locations = array(item.written_locations).map(raw => {
      const entry = object(raw);
      if (entry.input_id !== ids[side] || typeof entry.file_key !== "string" || entry.commit !== inputs[side].commit) malformed();
      const uri = source(side, entry.file_key);
      if (!location({ uri, range: entry.range })) malformed();
      return { uri, range: entry.range };
    });
    return { ...item, written_locations };
  };
  const rows = array(nav.rows).map(raw => {
    const row = object(raw);
    return { ...row, before: mapped("before", row.before), after: mapped("after", row.after) };
  });
  return projectDiff({ ...object(response.diff), sources: response.sources, navigation: { schema_version: 1, before: envelope("before"), after: envelope("after"), rows } }, ids.before, ids.after, true, ids, expectedSemantic);
}

export function normalizeChoices(value: unknown, defaults: DiffPreferences): DiffChoices {
  const row = record(value) ? value : {};
  const list = <T extends string>(value: unknown, allowed: readonly T[], fallback: T[]): T[] => Array.isArray(value) ? [...new Set(value.filter((item): item is T => typeof item === "string" && allowed.includes(item as T)))] : [...fallback];
  let group = typeof row.group === "string" ? row.group.slice(0, 1000) : null;
  if (group?.startsWith("aggregateEquations:") || group?.startsWith("heterogeneousEquations:")) {
    group = group.endsWith(":Model-local variables") ? "equations:Model-local variables" : "equations:Equations";
  }
  // Retired presentation, selection, layout, section and scope state do not hide rows.
  return {
    changeKinds: list(row.changeKinds, changeKinds, defaults.changeKinds),
    customChangeKinds: list(row.customChangeKinds, changeKinds, list(row.changeKinds, changeKinds, defaults.changeKinds)),
    search: typeof row.search === "string" ? row.search.slice(0, 10000) : "",
    group,
  };
}

export function sameTarget(left: DiffTarget | null, right: DiffTarget | null): boolean {
  return JSON.stringify(left) === JSON.stringify(right);
}
