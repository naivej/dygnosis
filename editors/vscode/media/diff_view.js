/* global acquireVsCodeApi, document, window */
(() => {
  "use strict";
  const api = acquireVsCodeApi(), saved = api.getState();
  const names = { symbols: "Symbols", parameters: "Parameters", equations: "Equations", shockSetup: "Shock setup", shockAnalysisSetup: "Shock analysis setup", steadyState: "Steady state", priors: "Priors", commands: "Commands", observables: "Observables", data: "Data", occbin: "OccBin", policy: "Policy", semiStructural: "Semi-structural", moments: "Moments and IRFs", msSbvar: "MS-SBVAR", heterogeneity: "Heterogeneity", externalFunctions: "External functions", trends: "Trends", operations: "Operations", macroContext: "Macro context" };
  const kinds = ["added", "removed", "changed", "unpaired"], marks = { added: "+", removed: "−", changed: "~", unpaired: "?" };
  const changeLabel = kind => kind === "changed" ? "Replaced" : titleCase(kind);
  const controls = Object.fromEntries(["models", "status", "search", "kinds", "counts", "results", "refresh"].map(id => [id, document.getElementById(id)]));
  let payload, choices, references = new Map(), unchangedBlockOpeners = new Set();
  const node = (tag, text, className) => { const element = document.createElement(tag); if (text !== undefined) element.textContent = text; if (className) element.className = className; return element; };
  const titleCase = text => text.replaceAll("_", " ").replace(/^./, letter => letter.toUpperCase());
  const scopeLabel = value => value === "aggregate" ? "Aggregate" : value === "global" ? "Global" : "Dimension: " + value;
  // These are display labels only. Source actions retain the host's exact keys.
  const basename = value => {
    const name = value.split(/[?#]/)[0].replaceAll("\\", "/").split("/").at(-1) || value;
    try { return decodeURIComponent(name); } catch { return name; }
  };
  const rowLabel = row => {
    if (row.label !== "Equation" || !row.semantic) return row.label;
    const indexes = [...new Set([row.semantic.before, row.semantic.after].filter(side => side?.equation_index !== null && side?.equation_index !== undefined).map(side => side.equation_index + 1))];
    return indexes.length ? "Equation " + indexes.join(" → ") : row.label;
  };
  const contextScopes = row => row.scopes.filter(scope => scope !== "aggregate" && scope !== "global" || row.sideScopes.before !== null && row.sideScopes.after !== null && row.sideScopes.before !== row.sideScopes.after);
  const post = (type, args = {}) => api.postMessage({ type, token: payload.token, ...args });
  const remember = () => { api.setState({ key: payload.key, choices }); api.postMessage({ type: "choices", key: payload.key, choices }); };
  function update() { renderRows(); remember(); }
  function renderControls() {
    controls.search.value = choices.search;
    const focused = document.activeElement?.getAttribute("data-change-type"), selected = choices.changeKinds;
    const summary = node("summary", "Type of changes"), options = node("div", undefined, "change-type-options");
    options.setAttribute("role", "group"); options.setAttribute("aria-label", "Type of changes");
    for (const kind of ["all", ...kinds]) {
      const label = node("label"), checkbox = node("input"); checkbox.type = "checkbox";
      checkbox.id = "change-type-" + kind; checkbox.setAttribute("data-change-type", kind);
      checkbox.checked = kind === "all" ? kinds.every(value => selected.includes(value)) : selected.includes(kind);
      checkbox.indeterminate = kind === "all" && selected.length > 0 && selected.length < kinds.length;
      checkbox.addEventListener("change", () => {
        choices.changeKinds = kind === "all" ? checkbox.checked ? [...kinds] : [] : kinds.filter(value => value === kind ? checkbox.checked : choices.changeKinds.includes(value));
        choices.customChangeKinds = [...choices.changeKinds]; renderControls(); update();
        document.getElementById(checkbox.id)?.focus();
      });
      label.append(checkbox, node("span", kind === "all" ? "All" : changeLabel(kind))); options.append(label);
    }
    summary.title = selected.map(changeLabel).join(", ") || "No types selected";
    controls.kinds.replaceChildren(summary, options);
    if (focused) document.getElementById("change-type-" + focused)?.focus();
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
    const target = row.navigation[side], files = row.sourceFiles?.[side] ?? [];
    const locations = target?.written_locations ?? [];
    const reason = payload.status !== "ready" ? "Refresh the comparison to enable source actions." : !locations.length && !files.length ? "No verified source is available on this side." : "";
    const button = action("Open " + titleCase(side) + " source", "openSource", { rowId: row.id, side }, reason);
    button.textContent = locations.length === 1 ? basename(locations[0].uri) + ":" + (locations[0].range.start.line + 1) : files.length === 1 ? basename(files[0]) : "Open source" + (locations.length > 1 || files.length > 1 ? "…" : "");
    button.className = "source-link";
    if (!reason) button.title = [...locations.map(location => location.uri), ...files].join("\n");
    return button;
  }
  function plainShockOpeners(expression) {
    // This changes color only. It does not parse facts or alter retained text.
    const ranges = [...expression.text.matchAll(/^[ \t]*(shocks|mshocks|heteroskedastic_shocks|shock_paths|shock_groups|perfect_foresight_controlled_paths|initval|endval|end)\b[ \t]*;?/gim)].map(match => [match.index, match.index + match[0].length]);
    for (const match of expression.text.matchAll(/^[ \t]*shock_groups\b[^\r\n]*;/gim)) {
      if (unchangedBlockOpeners.has(match[0].trim())) ranges.push([match.index, match.index + match[0].length]);
    }
    let offset = 0;
    return { runs: expression.runs.flatMap(run => {
      const start = offset, end = offset += run.text.length;
      const overlaps = ranges.filter(([a, b]) => a < end && b > start);
      const edges = [...new Set([start, end, ...overlaps.flatMap(([a, b]) => [Math.max(start, a), Math.min(end, b)])])].sort((a, b) => a - b);
      return edges.slice(0, -1).map((a, index) => ({ text: run.text.slice(a - start, edges[index + 1] - start), role: overlaps.some(([from, to]) => from <= a && a < to) ? "unchanged" : run.role }));
    }) };
  }
  function codeLines(parent, expression, side) {
    if (!expression) return;
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
      const line = node("div", undefined, "code-line"), code = node("code");
      code.append(...spans); line.append(code); block.append(line);
    }
    parent.append(block);
  }
  const textField = (detail, name, side) => {
    const field = detail.fields.find(field => field.name === name);
    return field?.comparison_availability === "complete" && field[side].value?.kind === "text" ? field[side].value.value : null;
  };
  const localRole = detail => ["before", "after"].map(side => textField(detail, "role", side)).find(role => role === "model_local_definition" || role === "model_local_declaration");
  function declaration(detail, side) {
    const written = textField(detail, "written_kind", side);
    return detail[side] && ["var", "varexo", "varexo_det", "parameters"].includes(written) ? written : localRole(detail) === "model_local_declaration" && detail[side] ? "model_local_variable" : null;
  }
  function visibleFields(detail) {
    if (["commands", "operations", "ms_sbvar"].includes(detail.family)) {
      return detail.fields.filter(field => ["order", "execution_order"].includes(field.name) && (field.changed || field.comparison_availability !== "complete"));
    }
    const assignment = detail.expressions.find(expression => expression.field === "expression");
    const evaluatedValueNeeded = ["before", "after"].some(side => {
      const text = assignment?.[side]?.text ?? textField(detail, "expression", side);
      return text !== null && text.trim() !== "" && !/^(?:[+-]\s*)?(?:\d+(?:\.\d*)?|\.\d+)(?:[eEdD][+-]?\d+)?$/.test(text.trim());
    });
    const statement = detail.family === "shocks" && detail.expressions.find(expression => expression.field === "statement_text");
    const statementComplete = statement && ["before", "after"].every(side => !detail[side] || statement[side]);
    return detail.fields.filter(field => {
      if (field.name === "evaluated_value") return evaluatedValueNeeded;
      if (statementComplete && statement.before?.text !== statement.after?.text && field.comparison_availability === "complete"
        && (field.name !== "status" || [field.before, field.after].every(side => side.state === "absent" || side.value?.kind === "text" && ["active", "written"].includes(side.value.value)))) return false;
      if (field.comparison_availability === "complete") {
        if (detail.family === "commands" && field.name === "role" && [field.before, field.after].every(side => side.state === "absent" || side.value?.kind === "text" && ["command", "block_context"].includes(side.value.value))) return false;
        if (detail.family === "symbols" && ["added", "removed"].includes(detail.change) && [field.before, field.after].every(side => ["absent", "empty"].includes(side.state) || ["log_transform", "predetermined"].includes(field.name) && side.value?.kind === "boolean" && !side.value.value)) return false;
        if (localRole(detail) && (field.name === "role" || field.name === "target" && ["before", "after"].every(side => !detail[side] || textField(detail, "target", side) === detail[side].name))) return false;
        if (field.name === "written_kind" && ["before", "after"].every(side => !detail[side] || declaration(detail, side))) return false;
        if (field.name === "kind" && ["before", "after"].every(side => !detail[side] || textField(detail, "kind", side) === declaration(detail, side))) return false;
        if (field.name === "written_dimension" && ["before", "after"].every(side => JSON.stringify(field[side]) === JSON.stringify(detail.fields.find(field => field.name === "dimension")?.[side]))) return false;
      }
      if (!field.changed && field.comparison_availability === "complete") {
        // Nonempty equation tags distinguish named regime variants.
        return field.name === "tags" && [field.before, field.after].some(side => side.value?.kind === "record" ? Object.keys(side.value.value).length : side.value?.kind === "list" ? side.value.value.length : fieldText(side) !== "");
      }
      const expression = detail.expressions.find(expression => expression.field === field.name);
      // Omit only exact duplicate text. Unknown or independently retained values remain visible.
      return !expression || field.comparison_availability !== "complete" || !["before", "after"].every(side => expression[side] === null ? field[side].state === "absent" : ["present", "empty"].includes(field[side].state) && field[side].value?.kind === "text" && field[side].value.value === expression[side].text);
    });
  }
  const visibleExpressions = detail => detail.expressions.filter(expression => ["commands", "operations", "ms_sbvar"].includes(detail.family) ? expression.field === "statement_text" : ["expression", "statement_text"].includes(expression.field) || expression.before?.text !== expression.after?.text || expression.availability !== "complete");
  function sidePanel(row, side) {
    const panel = node("div", undefined, "side " + side), heading = node("div", undefined, "side-title"), own = row.semantic?.[side];
    const scope = row.sideScopes[side], scopeShown = scope !== null && contextScopes(row).includes(scope);
    heading.append(node("strong", titleCase(side)));
    if (scopeShown) heading.append(node("span", scopeLabel(scope), "scope-label"));
    heading.append(sourceAction(row, side));
    panel.append(heading);
    if (row[side] === null) panel.append(node("p", "Not present", "source-note"));
    else if (!row.semantic) panel.append(node("pre", row[side]));
    else if (own) {
      const keyword = declaration(row.semantic, side), role = localRole(row.semantic);
      if (keyword) {
        codeLines(panel, { runs: [{ text: keyword + " ", role: "unchanged" }, { text: own.name, role: row.kind === "added" ? "added" : row.kind === "removed" ? "removed" : "unchanged" }, { text: ";", role: "unchanged" }] }, side);
      }
      const expressions = visibleExpressions(row.semantic);
      if (["commands", "operations", "ms_sbvar"].includes(row.semantic.family) && !expressions.some(expression => expression[side])) panel.append(node("p", "Written source text is unavailable for this instruction. Use Text diff to inspect the captured files.", "source-note"));
      for (const expression of expressions) {
        if (!["expression", "statement_text", "statement_tokens"].includes(expression.field) || expressions.length > 1) panel.append(node("h3", row.semantic.fields.find(field => field.name === expression.field)?.label ?? titleCase(expression.field), "field-label"));
        const value = expression[side] && row.semantic.family === "shocks" && expression.field === "statement_text" ? plainShockOpeners(expression[side]) : expression[side];
        const definitionRole = ["added", "removed"].includes(row.kind) ? row.kind : "unchanged";
        codeLines(panel, value && role === "model_local_definition" && expression.field === "expression" ? { runs: [{ text: "# " + own.name + " = ", role: definitionRole }, ...value.runs, { text: ";", role: definitionRole }] } : value, side);
      }
      for (const field of visibleFields(row.semantic)) {
        if (field.name === "dimension" && field.comparison_availability === "complete" && scopeShown && field[side].value?.kind === "text" && field[side].value.value === scope) continue;
        const value = node("div", undefined, "field"), available = field.comparison_availability === "complete";
        value.setAttribute("data-field", field.name);
        value.append(node("h3", field.label, "field-label"));
        const text = node("div", undefined, "field-content");
        text.append(fieldValue(field[side], field.changed && available ? side === "before" ? "token removed" : "token added" : ""));
        value.append(text); if (!available) value.append(node("p", "Comparison unavailable", "source-note")); panel.append(value);
      }
    }
    if (row.semantic) {
      const ownRefs = row.semantic.references.map(pointer => references.get(pointer)).filter(ref => ref?.side === side);
      if (ownRefs.length) {
        const conventionShown = row.semantic.family === "symbols" && visibleFields(row.semantic).some(field => field.name === "predetermined" && field.comparison_availability === "complete" && field[side].value?.kind === "boolean" && field[side].value.value);
        const refs = node("section", undefined, "refs"); refs.append(node("h3", "Direct equation references"));
        for (const ref of ownRefs) {
          const line = node("p", undefined, "reference"), target = ref.navigation[side];
          const button = action(ref.label, "openReference", { pointer: ref.pointer, side }, payload.status !== "ready" ? "Refresh to enable source actions." : !target?.written_locations.length ? "No verified written location is available." : "");
          button.setAttribute("aria-label", "Open " + titleCase(side) + " reference: " + ref.label + (button.disabled ? ". " + button.title : ""));
          const context = [ref.scope.dimension && (!scopeShown || ref.scope.dimension !== scope) ? scopeLabel(ref.scope.dimension) : "", ref.timing.written_offset ? "written " + ref.timing.written_offset : "", !conventionShown && ref.timing.converted_offset !== ref.timing.written_offset ? "after convention " + ref.timing.converted_offset : ""].filter(Boolean).join(" · ");
          line.append(button); if (context) line.append(node("span", context, "source-note")); refs.append(line);
        }
        panel.append(refs);
      }
    }
    return panel;
  }
  function valueText(value) {
    if (!value) return "Unknown";
    switch (value.kind) {
      case "text": return value.value;
      case "number": case "integer": case "boolean": return String(value.value);
      case "list": return "[" + value.value.map(valueText).join(", ") + "]";
      case "record": return Object.entries(value.value).map(([name, value]) => titleCase(name) + ": " + valueText(value)).join("\n");
      default: return "Unknown";
    }
  }
  const fieldText = field => field.state === "absent" || field.state === "empty" ? "" : field.state === "unknown" ? "Unknown" : valueText(field.value);
  function fieldValue(field, className) {
    const text = fieldText(field), value = field.value;
    if (text.length <= 240 || !value || value.kind !== "list" && value.kind !== "record") return node("span", text, className);
    return node("pre", text, "field-value " + className);
  }
  function rowDetail(row) {
    const article = node("article", undefined, "detail change " + row.kind), header = node("div", undefined, "detail-heading");
    article.setAttribute("data-row-id", row.id);
    article.setAttribute("aria-label", changeLabel(row.kind) + " " + rowLabel(row) + ", " + row.scopes.map(scopeLabel).join(" · "));
    header.append(node("span", marks[row.kind], "change-mark " + row.kind), node("h2", rowLabel(row))); article.append(header);
    const sides = node("div", undefined, "sides"); sides.append(sidePanel(row, "before"), sidePanel(row, "after")); article.append(sides);
    const instruction = row.semantic?.expressions.find(expression => expression.field === "statement_text");
    if (row.semantic?.family === "operations" && instruction?.before && instruction.after && instruction.before.text === instruction.after.text && row.semantic.fields.some(field => field.changed && !["role", "order"].includes(field.name))) article.append(node("p", "The written instruction is unchanged; its recorded effects differ.", "source-note"));
    return article;
  }
  function groupNotes(rows) {
    const entries = rows.flatMap(row => row.semantic ? [...row.semantic.limits, ...row.semantic.expressions.filter(expression => expression.availability !== "complete").map(expression => ({ reason: expression.reason ?? "Highlights: " + titleCase(expression.availability), omitted: null }))] : payload.semantic ? [{ reason: "Typed detail is unavailable for some rows. Structural detail remains.", omitted: null }] : []);
    const unique = new Map(entries.map(entry => [JSON.stringify([entry.reason, entry.omitted]), entry]));
    if (!unique.size) return null;
    const notes = node("aside", undefined, "group-notes"); notes.setAttribute("aria-label", "Comparison limits"); limits(notes, [...unique.values()]); return notes;
  }
  function searchText(row) { return [row.label, row.group, row.before ?? "", row.after ?? "", ...(row.semantic?.facets ?? []).map(titleCase), ...(row.semantic?.expressions ?? []).flatMap(expression => [expression.before?.text ?? "", expression.after?.text ?? ""]), ...(row.semantic?.fields ?? []).flatMap(field => [field.label, fieldText(field.before), fieldText(field.after)])].join("\n").toLocaleLowerCase(); }
  function modelRows() {
    const groupKey = row => row.section + ":" + row.group;
    const available = payload.rows, query = choices.search.trim().toLocaleLowerCase();
    const matches = row => choices.changeKinds.includes(row.kind) && (!query || searchText(row).includes(query));
    const shown = available.filter(matches);
    const breakdown = kinds.map(kind => shown.filter(row => row.kind === kind).length + " " + changeLabel(kind).toLowerCase()).join(" · ");
    controls.counts.textContent = shown.length + " of " + available.length + " model rows match filters · " + breakdown;
    const groups = new Map();
    for (const row of [...payload.rows].sort((a, b) => Object.keys(names).indexOf(a.section) - Object.keys(names).indexOf(b.section))) {
      const key = groupKey(row);
      if (!groups.has(key)) groups.set(key, { label: row.group, rows: [] });
      if (matches(row)) groups.get(key).rows.push(row);
    }
    if (groups.size && !groups.get(choices.group)?.rows.length) choices.group = [...groups].find(([, group]) => group.rows.length)?.[0] ?? null;
    const grid = node("div", undefined, "grouped"), list = node("nav", undefined, "group-list"); list.setAttribute("aria-label", "Change groups");
    for (const [key, group] of groups) {
      const button = node("button", undefined, "group-button" + (key === choices.group ? " selected" : "")); button.type = "button";
      button.disabled = !group.rows.length;
      button.append(node("span", group.label), node("span", String(group.rows.length), "group-count"));
      button.setAttribute("aria-label", group.label + ", " + group.rows.length + " changes"); button.setAttribute("aria-pressed", String(key === choices.group)); button.setAttribute("data-group", key);
      button.addEventListener("click", () => { choices.group = key; update(); [...document.querySelectorAll("[data-group]")].find(element => element.getAttribute("data-group") === key)?.focus(); });
      list.append(button);
    }
    const selected = groups.get(choices.group), rows = node("section", undefined, "group-rows"); rows.setAttribute("aria-label", selected?.label ?? "Model changes");
    if (selected) {
      for (const row of selected.rows) rows.append(rowDetail(row));
      const notes = groupNotes(selected.rows); if (notes) rows.append(notes);
    } else {
      const text = ["failure", "incomplete", "loading"].includes(payload.status) ? "" : payload.rows.length ? "No rows match these filters." : payload.sourceChanges?.files.length ? "No model changes. Captured text differs; use Text diff." : payload.coverage?.availability !== "complete" && payload.semantic ? "No model rows. Some changes could not be compared." : payload.status === "ready" ? "No structural changes. Written text can still differ." : "Refresh to compare current inputs.";
      if (text) rows.append(node("p", text, "empty"));
    }
    grid.append(list, rows); controls.results.append(grid);
  }
  function renderRows() {
    if (!payload || !choices) return;
    controls.results.className = "results"; controls.results.replaceChildren();
    modelRows();
    if (payload.semanticMessage) controls.results.append(node("p", payload.semanticMessage, "empty"));
  }
  controls.refresh.addEventListener("click", () => post("refresh"));
  for (const name of ["changeComparison", "swap", "rootTextDiff", "capturedTextDiff", "choosePath", "help"]) document.getElementById(name)?.addEventListener("click", () => post(name));
  controls.search.addEventListener("input", () => { choices.search = controls.search.value; update(); });
  controls.kinds.addEventListener("keydown", event => { if (event.key === "Escape") { controls.kinds.open = false; controls.kinds.children[0]?.focus(); } });
  document.addEventListener?.("pointerdown", event => { if (!controls.kinds.contains(event.target)) controls.kinds.open = false; });
  window.addEventListener("message", event => {
    const message = event.data; if (!message || message.type !== "render" || !Array.isArray(message.rows)) return;
    payload = message; references = new Map((message.references ?? []).map(reference => [reference.pointer, reference])); choices = message.choices;
    const openers = side => new Set(message.rows.filter(row => row.semantic?.family === "shocks").flatMap(row => row.semantic.expressions.filter(expression => expression.field === "statement_text").map(expression => expression[side]?.text.split(/\r?\n/)[0].trim()).filter(Boolean)));
    const beforeOpeners = openers("before"), afterOpeners = openers("after");
    unchangedBlockOpeners = new Set([...beforeOpeners].filter(opener => afterOpeners.has(opener)));
    controls.models.replaceChildren();
    for (const [label, input] of [["Before", message.before], ["After", message.after]]) {
      const model = node("div", undefined, "input-side"), parts = input.split(" · "); model.title = input;
      model.append(node("small", label), node("code", basename(parts[0])), node("span", parts.slice(1).join(" · "), "input-context")); controls.models.append(model);
    }
    controls.status.textContent = message.message; controls.status.className = message.status; controls.refresh.disabled = message.status === "loading";
    controls.status.hidden = message.status === "ready" && message.message === "Current comparison";
    const path = document.getElementById("choosePath"); if (path) path.hidden = message.status !== "failure" || !/model|root|path|file|source/i.test(message.message);
    const boundary = document.getElementById("sourceBoundary"); if (boundary) boundary.textContent = !message.coverage ? "Captured source boundary unavailable" : message.coverage.source_boundary === "parsed_models_only" ? "Parsed models only · Source unavailable" : message.coverage.source_boundary.startsWith("supplied_") ? "Supplied root + executed includes" : "Captured root + executed includes";
    const hasFiles = !!message.sourceChanges?.files.length;
    const root = document.getElementById("rootTextDiff"); if (root) { root.disabled = message.status !== "ready"; root.hidden = hasFiles; }
    const captured = document.getElementById("capturedTextDiff"); if (captured) { captured.disabled = message.status !== "ready" || !hasFiles; captured.hidden = !hasFiles; }
    renderControls(); renderRows(); remember();
  });
  api.postMessage({ type: "ready", key: saved?.key, choices: saved?.choices });
})();
