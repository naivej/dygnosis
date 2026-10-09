/* global acquireVsCodeApi, document, window */
(() => {
  "use strict";
  const api = acquireVsCodeApi(), saved = api.getState();
  const names = { symbols: "Symbols", parameters: "Parameters", aggregateEquations: "Aggregate equations", heterogeneousEquations: "Equations by dimension", shockSetup: "Shock setup", shockAnalysisSetup: "Shock analysis setup", steadyState: "Steady state", priors: "Priors", commands: "Commands", observables: "Observables", data: "Data", occbin: "OccBin", policy: "Policy", semiStructural: "Semi-structural", moments: "Moments and IRFs", msSbvar: "MS-SBVAR", heterogeneity: "Heterogeneity", externalFunctions: "External functions", trends: "Trends", operations: "Operations", macroContext: "Macro context" };
  const kinds = ["added", "removed", "changed", "unpaired"], marks = { added: "+", removed: "−", changed: "~", unpaired: "?" };
  const controls = Object.fromEntries(["models", "status", "search", "scope", "layout", "expansion", "kinds", "sections", "counts", "results", "refresh", "presentation", "modelTab", "sourceTab", "coverageTab"].map(id => [id, document.getElementById(id)]));
  let payload, choices, references = new Map();
  const node = (tag, text, className) => { const element = document.createElement(tag); if (text !== undefined) element.textContent = text; if (className) element.className = className; return element; };
  const titleCase = text => text.replaceAll("_", " ").replace(/^./, letter => letter.toUpperCase());
  const scopeLabel = value => value === "aggregate" ? "Aggregate" : value === "global" ? "Global" : "Dimension: " + value;
  // These are display labels only. Source actions retain the host's exact keys.
  const basename = value => value.replaceAll("\\", "/").split("/").at(-1) || value;
  const fileContext = value => value.replaceAll("\\", "/").split("/").slice(-2).join("/");
  const post = (type, args = {}) => api.postMessage({ type, token: payload.token, ...args });
  const remember = () => { api.setState({ key: payload.key, choices }); api.postMessage({ type: "choices", key: payload.key, choices }); };
  function update() { renderRows(); remember(); }
  function localState() { return Object.fromEntries(Object.entries(choices).filter(([key]) => !["presentations", "presentation"].includes(key))); }
  function switchPresentation(value) {
    choices.presentations[choices.presentation] = localState();
    const next = choices.presentations[value], defaults = payload.defaults ?? choices;
    choices = { ...(next ?? { ...defaults, search: "", scope: "all", expanded: {}, selected: null, sourceSelected: null, capture: "", tab: choices.tab }), presentation: value, presentations: choices.presentations };
    renderControls(); update(); controls.presentation?.focus();
  }
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
    if (controls.presentation) controls.presentation.value = choices.presentation;
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
    if (!row.semantic && payload.semantic) article.append(node("p", "Typed detail is unavailable for this row. Structural detail remains; review Coverage for limits.", "source-note"));
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
      const text = !choices.sections.length ? "All sections are hidden. Choose a section to show its rows." : ["failure", "incomplete", "loading"].includes(payload.status) ? "" : payload.rows.length ? "No rows match these filters." : payload.sourceChanges?.files.length ? "No model changes. Captured text differs; open Source changes." : payload.coverage?.availability !== "complete" && payload.semantic ? "No model rows. Review Coverage for comparison limits and the captured source boundary." : payload.status === "ready" ? "No structural changes. Written text can still differ." : "Refresh to compare current inputs.";
      if (text) controls.results.append(node("p", text, "empty")); return;
    }
    const groups = new Map();
    for (const row of shown) { const key = row.section + ":" + row.group; if (!groups.has(key)) groups.set(key, { label: row.group, rows: [] }); groups.get(key).rows.push(row); }
    if (choices.presentation === "focusedReview") {
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
    } else {
      for (const [key, group] of groups) {
        const section = node("details", undefined, "list-group"), heading = node("summary", undefined, "group-heading"); section.open = choices.expanded[key] ?? choices.expansion !== "none";
        heading.append(node("span", group.label), node("span", String(group.rows.length), "group-count")); heading.title = group.rows.length + " model row appearances"; section.append(heading);
        section.addEventListener("toggle", () => { choices.expanded[key] = section.open; remember(); });
        for (const row of group.rows) {
          const detail = node("details", undefined, "row-summary " + row.kind); detail.open = choices.expanded[row.id] ?? choices.expansion !== "none";
          detail.setAttribute("data-row-id", row.id);
          const summary = node("summary"); summary.append(node("span", marks[row.kind], "change-mark " + row.kind), node("span", row.label, "row-title"), node("span", (row.semantic?.facets.map(titleCase).join(" · ") || row.group) + " · " + row.scopes.map(scopeLabel).join(" · "), "row-subtitle")); summary.setAttribute("aria-label", titleCase(row.kind) + " " + row.label + ", " + row.scopes.map(scopeLabel).join(" · ")); detail.append(summary, rowDetail(row));
          detail.addEventListener("toggle", () => { choices.expanded[row.id] = detail.open; choices.selected = row.id; remember(); }); section.append(detail);
        }
        controls.results.append(section);
      }
    }
  }
  function capturedAction(file, side) {
    const own = file[side], reason = payload.status !== "ready" ? "Refresh to enable source actions." : !own?.exact_text_available ? "Complete captured text is unavailable on this side." : "";
    return action("Open " + titleCase(side) + " captured text", "openCapturedSource", { pointer: file.pointer, side }, reason);
  }
  function sourceDetail(file) {
    const detail = node("article", undefined, "detail change selected-detail " + file.change), heading = node("div", undefined, "detail-heading"), key = file.after?.file_key ?? file.before?.file_key;
    heading.append(node("span", marks[file.change], "change-mark " + file.change), node("h2", basename(key)), node("span", titleCase(file.change) + " · captured source", "facet"));
    const location = node("p", ["before", "after"].filter(side => file[side]).map(side => titleCase(side) + ": " + fileContext(file[side].file_key)).join(" · "), "location"); location.title = [file.before?.file_key, file.after?.file_key].filter(Boolean).join("\n");
    detail.append(heading, location, node("p", "Captured source text. Model rows can also describe edits in these hunks.", "summary-line"));
    const buttons = node("div", undefined, "actions"); buttons.append(capturedAction(file, "before"), capturedAction(file, "after"), action("Captured file text diff", "capturedTextDiff", { pointer: file.pointer }, payload.status !== "ready" ? "Refresh to enable source actions." : [file.before, file.after].filter(Boolean).some(side => !side.exact_text_available) ? "Complete captured text is unavailable." : ""));
    if (file.correspondence === "unpaired") detail.append(node("p", "Unpaired captured file. File correspondence is not proven.", "source-note"));
    for (const hunk of file.hunks) {
      const table = node("table", undefined, "source-hunk"), head = node("tr"); head.append(node("th", "Before line"), node("th", "After line"), node("th", "Captured text")); table.append(head);
      let before = hunk.before_start, after = hunk.after_start;
      for (const line of hunk.lines) {
        const tr = node("tr", undefined, line.role), old = line.role === "added" ? "" : String(before++), next = line.role === "removed" ? "" : String(after++);
        tr.append(node("td", old, "gutter"), node("td", next, "gutter")); const text = node("td"); text.append(node("span", line.role === "added" ? "+ " : line.role === "removed" ? "− " : "  ", "line-cue"), node("code", line.text, "token " + line.role)); tr.append(text); table.append(tr);
      }
      const scroll = node("div", undefined, "table-scroll"); scroll.append(table); detail.append(scroll);
    }
    if (file.omitted_hunks) detail.append(node("p", file.omitted_hunks + " source hunks omitted. Use Captured file text diff for complete retained text.")); limits(detail, file.limits); detail.append(buttons); return detail;
  }
  function sourceRows() {
    const all = payload.sourceChanges?.files ?? [], query = choices.search.trim().toLocaleLowerCase(), shown = all.filter(file => choices.changeKinds.includes(file.change) && (!query || [file.before?.file_key, file.after?.file_key, ...file.hunks.flatMap(h => h.lines.map(l => l.text))].join("\n").toLocaleLowerCase().includes(query)));
    controls.counts.textContent = shown.length + " of " + all.length + " captured files shown · " + shown.reduce((sum, f) => sum + f.hunks.length, 0) + " source hunks";
    if (!shown.some(file => file.pointer === choices.sourceSelected)) choices.sourceSelected = shown[0]?.pointer ?? null;
    if (!shown.length) controls.results.append(node("p", all.length ? "No captured files match these filters." : payload.sourceChanges ? "No captured file changes. Review Coverage for the captured source boundary." : "Captured Source detail is unavailable with this engine. Use Root file text diff.", "empty"));
    else if (choices.presentation === "focusedReview") {
      const grid = node("div", undefined, "focused"), list = node("nav", undefined, "change-tree"); list.setAttribute("aria-label", "Captured file changes");
      const heading = node("div", undefined, "group-heading"); heading.append(node("span", "Captured files"), node("span", String(shown.length))); list.append(heading);
      for (const file of shown) {
        const key = file.after?.file_key ?? file.before?.file_key, button = node("button", undefined, "list-row " + file.change + (choices.sourceSelected === file.pointer ? " selected" : "")), label = node("span");
        label.append(node("span", basename(key), "row-title"), node("span", fileContext(key) + " · " + (file.correspondence === "unpaired" ? "Unpaired" : file.hunks.length + " hunks"), "row-subtitle"));
        button.append(node("span", marks[file.change], "change-mark " + file.change), label); button.title = key; button.setAttribute("aria-label", titleCase(file.change) + " captured file " + fileContext(key));
        button.setAttribute("data-source-pointer", file.pointer); button.setAttribute("aria-pressed", String(choices.sourceSelected === file.pointer)); button.addEventListener("click", () => { choices.sourceSelected = file.pointer; update(); focusRow("data-source-pointer", file.pointer); }); list.append(button);
      }
      grid.append(list, sourceDetail(shown.find(f => f.pointer === choices.sourceSelected))); controls.results.append(grid);
    } else for (const file of shown) {
      const detail = node("details", undefined, "row-summary " + file.change), key = file.after?.file_key ?? file.before?.file_key, summary = node("summary"); detail.open = choices.expanded[file.pointer] ?? choices.expansion !== "none";
      summary.append(node("span", marks[file.change], "change-mark " + file.change), node("span", basename(key), "row-title"), node("span", fileContext(key), "row-subtitle")); summary.title = key; summary.setAttribute("aria-label", titleCase(file.change) + " captured file " + fileContext(key)); detail.append(summary, sourceDetail(file)); detail.addEventListener("toggle", () => { choices.expanded[file.pointer] = detail.open; remember(); }); controls.results.append(detail);
    }
    limits(controls.results, payload.sourceChanges?.limits);
  }
  function coverageRows() {
    controls.counts.textContent = "Comparison coverage and source boundary";
    const coverage = payload.coverage;
    if (!coverage) { controls.results.append(node("p", payload.semanticMessage ?? "Coverage detail is unavailable with this engine.")); return; }
    const boundary = coverage.source_boundary === "parsed_models_only" ? "Only parsed models were supplied. Source capture is unavailable." : coverage.source_boundary.startsWith("supplied_") ? "Only supplied roots and executed include text are compared. Other source files were not supplied." : "Captured roots and executed includes are compared.";
    const panel = node("section", undefined, "coverage");
    panel.append(node("h2", "Coverage: " + titleCase(coverage.availability)), node("p", "Source boundary: " + titleCase(coverage.source_boundary)), node("p", boundary + " Unexecuted child files, external MATLAB functions and data file contents are not captured."));
    limits(panel, coverage.limits); limits(panel, payload.semantic?.limits);
    const table = node("table"), head = node("tr"); for (const text of ["Family", "Availability", "Compared fields and limits"]) head.append(node("th", text)); table.append(head);
    for (const family of coverage.families) {
      const row = node("tr"), fields = node("td"); row.append(node("th", titleCase(family.family)), node("td", titleCase(family.availability))); fields.append(node("p", family.fields.map(titleCase).join(", ") || "None")); limits(fields, family.limits); row.append(fields); table.append(row);
    }
    const scroll = node("div", undefined, "table-scroll"); scroll.append(table); panel.append(scroll); controls.results.append(panel);
  }
  function renderRows() {
    if (!payload || !choices) return;
    controls.results.className = "results layout-" + choices.layout; controls.results.replaceChildren();
    if (choices.capture !== payload.capture && payload.status === "ready") { choices.capture = payload.capture ?? ""; choices.selected = null; choices.sourceSelected = null; }
    for (const tab of ["model", "source", "coverage"]) { const control = controls[tab + "Tab"]; if (control) { control.setAttribute("aria-selected", String(choices.tab === tab)); control.tabIndex = choices.tab === tab ? 0 : -1; } }
    controls.scope.disabled = choices.tab !== "model"; controls.sections.disabled = choices.tab !== "model"; controls.search.disabled = choices.tab === "coverage";
    const tools = document.getElementById("filterTools"); if (tools) tools.hidden = choices.tab === "coverage";
    for (const name of ["sectionFilter", "scopeFilter"]) { const filter = document.getElementById(name); if (filter) filter.hidden = choices.tab !== "model"; }
    controls.results.setAttribute("aria-labelledby", choices.tab + "Tab");
    if (choices.tab === "source") sourceRows(); else if (choices.tab === "coverage") coverageRows(); else modelRows();
    if (payload.semanticMessage) controls.results.append(node("p", payload.semanticMessage, "source-note"));
  }
  function focusRow(attribute, identity) { const entries = document.querySelectorAll?.("[" + attribute + "]") ?? []; [...entries].find(element => element.getAttribute(attribute) === identity)?.focus(); }
  controls.refresh.addEventListener("click", () => post("refresh"));
  for (const name of ["changeComparison", "swap", "rootTextDiff", "updateRevision", "choosePath", "details", "help"]) document.getElementById(name)?.addEventListener("click", () => { post(name); const more = document.getElementById("moreActions"); if (more) more.open = false; });
  document.getElementById("moreActions")?.addEventListener("keydown", event => { if (event.key === "Escape") { const more = document.getElementById("moreActions"); more.open = false; more.querySelector("summary")?.focus(); } });
  controls.search.addEventListener("input", () => { choices.search = controls.search.value; update(); });
  for (const [control, property, savedProperty, entries] of [[controls.kinds, "changeKinds", "customChangeKinds", kinds], [controls.sections, "sections", "customSections", Object.keys(names)]]) control.addEventListener("change", () => {
    choices[property] = control.value === "all" ? [...entries] : control.value === "saved" ? [...choices[savedProperty]] : [control.value];
    renderControls(); update();
  });
  for (const name of ["scope", "layout", "expansion"]) controls[name].addEventListener("change", () => { choices[name] = controls[name].value; if (name === "expansion") choices.expanded = {}; update(); });
  controls.presentation?.addEventListener("change", () => switchPresentation(controls.presentation.value));
  const tabs = ["model", "source", "coverage"];
  tabs.forEach((tab, index) => {
    const button = controls[tab + "Tab"]; if (!button) return;
    button.addEventListener("click", () => { choices.tab = tab; update(); });
    button.addEventListener("keydown", event => { const next = event.key === "ArrowRight" ? (index + 1) % 3 : event.key === "ArrowLeft" ? (index + 2) % 3 : event.key === "Home" ? 0 : event.key === "End" ? 2 : -1; if (next >= 0) { event.preventDefault(); choices.tab = tabs[next]; update(); controls[tabs[next] + "Tab"].focus(); } });
  });
  window.addEventListener("message", event => {
    const message = event.data; if (!message || message.type !== "render" || !Array.isArray(message.rows)) return;
    payload = message; references = new Map((message.references ?? []).map(reference => [reference.pointer, reference])); choices = message.choices; choices.presentations ??= {}; choices.tab ??= "model";
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
    for (const [name, count] of [["modelCount", message.rows.length], ["sourceCount", message.sourceChanges?.files.length ?? 0]]) { const badge = document.getElementById(name); if (badge) badge.textContent = String(count); }
    const boundary = document.getElementById("sourceBoundary"); if (boundary) boundary.textContent = !message.coverage ? "Review Coverage for the captured source boundary" : message.coverage.source_boundary === "parsed_models_only" ? "Parsed models only · Source unavailable" : message.coverage.source_boundary.startsWith("supplied_") ? "Supplied root + executed includes" : "Captured root + executed includes";
    const root = document.getElementById("rootTextDiff"); if (root) root.disabled = message.status !== "ready";
    renderControls(); renderRows(); remember();
  });
  api.postMessage({ type: "ready", key: saved?.key, choices: saved?.choices });
})();
