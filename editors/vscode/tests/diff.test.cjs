const assert = require("node:assert/strict");
const test = require("node:test");
const Module = require("node:module");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const { URL } = require("node:url");
const { parseDiff, normalizeChoices, diffSections, changeKinds } = require("../out/diff_view");
const before = "file:///before/main.mod", after = "file:///after/variant.mod";
function uri(value) { const parsed = new URL(value); return { scheme: parsed.protocol.slice(0, -1), path: parsed.pathname, fsPath: decodeURIComponent(parsed.pathname), toString: () => value }; }
function location(value = before, line = 3) { return { uri: value, range: { start: { line, character: 2 }, end: { line, character: 12 } } }; }
function target(value, dimension = null) { return { occurrence_id: value.includes("before") ? "old:1" : "new:1", domain: dimension ? "heterogeneous" : "aggregate", dimension, written_locations: [location(value)] }; }
function fixture(options = {}) {
  const result = { added_endogenous: ["n"], removed_endogenous: ["k"], common_endogenous: ["c"], added_exogenous: [], removed_exogenous: [], common_exogenous: [],
    added_parameters: [], removed_parameters: [], common_parameters: ["beta"],
    symbols_changed: [{ name: "c", before: { kind: "endogenous", long_name: "Consumption", tex_name: "c" }, after: { kind: "endogenous", long_name: "Household consumption", tex_name: "c_h" } }],
    changed_parameter_values: [{ name: "beta", old_raw: "0.99", old_value: .99, new_raw: "0.98", new_value: .98 }],
    added_equations: [{ index: 1, text: "n = u", name: null, tags: {}, domain: "aggregate", dimension: null }],
    removed_equations: [], changed_equations: [{ index_old: 2, index_new: 2, text_old: "c = y", text_new: "c = 2*y", name_old: "goods", name_new: "goods", tags_old: { name: "goods" }, tags_new: { name: "goods" }, domain: "aggregate", dimension: null }],
    unmatched_same_name: [{ name: "repeat", dimension: null, removed: [{ index: 3, text: "a = 1", name: "repeat", tags: {}, domain: "aggregate", dimension: null }], added: [{ index: 3, text: "b = 2", name: "repeat", tags: {}, domain: "aggregate", dimension: null }] }],
    heterogeneous_equations: [{ dimension: "households", added: [], removed: [], changed: [{ index_old: 1, index_new: 1, text_old: "c_h = w", text_new: "c_h = w+t", name_old: null, name_new: null, tags_old: {}, tags_new: {}, domain: "heterogeneous", dimension: "households" }], unmatched_same_name: [] }],
    shock_setup_changes: [{ form: "stochastic_shock", role: "variance", target: "u", change: "changed", before: { block: "shocks", values: ["1"], overwrite: false }, after: { block: "shocks", values: ["2"], overwrite: false } },
      { form: "shock_group", role: "membership", target: "supply", change: "added", after: { block: "shock_groups", references: ["u"], overwrite: false } }],
    navigation: { schema_version: 1, before: { root_uri: before, revision: "old", complete: true }, after: { root_uri: after, revision: "new", complete: true }, rows: [] }, markdown: "ignored engine Markdown" };
  const nav = (id, kind, old, next, dimension = null) => result.navigation.rows.push({ id, kind, before: old ? target(before, dimension) : null, after: next ? target(after, dimension) : null });
  nav("/added_endogenous/0", "symbol", false, true); nav("/removed_endogenous/0", "symbol", true, false); nav("/common_endogenous/0", "symbol", true, true); nav("/common_parameters/0", "symbol", true, true);
  nav("/symbols_changed/0", "symbol", true, true); nav("/changed_parameter_values/0", "parameter", true, true); nav("/added_equations/0", "equation", false, true); nav("/changed_equations/0", "equation", true, true);
  nav("/unmatched_same_name/0/removed/0", "equation", true, false); nav("/unmatched_same_name/0/added/0", "equation", false, true);
  nav("/heterogeneous_equations/0/changed/0", "equation", true, true, "households"); nav("/shock_setup_changes/0", "shock", true, true); nav("/shock_setup_changes/1", "shock", false, true);
  return Object.assign(result, options);
}
function noChanges() {
  const result = fixture();
  for (const key of Object.keys(result)) if (Array.isArray(result[key])) result[key] = [];
  result.navigation.rows = []; return result;
}
const defaults = { layout: "auto", expansion: "changes", sections: [...diffSections], changeKinds: [...changeKinds] };

test("every legacy comparison section keeps exact pointer, values, direction and engine pairing", () => {
  const parsed = parseDiff(fixture(), before, after);
  assert.equal(parsed.rows.length, 11); assert.equal(parsed.complete, true);
  assert.deepEqual([...new Set(parsed.rows.map(row => row.section))], [...diffSections]);
  assert.equal(parsed.rows.find(row => row.id === "/added_endogenous/0").before, null);
  assert.equal(parsed.rows.find(row => row.id === "/removed_endogenous/0").after, null);
  const unpaired = parsed.rows.filter(row => row.kind === "unpaired");
  assert.equal(unpaired.length, 2); assert.equal(unpaired[0].after, null); assert.equal(unpaired[1].before, null);
  assert.match(parsed.rows.find(row => row.section === "parameters").before, /0.99/);
  assert.equal(parsed.rows.some(row => row.id.includes("common_")), false);
});
test("symbol scope moves keep both independent dimensions and missing navigation stays unavailable", () => {
  const result = fixture(), source = result.navigation.rows.find(row => row.id === "/symbols_changed/0");
  source.before.dimension = "firms"; source.after.dimension = "households";
  result.navigation.rows.find(row => row.id === "/added_equations/0").after = null;
  const rows = parseDiff(result, before, after).rows;
  assert.deepEqual(rows.find(row => row.id === source.id).scopes, ["firms", "households"]);
  assert.equal(rows.find(row => row.id === "/added_equations/0").navigation.after, null);
});
test("unmapped shock rows retain supplied navigation and independent legacy dimensions", () => {
  for (const legacy of [false, true]) {
    const result = fixture(), nav = result.navigation.rows.find(row => row.id === "/shock_setup_changes/0");
    nav.before = null; nav.after = null;
    if (legacy) {
      result.shock_setup_changes[0].before.heterogeneity = "households";
      result.shock_setup_changes[0].after.heterogeneity = "firms";
    } else nav.dimension = "households";
    const row = parseDiff(result, before, after).rows.find(row => row.id === nav.id);
    assert.deepEqual(row.scopes, legacy ? ["households", "firms"] : ["households"]);
    assert.equal(row.sideScopes.before, "households"); assert.equal(row.sideScopes.after, legacy ? "firms" : "households");
    assert.equal(row.navigation.before, null); assert.equal(row.navigation.after, null);
  }
});
test("malformed, wrong-root, duplicate pointers and unsafe source schemes fail visibly", () => {
  const cases = [value => delete value.navigation, value => value.navigation.schema_version = 2,
    value => value.navigation.before.root_uri = after, value => value.navigation.rows.push(value.navigation.rows[0]),
    value => value.navigation.rows.shift(), value => value.navigation.rows[0].after.written_locations[0].uri = "command:bad",
    value => value.changed_parameter_values[0].old_value = {}, value => value.changed_equations[0].text_new = {}];
  for (const mutate of cases) { const value = fixture(); mutate(value); assert.throws(() => parseDiff(value, before, after), /unsupported comparison/); }
});
test("incomplete expansion withholds rows and no-change responses remain complete", () => {
  assert.equal(parseDiff(noChanges(), before, after).rows.length, 0);
  assert.equal(parseDiff({ status: "incomplete", message: "partial" }, before, after).complete, false);
  const result = fixture(); result.navigation.after.complete = false;
  assert.deepEqual(parseDiff(result, before, after).rows, []);
});
test("control state normalizes lists and preserves empty sections and per-view choices", () => {
  const choices = normalizeChoices({ layout: "stacked", expansion: "none", sections: [], changeKinds: ["changed", "changed", "bad"], search: "<img>", scope: "firms", expanded: { x: true, y: 3 } }, defaults);
  assert.deepEqual(choices.sections, []); assert.deepEqual(choices.changeKinds, ["changed"]); assert.deepEqual(choices.expanded, { x: true });
  assert.equal(choices.layout, "stacked"); assert.equal(choices.scope, "firms"); assert.equal(choices.search, "<img>");
});

const preferenceScopes = [], preferenceLogs = [];
const vscode = { workspace: { getConfiguration: (_section, scope) => {
  preferenceScopes.push(scope);
  const settings = { "diff.layout": "bad", "diff.sections": [], "diff.defaultChangeKinds": ["removed", "removed", "bad"] };
  return { get: (key, fallback) => settings[key] ?? fallback };
} } };
const originalLoad = Module._load;
Module._load = function(id, ...args) { return id === "vscode" ? vscode : originalLoad.call(this, id, ...args); };
const { diffPreferences } = require("../out/diff"); Module._load = originalLoad;
test("launching model preferences use resource scope, normalize settings and honor empty sections", () => {
  const result = diffPreferences(uri(after), message => preferenceLogs.push(message));
  assert.equal(result.layout, "auto"); assert.deepEqual(result.sections, []); assert.deepEqual(result.changeKinds, ["removed"]);
  assert.ok(preferenceScopes.every(scope => scope.uri.toString() === after && scope.languageId === "dynare"));
});

class Element {
  constructor(tag) { this.tag = tag; this.children = []; this.listeners = {}; this.attributes = {}; this._text = ""; }
  set textContent(value) { this._text = value; this.children = []; }
  get textContent() { return this._text + this.children.map(child => child.textContent).join(""); }
  append(...children) { this.children.push(...children); }
  replaceChildren(...children) { this.children = children; this._text = ""; }
  addEventListener(name, listener) { this.listeners[name] = listener; }
  setAttribute(name, value) { this.attributes[name] = value; }
  fire(name) { this.listeners[name]?.(); }
}
function descendants(element) { return [element, ...element.children.flatMap(descendants)]; }
function webview() {
  const elements = Object.fromEntries(["models", "status", "search", "scope", "layout", "expansion", "kinds", "sections", "counts", "results", "refresh", "help"].map(id => [id, new Element(id)]));
  const events = {}, posted = [], states = [];
  const sandbox = { document: { getElementById: id => elements[id], createElement: tag => new Element(tag) }, window: { addEventListener: (name, callback) => events[name] = callback },
    acquireVsCodeApi: () => ({ getState: () => undefined, setState: value => states.push(value), postMessage: message => posted.push(message) }) };
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, "../media/diff_view.js"), "utf8"), sandbox);
  const render = (extra = {}) => events.message({ data: { type: "render", key: "view", token: 4, before, after, rows: parseDiff(fixture(), before, after).rows,
    status: "ready", message: "Current comparison", choices: normalizeChoices({}, defaults), ...extra } });
  return { elements, posted, states, render };
}
test("webview uses text nodes, labels source sides and disables missing/stale locations", () => {
  const env = webview(); const rows = parseDiff(fixture(), before, after).rows; rows[0].label = "<img src=x onerror=bad()>";
  env.render({ rows }); assert.match(env.elements.results.textContent, /<img src=x onerror=bad\(\)>/);
  assert.equal(descendants(env.elements.results).some(element => element.tag === "img"), false);
  const buttons = descendants(env.elements.results).filter(element => element.tag === "button");
  assert.equal(buttons.length, 22); assert.equal(buttons[0].disabled, true); assert.equal(buttons[1].disabled, false);
  buttons[1].fire("click"); assert.equal(env.posted.at(-1).side, "after"); assert.equal(env.posted.at(-1).rowId, "/added_endogenous/0");
  env.render({ status: "stale", message: "Out of date" }); assert.ok(descendants(env.elements.results).filter(element => element.tag === "button").every(button => button.disabled));
});
test("webview text/kind/scope filters, counts, layout and expansion preserve state", () => {
  const env = webview(); env.render(); assert.match(env.elements.counts.textContent, /11 of 11 rows shown/);
  env.elements.search.value = "goods"; env.elements.search.fire("input"); assert.match(env.elements.counts.textContent, /1 of 11 rows shown/);
  env.elements.layout.value = "stacked"; env.elements.layout.fire("change"); assert.equal(env.elements.results.className, "results layout-stacked");
  env.elements.expansion.value = "none"; env.elements.expansion.fire("change"); assert.equal(env.elements.results.children[0].open, false);
  env.elements.search.value = ""; env.elements.search.fire("input"); env.elements.scope.value = "households"; env.elements.scope.fire("change"); assert.match(env.elements.counts.textContent, /1 of 11 rows shown/);
  assert.equal(env.states.at(-1).choices.scope, "households");
  const changed = descendants(env.elements.kinds).filter(element => element.tag === "input")[2]; changed.checked = false; changed.fire("change"); assert.match(env.elements.results.textContent, /No rows match/);
});
test("webview scope filters and side labels retain unmapped heterogeneous shock rows", () => {
  const result = fixture(), nav = result.navigation.rows.find(row => row.id === "/shock_setup_changes/0");
  nav.before = null; nav.after = null; nav.dimension = "households";
  const rows = parseDiff(result, before, after).rows.filter(row => row.id === nav.id), env = webview(); env.render({ rows });
  assert.match(env.elements.results.textContent, /Dimension: households/);
  env.elements.scope.value = "households"; env.elements.scope.fire("change"); assert.match(env.elements.counts.textContent, /1 of 1 rows shown/);
  env.elements.scope.value = "aggregate"; env.elements.scope.fire("change"); assert.match(env.elements.counts.textContent, /0 of 1 rows shown/);
  assert.ok(descendants(env.elements.results).filter(element => element.tag === "button").every(button => button.disabled));
});
