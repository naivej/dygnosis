/* global acquireVsCodeApi, document, window */
(() => {
  "use strict";
  const api = acquireVsCodeApi(), saved = api.getState();
  const names = { symbols: "Symbols", parameters: "Parameters", aggregateEquations: "Aggregate equations", heterogeneousEquations: "Equations by dimension", shockSetup: "Shock setup", shockAnalysisSetup: "Shock analysis setup", steadyState: "Steady state", priors: "Priors", commands: "Commands", observables: "Observables", data: "Data", occbin: "OccBin", policy: "Policy", semiStructural: "Semi-structural", moments: "Moments and IRFs", msSbvar: "MS-SBVAR", heterogeneity: "Heterogeneity", externalFunctions: "External functions", trends: "Trends", operations: "Operations", macroContext: "Macro context" };
  const kinds = ["added", "removed", "changed", "unpaired"], marks = { added: "+", removed: "−", changed: "~", unpaired: "?" };
  const controls = Object.fromEntries(["models", "status", "search", "scope", "layout", "expansion", "kinds", "sections", "counts", "results", "refresh", "comparisonLimits", "limitsSummary", "limitsBody", "layoutFilter"].map(id => [id, document.getElementById(id)]));
  let payload, choices, references = new Map();
  const node = (tag, text, className) => { const element = document.createElement(tag); if (text !== undefined) element.textContent = text; if (className) element.className = className; return element; };
  const titleCase = text => text.replaceAll("_", " ").replace(/^./, letter => letter.toUpperCase());
  const scopeLabel = value => value === "aggregate" ? "Aggregate" : value === "global" ? "Global" : "Dimension: " + value;
  // These are display labels only. Source actions retain the host's exact keys.
  const basename = value => value.replaceAll("\\", "/").split("/").at(-1) || value;
  const post = (type, args = {}) => api.postMessage({ type, token: payload.token, ...args });
  const remember = () => { api.setState({ key: payload.key, choices }); api.postMessage({ type: "choices", key: payload.key, choices }); };
  function update() { renderRows(); remember(); }
  function filterChoice(container, entries, property, savedProperty, label) {
    const selected = choices[property], all = entries.map(([value]) => value);
    const mode = selected.length === all.length && all.every(value => selected.includes(value)) ? "all" : selected.length === 1 ? selected[0] : "saved";
    if (mode === "saved") choices[savedProperty] = [...selected];
    choices[savedProperty] ??= [...selected];
    container.replaceChildren();
    const saved = choices[savedProperty], savedChoices = saved.length === 0 ? [["saved", "No " + label + " selected"]] : saved.length > 1 && saved.length < all.length ? [["saved", "Saved selection (" + saved.length + ")"]] : [];
    for (const [value, text] of [["all", "All " + label], ...entries, ...savedChoices]) {
      const option = node("option", text); option.value = value; container.append(option);
    }
    container.value = mode;
    container.title = selected.map(value => entries.find(([key]) => key === value)?.[1]).join(", ") || "No " + label + " selected";
  }
  function renderControls() {
    controls.search.value = choices.search; controls.layout.value = choices.layout; controls.expansion.value = choices.expansion;
    const scopes = [...new Set(payload.rows.flatMap(row => row.scopes))].sort(); controls.scope.replaceChildren();
    for (const value of ["all", ...scopes]) { const option = node("option", value === "all" ? "All scopes" : scopeLabel(value)); option.value = value; controls.scope.append(option); }
    if (choices.scope !== "all" && !scopes.includes(choices.scope)) { const option = node("option", scopeLabel(choices.scope) + " (no rows)"); option.value = choices.scope; controls.scope.append(option); }
    controls.scope.value = choices.scope;
    filterChoice(controls.kinds, kinds.map(kind => [kind, titleCase(kind)]), "changeKinds", "customChangeKinds", "kinds");
    filterChoice(controls.sections, Object.entries(names), "sections", "customSections", "sections");
  }
  function limits(parent, entries) {
    if (!entries?.length) return;
    const list = node("ul", undefined, "limits");
    for (const limit of entries) list.append(node("li", limit.reason + (limit.omitted === null ? "" : " (" + limit.omitted + " omitted)"))); parent.append(list);
  }
  function action(label, type, args, reason) {
    const button = node("button", label); button.type = "button"; button.disabled = !!reason;
    button.setAttribute("aria-label", reason ? label + " unavailable: " + reason : label); if (reason) button.title = reason;
    button.addEventListener("click", () => { if (!button.disabled) post(type, args); }); return button;
  }
  function sourceAction(row, side) {
    const value = row[side], target = row.navigation[side];
    const reason = value === null ? "This row is absent on this side." : payload.status !== "ready" ? "Refresh the comparison to enable source actions." : !target?.written_locations.length ? "No verified written location is available for this row." : "";
    return action("Open " + titleCase(side) + " source", "openSource", { rowId: row.id, side }, reason);
  }
  function codeLines(parent, expression, side) {
    if (!expression) { parent.append(node("p", "Not present", "source-note")); return; }
    const lines = [[]];
    // The host validates runs. Text insertion never creates HTML or actions.
    for (const run of expression.runs) run.text.split("\n").forEach((part, index, parts) => {
      const text = part + (index < parts.length - 1 ? "\n" : "");
      const span = node("span", text, "token " + run.role);
      if (run.role !== "unchanged") span.setAttribute("aria-label", titleCase(side) + ": " + run.role + " " + text);
      lines.at(-1).push(span); if (index < parts.length - 1) lines.push([]);
    });
    const block = node("div", undefined, "expression");
    for (const spans of lines) {
      const line = node("div", undefined, "code-line"), gutter = node("span", "·", "gutter"), code = node("code");
      gutter.setAttribute("aria-hidden", "true"); code.append(...spans); line.append(gutter, code); block.append(line);
    }
    parent.append(block);
  }
  function sidePanel(row, side) {
    const panel = node("div", undefined, "side " + side), heading = node("div", undefined, "side-title"), own = row.semantic?.[side];
    heading.append(node("strong", titleCase(side)));
    if (row.sideScopes[side] !== null) heading.append(node("span", scopeLabel(row.sideScopes[side]), "scope-label"));
    panel.append(heading);
    if (!row.semantic) panel.append(node("pre", row[side] === null ? "Not present" : row[side]));
    else if (!own) panel.append(node("p", "Not present", "source-note"));
    else {
      if (own.context) panel.append(node("p", own.context.name + " · occurrence " + (own.context.execution_order + 1) + (own.scope.block ? " · " + own.scope.block : ""), "source-note"));
      if (own.equation_index !== null) panel.append(node("p", "Equation " + (own.equation_index + 1), "source-note"));
      for (const expression of row.semantic.expressions) {
        panel.append(node("h3", titleCase(expression.field) + " · expanded text", "expression-heading")); codeLines(panel, expression[side], side);
        if (expression.highlight_basis === "unpaired_text_only") panel.append(node("p", "Text only. Occurrence pairing is not proven.", "source-note"));
        if (expression.availability !== "complete") panel.append(node("p", expression.reason ?? "Highlights: " + titleCase(expression.availability), "source-note"));
      }
    }
    return panel;
  }
  function valueText(value) {
    if (!value) return "Unknown";
    switch (value.kind) {
      case "text": return value.value === "" ? '"" (explicit empty)' : value.value;
      case "number": case "integer": case "boolean": return String(value.value);
      case "list": return "[" + value.value.map(valueText).join(", ") + "]";
      case "record": return Object.entries(value.value).map(([name, value]) => titleCase(name) + ": " + valueText(value)).join("\n");
      default: return "Unknown";
    }
  }
  const fieldText = field => field.state === "absent" ? "Not supplied" : field.state === "unknown" ? "Unknown" : valueText(field.value);
  function fieldValue(field, className) {
    const text = fieldText(field), value = field.value;
    if (text.length <= 240 || !value || value.kind !== "list" && value.kind !== "record") return node("span", text, className);
    const detail = node("details", undefined, "field-value"), label = value.kind === "list" ? value.value.length + " retained items" : Object.keys(value.value).length + " named fields";
    detail.append(node("summary", label + " · show values"), node("pre", text, className)); return detail;
  }
  function rowDetail(row) {
    const article = node("article", undefined, "detail change " + row.kind), header = node("div", undefined, "detail-heading");
    header.append(node("span", marks[row.kind], "change-mark " + row.kind), node("h2", row.label), node("span", titleCase(row.kind) + (row.semantic?.facets.length ? " · " + row.semantic.facets.map(titleCase).join(" · ") : ""), "facet")); article.append(header);
    const locations = ["before", "after"].flatMap(side => (row.navigation[side]?.written_locations ?? []).map(location => titleCase(side) + ": " + basename(location.uri) + ":" + (location.range.start.line + 1)));
    if (locations.length) {
      const location = node("p", [...new Set(locations)].join(" · "), "location");
      location.title = ["before", "after"].flatMap(side => (row.navigation[side]?.written_locations ?? []).map(location => titleCase(side) + ": " + location.uri)).join("\n"); article.append(location);
    }
    if (!row.semantic && payload.semantic) article.append(node("p", "Typed detail is unavailable for this row. Structural detail remains; review Comparison limits.", "source-note"));
    if (row.semantic) {
      const detail = row.semantic;
      const changed = detail.fields.filter(field => field.changed && field.comparison_availability === "complete");
      if (changed.length) article.append(node("p", "Changed fields: " + changed.map(field => field.label).join(", ") + ".", "summary-line"));
      article.append(node("p", "Count unit: " + (detail.count_unit === "final_fact" ? "final fact" : detail.count_unit === "operation" ? "operation" : "accepted occurrence") + ". References and context do not add changes.", "count-note"));
      if (detail.change === "unpaired") article.append(node("p", "Unpaired occurrence. Before and After do not prove a model transition.", "source-note"));
      if (detail.fields.length) {
        const table = node("table", undefined, "fields"), head = node("tr");
        for (const label of ["Field", "Before", "After"]) head.append(node("th", label)); table.append(head);
        for (const field of detail.fields) {
          const tr = node("tr"), th = node("th", field.label); th.setAttribute("scope", "row"); tr.append(th);
          for (const side of ["before", "after"]) {
            const cell = node("td"), available = field.comparison_availability === "complete";
            if (field.changed && available) cell.append(node("span", side === "before" ? "− " : "+ ", "field-cue"));
            cell.append(fieldValue(field[side], field.changed && available ? side === "before" ? "token removed" : "token added" : ""));
            if (!available) cell.append(node("p", "Comparison unavailable", "source-note")); tr.append(cell);
          }
          table.append(tr);
        }
        const scroll = node("div", undefined, "table-scroll"); scroll.append(table); article.append(scroll);
        for (const field of detail.fields) if (field.numeric_difference !== null) article.append(node("p", field.label + " · After − Before: " + String(field.numeric_difference), "note"));
      }
      if (detail.timing.length) {
        article.append(node("h3", "Timing"));
        for (const use of detail.timing) {
          const side = item => item ? item.name + " · written " + item.written_offset + " · after convention " + item.converted_offset + " · use " + (item.occurrence + 1) : "Not present";
          article.append(node("p", "Before: " + side(use.before) + " → After: " + side(use.after)));
        }
      }
    }
    const sides = node("div", undefined, "sides"); sides.append(sidePanel(row, "before"), sidePanel(row, "after")); article.append(sides);
    if (row.semantic) {
      const refs = node("section", undefined, "refs");
      if (row.semantic.references.length) refs.append(node("h3", "Direct equation references"));
      for (const pointer of row.semantic.references) {
        const ref = references.get(pointer); if (!ref) continue;
        const line = node("p", undefined, "reference"), target = ref.navigation[ref.side];
        const button = action(ref.label, "openReference", { pointer, side: ref.side }, payload.status !== "ready" ? "Refresh to enable source actions." : !target?.written_locations.length ? "No verified written location is available." : "");
        button.setAttribute("aria-label", "Open " + titleCase(ref.side) + " reference: " + ref.label + (button.disabled ? ". " + button.title : ""));
        line.append(button, node("span", titleCase(ref.side) + " · " + scopeLabel(ref.scope.dimension ?? ref.scope.domain) + " · written " + ref.timing.written_offset + ", after convention " + ref.timing.converted_offset, "source-note")); refs.append(line);
      }
      if (row.semantic.references.length) article.append(refs);
      limits(article, row.semantic.limits);
    }
    const actions = node("div", undefined, "actions"); actions.append(sourceAction(row, "before"), sourceAction(row, "after")); article.append(actions);
    return article;
  }
  function searchText(row) { return [row.label, row.group, row.before ?? "", row.after ?? "", ...(row.semantic?.facets ?? []).map(titleCase), ...(row.semantic?.fields ?? []).flatMap(field => [field.label, fieldText(field.before), fieldText(field.after)])].join("\n").toLocaleLowerCase(); }
  function modelRows() {
    const available = payload.rows.filter(row => choices.sections.includes(row.section)), query = choices.search.trim().toLocaleLowerCase();
    const shown = available.filter(row => choices.changeKinds.includes(row.kind) && (choices.scope === "all" || row.scopes.includes(choices.scope)) && (!query || searchText(row).includes(query))).sort((a, b) => choices.sections.indexOf(a.section) - choices.sections.indexOf(b.section));
    const breakdown = kinds.map(kind => shown.filter(row => row.kind === kind).length + " " + kind).join(" · ");
    controls.counts.textContent = shown.length + " of " + available.length + " model rows shown · " + breakdown;
    if (!shown.some(row => row.id === choices.selected)) choices.selected = shown[0]?.id ?? null;
    if (!shown.length) {
      const text = !choices.sections.length ? "All sections are hidden. Choose a section to show its rows." : ["failure", "incomplete", "loading"].includes(payload.status) ? "" : payload.rows.length ? "No rows match these filters." : payload.sourceChanges?.files.length ? "No model changes. Captured text differs; use Captured file text diff in More actions." : payload.coverage?.availability !== "complete" && payload.semantic ? "No model rows. Review Comparison limits before concluding that the model is unchanged." : payload.status === "ready" ? "No structural changes. Written text can still differ." : "Refresh to compare current inputs.";
      if (text) controls.results.append(node("p", text, "empty")); return;
    }
    const groups = new Map();
    for (const row of shown) { const key = row.section + ":" + row.group; if (!groups.has(key)) groups.set(key, { label: row.group, rows: [] }); groups.get(key).rows.push(row); }
    const grid = node("div", undefined, "focused"), list = node("nav", undefined, "change-tree"); list.setAttribute("aria-label", "Model changes");
    for (const [key, group] of groups) {
      const section = node("details", undefined, "tree-group"), heading = node("summary", undefined, "group-heading"); section.open = choices.expanded[key] ?? choices.expansion !== "none";
      heading.append(node("span", group.label), node("span", String(group.rows.length), "group-count")); heading.title = group.rows.length + " model row appearances"; section.append(heading);
      section.addEventListener("toggle", () => { choices.expanded[key] = section.open; remember(); });
      for (const row of group.rows) {
        const button = node("button", undefined, "list-row " + row.kind + (row.id === choices.selected ? " selected" : "")); button.type = "button"; button.setAttribute("aria-pressed", String(row.id === choices.selected));
        button.setAttribute("aria-label", titleCase(row.kind) + " " + row.label + ", " + row.scopes.map(scopeLabel).join(" · "));
        const label = node("span"); label.append(node("span", row.label, "row-title"), node("span", (row.semantic?.facets.map(titleCase).join(" · ") || row.group) + " · " + row.scopes.map(scopeLabel).join(" · "), "row-subtitle"));
        button.append(node("span", marks[row.kind], "change-mark " + row.kind), label);
        button.setAttribute("data-row-id", row.id);
        button.addEventListener("click", () => { choices.selected = row.id; update(); focusRow("data-row-id", row.id); }); section.append(button);
      }
      list.append(section);
    }
    const detail = node("section", undefined, "selected-detail"); detail.setAttribute("aria-label", "Selected change"); detail.append(rowDetail(shown.find(row => row.id === choices.selected))); grid.append(list, detail); controls.results.append(grid);
  }
  function comparisonLimits() {
    if (!controls.comparisonLimits) return;
    const coverage = payload.coverage, source = payload.sourceChanges;
    controls.comparisonLimits.hidden = payload.status === "loading";
    controls.limitsSummary.textContent = "Comparison limits" + (payload.status === "ready" && [coverage, source, payload.semantic].some(value => value && value.availability !== "complete") ? " · Partial" : "");
    const body = controls.limitsBody; body.replaceChildren();
    const boundary = !coverage || coverage.source_boundary === "parsed_models_only" ? "Captured source text is unavailable." : coverage.source_boundary.startsWith("supplied_") ? "Only supplied roots and executed include text are compared." : "Captured roots and executed includes are compared.";
    body.append(node("p", boundary + " Unexecuted child files, external MATLAB functions and data-file contents are not captured."));
    if (payload.semanticMessage) body.append(node("p", payload.semanticMessage));
    const entries = [...(coverage?.limits ?? []), ...(payload.semantic?.limits ?? []), ...(source?.limits ?? []), ...(coverage?.families ?? []).flatMap(family => family.limits), ...(source?.files ?? []).flatMap(file => (file.limits ?? []).map(limit => ({ ...limit, reason: (file.after?.file_key ?? file.before?.file_key) + ": " + limit.reason })))];
    const unique = new Map(entries.map(limit => [limit.reason + ":" + limit.omitted, limit]));
    limits(body, [...unique.values()]);
    for (const family of coverage?.families ?? []) if (family.availability !== "complete" && !family.limits.length) body.append(node("p", titleCase(family.family) + " comparison: " + titleCase(family.availability) + "."));
  }
  function renderRows() {
    if (!payload || !choices) return;
    controls.results.className = "results layout-" + choices.layout; controls.results.replaceChildren();
    if (choices.capture !== payload.capture && payload.status === "ready") { choices.capture = payload.capture ?? ""; choices.selected = null; }
    modelRows(); comparisonLimits();
    const selected = payload.rows.find(row => row.id === choices.selected);
    if (controls.layoutFilter) controls.layoutFilter.hidden = !selected || !!selected.semantic && !selected.semantic.expressions.length;
  }
  function focusRow(attribute, identity) { const entries = document.querySelectorAll?.("[" + attribute + "]") ?? []; [...entries].find(element => element.getAttribute(attribute) === identity)?.focus(); }
  controls.refresh.addEventListener("click", () => post("refresh"));
  for (const name of ["changeComparison", "swap", "rootTextDiff", "capturedTextDiff", "updateRevision", "choosePath", "details", "help"]) document.getElementById(name)?.addEventListener("click", () => { post(name); const more = document.getElementById("moreActions"); if (more) more.open = false; });
  document.getElementById("moreActions")?.addEventListener("keydown", event => { if (event.key === "Escape") { const more = document.getElementById("moreActions"); more.open = false; more.querySelector("summary")?.focus(); } });
  controls.search.addEventListener("input", () => { choices.search = controls.search.value; update(); });
  for (const [control, property, savedProperty, entries] of [[controls.kinds, "changeKinds", "customChangeKinds", kinds], [controls.sections, "sections", "customSections", Object.keys(names)]]) control.addEventListener("change", () => {
    choices[property] = control.value === "all" ? [...entries] : control.value === "saved" ? [...choices[savedProperty]] : [control.value];
    renderControls(); update();
  });
  for (const name of ["scope", "layout", "expansion"]) controls[name].addEventListener("change", () => { choices[name] = controls[name].value; if (name === "expansion") choices.expanded = {}; update(); });
  controls.comparisonLimits?.addEventListener("toggle", () => { if (payload && choices) { choices.limitsOpen = controls.comparisonLimits.open; remember(); } });
  window.addEventListener("message", event => {
    const message = event.data; if (!message || message.type !== "render" || !Array.isArray(message.rows)) return;
    payload = message; references = new Map((message.references ?? []).map(reference => [reference.pointer, reference])); choices = message.choices; if (controls.comparisonLimits) controls.comparisonLimits.open = choices.limitsOpen;
    controls.models.replaceChildren();
    for (const [label, input] of [["Before", message.before], ["After", message.after]]) {
      const model = node("div", undefined, "input-side"), parts = input.split(" · "); model.title = input;
      model.append(node("small", label), node("code", basename(parts[0])), node("span", parts.slice(1).join(" · "), "input-context")); controls.models.append(model);
    }
    controls.status.textContent = message.message; controls.status.className = message.status; controls.refresh.disabled = message.status === "loading";
    controls.status.hidden = message.status === "ready" && message.message === "Current comparison";
    const folders = document.getElementById("folders"); if (folders) folders.textContent = message.hasHistory ? "Extra include folders use the current settings. Historical sources use only their selected commits." : "";
    for (const name of ["swap", "changeComparison", "updateRevision"]) { const button = document.getElementById(name); if (button) button.disabled = name === "updateRevision" ? !message.hasHistory : false; }
    const path = document.getElementById("choosePath"); if (path) path.hidden = message.status !== "failure" || !/model|root|path|file|source/i.test(message.message);
    const boundary = document.getElementById("sourceBoundary"); if (boundary) boundary.textContent = !message.coverage ? "Captured source boundary unavailable" : message.coverage.source_boundary === "parsed_models_only" ? "Parsed models only · Source unavailable" : message.coverage.source_boundary.startsWith("supplied_") ? "Supplied root + executed includes" : "Captured root + executed includes";
    const root = document.getElementById("rootTextDiff"); if (root) root.disabled = message.status !== "ready";
    const captured = document.getElementById("capturedTextDiff"); if (captured) captured.disabled = message.status !== "ready" || !message.sourceChanges?.files.length;
    renderControls(); renderRows(); remember();
  });
  api.postMessage({ type: "ready", key: saved?.key, choices: saved?.choices });
})();
