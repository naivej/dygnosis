const { compiledModule } = require("./helpers/extension_modules.cjs");
const assert = require("node:assert/strict");
const test = require("node:test");
const Module = require("node:module");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const { URL } = require("node:url");
const { parseSnapshotDiff, normalizeChoices, diffSections, changeKinds } = require(compiledModule("diff_view"));
const { currentSnapshot, emptySemantic, parseCurrentSnapshot, summaryFor } = require("./helpers/current_snapshot.cjs");
const before = "file:///before/main.mod", after = "file:///after/variant.mod";
function uri(value) { const parsed = new URL(value); return { scheme: parsed.protocol.slice(0, -1), path: parsed.pathname, fsPath: decodeURIComponent(parsed.pathname), toString: () => value }; }
function location(value = before, line = 3) { return { uri: value, range: { start: { line, character: 2 }, end: { line, character: 12 } } }; }
function target(value, dimension = null) { return { occurrence_id: value.includes("before") ? "old:1" : "new:1", domain: dimension ? "heterogeneous" : "aggregate", dimension, written_locations: [location(value)] }; }
function fixture() {
  const diff = emptySemantic(), navigationRows = [];
  const state = value => value === null ? { state: "absent", value: null } : { state: "present", value: { kind: "text", value } };
  const field = (name, old, next) => ({ name, label: name, before: state(old), after: state(next), changed: old !== next, comparison_availability: "complete", numeric_difference: null });
  const add = (family, name, change, old, next, dimension = null, equation_index = null, fields = []) => {
    const pointer = "/semantic/rows/" + diff.semantic.rows.length;
    const side = { name, scope: { domain: dimension ? "heterogeneous" : "aggregate", dimension, block: null }, occurrence: equation_index, equation_index, context: null };
    diff.semantic.rows.push({ pointer, family, name, change, count_unit: "final_fact", facets: [], before: old === null ? null : { ...side },
      after: next === null ? null : { ...side }, fields: [field("expression", old, next), ...fields], expressions: [], timing: [], references: [], limits: [], statement_group: null });
    navigationRows.push({ id: pointer, kind: "semantic", family, name, before: old === null ? null : target(before, dimension), after: next === null ? null : target(after, dimension) });
  };
  add("symbols", "n", "added", null, "endogenous");
  add("symbols", "k", "removed", "endogenous", null);
  add("symbols", "c", "changed", "Consumption", "Household consumption");
  add("parameters", "beta", "changed", "0.99", "0.98");
  add("equations", "Equation", "added", null, "n = u", null, 1);
  add("equations", "goods", "changed", "c = y", "c = 2*y", null, 2);
  add("equations", "repeat", "removed", "a = 1", null, null, 3);
  add("equations", "repeat", "added", null, "b = 2", null, 4);
  add("equations", "Equation", "changed", "c_h = w", "c_h = w+t", "households", 1);
  add("shocks", "u", "changed", "1", "2", null, null, [field("form", "stochastic_shock", "stochastic_shock")]);
  add("shocks", "supply", "added", null, "u", null, null, [field("form", null, "shock_group")]);
  return currentSnapshot({ beforeRoot: before, afterRoot: after, diff, sources: { before: { [before]: "before source" }, after: { [after]: "after source" } }, navigationRows, revisions: { before: "old", after: "new" } });
}
function noChanges() {
  const result = fixture();
  result.diff.semantic.rows = [];
  result.diff.summary = summaryFor(result.diff);
  result.navigation.rows = [];
  return result;
}
const parseCurrent = snapshot => parseCurrentSnapshot(parseSnapshotDiff, snapshot);
const defaults = { sections: [...diffSections], changeKinds: [...changeKinds] };

test("current snapshot rows keep exact pointers, values, direction and engine pairing", () => {
  const parsed = parseCurrent(fixture());
  assert.equal(parsed.rows.length, 11); assert.equal(parsed.complete, true);
  assert.deepEqual([...new Set(parsed.rows.map(row => row.section))], [...diffSections]);
  assert.equal(parsed.rows.find(row => row.id === "/semantic/rows/0").before, null);
  assert.equal(parsed.rows.find(row => row.id === "/semantic/rows/1").after, null);
  const unpaired = parsed.rows.filter(row => row.label === "repeat");
  assert.equal(unpaired.length, 2); assert.equal(unpaired[0].after, null); assert.equal(unpaired[1].before, null);
  assert.deepEqual(unpaired.map(row => row.kind), ["removed", "added"]);
  assert.equal(parsed.rows.find(row => row.section === "parameters").semantic.fields[0].before.value.value, "0.99");
  assert.equal(parsed.rows.some(row => row.id.includes("common_")), false);
});
test("symbol scope moves keep both independent dimensions and missing navigation stays unavailable", () => {
  const result = fixture(), source = result.navigation.rows.find(row => row.id === "/semantic/rows/2");
  source.before.dimension = "firms"; source.after.dimension = "households";
  source.before.domain = source.after.domain = "heterogeneous";
  const row = result.diff.semantic.rows[2];
  row.before.scope = { ...row.before.scope, domain: "heterogeneous", dimension: "firms" };
  row.after.scope = { ...row.after.scope, domain: "heterogeneous", dimension: "households" };
  result.navigation.rows.find(row => row.id === "/semantic/rows/4").after = null;
  const rows = parseCurrent(result).rows;
  assert.deepEqual(rows.find(row => row.id === source.id).scopes, ["firms", "households"]);
  assert.equal(rows.find(row => row.id === "/semantic/rows/4").navigation.after, null);
});
test("unmapped shock rows retain navigation and side-specific dimensions", () => {
  for (const useSideDimensions of [false, true]) {
    const result = fixture(), nav = result.navigation.rows.find(row => row.id === "/semantic/rows/9");
    nav.before = null; nav.after = null;
    const fact = result.diff.semantic.rows[9];
    fact.before.scope = { ...fact.before.scope, dimension: "households", domain: "heterogeneous" };
    fact.after.scope = { ...fact.after.scope, dimension: useSideDimensions ? "firms" : "households", domain: "heterogeneous" };
    const row = parseCurrent(result).rows.find(row => row.id === nav.id);
    assert.deepEqual(row.scopes, useSideDimensions ? ["households", "firms"] : ["households"]);
    assert.equal(row.sideScopes.before, "households"); assert.equal(row.sideScopes.after, useSideDimensions ? "firms" : "households");
    assert.equal(row.navigation.before, null); assert.equal(row.navigation.after, null);
  }
});
test("malformed, wrong-root, duplicate pointers and unsafe source schemes fail visibly", () => {
  const cases = [value => delete value.navigation, value => value.navigation.schema_version = 3,
    value => value.navigation.before.root_file = after, value => value.navigation.rows.push(value.navigation.rows[0]),
    value => value.navigation.rows.shift(), value => value.navigation.rows[0].after.written_locations[0].file_key = "command:bad",
    value => value.diff.semantic.rows[3].fields[0].before.value.value = {}, value => value.diff.semantic.rows[5].fields[0].after.value.value = {}];
  for (const mutate of cases) { const value = fixture(); mutate(value); assert.throws(() => parseCurrent(value), /unsupported comparison/); }
});
test("current snapshot receipts require complete inputs and preserve no-change results", () => {
  assert.equal(parseCurrent(noChanges()).rows.length, 0);
  const result = fixture(); result.navigation.after.complete = false;
  assert.throws(() => parseCurrent(result), /unsupported comparison/);
});
test("control state normalizes kinds and ignores retired section and scope filters", () => {
  const choices = normalizeChoices({ layout: "stacked", expansion: "none", sections: [], changeKinds: ["changed", "changed", "bad"], search: "<img>", scope: "firms", expanded: { x: true, y: 3 }, group: "symbols:Symbols" }, defaults);
  assert.equal(Object.hasOwn(choices, "sections"), false); assert.deepEqual(choices.changeKinds, ["changed"]); assert.equal(choices.group, "symbols:Symbols");
  assert.equal(Object.hasOwn(choices, "expanded"), false); assert.equal(Object.hasOwn(choices, "expansion"), false);
  assert.equal(Object.hasOwn(choices, "layout"), false); assert.equal(Object.hasOwn(choices, "scope"), false); assert.equal(choices.search, "<img>");
  assert.equal(normalizeChoices({ group: "aggregateEquations:Aggregate equations" }, defaults).group, "aggregateEquations:Aggregate equations");
});

const preferenceScopes = [], preferenceLogs = [];
let preferenceKinds = ["removed", "removed", "bad"];
const vscode = { workspace: { getConfiguration: (_section, scope) => {
  preferenceScopes.push(scope);
  const settings = { "diff.layout": "bad", "diff.sections": [], "diff.defaultChangeKinds": preferenceKinds };
  return { get: (key, fallback) => settings[key] ?? fallback };
} } };
const originalLoad = Module._load;
Module._load = function(id, ...args) { return id === "vscode" ? vscode : originalLoad.call(this, id, ...args); };
const { diffPreferences } = require(compiledModule("diff")); Module._load = originalLoad;
test("launching model Kind preferences use resource scope and ignore retired section settings", () => {
  const result = diffPreferences(uri(after), message => preferenceLogs.push(message));
  assert.equal(Object.hasOwn(result, "layout"), false); assert.equal(Object.hasOwn(result, "sections"), false); assert.deepEqual(result.changeKinds, ["removed"]);
  assert.ok(preferenceScopes.every(scope => scope.uri.toString() === after && scope.languageId === "dynare"));
});

class Element {
  constructor(tag) { this.tag = tag; this.children = []; this.listeners = {}; this.attributes = {}; this._text = ""; }
  set textContent(value) { this._text = value; this.children = []; }
  get textContent() { return this._text + this.children.map(child => child.textContent).join(""); }
  append(...children) { this.children.push(...children); }
  replaceChildren(...children) { this.children = children; this._text = ""; }
  addEventListener(name, listener) { this.listeners[name] = listener; }
  querySelectorAll(selector) { return descendants(this).filter(element => element.className?.split(" ").includes(selector.slice(1))); }
  setAttribute(name, value) { this.attributes[name] = value; }
  getAttribute(name) { return this.attributes[name]; }
  focus() { this.focused = true; }
  fire(name, event) { this.listeners[name]?.(event); }
}
function descendants(element) { return [element, ...element.children.flatMap(descendants)]; }
function webview(saved) {
  const elements = Object.fromEntries(["models", "status", "search", "kinds", "counts", "results", "refresh", "help", "reviewContent", "limitsStatus"].map(id => [id, new Element(id)]));
  const events = {}, posted = [], states = [];
  const sandbox = { document: { getElementById: id => elements[id] ?? descendants(elements.results).find(element => element.id === id), createElement: tag => new Element(tag), querySelectorAll: () => descendants(elements.results).filter(element => element.attributes["data-group"]) }, window: { addEventListener: (name, callback) => events[name] = callback },
    acquireVsCodeApi: () => ({ getState: () => saved, setState: value => states.push(globalThis.structuredClone(value)), postMessage: message => posted.push(message) }) };
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, "../media/diff_view.js"), "utf8"), sandbox);
  const render = (extra = {}) => events.message({ data: { type: "render", key: "view", token: 4, captureId: "retained", before, after, rows: parseCurrent(fixture()).rows,
    status: "ready", message: "Current comparison", choices: normalizeChoices({}, defaults), ...extra } });
  const open = id => descendants(elements.results).find(element => element.id === "row-" + id).fire("click");
  return { elements, posted, states, render, open };
}
test("webview uses text nodes, labels source sides and disables missing/stale locations", () => {
  const env = webview(); const rows = parseCurrent(fixture()).rows; rows[0].label = "<img src=x onerror=bad()>";
  env.render({ rows }); assert.match(env.elements.results.textContent, /<img src=x onerror=bad\(\)>/);
  assert.equal(descendants(env.elements.results).some(element => element.tag === "img"), false);
  env.open(rows[0].id);
  const buttons = descendants(env.elements.results).filter(element => element.tag === "button" && element.attributes["aria-label"]?.startsWith("Open")); assert.equal(buttons[0].disabled, true); assert.equal(buttons[1].disabled, false);
  buttons[1].fire("click"); assert.equal(env.posted.at(-1).side, "after"); assert.equal(env.posted.at(-1).rowId, "/semantic/rows/0");
  env.render({ status: "stale", message: "Out of date" }); assert.ok(descendants(env.elements.results).filter(element => element.tag === "button" && element.attributes["aria-label"]?.startsWith("Open")).every(button => button.disabled));
});

test("comparison headers summarize choices, abnormal receipts and heterogeneous counts", () => {
  const env = webview();
  const analysis = { selection: { name: "Policy <A>", preview: true }, configuration: { defines: [{ name: "N", expression: "2" }], equation_selection: { mode: "include", tag: "name", values: ["policy"] }, overrides: [{ kind: "definition", target: { file: "main.mod" }, expression: "1" }] }, model_complete: true,
    overrides: [{ operation: { kind: "definition", target: { file: "main.mod" }, expression: "1" }, name: "x", application: "not_reached", known_execution_count: 0, execution_count: 0, cause: "The parent branch was not reached." }] };
  env.render({ before: "/before/main.mod @ HEAD · abc1234 · Source", inputNames: { before: "main.mod", after: "variant.mod" }, configurationLabels: { before: "Policy <A>", after: "Source" }, analysisSummaries: { before: { analysis: { ...analysis, configuration: { ...analysis.configuration, equation_selection: { mode: "exclude", tag: "name", values: ["policy"] } } }, counts: { n_equations: 1, n_endogenous: 1, heterogeneity_dimensions: [{ dimension: "households", n_equations: 2, n_endogenous: 3 }] }, search_paths: ["includes"] }, after: { analysis: { ...analysis, selection: { name: null, preview: false }, configuration: {}, overrides: [] } } } });
  const [beforeCard, afterCard] = env.elements.models.children;
  const text = env.elements.models.textContent;
  assert.equal(beforeCard.open, false);
  assert.equal(afterCard.open, false);
  assert.match(beforeCard.children[0].textContent, /Beforemain\.modPolicy <A>/);
  assert.doesNotMatch(beforeCard.children[0].textContent, /Macro:|Include paths|endogenous/);
  assert.match(text, /Macro: N=2/);
  assert.match(text, /Equation: name='policy' \(excluded\)/);
  assert.match(text, /Macro: x=1/);
  assert.match(text, /Include paths: includes/);
  assert.match(text, /Macro: x=1 \(not reached\).*parent branch was not reached/);
  assert.match(text, /Model counts are unavailable for this captured input/);
  assert.match(text, /Beforemain\.mod/);
  assert.match(beforeCard.children[1].textContent, /Aggregate model: 1 endogenous · 1 equation/);
  assert.match(beforeCard.children[1].textContent, /Heterogeneous model: households · 3 endogenous · 2 equations/);
  assert.match(beforeCard.children[1].textContent, /\/before\/main\.mod @ HEAD · abc1234 · Source/);
  assert.match(afterCard.children[0].textContent, /Source/);
  assert.doesNotMatch(text, /Complete|Completeness unavailable|Initial macro inputs|applied \d+ times/);
});
test("captured branch labels keep each application result on its own choice", () => {
  const env = webview(), operations = ["first", "second"].map(site_id => ({ kind: "branch", target: { file: "main.mod", site_id, source_revision: "captured" }, branch_id: "else" }));
  const analysis = { selection: { name: null, preview: true }, configuration: { overrides: operations }, overrides: operations.map((operation, index) => ({ operation, branch_label: index ? "@#if ela1" : "@#if ela → @#else", application: index ? "not_reached" : "applied", cause: index ? "The parent was not reached." : null })) };
  env.render({ analysisSummaries: { before: { analysis, counts: { n_endogenous: 1, n_equations: 1, heterogeneity_dimensions: [] } } } });
  const model = env.elements.models.children[0], choices = descendants(model).filter(child => child.className === "analysis-detail");
  assert.equal(choices[0].textContent, "Branch: @#if ela → @#else");
  assert.equal(choices[1].textContent, "Branch: @#if ela1 (not reached) — The parent was not reached.");
  assert.doesNotMatch(model.textContent, /Forced branch|Branch preview in|application unknown/);
});

test("captured repeated macro names identify their own definition lines", () => {
  // Guards Before/After choices that cannot distinguish two definitions of one macro.
  const env = webview(), operations = ["first", "second"].map(site_id => ({ kind: "skip_definition", target: {
    file: "file:///workspace/settings.inc", site_id, source_revision: "captured" } }));
  const analysis = { selection: { name: null, preview: true }, configuration: { overrides: operations },
    overrides: operations.map((operation, index) => ({ operation, name: "FLAG", source_line: [12, 40][index], application: "applied", cause: null })) };
  env.render({ analysisSummaries: { before: { analysis, counts: { n_endogenous: 1, n_equations: 1, heterogeneity_dimensions: [] } } } });
  const choices = descendants(env.elements.models.children[0]).filter(child => child.className === "analysis-detail" && child.textContent.startsWith("Skip "));
  assert.deepEqual(choices.map(choice => choice.textContent), ["Skip FLAG definition · settings.inc:12", "Skip FLAG definition · settings.inc:40"]);
});

test("incomplete comparison headers keep both choices and explain unavailable counts", () => {
  const env = webview();
  env.render({ rows: [], status: "incomplete", configurationLabels: { before: "Policy A", after: "Policy B" }, analysisSummaries: {
    before: { requested_analysis: { name: "Policy A", defines: [{ name: "ON", expression: "false" }], overrides: [{ kind: "definition", target: { file: "models/main.mod" }, expression: "1" }] }, complete: true, counts: null, counts_cause: "Comparison stopped before counting the models.", cause: { message: "The source operation could not be matched." } },
    after: { requested_analysis: { name: "Policy B", defines: [{ name: "ON", expression: "true" }] }, complete: false, counts: null, counts_cause: "The active include missing.inc is unavailable." },
  } });
  const text = env.elements.models.textContent;
  assert.match(text, /Macro: ON=false/);
  assert.match(text, /Macro: ON=true/);
  const unavailableOverride = "Macro: 1 · main.mod · application unavailable";
  assert.equal(text.split(unavailableOverride).length - 1, 1);
  assert.match(text, /Macro: 1 · main\.mod · application unavailable/);
  assert.doesNotMatch(text, /Definition override in main\.mod = 1/);
  assert.match(env.elements.models.children[0].children[0].textContent, /Policy A/);
  assert.match(env.elements.models.children[1].children[0].textContent, /Policy B/);
  assert.match(text, /Comparison stopped before counting the models/); assert.match(text, /active include missing.inc is unavailable/);
  assert.match(text, /The source operation could not be matched/);
  assert.doesNotMatch(text, /Complete|Completeness unavailable|Initial macro inputs|Analysis preview|Macro inputs/);
  assert.doesNotMatch(text, /0 equations|0 endogenous/);
});
test("compact rows keep Before and After details and source targets open together", () => {
  const env = webview(), rows = parseCurrent(fixture()).rows; env.render();
  const groups = descendants(env.elements.results).filter(element => element.attributes["data-group"]).map(element => element.attributes["data-group"]);
  assert.deepEqual(groups.filter(group => group.startsWith("equations:")), ["equations:Equations"]);
  const visited = [];
  for (const group of groups) {
    descendants(env.elements.results).find(element => element.attributes["data-group"] === group).fire("click");
    const articles = descendants(env.elements.results).filter(element => element.tag === "article");
    assert.deepEqual(articles.map(article => article.attributes["data-row-id"]), rows.filter(row => row.section + ":" + row.group === group).map(row => row.id));
    visited.push(...articles.map(article => article.attributes["data-row-id"]));
    assert.equal(descendants(env.elements.results).find(element => element.attributes["data-group"] === group).focused, true);
    for (const [index, article] of articles.entries()) {
      const row = rows.find(row => row.id === article.attributes["data-row-id"]);
      env.open(row.id);
      const open = descendants(env.elements.results).find(element => element.attributes["data-row-id"] === row.id);
      const cards = descendants(open).filter(element => element.className?.startsWith("side "));
      assert.deepEqual(cards.map(card => card.className), ["side before", "side after"]);
      assert.equal(descendants(env.elements.results).filter(element => element.className === "detail").length, index + 1);
      for (const [index, side] of ["before", "after"].entries()) {
        const button = descendants(cards[index]).find(element => element.className === "source-link");
        if (!button.disabled) { button.fire("click"); assert.equal(env.posted.at(-1).rowId, row.id); assert.equal(env.posted.at(-1).side, side); }
      }
    }
  }
  assert.deepEqual(visited, rows.map(row => row.id));
  const restored = normalizeChoices(env.states.at(-1).choices, defaults); env.render({ choices: restored });
  assert.equal(env.states.at(-1).choices.group, groups.at(-1));
});
test("webview text and kind filters preserve state and choose a visible group", () => {
  const env = webview(); env.render(); assert.equal(env.elements.counts.textContent, "11 changes");
  env.elements.search.value = "goods"; env.elements.search.fire("input"); assert.equal(env.elements.counts.textContent, "1 of 11 changes");
  assert.equal(env.states.at(-1).choices.group, "equations:Equations");
  env.elements.search.value = ""; env.elements.search.fire("input"); assert.equal(env.elements.counts.textContent, "11 changes");
  const all = descendants(env.elements.kinds).find(element => element.attributes["data-change-type"] === "all"); all.checked = false; all.fire("change");
  const removed = descendants(env.elements.kinds).find(element => element.attributes["data-change-type"] === "removed"); removed.checked = true; removed.fire("change");
  assert.equal(env.elements.counts.textContent, "2 of 11 changes");
  env.elements.search.value = "no such symbol"; env.elements.search.fire("input");
  descendants(env.elements.results).find(element => element.className === "reset-filters").fire("click");
  assert.equal(env.elements.counts.textContent, "11 changes");
});
test("side labels retain unmapped heterogeneous shock rows without a scope filter", () => {
  const result = fixture(), nav = result.navigation.rows.find(row => row.id === "/semantic/rows/9");
  nav.before = null; nav.after = null;
  const fact = result.diff.semantic.rows[9];
  fact.before.scope = fact.after.scope = { ...fact.before.scope, dimension: "households", domain: "heterogeneous" };
  const rows = parseCurrent(result).rows.filter(row => row.id === nav.id), env = webview(); env.render({ rows });
  assert.match(env.elements.results.textContent, /Dimension: households/);
  assert.equal(env.elements.counts.textContent, "1 change");
  env.open(rows[0].id);
  const actions = descendants(env.elements.results).filter(element => element.className === "source-link");
  assert.equal(actions.length, 2);
  assert.ok(actions.every(button => button.disabled));
});

test("returning to source restores the same captured row and discards reused IDs on a new capture", () => {
  const env = webview(), id = "/semantic/rows/0";
  env.render();
  env.open(id);
  env.elements.reviewContent.scrollTop = 81;
  descendants(env.elements.results).find(element => element.id === "source-after-" + id).fire("click");
  const saved = env.states.at(-1), restored = webview(saved);
  restored.render({ choices: saved.choices });
  assert.equal(restored.elements.reviewContent.scrollTop, 81);
  assert.equal(descendants(restored.elements.results).find(element => element.id === "source-after-" + id).focused, true);
  restored.render({ captureId: "new" });
  assert.equal(descendants(restored.elements.results).some(element => element.className === "detail"), false);
});
