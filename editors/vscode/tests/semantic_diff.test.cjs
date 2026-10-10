/* global structuredClone */
const assert = require("node:assert/strict");
const test = require("node:test");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const { parseSemantic, semanticCapability, semanticFamilies } = require("../out/semantic_view");
const { parseDiff, parseSnapshotDiff, normalizeChoices, allDiffSections, changeKinds } = require("../out/diff_view");
const { createHost, captured, deferred, flush, load, working, comparison, anchorUri, baselineUri } = require("./helpers/changes_host.cjs");
const roots = { before: "file:///before/main.mod", after: "file:///after/main.mod" };
const scope = { domain: "global", dimension: null, block: null };
const side = name => ({ name, scope, occurrence: 2, equation_index: null, context: { kind: "command", name: "estimated_params", execution_order: 3, scope, pointer: null } });
const value = value => ({ state: "present", value: { kind: "text", value } });
const registry = { before: { [roots.before]: "var y;\n// old\n" }, after: { [roots.after]: "var y;\n// new\n" } };
function fixture() {
  return {
    comparison_versions: { semantic: 1, source_changes: 1, coverage: 1 },
    semantic: { schema_version: 1, availability: "partial", budgets: { token_alignment_cells: 250000, source_alignment_cells: 1000000, references_per_side: 2000, source_hunks: 2000, serialized_output_bytes: 8388608 },
      rows: [{ pointer: "/semantic/rows/0", family: "priors", change: "changed", name: "rho", count_unit: "accepted_occurrence", facets: ["prior", "expression"], before: side("rho"), after: side("rho"),
        fields: [{ name: "mean", label: "Prior mean", before: value("0.8"), after: value("0.9"), changed: true, comparison_availability: "complete", numeric_difference: null }],
        expressions: [{ field: "mean", before: { text: "0.8", runs: [{ text: "0.8", role: "removed" }] }, after: { text: "0.9", runs: [{ text: "0.9", role: "added" }] }, highlight_basis: "paired_expression", availability: "complete", reason: null }],
        timing: [], references: ["/semantic/references/0"], limits: [{ code: "prior_limit", reason: "Effective inherited prior is not established.", omitted: null, owner: "priors" }] }],
      references: [{ pointer: "/semantic/references/0", symbol: "rho", side: "after", equation_pointer: "/semantic/references/0", equation_index: 1, label: "Unchanged production", scope: { domain: "aggregate", dimension: null, block: null }, occurrence: 0, timing: { name: "rho", class: "parameter", written_offset: 0, converted_offset: 0, occurrence: 0 } }], limits: [] },
    source_changes: { schema_version: 1, availability: "complete", files: [{ pointer: "/source_changes/files/0", change: "changed", correspondence: "selected_roots", before: { input_id: null, file_key: roots.before, exact_text_available: true }, after: { input_id: null, file_key: roots.after, exact_text_available: true }, availability: "complete", omitted_hunks: null, limits: [], hunks: [{ before_start: 2, before_lines: 1, after_start: 2, after_lines: 1, lines: [{ text: "// old", role: "removed" }, { text: "// new", role: "added" }] }] }], limits: [] },
    coverage: { schema_version: 1, availability: "partial", source_boundary: "captured_roots_and_executed_includes", families: [{ family: "priors", availability: "partial", fields: ["mean"], limits: [{ code: "prior_limit", reason: "Effective inherited prior is not established.", omitted: null, owner: "priors" }] }], limits: [] },
  };
}
function response() {
  const result = fixture();
  for (const category of ["endogenous", "exogenous", "parameters"]) for (const change of ["added", "removed", "common"]) result[change + "_" + category] = [];
  for (const list of ["symbols_changed", "changed_parameter_values", "added_equations", "removed_equations", "changed_equations", "unmatched_same_name", "heterogeneous_equations", "shock_setup_changes"]) result[list] = [];
  const target = side => ({ occurrence_id: side + ":2", domain: "aggregate", dimension: null, written_locations: [{ uri: roots[side], range: { start: { line: 0, character: 0 }, end: { line: 0, character: 4 } } }] });
  result.navigation = { schema_version: 1, before: { root_uri: roots.before, revision: "old", complete: true }, after: { root_uri: roots.after, revision: "new", complete: true }, rows: [
    { id: "/semantic/rows/0", kind: "semantic", family: "priors", name: "rho", before: target("before"), after: target("after") },
    { id: "/semantic/references/0", kind: "reference", name: "rho", equation_pointer: "/semantic/references/0", before: null, after: target("after") },
  ] };
  result.sources = registry; return result;
}
const defaults = { sections: [...allDiffSections], changeKinds: [...changeKinds] };

// The real Working/Working LSP producer keeps duplicate-name occurrences in
// legacy add/remove arrays and keeps their semantic owners as separate side facts.
function uncertainEquations(dimension = null) {
  const r = response(), prefix = dimension === null ? "" : "/heterogeneous_equations/0";
  const owner = dimension === null ? r : { dimension, added: [], removed: [], changed: [], unmatched_same_name: [] };
  if (dimension !== null) r.heterogeneous_equations.push(owner);
  const suffix = dimension === null ? "_equations" : "", equationScope = { domain: dimension === null ? "aggregate" : "heterogeneous", dimension, block: null };
  r.semantic.rows = []; r.semantic.references = []; r.navigation.rows = [];
  for (const [kind, activeSide, text, index] of [["removed", "before", "k = y", 0], ["removed", "before", "x = 1", 1], ["added", "after", "k = y+1", 0], ["added", "after", "z = 2", 1]]) {
    const equation = { domain: equationScope.domain, dimension, index: index + 1, name: "Ambiguous", tags: { name: "Ambiguous" }, text };
    owner[kind + suffix].push(equation);
    const pointer = `${prefix}/${kind}${suffix}/${index}`, target = { occurrence_id: activeSide + ":" + index, domain: equationScope.domain, dimension, written_locations: [{ uri: roots[activeSide], range: { start: { line: 0, character: 0 }, end: { line: 0, character: 4 } } }] };
    const own = { name: "Ambiguous", scope: equationScope, occurrence: 48 + index, equation_index: index + 1, context: { kind: "block", name: "model", execution_order: 4, scope: equationScope, pointer: null } };
    r.semantic.rows.push({ pointer, family: "equations", change: kind, name: "Ambiguous", count_unit: "accepted_occurrence", facets: ["expression"], before: activeSide === "before" ? own : null, after: activeSide === "after" ? own : null,
      fields: [{ name: "expression", label: "Expression", before: activeSide === "before" ? value(text) : { state: "absent", value: null }, after: activeSide === "after" ? value(text) : { state: "absent", value: null }, changed: true, comparison_availability: "complete", numeric_difference: null }],
      expressions: [{ field: "expression", before: activeSide === "before" ? { text, runs: [{ text, role: "unchanged" }] } : null, after: activeSide === "after" ? { text, runs: [{ text, role: "unchanged" }] } : null, highlight_basis: "unpaired_text_only", availability: "complete", reason: null }],
      timing: [], references: [], limits: [{ code: "equation_correspondence_unpaired", reason: "Text-only highlights do not establish equation, reference or timing correspondence.", owner: "semantic_equations", omitted: null }] });
    r.navigation.rows.push({ id: pointer, kind: "equation", before: activeSide === "before" ? target : null, after: activeSide === "after" ? target : null });
    r.navigation.rows.push({ id: `${prefix}/unmatched_same_name/0/${kind}/${index}`, kind: "equation", before: activeSide === "before" ? target : null, after: activeSide === "after" ? target : null });
  }
  owner.unmatched_same_name.push({ name: "Ambiguous", dimension, removed: owner["removed" + suffix], added: owner["added" + suffix] });
  return r;
}

test("uncertain equations keep legacy add/remove owners and separate side appearances", () => {
  for (const dimension of [null, "households"]) {
    const r = uncertainEquations(dimension);
    const parsed = parseDiff(r, roots.before, roots.after, true);
    assert.equal(parsed.rows.length, 8); assert.ok(parsed.rows.every(row => row.kind === (row.before ? "removed" : "added") && row.section === "equations" && row.group === "Equations"));
    assert.equal(parsed.rows.filter(row => row.semantic).length, 4);
    for (const row of parsed.rows) assert.notEqual(row.before === null, row.after === null);
    const snapshot = structuredClone(r), ids = { before: "before", after: "after" }, inputs = { before: { root_file: roots.before, revision: "old" }, after: { root_file: roots.after, revision: "new" } };
    for (const side of ["before", "after"]) {
      snapshot.navigation[side] = { input_id: ids[side], ...inputs[side], complete: true };
      snapshot.source_changes.files[0][side].input_id = ids[side];
      for (const row of snapshot.navigation.rows) if (row[side]) row[side] = { ...row[side], written_locations: row[side].written_locations.map(location => ({ input_id: ids[side], file_key: location.uri, range: location.range })) };
    }
    const projected = parseSnapshotDiff({ state: "result", diff: snapshot, navigation: { ...snapshot.navigation, schema_version: 2 }, sources: registry }, ids, (side, key) => key, inputs, true);
    assert.deepEqual(projected.rows.map(row => [row.id, row.kind]), parsed.rows.map(row => [row.id, row.kind]));
  }
});

test("typed equation detail cannot replace another family, change domain or reverse an owner side", () => {
  for (const mutate of [r => r.semantic.rows[0].family = "symbols", r => { r.semantic.rows[0].after = r.semantic.rows[0].before; r.semantic.rows[0].before = null; }, r => r.semantic.rows[0].change = "added"]) {
    const r = uncertainEquations(); mutate(r);
    assert.throws(() => parseDiff(r, roots.before, roots.after, true), /unsupported comparison/);
  }
  for (const dimension of [null, "households"]) {
    const r = uncertainEquations(dimension);
    r.navigation.rows[0].before = null;
    r.semantic.rows[0].before.scope = { domain: dimension === null ? "heterogeneous" : "aggregate", dimension: dimension === null ? "firms" : null, block: null };
    assert.throws(() => parseDiff(r, roots.before, roots.after, true), /unsupported comparison/);
  }
});

test("v1 retains typed values, exact runs, field limits and source registry identities", () => {
  const parsed = parseSemantic(fixture(), { before: null, after: null }, registry, true);
  assert.equal(parsed.semantic.rows[0].fields[0].after.value.value, "0.9");
  assert.equal(parsed.sourceChanges.files[0].after.file_key, roots.after);
  const projection = parseDiff(response(), roots.before, roots.after, true);
  assert.equal(projection.rows.length, 1); assert.equal(projection.rows[0].section, "priors");
  assert.equal(projection.references.length, 1); assert.equal(projection.rows[0].scopes[0], "global");
});
test("closed schemas reject malformed advertised versions, values, runs and cross-side identity", () => {
  const mutations = [
    r => r.semantic.schema_version = 2, r => r.comparison_versions.coverage = 9,
    r => r.semantic.extra = "unsafe", r => r.semantic.rows[0].family = "invented",
    r => r.semantic.rows[0].fields[0].before.value.kind = "json",
    r => r.semantic.rows[0].fields[0].numeric_difference = Infinity,
    r => r.semantic.rows[0].fields[0].before.state = "absent",
    r => r.semantic.rows[0].expressions[0].before.runs[0].text = "wrong",
    r => r.semantic.rows[0].expressions[0].after.runs[0].role = "removed",
    r => r.semantic.rows[0].references[0] = "/semantic/references/4",
    r => r.source_changes.files[0].after.input_id = "before",
    r => r.source_changes.files[0].after.file_key = roots.before,
    r => r.source_changes.files[0].hunks[0].after_lines = 9,
    r => r.semantic.rows.push(structuredClone(r.semantic.rows[0])),
  ];
  for (const mutate of mutations) { const r = fixture(); mutate(r); assert.throws(() => parseSemantic(r, { before: null, after: null }, registry, true), /unsupported comparison/); }
  assert.throws(() => parseSemantic({}, { before: null, after: null }, registry, true), /unsupported/);
  assert.throws(() => parseSemantic(fixture(), { before: null, after: null }, registry, false), /unsupported/);
});
test("older advertisement falls back while partial and unsupported advertisements fail", () => {
  assert.equal(semanticCapability({ schema_version: 1 }), false);
  assert.equal(semanticCapability({ semantic_schema_version: 1, source_changes_schema_version: 1, coverage_schema_version: 1 }), true);
  assert.throws(() => semanticCapability({ semantic_schema_version: 1 }), /unsupported/);
  assert.throws(() => semanticCapability({ semantic_schema_version: 2, source_changes_schema_version: 1, coverage_schema_version: 1 }), /unsupported/);
  const r = response(); for (const key of ["comparison_versions", "semantic", "source_changes", "coverage"]) delete r[key];
  const parsed = parseDiff(r, roots.before, roots.after, false); assert.match(parsed.semanticMessage, /unavailable/);
});
test("wrong navigation kind, identity and reference side fail before rendering", () => {
  for (const mutate of [r => r.navigation.rows[0].family = "commands", r => r.navigation.rows[0].after.dimension = "firms", r => r.navigation.rows[1].before = r.navigation.rows[1].after, r => r.navigation.rows[1].equation_pointer = "/bad"]) {
    const r = response(); mutate(r); assert.throws(() => parseDiff(r, roots.before, roots.after, true), /unsupported comparison/);
  }
});
test("new semantic rows require real owners and unavailable highlights stay plain", () => {
  for (const pointer of ["/missing/row", "/changed_parameter_values/999", "/semantic/rows/9"]) {
    const r = response(); r.semantic.rows[0].pointer = pointer; r.navigation.rows[0].id = pointer;
    assert.throws(() => parseDiff(r, roots.before, roots.after, true), /unsupported comparison/);
  }
  const missing = response(), ref = missing.semantic.references[0];
  ref.equation_pointer = "/missing/equation"; missing.navigation.rows[1].equation_pointer = ref.equation_pointer;
  missing.navigation.rows.push({ id: ref.equation_pointer, kind: "equation", before: null, after: missing.navigation.rows[1].after });
  assert.throws(() => parseDiff(missing, roots.before, roots.after, true), /unsupported comparison/);
  const r = fixture(), expression = r.semantic.rows[0].expressions[0];
  expression.availability = "limit_exceeded"; expression.reason = "Alignment budget exceeded.";
  assert.throws(() => parseSemantic(r, { before: null, after: null }, registry, true), /unsupported comparison/);
  expression.highlight_basis = "none";
  assert.throws(() => parseSemantic(r, { before: null, after: null }, registry, true), /unsupported comparison/);
  for (const side of [expression.before, expression.after]) side.runs = [{ text: side.text, role: "unchanged" }];
  assert.equal(parseSemantic(r, { before: null, after: null }, registry, true).semantic.rows[0].expressions[0].highlight_basis, "none");
});
test("all retained families have a frontend section and preserve their count unit", () => {
  for (const family of semanticFamilies) {
    const r = response(); r.semantic.rows[0].family = family; r.navigation.rows[0].family = family;
    const row = parseDiff(r, roots.before, roots.after, true).rows[0];
    assert.ok(allDiffSections.includes(row.section)); assert.equal(row.semantic.count_unit, "accepted_occurrence");
  }
});

// Guards forecast instructions being filed under their backend collection family.
test("forecast commands and paths have a Forecast category; state blocks have State", () => {
  for (const [family, command] of [["commands", "forecast"], ["commands", "bvar_forecast"], ["ms_sbvar", "conditional_forecast"], ["ms_sbvar", "conditional_forecast_paths"], ["ms_sbvar", "plot_conditional_forecast"]]) {
    const r = response(), detail = r.semantic.rows[0];
    detail.family = r.navigation.rows[0].family = family;
    detail.before.context.name = detail.after.context.name = command;
    const row = parseDiff(r, roots.before, roots.after, true).rows[0];
    assert.equal(row.section, "forecast"); assert.equal(row.group, "Forecast");
  }
  const r = response(); r.semantic.rows[0].family = r.navigation.rows[0].family = "steady_state";
  r.semantic.rows[0].before.context.name = r.semantic.rows[0].after.context.name = "steady_state_model";
  assert.equal(parseDiff(r, roots.before, roots.after, true).rows[0].group, "State");
});

// Guards one accepted instruction being spread across its retained family and Commands.
test("retained facts and residual instruction context share one category", () => {
  for (const [name, family, section, group] of [
    ["steady_state_model", "steady_state", "steadyState", "State"],
    ["varobs", "observables", "data", "Data"],
    ["varexobs", "observables", "data", "Data"],
    ["estimation", "data", "commands", "Commands"],
    ["matched_moments", "moments", "moments", "Method of moments"],
  ]) {
    const r = response(), retained = r.semantic.rows[0], residual = structuredClone(retained);
    retained.family = r.navigation.rows[0].family = family;
    retained.before.context.name = retained.after.context.name = name;
    residual.pointer = "/semantic/rows/1"; residual.family = "commands";
    residual.before.context.name = residual.after.context.name = name;
    r.semantic.rows.push(residual);
    r.navigation.rows.push({ ...r.navigation.rows[0], id: residual.pointer, family: "commands" });
    const rows = parseDiff(r, roots.before, roots.after, true).rows;
    assert.deepEqual(rows.map(row => [row.section, row.group]), [[section, group], [section, group]], name);
    assert.deepEqual(rows.map(row => row.id), [retained.pointer, residual.pointer]);
  }
});

// Guards each observable using its own target instead of the entire parent list.
test("observable cards show one declaration per name in the merged Data category", () => {
  for (const [keyword, names, capturedKind] of [["varobs", ["y", "c"], "var"], ["varexobs", ["e", "u"], "varexo"]]) for (const [change, activeSide] of [["added", "after"], ["removed", "before"]]) {
    const r = response(); r.semantic.rows = []; r.semantic.references = []; r.navigation.rows = [];
    for (const [index, name] of names.entries()) {
      const own = side(name); own.context.name = keyword;
      const text = `${keyword} ${names.join(" ")};`;
      const pointer = `/semantic/rows/${index}`;
      const present = { text, runs: [{ text, role: change }] }, absent = { state: "absent", value: null };
      r.semantic.rows.push({ pointer, family: "observables", change, name, count_unit: "accepted_occurrence", facets: ["target"], before: activeSide === "before" ? own : null, after: activeSide === "after" ? own : null,
        fields: [["role", keyword], ["target", name], ["captured_kind", capturedKind]].map(([name, text]) => ({ name, label: name, before: activeSide === "before" ? value(text) : absent, after: activeSide === "after" ? value(text) : absent, changed: true, comparison_availability: "complete", numeric_difference: null })),
        expressions: [{ field: "statement_text", before: activeSide === "before" ? present : null, after: activeSide === "after" ? present : null, availability: "complete", highlight_basis: "paired_expression", reason: null }], timing: [], references: [], limits: [] });
      const target = { occurrence_id: `${activeSide}:${index}`, domain: "aggregate", dimension: null, written_locations: [] };
      r.navigation.rows.push({ id: pointer, kind: "semantic", family: "observables", name, before: activeSide === "before" ? target : null, after: activeSide === "after" ? target : null });
    }
    const snapshot = parseDiff(r, roots.before, roots.after, true), env = webview(); env.render(snapshot);
    assert.ok(snapshot.rows.every(row => row.section === "data" && row.group === "Data"));
    const expressions = descendants(env.elements.results).filter(node => node.className === "expression");
    assert.deepEqual(expressions.map(node => node.textContent), names.map(name => `${keyword} ${name};`));
    const highlighted = descendants(env.elements.results).filter(node => node.className === `token ${change}`);
    assert.deepEqual(highlighted.map(node => node.textContent), names.flatMap(name => [keyword, name]));
    assert.equal(descendants(env.elements.results).filter(node => node.attributes["data-field"]).length, 0);
  }
});

// Guards the renderer using engine keyword runs for an added name inside an
// existing observable declaration, instead of coloring the shared keyword.
test("a new observable in an existing declaration keeps its keyword plain", () => {
  const r = response(), row = r.semantic.rows[0];
  row.family = "observables"; row.name = "c"; row.change = "added";
  row.before = null; row.after = side("c"); row.after.context.name = "varobs";
  row.references = []; r.semantic.references = [];
  row.fields = [["role", "varobs"], ["target", "c"], ["captured_kind", "var"]].map(([name, text]) => ({ name, label: name, before: { state: "absent", value: null }, after: value(text), changed: true, comparison_availability: "complete", numeric_difference: null }));
  row.expressions = [{ field: "statement_text", before: null, after: { text: "varobs y c;", runs: [{ text: "varobs", role: "unchanged" }, { text: " y c", role: "added" }, { text: ";", role: "unchanged" }] }, highlight_basis: "paired_expression", availability: "complete", reason: null }];
  r.navigation.rows = [{ ...r.navigation.rows[0], family: "observables", name: "c", before: null }];
  const env = webview(); env.render(parseDiff(r, roots.before, roots.after, true));
  assert.deepEqual(descendants(env.elements.results).filter(node => node.className === "expression").map(node => node.textContent), ["varobs c;"]);
  assert.deepEqual(descendants(env.elements.results).filter(node => node.className === "token added").map(node => node.textContent), ["c"]);
});

test("Unpaired is rejected as a semantic change type", () => {
  const r = response(), row = r.semantic.rows[0];
  row.change = "unpaired";
  for (const expression of row.expressions) expression.highlight_basis = "unpaired_text_only";
  assert.throws(() => parseSemantic(r, { before: null, after: null }, registry), /unsupported comparison/);
});

// Guards uncertain moment occurrences staying separate while using Added/Removed filters.
test("uncertain moment occurrences retain engine removal and addition types", () => {
  const r = uncertainEquations();
  r.added_equations = []; r.removed_equations = []; r.unmatched_same_name = [];
  r.navigation.rows = r.navigation.rows.filter(row => !row.id.includes("unmatched_same_name"));
  for (const [index, row] of r.semantic.rows.entries()) {
    const nav = r.navigation.rows[index]; row.pointer = nav.id = `/semantic/rows/${index}`;
    row.family = nav.family = "moments"; row.name = nav.name = "matched moment";
    nav.kind = "semantic";
    for (const side of ["before", "after"]) if (row[side]) row[side].context.name = "matched_moments";
  }
  const snapshot = parseDiff(r, roots.before, roots.after, true);
  assert.deepEqual(snapshot.rows.map(row => row.kind), ["removed", "removed", "added", "added"]);
  assert.ok(snapshot.rows.every(row => row.semantic.change === row.kind));
  const env = webview();
  for (const [kind, side] of [["removed", "before"], ["added", "after"]]) {
    env.render({ ...snapshot, choices: normalizeChoices({ changeKinds: [kind] }, defaults) });
    const cards = descendants(env.elements.results).filter(node => node.attributes["data-row-id"]);
    assert.deepEqual(cards.map(card => card.attributes["data-row-id"]), snapshot.rows.filter(row => row[side]).map(row => row.id));
  }
  assert.doesNotMatch(env.elements.kinds.textContent, /Unpaired/);
});

test("retired Unpaired filters and renamed groups restore under current choices", () => {
  const choices = normalizeChoices({ changeKinds: ["changed", "unpaired"], customChangeKinds: ["unpaired"], group: "moments:Moments and IRFs" }, defaults);
  assert.deepEqual(choices.changeKinds, ["changed", "added", "removed"]);
  assert.deepEqual(choices.customChangeKinds, ["added", "removed"]);
  assert.equal(choices.group, "moments:Method of moments");
  assert.equal(normalizeChoices({ group: "observables:Observables" }, defaults).group, "data:Data");
});
test("typed parameter details enrich the legacy owner under Parameters", () => {
  const r = response(), row = r.semantic.rows[0];
  row.pointer = "/changed_parameter_values/0"; row.family = "parameters";
  r.changed_parameter_values = [{ name: "rho", old_raw: "0.8", old_value: 0.8, new_raw: "0.9", new_value: 0.9 }];
  const nav = r.navigation.rows[0]; nav.id = row.pointer; nav.kind = "parameter"; delete nav.family;
  const parsed = parseDiff(r, roots.before, roots.after, true);
  assert.equal(parsed.rows.length, 1); assert.equal(parsed.rows[0].id, row.pointer); assert.equal(parsed.rows[0].group, "Parameters");
});
test("unavailable comparisons remain plain and unknown, empty, absent and zero stay distinct", () => {
  const r = fixture(), field = r.semantic.rows[0].fields[0];
  field.before = { state: "empty", value: { kind: "text", value: "" } }; field.after = { state: "unknown", value: null };
  field.changed = false; field.comparison_availability = "limit_exceeded";
  assert.equal(parseSemantic(r, { before: null, after: null }, registry).semantic.rows[0].fields[0].changed, false);
  field.changed = true; assert.throws(() => parseSemantic(r, { before: null, after: null }, registry), /unsupported/);
});
test("empty captured files are valid registry entries and opaque keys are not rewritten", () => {
  const r = fixture(), key = "folder/../empty.data";
  r.source_changes.files[0].before.file_key = key;
  const source = { before: { [key]: "" }, after: registry.after };
  assert.equal(parseSemantic(r, { before: null, after: null }, source).sourceChanges.files[0].before.file_key, key);
});
test("retired presentation, layout and selection state are ignored while current filters survive", () => {
  const { host, vscode } = createHost(), { diffPreferences } = load("diff", vscode);
  host.settings.set(anchorUri.toString(), { "diff.presentation": "changeList", "diff.layout": "stacked" });
  assert.equal(Object.hasOwn(diffPreferences(anchorUri, () => {}), "layout"), false);
  const restored = normalizeChoices({ presentation: "changeList", tab: "coverage", presentations: { focusedReview: { search: "older" } }, search: "retained", layout: "stacked", selected: "/old", capture: "old", limitsOpen: true }, defaults);
  assert.equal(restored.search, "retained");
  for (const key of ["layout", "selected", "capture", "limitsOpen"]) assert.equal(Object.hasOwn(restored, key), false);
  assert.equal(Object.hasOwn(restored, "presentation"), false); assert.equal(Object.hasOwn(restored, "tab"), false);
  host.settings.delete(anchorUri.toString());
});

test("current LSP native source keys remain exact while URI navigation uses the registered mapping", async t => {
  const { host, service, vscode, git, token } = createHost(), { HistoricalSources } = load("history_sources", vscode), history = new HistoricalSources(async () => "unused"); t.after(() => history.dispose());
  service.client.initializeResult.capabilities.experimental = { dygnosis: { compareModels: { schema_version: 1, navigation_schema_version: 1, semantic_schema_version: 1, source_changes_schema_version: 1, coverage_schema_version: 1 } } };
  host.engine = () => {
    const r = response(), selected = { before: baselineUri, after: anchorUri };
    r.sources = { before: { [baselineUri.fsPath]: "var y;" }, after: { [anchorUri.fsPath]: "var y;" } };
    for (const side of ["before", "after"]) {
      r.navigation[side] = { root_uri: selected[side].toString(), revision: "revision:" + selected[side].toString(), complete: true };
      for (const nav of r.navigation.rows) if (nav[side]) nav[side].written_locations[0].uri = selected[side].toString();
      r.source_changes.files[0][side].file_key = selected[side].fsPath;
    }
    return r;
  };
  const { captureComparison } = load("snapshot_compare", vscode), result = await captureComparison(service, async () => git, history, comparison(working(baselineUri), working(anchorUri)), token());
  assert.equal(result.legacy, false); assert.equal(result.snapshot.rows[0].section, "priors");
  assert.equal(result.sourceUris.after.get(anchorUri.fsPath).toString(), anchorUri.toString());
  assert.equal(result.snapshot.sourceChanges.files[0].after.file_key, anchorUri.fsPath);
});
test("valid older compareModels capability preserves Working structural fallback", async t => {
  const { host, service, vscode, git, token } = createHost();
  const { HistoricalSources } = load("history_sources", vscode), history = new HistoricalSources(async () => "unused");
  t.after(() => history.dispose());
  service.client.initializeResult.capabilities.experimental.dygnosis = { compareModels: { command: "dynare/compareModels", navigation_schema_version: 1 } };
  host.engine = (_command, [before, after]) => {
    const r = response();
    for (const key of ["semantic", "source_changes", "coverage", "comparison_versions", "sources"]) delete r[key];
    r.navigation.rows = [];
    r.navigation.before = { root_uri: before, revision: `revision:${before}`, complete: true };
    r.navigation.after = { root_uri: after, revision: `revision:${after}`, complete: true };
    return r;
  };
  const { captureComparison } = load("snapshot_compare", vscode);
  const result = await captureComparison(service, async () => git, history, comparison(working(baselineUri), working(anchorUri)), token());
  assert.equal(result.legacy, true);
  assert.match(result.snapshot.semanticMessage, /Semantic detail is unavailable/);
});
class Element {
  constructor(tag) { this.tag = tag; this.children = []; this.listeners = {}; this.attributes = {}; this._text = ""; }
  set textContent(value) { this._text = value; this.children = []; }
  get textContent() { return this._text + this.children.map(child => child.textContent).join(""); }
  append(...children) { this.children.push(...children); }
  replaceChildren(...children) { this.children = children; this._text = ""; }
  addEventListener(name, listener) { this.listeners[name] = listener; }
  setAttribute(name, value) { this.attributes[name] = value; }
  getAttribute(name) { return this.attributes[name]; }
  focus() { this.focused = true; }
  fire(name, event = {}) { this.listeners[name]?.(event); }
}
const descendants = element => [element, ...element.children.flatMap(descendants)];
test("model-local definitions appear with equations and keep their exact source identity", () => {
  const r = response(), row = r.semantic.rows[0];
  row.family = "symbols"; row.name = "helper"; row.references = [];
  row.fields.push({ name: "role", label: "Role", before: value("model_local_definition"), after: value("model_local_definition"), changed: false, comparison_availability: "complete", numeric_difference: null });
  Object.assign(r.navigation.rows[0], { family: "symbols", name: "helper" }); r.semantic.references = []; r.navigation.rows.pop();
  const parsed = parseDiff(r, roots.before, roots.after, true).rows[0];
  assert.equal(parsed.section, "equations");
  assert.equal(parsed.group, "Model-local variables");
  assert.equal(parsed.id, row.pointer);
});

test("unassigned parameter declarations keep their Symbols category and old section filters cannot hide them", () => {
  const r = response(), detail = r.semantic.rows[0], source = r.navigation.rows[0];
  r.added_parameters = ["cdxx"];
  Object.assign(detail, { pointer: "/added_parameters/0", family: "symbols", change: "added", name: "cdxx", before: null, after: side("cdxx"), expressions: [], references: [], limits: [] });
  detail.fields = [{ name: "kind", label: "Symbol kind", before: { state: "absent", value: null }, after: value("parameters"), changed: true, comparison_availability: "complete", numeric_difference: null }];
  Object.assign(source, { id: detail.pointer, kind: "symbol", name: "cdxx", before: null });
  r.semantic.references = []; r.navigation.rows.pop();
  for (const semantic of [true, false]) {
    const input = structuredClone(r); if (!semantic) { delete input.semantic; delete input.comparison_versions; delete input.coverage; delete input.source_changes; }
    const snapshot = parseDiff(input, roots.before, roots.after, semantic), env = webview();
    assert.equal(snapshot.rows[0].section, "symbols");
    env.render({ ...snapshot, choices: normalizeChoices({ sections: [], groupVisibility: { "symbols:Symbols": false } }, defaults) });
    assert.match(env.elements.results.textContent, /cdxx/);
    assert.equal(descendants(env.elements.results).some(element => element.tag === "input"), false);
  }
});

test("added symbol cards show one declaration and omit repeated or empty metadata", () => {
  const env = webview(), snapshot = parseDiff(response(), roots.before, roots.after, true), row = snapshot.rows[0], detail = row.semantic;
  Object.assign(row, { kind: "added", before: null, label: "xxx", after: "xxx", scopes: ["households"], sideScopes: { before: null, after: "households" } });
  Object.assign(detail, { family: "symbols", change: "added", name: "xxx", before: null, after: { ...side("xxx"), scope: { domain: "heterogeneous", dimension: "households", block: null } }, expressions: [], references: [], limits: [] });
  detail.fields = [["kind", "Symbol kind", "var"], ["written_kind", "Written declaration kind", "var"], ["long_name", "Long name", null], ["log_transform", "Log transformed", false], ["dimension", "Dimension", "households"], ["written_dimension", "Written declaration dimension", "households"]].map(([name, label, item]) => ({ name, label, before: { state: "absent", value: null }, after: item === null ? { state: "absent", value: null } : { state: "present", value: { kind: typeof item === "boolean" ? "boolean" : "text", value: item } }, changed: true, comparison_availability: "complete", numeric_difference: null }));
  row.navigation.before = null; env.render(snapshot);
  assert.match(env.elements.results.textContent, /var xxx;/);
  assert.doesNotMatch(env.elements.results.textContent, /Symbol kind|Written declaration kind|Long name|false/);
  assert.match(env.elements.results.textContent, /Not present/);
  assert.deepEqual(descendants(env.elements.results).filter(element => element.className === "token added").map(element => element.textContent), ["xxx"]);
  assert.deepEqual(descendants(env.elements.results).filter(element => element.className === "scope-label").map(element => element.textContent), ["Dimension: households"]);
  assert.equal(descendants(env.elements.results).some(element => ["dimension", "written_dimension"].includes(element.attributes["data-field"])), false);
  assert.doesNotMatch(env.elements.counts.textContent, /· \d+ in /);
  detail.fields[0].after = value("parameters"); env.render(snapshot);
  assert.match(env.elements.results.textContent, /Symbol kind/); assert.match(env.elements.results.textContent, /parameters/);
});
test("direct references belong to their card side and retain exact actions", () => {
  const env = webview(); env.render();
  const panels = descendants(env.elements.results).filter(element => ["side before", "side after"].includes(element.className));
  assert.doesNotMatch(panels[0].textContent, /Direct equation references/);
  assert.match(panels[1].textContent, /Direct equation references.*Unchanged production/);
  const button = descendants(panels[1]).find(element => element.attributes["aria-label"]?.startsWith("Open After reference:"));
  button.fire("click"); assert.equal(env.posted.at(-1).pointer, "/semantic/references/0"); assert.equal(env.posted.at(-1).side, "after");
  const snapshot = parseDiff(response(), roots.before, roots.after, true), row = snapshot.rows[0];
  row.semantic.after.scope = { domain: "heterogeneous", dimension: "households", block: null };
  row.sideScopes.after = "households"; row.scopes.push("households"); snapshot.references[0].scope.dimension = "households";
  env.render(snapshot);
  const after = descendants(env.elements.results).find(element => element.className === "side after");
  assert.match(after.children[0].textContent, /Dimension: households/);
  assert.doesNotMatch(descendants(after).find(element => element.className === "refs").textContent, /Dimension:/);
  snapshot.references[0].scope.dimension = "firms"; env.render(snapshot);
  assert.match(descendants(env.elements.results).find(element => element.className === "refs").textContent, /Dimension: firms/);
  row.semantic.family = "symbols"; snapshot.references[0].timing.converted_offset = -1;
  env.render(snapshot); assert.match(env.elements.results.textContent, /after convention -1/);
  row.semantic.fields.push({ name: "predetermined", label: "Predetermined convention", before: { state: "absent", value: null }, after: { state: "present", value: { kind: "boolean", value: true } }, changed: true, comparison_availability: "complete", numeric_difference: null });
  env.render(snapshot); assert.match(env.elements.results.textContent, /Predetermined convention.*true/); assert.doesNotMatch(env.elements.results.textContent, /after convention -1/);
  assert.deepEqual(descendants(env.elements.results).filter(element => element.attributes["data-field"] === "predetermined").map(element => element.children[1].textContent), ["", "true"]);
});
function webview() {
  const elements = Object.fromEntries(["models", "status", "search", "scope", "kinds", "sections", "sectionsSummary", "counts", "results", "refresh", "help", "rootTextDiff", "capturedTextDiff", "filterTools", "kindFilter", "sectionFilter", "scopeFilter", "choosePath"].map(id => [id, new Element(id)]));
  const events = {}, posted = [], states = [], snapshot = parseDiff(response(), roots.before, roots.after, true);
  const sandbox = { document: { getElementById: id => elements[id] ?? descendants(elements.results).find(element => element.id === id), createElement: tag => new Element(tag), querySelectorAll: () => descendants(elements.results).filter(element => element.attributes["data-group"]) }, window: { addEventListener: (name, callback) => events[name] = callback }, acquireVsCodeApi: () => ({ getState: () => undefined, setState: value => states.push(structuredClone(value)), postMessage: value => posted.push(value) }) };
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, "../media/diff_view.js"), "utf8"), sandbox);
  const render = (extra = {}) => events.message({ data: { ...snapshot, type: "render", key: "view", capture: "capture1", token: 3, status: "ready", message: "Current comparison", choices: normalizeChoices({}, defaults), defaults, ...extra, before: typeof extra.before === "string" ? extra.before : roots.before, after: typeof extra.after === "string" ? extra.after : roots.after } });
  return { elements, posted, states, render };
}
test("grouped rows preserve filters and the selected group through reload", () => {
  const env = webview(); env.render();
  assert.match(env.elements.results.textContent, /Prior mean/); assert.match(env.elements.results.textContent, /0.8/);
  env.elements.search.value = "rho"; env.elements.search.fire("input");
  env.render({ choices: normalizeChoices(env.states.at(-1).choices, defaults) });
  assert.equal(env.elements.search.value, "rho");
  assert.equal(descendants(env.elements.results).find(element => element.attributes["data-group"]).attributes["aria-pressed"], "true");
});

test("input cards keep the full model path in the title and a short name in the text", () => {
  const env = webview(); env.render({ before: "C:/models/root.mod · Working", after: "tree/root.mod @ main · aaaaaaa" });
  assert.equal(env.elements.models.children[0].title, "C:/models/root.mod · Working");
  assert.doesNotMatch(env.elements.models.textContent, /C:\/models/); assert.match(env.elements.models.textContent, /root.mod/);
});

test("all categories stay in the left panel when Find changes filters out their rows", () => {
  const env = webview(), row = parseDiff(response(), roots.before, roots.after, true).rows[0];
  const rows = [{ ...row, id: "equation", section: "equations", group: "Equations", label: "Output" }, { ...row, id: "local", section: "equations", group: "Model-local variables", label: "helper" }];
  env.render({ rows }); env.elements.search.value = "helper"; env.elements.search.fire("input");
  const groups = descendants(env.elements.results).filter(element => element.attributes["data-group"]);
  assert.deepEqual(groups.map(group => group.attributes["data-group"]), ["equations:Equations", "equations:Model-local variables"]);
  assert.equal(groups[0].disabled, true); assert.equal(groups[1].disabled, false);
  assert.equal(descendants(env.elements.results).find(element => element.tag === "article").attributes["data-row-id"], "local");
  env.elements.search.value = ""; env.elements.search.fire("input");
  assert.equal(descendants(env.elements.results).filter(element => element.attributes["data-group"]).every(group => !group.disabled), true);
});

test("a loading refresh retains the selected category until captured rows return", () => {
  const env = webview(), row = parseDiff(response(), roots.before, roots.after, true).rows[0];
  const rows = [{ ...row, id: "symbol", section: "symbols", group: "Symbols" }, row];
  env.render({ rows, choices: normalizeChoices({ group: "priors:Priors" }, defaults) });
  env.render({ rows: [], status: "loading", choices: normalizeChoices(env.states.at(-1).choices, defaults) });
  env.render({ rows, choices: normalizeChoices(env.states.at(-1).choices, defaults) });
  assert.equal(env.states.at(-1).choices.group, "priors:Priors");
  assert.equal(descendants(env.elements.results).find(element => element.tag === "article").attributes["data-row-id"], row.id);
});
test("card source links show decoded file labels and send exact row and side identities", () => {
  const env = webview(), snapshot = parseDiff(response(), roots.before, roots.after, true), row = snapshot.rows[0];
  row.navigation.before.written_locations[0].uri = "dygnosis-history:/model%20old.mod%20%C2%B7%20a95df59?%7B%22file_key%22%3A%22private%2Fpath%22%7D";
  row.navigation.before.written_locations[0].range.start.line = 75;
  row.navigation.after.written_locations[0].uri = "file:///models/include%20new.mod";
  row.navigation.after.written_locations[0].range.start.line = 18;
  env.render(snapshot);
  const headings = descendants(env.elements.results).filter(element => element.className === "side-title");
  const [before, after] = headings.map(heading => heading.children.find(element => element.className === "source-link"));
  assert.equal(before.textContent, "model old.mod · a95df59:76"); assert.equal(after.textContent, "include new.mod:19");
  assert.doesNotMatch(env.elements.results.textContent, /file_key|private|%7B/);
  assert.equal(before.title, row.navigation.before.written_locations[0].uri);
  before.fire("click"); assert.equal(env.posted.at(-1).type, "openSource"); assert.equal(env.posted.at(-1).rowId, row.id); assert.equal(env.posted.at(-1).side, "before");
  after.fire("click"); assert.equal(env.posted.at(-1).rowId, row.id); assert.equal(env.posted.at(-1).side, "after");
});
test("expression cards omit duplicate values and property prefixes but retain changed facts and regime tags", () => {
  const env = webview(), snapshot = parseDiff(response(), roots.before, roots.after, true), row = snapshot.rows[0];
  row.label = "Equation"; row.semantic.before.equation_index = 4; row.semantic.after.equation_index = 4;
  row.semantic.expressions[0].field = row.semantic.fields[0].name = "expression";
  row.semantic.fields[0].label = "Expression";
  row.semantic.fields.push({ name: "tags", label: "Tags", before: { state: "present", value: { kind: "record", value: {} } }, after: { state: "present", value: { kind: "record", value: {} } }, changed: false, comparison_availability: "complete", numeric_difference: null });
  env.render(snapshot);
  const detail = descendants(env.elements.results).find(element => element.className === "detail change changed");
  assert.match(detail.textContent, /^~Equation 5/);
  assert.equal(detail.textContent.match(/0.8/g).length, 1); assert.equal(detail.textContent.match(/0.9/g).length, 1);
  assert.doesNotMatch(detail.textContent, /Changed fields|Count unit|occurrence|expanded text|Tags|Expression/);
  assert.equal(descendants(detail).some(element => element.tag === "details"), false);
  const field = structuredClone(row.semantic.fields[0]); field.name = "tag_role"; field.label = "Regime"; field.before = value("bind"); field.after = value("relax"); row.semantic.fields.push(field);
  const tags = row.semantic.fields[1]; tags.before.value.value.bind = { kind: "text", value: "constraint" }; tags.after = structuredClone(tags.before);
  env.render(snapshot);
  assert.match(env.elements.results.textContent, /Regime/); assert.match(env.elements.results.textContent, /bind/); assert.match(env.elements.results.textContent, /relax/); assert.match(env.elements.results.textContent, /Tags/);
  assert.deepEqual(descendants(env.elements.results).filter(element => element.attributes["data-field"] === "tag_role").map(element => element.children[1].textContent), ["bind", "relax"]);
  const assignment = row.semantic.fields[0], expression = row.semantic.expressions[0];
  const evaluated = { name: "evaluated_value", label: "Evaluated value", comparison_availability: "complete" };
  row.section = row.semantic.family = "parameters"; row.group = "Parameters"; row.label = "rho";
  row.semantic.before.equation_index = row.semantic.after.equation_index = null;
  row.semantic.fields = [assignment, evaluated]; assignment.label = "Assigned expression";
  for (const [before, after, oldValue, newValue, shown] of [
    ["0.8", "0.9", 0.8, 0.9, false],
    ["-0.8", "+8D-1", -0.8, 0.8, false],
    ["8e-1", ".9", 0.8, 0.9, false],
    ["0.4+0.4", "0.9", 0.8, 0.9, true],
    ["0.8", "0.3*3", 0.8, 0.9, true],
    ["1/2", "0.5", 0.5, 0.5, true],
    ["0.5", "1/2", 0.5, 0.5, true],
  ]) {
    assignment.before = value(before); assignment.after = value(after);
    expression.before = { text: before, runs: [{ text: before, role: "removed" }] };
    expression.after = { text: after, runs: [{ text: after, role: "added" }] };
    evaluated.before = { state: "present", value: { kind: "number", value: oldValue } };
    evaluated.after = { state: "present", value: { kind: "number", value: newValue } };
    evaluated.changed = oldValue !== newValue; evaluated.numeric_difference = newValue - oldValue;
    env.render(snapshot);
    const fields = descendants(env.elements.results).filter(element => element.attributes["data-field"] === "evaluated_value");
    assert.deepEqual(fields.map(element => element.children[1].textContent), shown ? [String(oldValue), String(newValue)] : []);
    assert.doesNotMatch(env.elements.results.textContent, /After − Before:/);
    if (shown && evaluated.changed) assert.deepEqual(fields.map(element => element.children[1].children[0].className), ["token removed", "token added"]);
  }
});
test("equation cards omit timing and convention prose already owned by symbol facts", () => {
  const env = webview(), snapshot = parseDiff(response(), roots.before, roots.after, true), detail = snapshot.rows[0].semantic;
  const before = { name: "rho", class: "endogenous", written_offset: 0, converted_offset: 0, occurrence: 0 };
  detail.timing = [{ before, after: { ...before, written_offset: -1, converted_offset: -1 } }, { before: { ...before, name: "unchanged" }, after: { ...before, name: "unchanged" } }];
  env.render(snapshot); assert.doesNotMatch(env.elements.results.textContent, /rho · after convention|unchanged · after convention|Timing changed/);
  detail.timing[0].after.converted_offset = -2; env.render(snapshot);
  assert.doesNotMatch(env.elements.results.textContent, /after convention/);
  assert.doesNotMatch(env.elements.results.textContent, /unchanged · after convention/);
});
test("Type of changes allows multiple checks and preserves them through reload, refresh and split views", () => {
  const env = webview(), other = webview(), initial = normalizeChoices({ sections: [], changeKinds: ["changed"] }, defaults);
  env.render({ choices: initial }); other.render();
  const option = (view, kind) => descendants(view.elements.kinds).find(element => element.attributes["data-change-type"] === kind);
  const tick = (kind, checked) => { const input = option(env, kind); input.checked = checked; input.fire("change"); };
  assert.equal(env.elements.kinds.children[0].textContent, "Type of changes");
  assert.equal(option(env, "all").indeterminate, true);
  assert.equal(option(env, "changed").checked, true); assert.equal(option(env, "unpaired"), undefined);
  tick("changed", false); assert.match(env.elements.results.textContent, /No rows match/);
  tick("changed", true); tick("added", true); assert.match(env.elements.results.textContent, /Prior mean/);
  env.render({ choices: normalizeChoices(env.states.at(-1).choices, defaults), capture: "capture2" });
  assert.deepEqual(env.states.at(-1).choices.changeKinds, ["added", "changed"]);
  assert.equal(Object.hasOwn(env.states.at(-1).choices, "sections"), false);
  assert.equal(option(other, "all").checked, true);
  tick("all", true); assert.ok(changeKinds.every(kind => option(env, kind).checked));
  tick("removed", false); assert.equal(option(env, "all").indeterminate, true);
  tick("all", false); assert.ok(changeKinds.every(kind => !option(env, kind).checked));
  assert.match(env.elements.counts.textContent, /0 replaced/);
  assert.match(env.elements.kinds.textContent, /Replaced/); assert.doesNotMatch(env.elements.kinds.textContent, /Changed/);
});

test("grouped model rows show distinguishing dimensions and keep change kinds accessible", () => {
  for (const dimension of [null, "households"]) {
    const env = webview(), snapshot = parseDiff(uncertainEquations(dimension), roots.before, roots.after, true);
    env.render({ ...snapshot, choices: normalizeChoices({ expansion: "none" }, defaults) });
    const rows = descendants(env.elements.results).filter(element => element.attributes["data-row-id"]);
    assert.deepEqual(rows.map(row => row.attributes["data-row-id"]), snapshot.rows.map(row => row.id));
    const expectedScope = dimension === null ? "Aggregate" : "Dimension: households";
    for (const row of rows) {
      const owner = snapshot.rows.find(item => item.id === row.attributes["data-row-id"]);
      if (dimension !== null) assert.match(row.textContent, new RegExp(expectedScope));
      else assert.doesNotMatch(row.textContent, /Aggregate/);
      assert.equal(row.attributes["aria-label"], (owner.kind === "added" ? "Added " : "Removed ") + owner.label + ", " + expectedScope);
    }
  }
});

test("empty filters have a route to All and field rows retain both cards", () => {
  const env = webview(); env.render({ choices: normalizeChoices({ sections: [], changeKinds: [] }, defaults) });
  const all = descendants(env.elements.kinds).find(element => element.attributes["data-change-type"] === "all");
  assert.equal(all.checked, false); assert.equal(all.indeterminate, false);
  all.checked = true; all.fire("change");
  assert.match(env.elements.results.textContent, /Prior mean/);
  const snapshot = parseDiff(response(), roots.before, roots.after, true); snapshot.rows[0].semantic.expressions = [];
  env.render(snapshot); assert.equal(descendants(env.elements.results).filter(element => element.className?.startsWith("side ")).length, 2);
  env.render(); assert.doesNotMatch(env.elements.results.textContent, /Saved selection/); assert.doesNotMatch(env.elements.kinds.textContent, /Saved selection/);
});

test("absent and explicit empty card values stay blank while zero and unknown remain visible", () => {
  const env = webview(), snapshot = parseDiff(response(), roots.before, roots.after, true);
  const field = snapshot.rows[0].semantic.fields[0];
  for (const state of ["absent", "empty"]) {
    field.before = { state, value: state === "empty" ? { kind: "text", value: "" } : null };
    field.after = { state: "present", value: { kind: "number", value: 0 } };
    env.render(snapshot);
    const values = descendants(env.elements.results).filter(element => element.className === "field-content");
    assert.equal(values[0].textContent, "");
    assert.match(values[1].textContent, /0/);
  }
  field.before = { state: "unknown", value: null }; env.render(snapshot);
  assert.match(env.elements.results.textContent, /Unknown/);
});
test("direct text diff chooses captured files or root fallback and forwards the capture token", () => {
  const env = webview(); env.render();
  assert.equal(env.elements.capturedTextDiff.hidden, false); assert.equal(env.elements.rootTextDiff.hidden, true);
  env.elements.capturedTextDiff.fire("click");
  assert.equal(env.posted.at(-1).type, "capturedTextDiff"); assert.equal(env.posted.at(-1).token, 3);
  env.render({ sourceChanges: null, hasHistory: true });
  assert.equal(env.elements.capturedTextDiff.hidden, true); assert.equal(env.elements.rootTextDiff.hidden, false);
  env.elements.rootTextDiff.fire("click"); assert.equal(env.posted.at(-1).type, "rootTextDiff"); assert.equal(env.posted.at(-1).token, 3);
  assert.equal(env.elements.choosePath.hidden, true); env.render({ status: "failure", message: "Before model path is absent" }); assert.equal(env.elements.choosePath.hidden, false);
  env.render({ status: "failure", message: "Unsupported comparison detail" }); assert.equal(env.elements.choosePath.hidden, true);
});
test("long retained values remain readable without an expander", () => {
  const env = webview(), snapshot = parseDiff(response(), roots.before, roots.after, true), field = snapshot.rows[0].semantic.fields[0];
  field.before = { state: "present", value: { kind: "list", value: Array.from({ length: 8 }, () => ({ kind: "record", value: { name: { kind: "text", value: "retained_expression_node_with_a_long_name" } } })) } };
  env.render(snapshot);
  const detail = descendants(env.elements.results).find(element => element.className?.startsWith("field-value ")); assert.ok(detail); assert.equal(detail.tag, "pre");
  assert.match(detail.textContent, /retained_expression_node/);
  assert.match(env.elements.results.textContent, /0.9/);
});
test("retired comparison explainer contents do not leak into results", () => {
  const env = webview(), snapshot = parseDiff(response(), roots.before, roots.after, true);
  env.render({ ...snapshot, rows: [] });
  assert.doesNotMatch(env.elements.results.textContent, /Partial comparison|not captured/);
  assert.doesNotMatch(env.elements.results.textContent, /Effective inherited prior/);
  assert.match(env.elements.results.textContent, /Text diff/);
  snapshot.coverage.limits = [...snapshot.coverage.families[0].limits]; env.render(snapshot);
  assert.equal(env.elements.results.textContent.match(/Effective inherited prior/g).length, 1, "only the affected row's own limit remains");
  snapshot.sourceChanges.files[0].limits = [{ code: "source_alignment_limit", reason: "Source alignment budget exceeded.", omitted: null }]; env.render(snapshot);
  assert.doesNotMatch(env.elements.results.textContent, /Source alignment budget exceeded/);
  env.elements.capturedTextDiff.fire("click");
  assert.equal(env.posted.at(-1).type, "capturedTextDiff"); assert.equal(env.posted.at(-1).token, 3);
  env.render({ status: "stale" }); assert.equal(env.elements.capturedTextDiff.disabled, true);
});

test("repeated row limitations appear once after all changes in the selected group", () => {
  const env = webview(), snapshot = parseDiff(response(), roots.before, roots.after, true), original = snapshot.rows[0];
  const reason = "These remaining accepted tokens have no complete named option facts; token detail makes no numerical-effect claim.";
  original.semantic.limits = [{ code: "statement_options_text_only", reason, owner: "semantic_surfaces", omitted: null }];
  const next = structuredClone(original); next.id = next.semantic.pointer = next.navigation.id = "/semantic/rows/1"; next.label = "other"; snapshot.rows.push(next);
  env.render(snapshot);
  const articles = descendants(env.elements.results).filter(element => element.tag === "article");
  assert.equal(articles.length, 2); assert.ok(articles.every(article => !article.textContent.includes(reason)));
  assert.equal(env.elements.results.textContent.split(reason).length - 1, 1);
  const group = descendants(env.elements.results).find(element => element.className === "group-rows");
  assert.equal(group.children.at(-1).className, "group-notes"); assert.match(group.children.at(-1).textContent, /remaining accepted tokens/);
});

test("shock cards use captured statement text and omit its duplicate named attributes", () => {
  const env = webview(), snapshot = parseDiff(response(), roots.before, roots.after, true), row = snapshot.rows[0];
  row.section = "shockSetup"; row.group = "Shock setup"; row.label = row.semantic.name = "e"; row.semantic.family = "shocks"; row.semantic.references = [];
  const before = "shocks;\nvar e; stderr 1;\nend;", after = "shocks(overwrite);\nvar e; stderr 2;\nend;";
  row.semantic.expressions = [{ field: "statement_text", before: { text: before, runs: [{ text: "shocks;\nvar e; stderr ", role: "unchanged" }, { text: "1", role: "removed" }, { text: ";\nend;", role: "unchanged" }] }, after: { text: after, runs: [{ text: "shocks", role: "unchanged" }, { text: "(overwrite)", role: "added" }, { text: ";\nvar e; stderr ", role: "unchanged" }, { text: "2", role: "added" }, { text: ";\nend;", role: "unchanged" }] }, highlight_basis: "paired_expression", availability: "complete", reason: null }];
  row.semantic.fields = ["form", "role", "target", "overwrite", "values"].map(name => ({ name, label: name, before: value("old " + name), after: value("new " + name), changed: true, comparison_availability: "complete", numeric_difference: null }));
  row.semantic.fields.push({ name: "status", label: "Status", before: value("active"), after: value("active"), changed: false, comparison_availability: "complete", numeric_difference: null });
  env.render(snapshot);
  const cards = descendants(env.elements.results).filter(element => element.className?.startsWith("side "));
  assert.match(cards[0].textContent, /var e; stderr 1;/); assert.match(cards[1].textContent, /shocks\(overwrite\);/);
  assert.equal(descendants(env.elements.results).filter(element => element.attributes["data-field"]).length, 0);
  assert.equal(descendants(cards[0]).find(element => element.className === "token removed").textContent, "1");
  assert.equal(descendants(cards[1]).filter(element => element.className === "token added").at(-1).textContent, "2");
  row.kind = row.semantic.change = "added"; row.before = row.semantic.before = null;
  row.semantic.expressions[0].before = null;
  row.semantic.expressions[0].after.runs = [{ text: after, role: "added" }]; env.render(snapshot);
  const highlights = descendants(env.elements.results).filter(element => element.className === "token added").map(element => element.textContent).join("");
  assert.equal(highlights, after, "a new shock block retains its boundary highlights");
  row.kind = row.semantic.change = "changed"; row.before = row.semantic.before = side("e");
  row.semantic.expressions[0].before = { text: before, runs: [{ text: before, role: "unchanged" }] };
  row.semantic.expressions[0].after = structuredClone(row.semantic.expressions[0].before); env.render(snapshot);
  assert.match(env.elements.results.textContent, /new values/, "equal written text cannot hide independently changed facts");
  row.semantic.expressions = []; env.render(snapshot);
  assert.match(env.elements.results.textContent, /old values/); assert.match(env.elements.results.textContent, /new values/);
});

test("removed commands show the statement without a redundant role or token heading", () => {
  const env = webview(), snapshot = parseDiff(response(), roots.before, roots.after, true), row = snapshot.rows[0];
  row.section = "commands"; row.group = "Commands"; row.label = row.semantic.name = "steady"; row.kind = row.semantic.change = "removed";
  row.after = row.semantic.after = null; row.semantic.family = "commands"; row.semantic.references = [];
  row.semantic.expressions = [{ field: "statement_text", before: { text: "steady;", runs: [{ text: "steady;", role: "removed" }] }, after: null, highlight_basis: "paired_expression", availability: "complete", reason: null }];
  row.semantic.fields = [{ name: "role", label: "Role", before: value("command"), after: { state: "absent", value: null }, changed: true, comparison_availability: "complete", numeric_difference: null }];
  env.render(snapshot);
  assert.equal(descendants(env.elements.results).find(element => element.className === "expression").textContent, "steady;");
  assert.equal(descendants(env.elements.results).some(element => element.attributes["data-field"] === "role"), false);
  assert.equal(descendants(env.elements.results).some(element => element.className === "field-label"), false);
});

// Guards lost block-boundary highlights when the complete block is added or removed.
test("new and removed blocks retain the engine's opener and closer highlights", () => {
  for (const [block, body, family] of [
    ["estimated_params", "a, beta_pdf, 0.6, 0.1;", "priors"],
    ["steady_state_model", "y = 0;", "steady_state"],
    ["shocks", "var e; stderr 0.02;", "shocks"],
    ["matched_irfs", "var y; varexo e; periods 1; values 0.02;", "moments"],
    ["matched_irfs_weights", "y(1), e, c(2), e, 0.75;", "moments"],
  ]) for (const [kind, activeSide] of [["added", "after"], ["removed", "before"]]) {
    const env = webview(), snapshot = parseDiff(response(), roots.before, roots.after, true), row = snapshot.rows[0];
    const text = block + ";\n" + body + "\nend;";
    row.kind = row.semantic.change = kind; row.semantic.references = []; row.semantic.fields = [];
    row.semantic.family = family;
    row.before = row.semantic.before = null; row.after = row.semantic.after = null;
    row[activeSide] = row.semantic[activeSide] = side("a");
    row.semantic[activeSide].context = { ...row.semantic[activeSide].context, kind: "block", name: block };
    row.semantic.expressions = [{ field: "statement_text", before: null, after: null, [activeSide]: { text, runs: [{ text, role: kind }] }, highlight_basis: "paired_expression", availability: "complete", reason: null }];
    env.render(snapshot);
    const highlights = descendants(env.elements.results).filter(element => element.className === "token " + kind).map(element => element.textContent).join("");
    assert.equal(highlights, text);
  }
});

// Guards an added child with no Before card or other changed rows in its shared parent.
test("an added prior in an existing block retains plain engine boundary runs", () => {
  const env = webview(), snapshot = parseDiff(response(), roots.before, roots.after, true), row = snapshot.rows[0];
  const header = "estimated_params;\n", body = "a, beta_pdf, 0.6, 0.1;", footer = "\nend;";
  row.kind = row.semantic.change = "added"; row.semantic.references = []; row.semantic.fields = [];
  row.before = row.semantic.before = null;
  row.semantic.expressions = [{ field: "statement_text", before: null, after: { text: header + body + footer, runs: [{ text: header, role: "unchanged" }, { text: body, role: "added" }, { text: footer, role: "unchanged" }] }, highlight_basis: "paired_expression", availability: "complete", reason: null }];
  env.render(snapshot);
  assert.equal(descendants(env.elements.results).filter(element => element.className === "token added").map(element => element.textContent).join(""), body);
});

test("operation cards use captured written instructions instead of internal records", () => {
  const r = response(), row = r.semantic.rows[0], source = "model_replace( 'Consumption' );\n  [name='Consumption'] c = 0.8*y;\nend;";
  Object.assign(row, { family: "operations", change: "added", name: "model_replace", before: null, after: side("model_replace"), references: [], limits: [] });
  row.fields = [
    { name: "role", label: "Role", before: { state: "absent", value: null }, after: value("equation_surgery"), changed: true, comparison_availability: "complete", numeric_difference: null },
    { name: "removed_equations", label: "Removed equations", before: { state: "absent", value: null }, after: { state: "present", value: { kind: "list", value: [{ kind: "record", value: { expression: { kind: "text", value: "c = y" }, dynamic_tag: { kind: "boolean", value: false } } }] } }, changed: true, comparison_availability: "complete", numeric_difference: null },
  ];
  row.expressions = [{ field: "statement_text", before: null, after: { text: source, runs: [{ text: source, role: "added" }] }, highlight_basis: "paired_expression", availability: "complete", reason: null }];
  Object.assign(r.navigation.rows[0], { family: "operations", name: row.name, before: null }); r.navigation.rows.pop(); r.semantic.references = [];
  r.sources.after[roots.after] = source;
  const snapshot = parseDiff(r, roots.before, roots.after, true), env = webview(); env.render(snapshot);
  assert.equal(descendants(env.elements.results).find(element => element.className === "expression").textContent, source);
  assert.doesNotMatch(env.elements.results.textContent, /Removed equations|Dynamic tag|equation_surgery/);
  snapshot.rows[0].semantic.expressions = []; env.render(snapshot);
  assert.match(env.elements.results.textContent, /Written source text is unavailable.*Text diff/);
  assert.doesNotMatch(env.elements.results.textContent, /Removed equations|Dynamic tag|equation_surgery/);
});

test("MS-SBVAR cards render written path context with only replaced numbers highlighted", () => {
  const r = response(), row = r.semantic.rows[0], prefix = "conditional_forecast_paths;\nvar y;\nperiods 1 2 3;\nvalues ", suffix = " 0.25 0.1;\nend;";
  row.family = r.navigation.rows[0].family = "ms_sbvar";
  row.references = []; r.semantic.references = []; r.navigation.rows.pop();
  row.expressions = [{ field: "statement_text", before: { text: prefix + "0.1" + suffix, runs: [{ text: prefix, role: "unchanged" }, { text: "0.1", role: "removed" }, { text: suffix, role: "unchanged" }] }, after: { text: prefix + "0.2" + suffix, runs: [{ text: prefix, role: "unchanged" }, { text: "0.2", role: "added" }, { text: suffix, role: "unchanged" }] }, highlight_basis: "paired_expression", availability: "complete", reason: null }];
  const env = webview(); env.render(parseDiff(r, roots.before, roots.after, true));
  assert.deepEqual(descendants(env.elements.results).filter(element => element.className === "token added").map(element => element.textContent), ["0.2"]);
  assert.deepEqual(descendants(env.elements.results).filter(element => element.className === "expression").map(element => element.textContent), [prefix + "0.1" + suffix, prefix + "0.2" + suffix]);
  assert.equal(descendants(env.elements.results).some(element => element.attributes["data-field"]), false);
});

// Guards duplicate RHS/record rendering and whole-assignment highlights in state cards.
test("state cards show paired written assignments with only changed tokens", () => {
  const r = response(), row = r.semantic.rows[0], prefix = "histval;\ny(0) = ", suffix = ";\nend;";
  row.family = r.navigation.rows[0].family = "steady_state"; row.references = []; r.semantic.references = []; r.navigation.rows.pop();
  row.expressions.push({ field: "statement_text", before: { text: prefix + "0.1" + suffix, runs: [{ text: prefix, role: "unchanged" }, { text: "0.1", role: "removed" }, { text: suffix, role: "unchanged" }] }, after: { text: prefix + "0.2" + suffix, runs: [{ text: prefix, role: "unchanged" }, { text: "0.2", role: "added" }, { text: suffix, role: "unchanged" }] }, highlight_basis: "paired_expression", availability: "complete", reason: null });
  const env = webview(); env.render(parseDiff(r, roots.before, roots.after, true));
  assert.deepEqual(descendants(env.elements.results).filter(element => element.className === "expression").map(element => element.textContent), [prefix + "0.1" + suffix, prefix + "0.2" + suffix]);
  assert.deepEqual(descendants(env.elements.results).filter(element => element.className === "token added").map(element => element.textContent), ["0.2"]);
  assert.equal(descendants(env.elements.results).some(element => element.attributes["data-field"]), false);
});

// Guards fragmented history cards and loss of per-period filters when combining written context.
test("changed periods of one history variable share a block and retain row filters and source actions", () => {
  const snapshot = parseDiff(response(), roots.before, roots.after, true), base = snapshot.rows[0];
  snapshot.rows = [[0, "0.1", "0.2"], [-1, "0.05", "-0.05"]].map(([lag, before, after], index) => {
    const row = structuredClone(base), prefix = `histval;\ny(${lag}) = `, suffix = ";\nend;";
    row.id = row.navigation.id = "/semantic/rows/" + index;
    row.section = "steadyState"; row.group = "State"; row.label = row.semantic.name = `y(${lag})`;
    row.semantic.family = "steady_state"; row.semantic.references = [];
    for (const side of ["before", "after"]) row.semantic[side].context.name = "histval";
    row.semantic.fields = [{ name: "target", label: "Historical target", before: value("y"), after: value("y"), changed: false, comparison_availability: "complete", numeric_difference: null }];
    row.semantic.expressions = [{ field: "statement_text", before: { text: prefix + before + suffix, runs: [{ text: prefix, role: "unchanged" }, { text: before, role: "removed" }, { text: suffix, role: "unchanged" }] }, after: { text: prefix + after + suffix, runs: [{ text: prefix, role: "unchanged" }, { text: after, role: "added" }, { text: suffix, role: "unchanged" }] }, highlight_basis: "paired_expression", availability: "complete", reason: null }];
    return row;
  });
  const env = webview(); env.render(snapshot);
  assert.equal(descendants(env.elements.results).filter(element => element.tag === "article").length, 1);
  assert.deepEqual(descendants(env.elements.results).filter(element => element.className === "expression").map(element => element.textContent), ["histval;\ny(0) = 0.1;\ny(-1) = 0.05;\nend;", "histval;\ny(0) = 0.2;\ny(-1) = -0.05;\nend;"]);
  assert.match(env.elements.counts.textContent, /2 of 2 model rows/);
  descendants(env.elements.results).find(element => element.className === "source-link").fire("click");
  assert.equal(env.posted.at(-1).rowId, snapshot.rows[0].id);
  env.elements.search.value = "y(-1)"; env.elements.search.fire("input");
  assert.doesNotMatch(env.elements.results.textContent, /y\(0\)/);
  assert.match(env.elements.counts.textContent, /1 of 2 model rows/);
});

test("unchanged named shock-group headings stay plain across added and removed members", () => {
  const snapshot = parseDiff(response(), roots.before, roots.after, true), base = snapshot.rows[0];
  const header = "shock_groups(name=drivers);\n", footer = "\nend;";
  snapshot.rows = [["removed", "before", "demand = eps_demand;"], ["added", "after", "joint = eps_demand eps_supply;"]].map(([kind, side, body], index) => {
    const row = structuredClone(base); row.id += index; row.kind = kind; row.before = side === "before" ? "demand" : null; row.after = side === "after" ? "joint" : null;
    row.semantic.family = "shocks"; row.semantic.change = kind; row.semantic[side === "before" ? "after" : "before"] = null; row.semantic.fields = []; row.semantic.references = [];
    const text = header + body + footer;
    row.semantic.expressions = [{ field: "statement_text", before: null, after: null, [side]: { text, runs: [{ text: header, role: "unchanged" }, { text: body, role: kind }, { text: footer, role: "unchanged" }] }, highlight_basis: "paired_expression", availability: "complete", reason: null }];
    return row;
  });
  const env = webview(); env.render(snapshot);
  const highlighted = descendants(env.elements.results).filter(element => ["token added", "token removed"].includes(element.className)).map(element => element.textContent).join("");
  assert.doesNotMatch(highlighted, /shock_groups|name=drivers/);
  assert.match(highlighted, /demand = eps_demand/); assert.match(highlighted, /joint = eps_demand eps_supply/);
});

test("an added local definition highlights its whole instruction", () => {
  const env = webview(), snapshot = parseDiff(response(), roots.before, roots.after, true), row = snapshot.rows[0];
  row.section = "equations"; row.group = "Model-local variables"; row.kind = row.semantic.change = "added";
  row.before = row.semantic.before = null; row.after = "xdddd"; row.semantic.after = side("xdddd"); row.semantic.family = "symbols"; row.semantic.references = [];
  row.semantic.fields = [{ name: "role", label: "Role", before: { state: "absent", value: null }, after: value("model_local_definition"), changed: true, comparison_availability: "complete", numeric_difference: null }];
  row.semantic.expressions = [{ field: "expression", before: null, after: { text: "yf", runs: [{ text: "yf", role: "added" }] }, highlight_basis: "paired_expression", availability: "complete", reason: null }];
  env.render(snapshot);
  assert.equal(descendants(env.elements.results).filter(element => element.className === "token added").map(element => element.textContent).join(""), "# xdddd = yf;");
});

test("filters can hide all rows, retired selection cannot hide rows and stale actions remain disabled", () => {
  const env = webview(); env.render(); env.elements.search.value = "nothing"; env.elements.search.fire("input");
  assert.match(env.elements.results.textContent, /No rows match/);
  env.render({ status: "stale" });
  const actions = descendants(env.elements.results).filter(element => element.tag === "button" && element.attributes["aria-label"]?.startsWith("Open"));
  assert.ok(actions.length); assert.ok(actions.every(button => button.disabled));
  env.render({ capture: "capture2", choices: normalizeChoices({ capture: "capture1", selected: "/old", presentation: "focusedReview" }, defaults) });
  assert.equal(descendants(env.elements.results).filter(element => element.tag === "article").length, 1);
  assert.equal(Object.hasOwn(env.states.at(-1).choices, "selected"), false);
});
test("source-only changes lead to native text diff, text cannot inject DOM or source actions", () => {
  const env = webview(), snapshot = parseDiff(response(), roots.before, roots.after, true);
  env.render({ rows: [] }); assert.match(env.elements.results.textContent, /Captured text differs/);
  snapshot.rows[0].label = "<img src=x onerror=bad()>"; snapshot.rows[0].semantic.fields[0].after.value.value = "command:bad";
  env.render(snapshot); assert.match(env.elements.results.textContent, /<img/);
  assert.equal(descendants(env.elements.results).some(e => e.tag === "img" || e.tag === "a"), false);
});
test("field limits do not mark budget-unavailable values as added or removed", () => {
  const env = webview(), snapshot = parseDiff(response(), roots.before, roots.after, true);
  const field = snapshot.rows[0].semantic.fields[0]; field.changed = false; field.comparison_availability = "limit_exceeded";
  snapshot.rows[0].semantic.expressions = [];
  env.render(snapshot); assert.match(env.elements.results.textContent, /Comparison unavailable/);
  assert.equal(descendants(env.elements.results).some(e => e.className === "token added" || e.className === "token removed"), false);
});
test("reference and captured-file picker actions use exact retained text and stale guards", async t => {
  const { host, service, vscode } = createHost(); host.install(); t.after(() => host.registration.dispose());
  host.capture = (resource, cancel, history) => {
    const result = captured(resource, history, host, service.currentInstance), row = result.snapshot.rows[0];
    result.snapshot.references = [{ pointer: "/semantic/references/0", side: "after", navigation: { ...row.navigation, id: "/semantic/references/0" } }];
    const before = result.inputs.before.root_file, after = result.inputs.after.root_file;
    result.snapshot.sourceChanges = { files: [{ pointer: "/source_changes/files/0", change: "changed", correspondence: "selected_roots", before: { input_id: "before", file_key: before, exact_text_available: true }, after: { input_id: "after", file_key: after, exact_text_available: true } }] };
    return result;
  };
  await host.open();
  host.message({ type: "openReference", token: host.last().token, pointer: "/semantic/references/0", side: "after" }); await flush();
  assert.equal(host.shown.length, 1);
  host.message({ type: "capturedTextDiff", token: host.last().token }); await flush();
  const diff = host.calls.findLast(call => call.id === "vscode.diff"); assert.ok(diff);
  assert.equal(diff.args[2], "main.mod (Working Tree)");
  assert.equal(host.picks.at(-1).items[0].label, "main.mod");
  assert.equal(diff.args[0].scheme, "dygnosis-history"); assert.equal(diff.args[1].scheme, "dygnosis-captured");
  assert.equal((await vscode.workspace.openTextDocument(diff.args[1])).getText(), host.panels[0].document.result.texts.after[host.panels[0].document.result.inputs.after.root_file]);
  assert.match(host.picks.at(-1).options.placeHolder, /captured file/);
  const count = host.calls.filter(call => call.id === "vscode.diff").length;
  host.changed.fire({ reason: "lifecycle" }); host.message({ type: "capturedTextDiff", token: host.last().token }); await flush();
  assert.equal(host.calls.filter(call => call.id === "vscode.diff").length, count);
});

test("a captured-file picker cannot open an old capture after refresh or engine restart", async t => {
  for (const invalidation of ["lifecycle", "refresh"]) {
    const { host, service } = createHost(); host.install(); t.after(() => host.registration.dispose());
    host.capture = (resource, cancel, history) => {
      const result = captured(resource, history, host, service.currentInstance);
      result.snapshot.sourceChanges = { files: [{ pointer: "/source_changes/files/0", change: "added", correspondence: "unpaired", after: { input_id: "after", file_key: result.inputs.after.root_file, exact_text_available: true } }] };
      return result;
    };
    await host.open(); const pending = deferred(); let selected;
    host.pick = items => { selected = items[0]; return pending.promise; };
    host.message({ type: "capturedTextDiff", token: host.last().token }); await flush(); assert.ok(selected);
    if (invalidation === "refresh") host.message({ type: "refresh" }); else host.changed.fire({ reason: "lifecycle" });
    await flush(); pending.resolve(selected); await flush();
    assert.equal(host.calls.some(call => call.id === "vscode.diff"), false, invalidation);
  }
});

test("captured-file revalidation marks changed Working inputs stale before showing a picker", async t => {
  const { host, service } = createHost(); host.install(); t.after(() => host.registration.dispose());
  host.capture = (resource, cancel, history) => {
    const result = captured(resource, history, host, service.currentInstance);
    result.snapshot.sourceChanges = { files: [{ pointer: "/source_changes/files/0", change: "added", after: { file_key: result.inputs.after.root_file, exact_text_available: true } }] };
    return result;
  };
  await host.open(); const picks = host.picks.length; host.validate = () => undefined;
  host.message({ type: "capturedTextDiff", token: host.last().token }); await flush();
  assert.equal(host.last().status, "stale"); assert.equal(host.picks.length, picks);
  assert.equal(host.calls.some(call => call.id === "vscode.diff"), false);
});

test("unavailable captured text has a picker reason and cannot open a partial text diff", async t => {
  const { host, service } = createHost(); host.install(); t.after(() => host.registration.dispose());
  host.capture = (resource, cancel, history) => {
    const result = captured(resource, history, host, service.currentInstance);
    result.snapshot.sourceChanges = { files: [{ pointer: "/source_changes/files/0", change: "added", after: { file_key: result.inputs.after.root_file, exact_text_available: false } }] };
    return result;
  };
  await host.open(); host.message({ type: "capturedTextDiff", token: host.last().token }); await flush();
  assert.match(host.picks.at(-1).items[0].description, /unavailable.*complete captured text/i);
  assert.match(host.logs.at(-1), /Complete captured text is unavailable/);
  assert.equal(host.calls.some(call => call.id === "vscode.diff"), false);
});
