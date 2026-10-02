import { Location, location, record } from "./protocol";

export const diffSections = ["symbols", "parameters", "aggregateEquations", "heterogeneousEquations", "shockSetup", "shockAnalysisSetup"] as const;
export const changeKinds = ["added", "removed", "changed", "unpaired"] as const;
export type DiffSection = typeof diffSections[number];
export type ChangeKind = typeof changeKinds[number];
export type DiffSide = "before" | "after";
export interface DiffPreferences {
  layout: "auto" | "sideBySide" | "stacked";
  expansion: "changes" | "all" | "none";
  sections: DiffSection[]; changeKinds: ChangeKind[];
}
export interface DiffChoices extends DiffPreferences { search: string; scope: string; expanded: Record<string, boolean> }
export interface DiffTarget {
  occurrence_id: string; domain: string; dimension: string | null; written_locations: Location[];
}
export interface DiffNavigationRow { id: string; kind: string; dimension?: string | null; before: DiffTarget | null; after: DiffTarget | null }
export interface DiffEnvelope { root_uri: string; revision: string | null; complete: boolean }
export interface DiffRow {
  id: string; section: DiffSection; group: string; kind: ChangeKind; label: string;
  before: string | null; after: string | null; scopes: string[]; sideScopes: { before: string | null; after: string | null }; navigation: DiffNavigationRow;
}
export interface DiffSnapshot { before: DiffEnvelope; after: DiffEnvelope; rows: DiffRow[]; complete: boolean }
function malformed(): never { throw new Error("This engine returned an unsupported comparison. Update dynare.serverPath or use the bundled binary."); }
const string = (value: unknown): string => typeof value === "string" ? value : malformed();
const object = (value: unknown): Record<string, unknown> => record(value) ? value : malformed();
const array = (value: unknown): unknown[] => Array.isArray(value) ? value : malformed();
const nullableString = (value: unknown): string | null => value === null ? null : string(value);
function target(value: unknown): DiffTarget | null {
  if (value === null) return null;
  const row = object(value), locations = array(row.written_locations);
  if (!locations.every(item => location(item) && /^(file|untitled):/.test(item.uri))) malformed();
  return { occurrence_id: string(row.occurrence_id), domain: string(row.domain), dimension: nullableString(row.dimension),
    written_locations: (locations as Location[]).map(item => ({ uri: item.uri, range: { start: { ...item.range.start }, end: { ...item.range.end } } })) };
}
function envelope(value: unknown, root: string): DiffEnvelope {
  const row = object(value);
  if (row.root_uri !== root || typeof row.complete !== "boolean") malformed();
  return { root_uri: root, revision: nullableString(row.revision), complete: row.complete };
}
/** Project legacy rows for display; their exact pointers carry the engine's pairing. */
export function parseDiff(value: unknown, beforeRoot: string, afterRoot: string): DiffSnapshot {
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
    navigation.set(id, { id, kind: string(row.kind), dimension: row.dimension === undefined ? undefined : nullableString(row.dimension), before: target(row.before), after: target(row.after) });
  }
  const rows: DiffRow[] = [];
  const add = (id: string, section: DiffSection, group: string, kind: ChangeKind, label: string, old: string | null, next: string | null, dimension: string | null = null, sideDimensions?: { before: string | null | undefined; after: string | null | undefined }): void => {
    const source = navigation.get(id);
    if (!source) malformed();
    const expected = section === "parameters" ? "parameter" : section.endsWith("Equations") ? "equation" : section === "symbols" ? "symbol" : "shock";
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
    const section = dimension === null ? "aggregateEquations" : "heterogeneousEquations", group = dimension === null ? "Aggregate equations" : `Equations · ${dimension}`;
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
  return { before, after, rows: complete ? rows : [], complete };
}

export function normalizeChoices(value: unknown, defaults: DiffPreferences): DiffChoices {
  const row = record(value) ? value : {};
  const list = <T extends string>(value: unknown, allowed: readonly T[], fallback: T[]): T[] => Array.isArray(value) ? [...new Set(value.filter((item): item is T => typeof item === "string" && allowed.includes(item as T)))] : [...fallback];
  return {
    layout: ["auto", "sideBySide", "stacked"].includes(String(row.layout)) ? row.layout as DiffPreferences["layout"] : defaults.layout,
    expansion: ["changes", "all", "none"].includes(String(row.expansion)) ? row.expansion as DiffPreferences["expansion"] : defaults.expansion,
    sections: list(row.sections, diffSections, defaults.sections), changeKinds: list(row.changeKinds, changeKinds, defaults.changeKinds),
    search: typeof row.search === "string" ? row.search.slice(0, 10000) : "", scope: typeof row.scope === "string" ? row.scope.slice(0, 1000) : "all",
    expanded: record(row.expanded) ? Object.fromEntries(Object.entries(row.expanded).filter(([key, value]) => key.length <= 1000 && typeof value === "boolean")) as Record<string, boolean> : {},
  };
}

export function sameTarget(left: DiffTarget | null, right: DiffTarget | null): boolean {
  return JSON.stringify(left) === JSON.stringify(right);
}
