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
const defaults = { presentation: "focusedReview", layout: "auto", expansion: "changes", sections: [...allDiffSections], changeKinds: [...changeKinds] };

// The real Working/Working LSP producer keeps duplicate-name occurrences in
// legacy add/remove arrays and refines their semantic owners to Unpaired.
function unpairedEquations(dimension = null) {
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
    r.semantic.rows.push({ pointer, family: "equations", change: "unpaired", name: "Ambiguous", count_unit: "accepted_occurrence", facets: ["expression"], before: activeSide === "before" ? own : null, after: activeSide === "after" ? own : null,
      fields: [{ name: "expression", label: "Expression", before: activeSide === "before" ? value(text) : { state: "absent", value: null }, after: activeSide === "after" ? value(text) : { state: "absent", value: null }, changed: true, comparison_availability: "complete", numeric_difference: null }],
      expressions: [{ field: "expression", before: activeSide === "before" ? { text, runs: [{ text, role: "unchanged" }] } : null, after: activeSide === "after" ? { text, runs: [{ text, role: "unchanged" }] } : null, highlight_basis: "unpaired_text_only", availability: "complete", reason: null }],
      timing: [], references: [], limits: [{ code: "equation_correspondence_unpaired", reason: "Text-only highlights do not establish equation, reference or timing correspondence.", owner: "semantic_equations", omitted: null }] });
    r.navigation.rows.push({ id: pointer, kind: "equation", before: activeSide === "before" ? target : null, after: activeSide === "after" ? target : null });
    r.navigation.rows.push({ id: `${prefix}/unmatched_same_name/0/${kind}/${index}`, kind: "equation", before: activeSide === "before" ? target : null, after: activeSide === "after" ? target : null });
  }
  owner.unmatched_same_name.push({ name: "Ambiguous", dimension, removed: owner["removed" + suffix], added: owner["added" + suffix] });
  return r;
}

test("real producer Unpaired equation refinements keep legacy owners and separate side appearances", () => {
  for (const dimension of [null, "households"]) {
    const r = unpairedEquations(dimension), expected = dimension === null ? "aggregateEquations" : "heterogeneousEquations";
    const parsed = parseDiff(r, roots.before, roots.after, true);
    assert.equal(parsed.rows.length, 8); assert.ok(parsed.rows.every(row => row.kind === "unpaired" && row.section === expected));
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

test("Unpaired refinement cannot replace another family or reverse an equation owner side", () => {
  for (const mutate of [r => r.semantic.rows[0].family = "symbols", r => { r.semantic.rows[0].after = r.semantic.rows[0].before; r.semantic.rows[0].before = null; }, r => r.semantic.rows[0].change = "added"]) {
    const r = unpairedEquations(); mutate(r);
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
test("Presentation defaults, resource overrides, invalid values and reset preserve detail layout", () => {
  const { host, vscode } = createHost(), { diffPreferences } = load("diff", vscode);
  assert.equal(diffPreferences(anchorUri, () => {}).presentation, "focusedReview");
  host.settings.set(anchorUri.toString(), { "diff.presentation": "changeList", "diff.layout": "stacked" });
  assert.equal(diffPreferences(anchorUri, () => {}).presentation, "changeList");
  assert.equal(diffPreferences(baselineUri, () => {}).presentation, "focusedReview");
  const messages = []; host.settings.set(anchorUri.toString(), { "diff.presentation": "bad" });
  assert.equal(diffPreferences(anchorUri, message => messages.push(message)).presentation, "focusedReview");
  assert.match(messages[0], /diff.presentation/);
  host.settings.delete(anchorUri.toString()); assert.equal(diffPreferences(anchorUri, () => {}).layout, "auto");
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
function webview() {
  const elements = Object.fromEntries(["models", "status", "search", "scope", "layout", "expansion", "kinds", "sections", "counts", "results", "refresh", "presentation", "modelTab", "sourceTab", "coverageTab", "help", "rootTextDiff", "moreActions", "filterTools", "kindFilter", "sectionFilter", "scopeFilter", "choosePath"].map(id => [id, new Element(id)]));
  const events = {}, posted = [], states = [], snapshot = parseDiff(response(), roots.before, roots.after, true);
  const sandbox = { document: { getElementById: id => elements[id], createElement: tag => new Element(tag) }, window: { addEventListener: (name, callback) => events[name] = callback }, acquireVsCodeApi: () => ({ getState: () => undefined, setState: value => states.push(structuredClone(value)), postMessage: value => posted.push(value) }) };
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, "../media/diff_view.js"), "utf8"), sandbox);
  const render = (extra = {}) => events.message({ data: { ...snapshot, type: "render", key: "view", capture: "capture1", token: 3, status: "ready", message: "Current comparison", choices: normalizeChoices({}, defaults), defaults, ...extra, before: typeof extra.before === "string" ? extra.before : roots.before, after: typeof extra.after === "string" ? extra.after : roots.after } });
  return { elements, posted, states, render };
}
test("both presentations safely render typed fields and preserve independent filters/layout/focus", () => {
  const env = webview(); env.render();
  assert.match(env.elements.results.textContent, /Prior mean/); assert.match(env.elements.results.textContent, /0.8/);
  env.elements.search.value = "rho"; env.elements.search.fire("input");
  env.elements.layout.value = "stacked"; env.elements.layout.fire("change");
  env.elements.presentation.value = "changeList"; env.elements.presentation.fire("change");
  assert.equal(env.elements.presentation.focused, true); assert.equal(env.elements.search.value, "");
  env.elements.search.value = "0.9"; env.elements.search.fire("input");
  env.elements.presentation.value = "focusedReview"; env.elements.presentation.fire("change");
  assert.equal(env.elements.search.value, "rho"); assert.equal(env.elements.layout.value, "stacked");
  assert.equal(env.states.at(-1).choices.presentations.changeList.search, "0.9");
});
test("accepted composition has compact selects, disclosed actions, input cards and four plain marks", () => {
  const { vscode } = createHost(), { diffHtml } = load("diff", vscode);
  const html = diffHtml({ cspSource: "vscode-webview:", asWebviewUri: uri => uri }, vscode.Uri.file("/extension/media"));
  assert.doesNotMatch(html, /fieldset|type="checkbox"|editor-tab/);
  assert.match(html, /<select id="kinds"/); assert.match(html, /<select id="sections"/);
  assert.match(html, /<details id="moreActions"[\s\S]*<summary>More actions<\/summary>[\s\S]*id="rootTextDiff"[\s\S]*id="help"/);
  assert.ok(html.indexOf('id="presentation"') < html.indexOf('id="tabs"'));
  assert.ok(html.indexOf('id="tabs"') < html.indexOf('id="filterTools"'));
  assert.match(html, /aria-label="Change colors"/);
  const env = webview(); env.render({ before: "C:/models/root.mod · Working", after: "tree/root.mod @ main · aaaaaaa" });
  assert.equal(env.elements.models.children[0].className, "input-side");
  assert.equal(env.elements.models.children[0].title, "C:/models/root.mod · Working");
  assert.doesNotMatch(env.elements.models.textContent, /C:\/models/); assert.match(env.elements.models.textContent, /root.mod/);
  assert.equal(descendants(env.elements.results).some(element => element.className === "badge"), false);
  const css = fs.readFileSync(path.join(__dirname, "../media/diff_view.css"), "utf8");
  assert.match(css, /grid-template-columns: 245px minmax\(0, 1fr\)/);
  assert.match(css, /^body \{\s+--line:/); assert.doesNotMatch(css, /:root/);
  assert.match(css, /--change: var\(--vscode-dynare-diff\\\.changedForeground, var\(--vscode-dynare-diff-changedForeground, var\(--vscode-focusBorder\)\)\)/);
  assert.match(css, /\.change-mark\.changed\s*\{\s*color: var\(--change\)/);
});
test("compact filters preserve saved multi-selections through switching, reload, refresh and split views", () => {
  const env = webview(), other = webview(), initial = normalizeChoices({ sections: ["priors", "commands"], changeKinds: ["changed", "unpaired"] }, defaults);
  env.render({ choices: initial }); other.render();
  assert.equal(env.elements.sections.value, "saved"); assert.equal(env.elements.kinds.value, "saved");
  assert.match(env.elements.sections.textContent, /Saved selection \(2\)/);
  env.elements.kinds.value = "removed"; env.elements.kinds.fire("change"); assert.match(env.elements.results.textContent, /No rows match/);
  env.elements.kinds.value = "saved"; env.elements.kinds.fire("change"); assert.match(env.elements.results.textContent, /Prior mean/);
  env.elements.sections.value = "commands"; env.elements.sections.fire("change");
  env.elements.sections.value = "saved"; env.elements.sections.fire("change");
  env.elements.presentation.value = "changeList"; env.elements.presentation.fire("change");
  env.elements.sections.value = "symbols"; env.elements.sections.fire("change");
  env.elements.presentation.value = "focusedReview"; env.elements.presentation.fire("change");
  assert.equal(env.elements.sections.value, "saved");
  const restored = normalizeChoices(env.states.at(-1).choices, defaults); env.render({ choices: restored, capture: "capture2" });
  assert.deepEqual(env.states.at(-1).choices.sections, ["priors", "commands"]);
  assert.deepEqual(env.states.at(-1).choices.customChangeKinds, ["changed", "unpaired"]);
  assert.equal(env.states.at(-1).choices.presentations.changeList.sections[0], "symbols");
  assert.equal(other.elements.sections.value, "all"); assert.equal(other.elements.kinds.value, "all");
});
test("collapsed Change list summaries retain facets and scope in text and accessible names", () => {
  for (const dimension of [null, "households"]) {
    const env = webview(), snapshot = parseDiff(unpairedEquations(dimension), roots.before, roots.after, true);
    env.render({ ...snapshot, choices: normalizeChoices({ presentation: "changeList", expansion: "none" }, defaults) });
    const rows = descendants(env.elements.results).filter(element => element.className === "row-summary unpaired");
    assert.equal(rows.length, 8);
    const expectedScope = dimension === null ? "Aggregate" : "Dimension: households";
    for (const row of rows) {
      assert.equal(row.open, false);
      const summary = row.children[0], owner = snapshot.rows.find(item => item.id === row.attributes["data-row-id"]);
      const subtitle = summary.children.find(element => element.className === "row-subtitle");
      assert.equal(subtitle.textContent, (owner.semantic ? "Expression" : owner.group) + " · " + expectedScope);
      assert.equal(summary.attributes["aria-label"], "Unpaired " + owner.label + ", " + expectedScope);
    }
  }
});
test("empty filters have a clear route to All and irrelevant controls hide by tab", () => {
  const env = webview(); env.render({ choices: normalizeChoices({ sections: [], changeKinds: [] }, defaults) });
  assert.match(env.elements.sections.textContent, /No sections selected/); assert.match(env.elements.kinds.textContent, /No kinds selected/);
  env.elements.sections.value = "all"; env.elements.sections.fire("change"); env.elements.kinds.value = "all"; env.elements.kinds.fire("change");
  assert.match(env.elements.results.textContent, /Prior mean/);
  env.elements.sourceTab.fire("click"); assert.equal(env.elements.scopeFilter.hidden, true); assert.equal(env.elements.sectionFilter.hidden, true); assert.equal(env.elements.filterTools.hidden, false);
  env.elements.coverageTab.fire("click"); assert.equal(env.elements.filterTools.hidden, true);
  env.render(); assert.doesNotMatch(env.elements.sections.textContent, /Saved selection/); assert.doesNotMatch(env.elements.kinds.textContent, /Saved selection/);
});
test("More actions forwards capture tokens and paths stay in the relevant failure state", () => {
  const env = webview(); env.render(); env.elements.moreActions.open = true;
  env.elements.rootTextDiff.fire("click"); assert.equal(env.elements.moreActions.open, false);
  assert.equal(env.posted.at(-1).type, "rootTextDiff"); assert.equal(env.posted.at(-1).token, 3);
  const summary = new Element("summary"); env.elements.moreActions.querySelector = () => summary;
  env.elements.moreActions.open = true; env.elements.moreActions.fire("keydown", { key: "Escape" }); assert.equal(env.elements.moreActions.open, false); assert.equal(summary.focused, true);
  assert.equal(env.elements.choosePath.hidden, true); env.render({ status: "failure", message: "Before model path is absent" }); assert.equal(env.elements.choosePath.hidden, false);
  env.render({ status: "failure", message: "Unsupported comparison detail" }); assert.equal(env.elements.choosePath.hidden, true);
});
test("long retained values disclose full facts without covering readable expressions", () => {
  const env = webview(), snapshot = parseDiff(response(), roots.before, roots.after, true), field = snapshot.rows[0].semantic.fields[0];
  field.before = { state: "present", value: { kind: "list", value: Array.from({ length: 8 }, () => ({ kind: "record", value: { name: { kind: "text", value: "retained_expression_node_with_a_long_name" } } })) } };
  env.render(snapshot);
  const detail = descendants(env.elements.results).find(element => element.className === "field-value"); assert.ok(detail); assert.notEqual(detail.open, true);
  assert.match(detail.textContent, /8 retained items · show values/); assert.match(detail.textContent, /retained_expression_node/);
  assert.match(env.elements.results.textContent, /0.9/);
});
test("source/coverage tabs keep separate counts, hunks, limits and keyboard navigation", () => {
  const env = webview(); env.render(); env.elements.sourceTab.fire("click");
  assert.match(env.elements.counts.textContent, /1 of 1 captured files shown · 1 source hunks/);
  assert.match(env.elements.results.textContent, /\/\/ old/); assert.match(env.elements.results.textContent, /Captured file text diff/);
  assert.ok(descendants(env.elements.results).some(element => element.className === "change-mark changed" && element.textContent === "~"));
  env.elements.presentation.value = "changeList"; env.elements.presentation.fire("change"); env.elements.sourceTab.fire("click");
  assert.ok(descendants(env.elements.results).some(element => element.tag === "details" && element.className === "row-summary changed"));
  env.elements.sourceTab.fire("keydown", { key: "ArrowRight", preventDefault() {} });
  assert.equal(env.elements.coverageTab.focused, true); assert.match(env.elements.results.textContent, /Effective inherited prior/);
  assert.match(env.elements.results.textContent, /not captured/);
});
test("filtered selections clear, new captures cannot reuse pointers, stale actions remain disabled", () => {
  const env = webview(); env.render(); env.elements.search.value = "nothing"; env.elements.search.fire("input");
  assert.equal(env.states.at(-1).choices.selected, null); assert.match(env.elements.results.textContent, /No rows match/);
  env.render({ status: "stale" });
  const actions = descendants(env.elements.results).filter(element => element.tag === "button" && element.attributes["aria-label"]?.startsWith("Open"));
  assert.ok(actions.length); assert.ok(actions.every(button => button.disabled));
  env.render({ capture: "capture2", choices: normalizeChoices({ capture: "capture1", selected: "/old", presentation: "focusedReview" }, defaults) });
  env.elements.layout.fire("change"); assert.equal(env.states.at(-1).choices.capture, "capture2");
  assert.equal(env.states.at(-1).choices.selected, "/semantic/rows/0");
});
test("source-only changes lead to Source, text cannot inject DOM or source actions", () => {
  const env = webview(), snapshot = parseDiff(response(), roots.before, roots.after, true);
  env.render({ rows: [] }); assert.match(env.elements.results.textContent, /Captured text differs/);
  snapshot.rows[0].label = "<img src=x onerror=bad()>"; snapshot.rows[0].semantic.fields[0].after.value.value = "command:bad";
  env.render(snapshot); assert.match(env.elements.results.textContent, /<img/);
  assert.equal(descendants(env.elements.results).some(e => e.tag === "img" || e.tag === "a"), false);
});
test("field limits do not color budget-unavailable values and color roles have text cues", () => {
  const env = webview(), snapshot = parseDiff(response(), roots.before, roots.after, true);
  const field = snapshot.rows[0].semantic.fields[0]; field.changed = false; field.comparison_availability = "limit_exceeded";
  snapshot.rows[0].semantic.expressions = [];
  env.render(snapshot); assert.match(env.elements.results.textContent, /Comparison unavailable/);
  assert.equal(descendants(env.elements.results).some(e => e.className === "token added" || e.className === "token removed"), false);
  const css = fs.readFileSync(path.join(__dirname, "../media/diff_view.css"), "utf8");
  assert.match(css, /diffEditor-removedTextBackground/); assert.match(css, /text-decoration: underline/); assert.doesNotMatch(css, /line-through/);
  assert.doesNotMatch(css.match(/\.change-mark\.removed\s*\{([^}]+)\}/)[1], /background/);
  const properties = require("../package.json").contributes.configuration.find(group => group.title === "Dygnosis: Changes").properties;
  assert.equal(properties["dynare.diff.presentation"].default, "focusedReview");
  assert.deepEqual(properties["dynare.diff.layout"].enum, ["auto", "sideBySide", "stacked"]);
});
test("semantic reference and captured-file actions use host IDs, exact read-only text and stale guards", async t => {
  const { host, service } = createHost(); host.install(); t.after(() => host.registration.dispose());
  host.capture = (resource, cancel, history) => {
    const result = captured(resource, history, host, service.currentInstance), row = result.snapshot.rows[0];
    result.snapshot.references = [{ pointer: "/semantic/references/0", side: "after", navigation: { ...row.navigation, id: "/semantic/references/0" } }];
    const before = result.inputs.before.root_file, after = result.inputs.after.root_file;
    result.snapshot.sourceChanges = { files: [{ pointer: "/source_changes/files/0", before: { input_id: "before", file_key: before, exact_text_available: true }, after: { input_id: "after", file_key: after, exact_text_available: true } }] };
    return result;
  };
  await host.open();
  host.message({ type: "openReference", token: host.last().token, pointer: "/semantic/references/0", side: "after" }); await flush();
  assert.equal(host.shown.length, 1);
  host.message({ type: "openCapturedSource", token: host.last().token, pointer: "/source_changes/files/0", side: "after" }); await flush();
  const source = host.shown.at(-1).document;
  assert.equal(source.uri.scheme, "dygnosis-captured"); assert.equal(source.getText(), host.panels[0].document.result.texts.after[host.panels[0].document.result.inputs.after.root_file]);
  host.message({ type: "capturedTextDiff", token: host.last().token, pointer: "/source_changes/files/0" }); await flush();
  const diff = host.calls.findLast(call => call.id === "vscode.diff"); assert.ok(diff); assert.equal(diff.args[0].scheme, "dygnosis-history"); assert.equal(diff.args[1].scheme, "dygnosis-captured");
  const count = host.calls.filter(call => call.id === "vscode.diff").length;
  host.changed.fire({ reason: "lifecycle" }); host.message({ type: "capturedTextDiff", token: host.last().token, pointer: "/source_changes/files/0" }); await flush();
  assert.equal(host.calls.filter(call => call.id === "vscode.diff").length, count);
});
test("captured Source refuses stale actions after a delayed language change", async t => {
  for (const invalidation of ["lifecycle", "refresh"]) {
    const { host, service, vscode } = createHost(); host.install(); t.after(() => host.registration.dispose());
    host.capture = (resource, cancel, history) => {
      const result = captured(resource, history, host, service.currentInstance);
      result.snapshot.sourceChanges = { files: [{ pointer: "/source_changes/files/0", after: { input_id: "after", file_key: result.inputs.after.root_file, exact_text_available: true } }] };
      return result;
    };
    await host.open();
    const pending = deferred(); let started = false;
    vscode.languages.setTextDocumentLanguage = async document => { started = true; await pending.promise; return document; };
    host.message({ type: "openCapturedSource", token: host.last().token, pointer: "/source_changes/files/0", side: "after" }); await flush();
    assert.equal(started, true);
    if (invalidation === "refresh") host.message({ type: "refresh" });
    else host.changed.fire({ reason: "lifecycle" });
    await flush(); pending.resolve(); await flush();
    assert.equal(host.shown.length, 0, invalidation);
  }
});
