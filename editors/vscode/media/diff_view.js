/* global acquireVsCodeApi, document, window */
(() => {
  "use strict";
  const api = acquireVsCodeApi(), saved = api.getState();
  const names = { symbols: "Symbols", parameters: "Parameters", equations: "Equations", shockSetup: "Shock setup", shockAnalysisSetup: "Shock analysis setup", steadyState: "State", forecast: "Forecast", priors: "Priors", commands: "Commands", data: "Data", occbin: "OccBin", policy: "Policy", semiStructural: "Semi-structural", moments: "Method of moments", msSbvar: "MS-SBVAR", heterogeneity: "Heterogeneity", externalFunctions: "External functions", trends: "Trends", operations: "Operations", macroContext: "Macro context" };
  const kinds = ["added", "removed", "changed"], marks = { added: "+", removed: "−", changed: "~" };
  const changeLabel = kind => kind === "changed" ? "Replaced" : titleCase(kind);
  const controls = Object.fromEntries(["models", "status", "search", "kinds", "counts", "results", "refresh"].map(id => [id, document.getElementById(id)]));
  let payload, choices, groupList, references = new Map(), groups = new Map();
  let presentation = saved?.presentation ?? {};
  const scrollArea = document.getElementById("reviewContent");
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
  const persist = () => api.setState({ key: payload.key, choices, presentation });
  const remember = () => { persist(); api.postMessage({ type: "choices", key: payload.key, choices }); };
  function update(focusId) { renderRows(focusId); remember(); }
  function rememberSource(button, rowId) {
    presentation.returnFocus = { id: button.id, rowId, scrollTop: scrollArea?.scrollTop ?? 0 };
    persist();
  }
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
  function action(label, type, args, reason, rowId) {
    const button = node("button", label); button.type = "button"; button.disabled = !!reason;
    button.setAttribute("aria-label", reason ? label + " unavailable: " + reason : label); if (reason) button.title = reason;
    button.addEventListener("click", () => {
      if (button.disabled) return;
      rememberSource(button, rowId);
      post(type, args);
    });
    return button;
  }
  function sourceAction(row, side) {
    const target = row.navigation[side] ?? row.navigation.block_openers?.[side], files = row.sourceFiles?.[side] ?? [];
    const locations = target?.written_locations ?? [];
    const reason = payload.status !== "ready" ? "Refresh to open source links." : !locations.length && !files.length ? "The source for this side is unavailable." : "";
    const button = action("Open " + titleCase(side) + " source", "openSource", { rowId: row.id, side }, reason, row.id);
    button.id = "source-" + side + "-" + row.id;
    button.textContent = locations.length === 1 ? basename(locations[0].uri) + ":" + (locations[0].range.start.line + 1) : files.length === 1 ? basename(files[0]) : "Open source" + (locations.length > 1 || files.length > 1 ? "…" : "");
    button.className = "source-link";
    if (!reason) button.title = [...locations.map(location => location.uri), ...files].join("\n");
    return button;
  }
  function* codeRanges(text) {
    const lineEnd = start => { const end = text.indexOf("\n", start); return end < 0 ? text.length : end; };
    let position = 0, start = 0;
    while (position < text.length) {
      let commentStart = position, end;
      if (text.startsWith("/*", position)) {
        const closer = text.indexOf("*/", position + 2); end = closer < 0 ? text.length : closer + 2;
      } else if (text.startsWith("//", position) || text[position] === "%") end = lineEnd(position);
      else if (text.startsWith("...", position)) { commentStart += 3; end = lineEnd(position); }
      if (end !== undefined) {
        if (start < commentStart) yield [start, commentStart];
        position = start = end;
      } else if (text[position] === '"' || text[position] === "$" || text[position] === "'" && !/[\w\u0080-\uffff)\]}'".]/.test(text[position - 1] ?? "")) {
        if (start < position) yield [start, position];
        const literalStart = position;
        const quote = text[position++];
        while (position < text.length) {
          if (text[position++] !== quote) continue;
          if (quote !== "$" && text[position] === quote) ++position;
          else break;
        }
        yield [literalStart, position, true];
        start = position;
      } else ++position;
    }
    if (start < text.length) yield [start, text.length];
  }
  function displayedCode(runs) {
    const text = runs.map(run => run.text).join(""), ranges = codeRanges(text), output = [];
    let range = ranges.next().value, position = 0, lastEnd = 0, kept = 0;
    for (const run of runs) {
      const end = position + run.text.length;
      while (range && range[0] < end) {
        const from = Math.max(position, range[0]), to = Math.min(end, range[1]);
        if (from < to) {
          const part = text.slice(from, to), previous = output.at(-1)?.text.at(-1);
          if (lastEnd < from && previous && !/\s/.test(previous) && !/\s/.test(part[0])) output.push({ text: " ", role: "unchanged" });
          output.push({ text: part, role: run.role, literal: !!range[2] }); kept += to - from; lastEnd = to;
        }
        if (range[1] > end) break;
        range = ranges.next().value;
      }
      position = end;
    }
    return { runs: output, compact: kept < text.length };
  }
  function codeLines(parent, expression, side) {
    if (!expression) return;
    const display = displayedCode(expression.runs);
    let lines = [[]];
    // The host validates runs. Text insertion never creates HTML or actions.
    for (const run of display.runs) run.text.split("\n").forEach((text, index) => {
      if (index) lines.push([]);
      lines.at(-1).push({ text, role: run.role, literal: run.literal });
    });
    if (display.compact) {
      lines = lines.filter(parts => parts.some(part => part.literal || part.text.trim()));
      for (const parts of lines) for (let index = parts.length - 1; index >= 0; --index) {
        if (parts[index].literal) break;
        parts[index].text = parts[index].text.trimEnd(); if (parts[index].text) break;
      }
    }
    const block = node("div", undefined, "expression");
    lines.forEach((parts, index) => {
      const line = node("div", undefined, "code-line"), code = node("code");
      parts.forEach((part, partIndex) => {
        const text = part.text + (index < lines.length - 1 && partIndex === parts.length - 1 ? "\n" : "");
        const span = node("span", text, "token " + part.role);
        if (part.role !== "unchanged") span.setAttribute("aria-label", titleCase(side) + ": " + part.role + " " + text);
        code.append(span);
      });
      line.append(code); block.append(line);
    });
    parent.append(block);
  }
  const textField = (detail, name, side) => {
    const field = detail.fields.find(field => field.name === name);
    return field?.comparison_availability === "complete" && field[side].value?.kind === "text" ? field[side].value.value : null;
  };
  const localRole = detail => ["before", "after"].map(side => textField(detail, "role", side)).find(role => role === "model_local_definition" || role === "model_local_declaration");
  const observableKeyword = (detail, side) => {
    const role = textField(detail, "role", side);
    return detail.family === "observables" && detail[side] && ["varobs", "varexobs"].includes(role) ? role : null;
  };
  const observableDeclaration = detail => detail.family === "observables" && ["before", "after"].every(side => !detail[side] || observableKeyword(detail, side));
  function declaration(detail, side) {
    if (observableKeyword(detail, side)) return observableKeyword(detail, side);
    const written = textField(detail, "written_kind", side);
    return detail[side] && ["var", "varexo", "varexo_det", "parameters"].includes(written) ? written : localRole(detail) === "model_local_declaration" && detail[side] ? "model_local_variable" : null;
  }
  const writtenStatement = detail => detail.expressions.find(expression => expression.field === "statement_text" && ["before", "after"].every(side => !detail[side] || expression[side]));
  function declarationKeywordRole(detail, side, keyword) {
    if (!observableKeyword(detail, side)) return "unchanged";
    const text = writtenStatement(detail)?.[side];
    if (!text) return "unchanged";
    const start = text.text.length - text.text.trimStart().length;
    if (text.text.slice(start, start + keyword.length).toLowerCase() !== keyword) return "unchanged";
    const roles = new Set(sliceRuns(text, start, start + keyword.length).map(run => run.role));
    return roles.size === 1 ? [...roles][0] : "unchanged";
  }
  function visibleFields(detail) {
    const assignment = detail.expressions.find(expression => expression.field === "expression");
    const evaluatedValueNeeded = ["before", "after"].some(side => {
      const text = assignment?.[side]?.text ?? textField(detail, "expression", side);
      return text !== null && text.trim() !== "" && !/^(?:[+-]\s*)?(?:\d+(?:\.\d*)?|\.\d+)(?:[eEdD][+-]?\d+)?$/.test(text.trim());
    });
    const statement = writtenStatement(detail);
    const expressions = visibleExpressions(detail);
    return detail.fields.filter(field => {
      if (field.name === "dependent_on") return false;
      if (observableDeclaration(detail) && field.comparison_availability === "complete") {
        if (field.name === "role" || field.name === "target" && ["before", "after"].every(side => !detail[side] || textField(detail, "target", side) === detail[side].name)) return false;
        if (field.name === "captured_kind" && ["before", "after"].every(side => !detail[side] || textField(detail, "captured_kind", side) === (observableKeyword(detail, side) === "varobs" ? "var" : "varexo"))) return false;
      }
      if (field.name === "evaluated_value") return evaluatedValueNeeded;
      if (statement && statement.before?.text !== statement.after?.text && field.comparison_availability === "complete" && !["order", "execution_order"].includes(field.name)
        && (field.name !== "status" || [field.before, field.after].every(side => side.state === "absent" || side.value?.kind === "text" && ["active", "written"].includes(side.value.value)))) return false;
      if (field.comparison_availability === "complete") {
        if (field.name === "role" && ["commands", "operations", "ms_sbvar"].includes(detail.family)) return false;
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
      const expression = expressions.find(expression => expression.field === field.name);
      // Omit only exact duplicate text. Unknown or independently retained values remain visible.
      return !expression || field.comparison_availability !== "complete" || !["before", "after"].every(side => expression[side] === null ? field[side].state === "absent" : ["present", "empty"].includes(field[side].state) && field[side].value?.kind === "text" && field[side].value.value === expression[side].text);
    });
  }
  function visibleExpressions(detail) {
    if (observableDeclaration(detail)) return [];
    const written = writtenStatement(detail);
    // Captured text can stay identical while macro expansion or retained
    // operation results change. Only hide detail that the written edit explains.
    if (written && written.before?.text !== written.after?.text) return [written];
    const values = detail.expressions.filter(expression => expression !== written &&
      (expression.before?.text !== expression.after?.text || expression.availability !== "complete" || !written && expression.field === "expression"));
    return written ? [written, ...values] : values;
  }
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
        codeLines(panel, { runs: [{ text: keyword, role: declarationKeywordRole(row.semantic, side, keyword) }, { text: " ", role: "unchanged" }, { text: own.name, role: row.kind === "added" ? "added" : row.kind === "removed" ? "removed" : "unchanged" }, { text: ";", role: "unchanged" }] }, side);
      }
      const expressions = visibleExpressions(row.semantic);
      if (!keyword && !["symbols", "parameters", "equations", "shocks", "macro_context"].includes(row.semantic.family) && !writtenStatement(row.semantic)?.[side]) panel.append(node("p", "The file text for this instruction is unavailable. Use Text diff to review the files.", "source-note"));
      for (const expression of expressions) {
        if (!["expression", "statement_text"].includes(expression.field) || expressions.length > 1) panel.append(node("h3", expression.field === "statement_text" ? "Written instruction" : expression.field === "statement_tokens" ? "Expanded instruction" : row.semantic.fields.find(field => field.name === expression.field)?.label ?? titleCase(expression.field), "field-label"));
        const value = expression[side];
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
        value.append(text); if (!available) value.append(node("p", "This value could not be compared.", "source-note")); panel.append(value);
      }
    }
    const refs = referenceDisclosure(row, side, scopeShown ? scope : null);
    if (refs) panel.append(refs);
    return panel;
  }
  function referenceDisclosure(row, side, shownScope) {
    const equations = new Map();
    for (const pointer of row.semantic?.references ?? []) {
      const ref = references.get(pointer);
      if (ref?.side !== side) continue;
      // Each written use is retained; this list opens each equation once.
      const key = JSON.stringify([ref.scope.domain, ref.scope.dimension, ref.equation_index]);
      if (!equations.has(key)) equations.set(key, ref);
    }
    if (!equations.size) return null;

    const section = node("section", undefined, "refs");
    const toggle = node("button", equations.size + (equations.size === 1 ? " equation reference" : " equation references"), "reference-toggle");
    toggle.type = "button";
    toggle.id = "reference-toggle-" + side + "-" + row.id;
    toggle.setAttribute("aria-label", titleCase(side) + ": " + toggle.textContent);
    const list = node("div");
    list.id = "references-" + side + "-" + row.id;
    toggle.setAttribute("aria-controls", list.id);
    const render = () => {
      const open = presentation.referenceSides.includes(side);
      toggle.setAttribute("aria-expanded", String(open));
      list.hidden = !open;
      list.replaceChildren();
      if (!open) return;
      for (const ref of equations.values()) {
        const line = node("p", undefined, "reference"), target = ref.navigation[side];
        const reason = payload.status !== "ready" ? "Refresh to open this equation." : !target?.written_locations.length ? "The source line for this equation is unavailable." : "";
        const button = action("Open " + titleCase(side) + " reference: " + ref.label, "openReference", { pointer: ref.pointer, side }, reason, row.id);
        button.id = "reference-" + ref.pointer;
        button.textContent = ref.label;
        line.append(button);
        if (ref.scope.dimension && ref.scope.dimension !== shownScope) line.append(node("span", scopeLabel(ref.scope.dimension), "source-note"));
        list.append(line);
      }
    };
    toggle.addEventListener("click", () => {
      const sides = presentation.referenceSides;
      presentation.referenceSides = sides.includes(side) ? sides.filter(value => value !== side) : [...sides, side];
      render();
      persist();
    });
    section.append(toggle, list);
    render();
    return section;
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
    const detail = node("div", undefined, "detail");
    const sides = node("div", undefined, "sides");
    sides.append(sidePanel(row, "before"), sidePanel(row, "after"));
    detail.append(sides);
    const instruction = row.semantic?.expressions.find(expression => expression.field === "statement_text");
    if (row.semantic?.family === "operations" && instruction?.before && instruction.after && instruction.before.text === instruction.after.text && row.semantic.fields.some(field => field.changed && !["role", "order"].includes(field.name))) detail.append(node("p", "The same instruction changes the model in different ways.", "source-note"));
    const notes = rowNotes(row);
    if (notes) detail.append(notes);
    return detail;
  }
  function sliceRuns(expression, start, end) {
    let offset = 0;
    return expression.runs.flatMap(run => {
      const from = offset, to = offset += run.text.length;
      return from < end && to > start ? [{ text: run.text.slice(Math.max(start, from) - from, Math.min(end, to) - from), role: run.role }] : [];
    });
  }
  function historyCards(rows) {
    // Group display context only after filtering. Each period keeps its semantic
    // owner/count; the source action opens the first visible instruction.
    const cards = [], groups = new Map();
    for (const row of rows) {
      const detail = row.semantic, statement = detail && writtenStatement(detail);
      const target = detail && (textField(detail, "target", "after") ?? textField(detail, "target", "before"));
      const history = detail && ["before", "after"].every(side => !detail[side] || ["histval", "filter_initial_state"].includes(detail[side].context?.name));
      const parts = statement && Object.fromEntries(["before", "after"].map(side => {
        const text = statement[side]?.text, match = text?.match(/^([^\n]*;\r?\n)([\s\S]*)(\r?\nend;)$/);
        return [side, match && { start: match[1].length, end: text.length - match[3].length, header: match[1], footer: match[3] }];
      }));
      if (!history || !target || detail.limits.some(limit => limit.code === "occurrence_correspondence_unpaired") || !parts || ["before", "after"].some(side => detail[side] && !parts[side]) || detail.fields.some(field => field.changed && field.comparison_availability === "complete" && !["expression", "role", "target", "lag"].includes(field.name))) { cards.push(row); continue; }
      const key = JSON.stringify([target, row.kind, ...["before", "after"].map(side => [detail[side]?.context, row.navigation[side]?.occurrence_id, row.navigation[side]?.written_locations.map(location => location.uri), parts[side]?.header, parts[side]?.footer])]);
      const existing = groups.get(key);
      if (!existing) {
        const card = { ...row };
        groups.set(key, { card, parts, statement });
        cards.push(card);
        continue;
      }
      existing.card.label = target;
      if (existing.card.id !== presentation.detailId) continue;
      if (!existing.combined) {
        existing.combined = { ...existing.statement };
        existing.card.semantic = { ...existing.card.semantic, expressions: [existing.combined] };
      }
      const combined = existing.combined;
      for (const side of ["before", "after"]) {
        if (!combined[side]) continue;
        const own = combined[side], footer = existing.parts[side].footer;
        const runs = [...sliceRuns(own, 0, own.text.length - footer.length), { text: "\n", role: "unchanged" }, ...sliceRuns(statement[side], parts[side].start, parts[side].end), ...sliceRuns(own, own.text.length - footer.length, own.text.length)];
        combined[side] = { text: runs.map(run => run.text).join(""), runs };
      }
    }
    return cards;
  }
  // Engine reasons describe the data contract. The view explains only what a
  // reader needs to know about the displayed changes, with no raw reason fallback.
  const comparisonNotes = {
    identifier_correspondence_unpaired: null, timing_correspondence_limit: null,
    statement_options_text_only: null, matched_irf_expression_positions: null,
    command_list_empty_occurrences_not_retained: null, type_result_history_association: null,
    prior_function_history_unavailable: null,
    equation_correspondence_unpaired: "Equation pairing unavailable; shown as Removed and Added.",
    condition_correspondence_unpaired: "Condition pairing unavailable; shown as Removed and Added.",
    occurrence_correspondence_unpaired: "Occurrence pairing unavailable; shown as Removed and Added.",
    complementarity_unmatched: "Condition compared as text only.",
    references_partial: "Some equation references were omitted to limit the comparison size.",
    reference_limit: "Equation reference list reached its limit.",
    detail_output_partial: "This comparison is too large to show every detail. Use Text diff to review the files.",
    history_identity_unavailable: "Source text and source links are unavailable for some historical values.",
    producer_occurrence_context: "The source lines for some instructions could not be found.",
    setting_origin_unavailable: "The source lines for some settings could not be found.",
    prior_copy_effective_unavailable: "Copied prior and optimizer values are not compared.",
    prior_positional_fields_unavailable: "Some prior settings could only be compared as text.",
    planner_objective_first_only: "Only the first planner objective is shown here.",
    planner_discount_first_only: "The discount expression and value shown here each come from the first available setting.",
    planner_discount_selected_origins: "The discount expression and value come from different settings.",
    instrument_union_first_seen: "Each instrument is listed once, in the order it first appears.",
    load_params_history_first_only: "Only the first filename is shown here. Loaded file contents are not compared.",
    heterogeneity_numerical_data_not_captured: "Data files and computed results used by these settings are not compared.",
    external_function_body_not_captured: "Only the function declaration is compared. Function files and computed results are not compared.",
    macro_context_written_boundary: "Macro lines are compared as written, including branches that were not used.",
  };
  function comparisonNote(detail, limit) {
    const code = limit.code;
    if (Object.hasOwn(comparisonNotes, code)) return comparisonNotes[code];
    if (["token_alignment_limit", "occurrence_token_alignment_limit"].includes(code) || code.endsWith("_path_owner")) return null;
    const instructionShown = ["before", "after"].every(side => !detail[side] || visibleExpressions(detail).some(expression => ["statement_text", "statement_tokens", "written_row"].includes(expression.field) && expression[side]?.text));
    if (["policy_options_context", "estimation_other_options_context", "dsge_var_calibration_value_context"].includes(code) || code.endsWith("_retained_history")) return instructionShown ? null : "Some command options are not shown here. Use Text diff to review them.";
    if (code === "observation_trend_expression_not_retained") return instructionShown ? null : "Trend expressions are not shown here. Use Text diff to review them.";
    if (code === "homotopy_endpoints_not_retained") return instructionShown ? null : "Initial and final homotopy values are not shown here. Use Text diff to review them.";
    if (code === "pruned_initialization_rhs_not_retained") return instructionShown ? null : "The former value of a removed assignment is unavailable.";
    if (/_evaluation_unavailable$/.test(code)) {
      const name = code.replace(/_evaluation_unavailable$/, "") + "_value";
      const field = detail.fields.find(field => field.name.replaceAll(".", "_") === name);
      if (field && ["before", "after"].every(side => !detail[side] || field[side].state !== "unknown")) return null;
      const sides = field && ["before", "after"].filter(side => detail[side] && field[side].state === "unknown").map(titleCase).join(" and ");
      return `${field?.label.replace(/: known value$/, "") ?? "Expression"}: ${sides ? `${sides} ` : ""}value unavailable; expression retained.`;
    }
    if (/_number_spelling_unavailable$/.test(code)) return "Some numbers could not be compared exactly.";
    if (/_named_operator_association_unavailable$/.test(code)) return "Some expressions have an unknown model name.";
    if (/_tree_recovered$/.test(code)) return "Some expressions could not be read. Use Text diff to review them.";
    if (/_tree_(?:node|materialization)_limit$/.test(code) || code === "expression_tree_materialization_limit") return "This comparison is too large to show every value.";
    if (/_(?:selector_(?:visit|token)|text_materialization)_limit$/.test(code)) return instructionShown ? null : "This comparison is too large to show all instruction text. Use Text diff to review the files.";
    if (/_text_unavailable$/.test(code)) return instructionShown ? null : "The text for some values is unavailable. Use Text diff to review the files.";
    return "Some details could not be compared. Use Text diff to review the files.";
  }
  function rowNotes(row) {
    const entries = row.semantic ? [
      ...row.semantic.limits.map(limit => {
        const text = comparisonNote(row.semantic, limit);
        return text && `${text}${limit.omitted !== null ? ` ${limit.omitted} omitted.` : ""}`;
      }),
      ...visibleExpressions(row.semantic).filter(expression => expression.availability !== "complete").map(expression => `${titleCase(expression.field)}: ${expression.availability === "not_available" ? "text comparison unavailable" : "highlighting is partial"}.`),
    ] : payload.semantic ? ["Comparison details unavailable; the returned change is shown."] : [];
    const unique = new Set(entries.filter(Boolean));
    if (!unique.size) return null;
    const notes = node("aside", undefined, "group-notes"), list = node("ul", undefined, "limits"); notes.setAttribute("aria-label", "Comparison limits");
    for (const text of unique) list.append(node("li", text)); notes.append(list); return notes;
  }
  function searchText(row) {
    const fields = row.semantic?.fields.filter(field => field.name !== "dependent_on") ?? [];
    return [rowLabel(row), row.group, row.before ?? "", row.after ?? "", ...contextScopes(row).map(scopeLabel),
      ...(row.semantic?.facets ?? []).map(titleCase),
      ...(row.semantic?.expressions ?? []).flatMap(expression => [expression.before?.text ?? "", expression.after?.text ?? ""]),
      ...fields.flatMap(field => [field.label, fieldText(field.before), fieldText(field.after)])].join("\n").toLocaleLowerCase();
  }

  const parameterField = (row, name) => row.semantic?.fields.find(field => field.name === name && field.comparison_availability === "complete");
  const pairedParameter = row => row.kind === "changed" && row.semantic?.family === "parameters" && row.semantic.count_unit === "final_fact";
  const valueOnly = row => pairedParameter(row) && parameterField(row, "evaluated_value")?.changed && parameterField(row, "expression")?.changed === false;
  function parameterParents(rows) {
    const candidates = new Map(), parents = new Map();
    for (const row of rows) {
      if (!pairedParameter(row)) continue;
      const name = row.semantic.name;
      candidates.set(name, candidates.has(name) ? null : row);
    }
    for (const row of rows) {
      if (!valueOnly(row)) continue;
      const proof = parameterField(row, "dependent_on"), before = proof?.before, after = proof?.after;
      if (!proof || proof.changed || before.state !== "present" || after.state !== "present" ||
          before.value?.kind !== "text" || after.value?.kind !== "text" || before.value.value !== after.value.value) continue;
      const parent = candidates.get(before.value.value);
      if (parent && parent !== row && parameterField(parent, "expression")?.changed) parents.set(row.id, parent.id);
    }
    return parents;
  }
  function indexGroups(rows) {
    const indexed = new Map(), sections = Object.keys(names);
    for (const row of rows) {
      const key = row.section + ":" + row.group;
      if (!indexed.has(key)) indexed.set(key, { key, label: row.group, section: row.section, rows: [] });
      indexed.get(key).rows.push(row);
    }
    const ordered = [...indexed.values()].sort((left, right) => sections.indexOf(left.section) - sections.indexOf(right.section));
    for (const group of ordered) group.parents = group.section === "parameters" ? parameterParents(group.rows) : new Map();
    return new Map(ordered.map(group => [group.key, group]));
  }

  function changeSummary(row) {
    if (pairedParameter(row)) {
      const value = parameterField(row, "evaluated_value");
      if (value?.changed) {
        const before = fieldText(value.before), after = fieldText(value.after);
        if (before && after) return before + " → " + after;
      }
      const expression = parameterField(row, "expression");
      if (expression?.changed) return "Expression changed";
    }
    if (row.kind !== "changed") return "";
    if (row.semantic?.facets.includes("expression")) return "Expression changed";
    if (row.semantic?.facets.includes("timing")) return "Timing changed";
    if (row.semantic?.facets.includes("tags")) return "Tags changed";
    return "";
  }
  function toggleRow(id) {
    presentation.detailId = presentation.detailId === id ? null : id;
    presentation.referenceSides = [];
    presentation.returnFocus = null;
    update("row-" + id);
  }
  function compactRow(row, { child = false, context = false, dependents = [] } = {}) {
    const article = node("article", undefined, "change-item" + (child ? " dependent-row" : ""));
    article.setAttribute("data-row-id", row.id);
    const open = presentation.detailId === row.id, title = rowLabel(row), summary = changeSummary(row);
    const line = node("div", undefined, "change-line"), heading = node("h2", undefined, "row-heading");
    const toggle = node("button", undefined, "change-row");
    toggle.type = "button";
    toggle.id = "row-" + row.id;
    toggle.setAttribute("aria-expanded", String(open));
    toggle.setAttribute("aria-controls", "detail-" + row.id);
    const mark = node("span", marks[row.kind], "change-mark " + row.kind);
    mark.setAttribute("aria-hidden", "true");
    const twisty = node("span", undefined, "twisty");
    twisty.setAttribute("aria-hidden", "true");
    toggle.append(twisty, mark, node("span", changeLabel(row.kind) + " ", "sr-only"), node("span", title, "row-label"));
    const scope = contextScopes(row).map(scopeLabel).join(" · ");
    if (scope) toggle.append(node("span", scope, "row-scope"));
    if (summary) toggle.append(node("span", summary, "row-summary"));
    if (!child && valueOnly(row)) toggle.append(node("span", "Value only", "row-note"));
    toggle.addEventListener("click", () => toggleRow(row.id));
    heading.append(toggle);
    line.append(heading);
    article.append(line);
    const detail = open ? rowDetail(row) : node("div");
    detail.id = "detail-" + row.id;
    detail.hidden = !open;
    article.append(detail);
    if (dependents.length) {
      const id = "dependents-" + row.id;
      const count = dependents.length + (dependents.length === 1 ? " dependent value" : " dependent values");
      const expanded = context || presentation.openDependents.includes(row.id);
      const children = node("div", undefined, "dependent-list");
      children.id = id;
      children.hidden = !expanded;
      if (context) line.append(node("span", count, "dependent-context"));
      else {
        const disclosure = node("button", undefined, "dependent-toggle");
        disclosure.type = "button";
        disclosure.id = "dependents-toggle-" + row.id;
        disclosure.setAttribute("aria-expanded", String(expanded));
        disclosure.setAttribute("aria-controls", id);
        disclosure.setAttribute("aria-label", title + ": " + count);
        const arrow = node("span", undefined, "twisty");
        arrow.setAttribute("aria-hidden", "true");
        disclosure.append(arrow, node("span", count));
        disclosure.addEventListener("click", () => {
          presentation.openDependents = expanded ? presentation.openDependents.filter(id => id !== row.id) : [...presentation.openDependents, row.id];
          if (expanded && dependents.some(item => item.id === presentation.detailId)) {
            presentation.detailId = null;
            presentation.referenceSides = [];
          }
          update(disclosure.id);
        });
        line.append(disclosure);
      }
      if (expanded) for (const item of dependents) children.append(compactRow(item, { child: true }));
      article.append(children);
    }
    return article;
  }
  function appendRows(container, group, shown) {
    if (!group.parents.size) {
      for (const row of historyCards(shown)) container.append(compactRow(row));
      return;
    }
    const matched = new Set(), children = new Map();
    for (const row of shown) {
      matched.add(row.id);
      const parent = group.parents.get(row.id);
      if (!parent) continue;
      if (!children.has(parent)) children.set(parent, []);
      children.get(parent).push(row);
    }
    for (const row of group.rows) {
      if (group.parents.has(row.id)) continue;
      const context = !matched.has(row.id), dependents = children.get(row.id);
      if (!context || dependents) container.append(compactRow(row, { context, dependents }));
    }
  }
  function countText(count) { return count + (count === 1 ? " change" : " changes"); }
  function revealGroup(list, button, scrollLeft, scrollTop) {
    const { offsetLeft, offsetTop, offsetWidth, offsetHeight } = button;
    const left = Math.min(offsetLeft, Math.max(scrollLeft, offsetLeft + offsetWidth - list.clientWidth));
    const top = Math.min(offsetTop, Math.max(scrollTop, offsetTop + offsetHeight - list.clientHeight));
    if (left) list.scrollLeft = left;
    if (top) list.scrollTop = top;
  }
  function renderRows(focusId = document.activeElement?.id) {
    if (!payload || !choices) return;
    const query = choices.search.trim().toLocaleLowerCase(), filtered = new Map();
    let total = 0;
    for (const [key, group] of groups) {
      const rows = group.rows.filter(row => choices.changeKinds.includes(row.kind) && (!query || searchText(row).includes(query)));
      filtered.set(key, rows);
      total += rows.length;
    }
    const { available, incompleteRows } = payload;
    controls.counts.hidden = !available && !incompleteRows;
    controls.counts.textContent = incompleteRows ? countText(total) + " shown" : total === available ? countText(total) : total + " of " + available + " changes";
    const scrollLeft = groupList?.scrollLeft ?? 0, scrollTop = groupList?.scrollTop ?? 0;
    groupList = null;
    controls.results.className = "results";
    controls.results.replaceChildren();
    if (!available) return;
    if (!total) {
      const empty = node("div", undefined, "empty"), reset = node("button", "Clear filters", "reset-filters");
      reset.type = "button";
      reset.addEventListener("click", () => {
        choices.search = "";
        choices.changeKinds = [...kinds];
        choices.customChangeKinds = [...kinds];
        renderControls();
        update("search");
      });
      empty.append(node("p", "No changes match these filters."), reset);
      controls.results.append(empty);
      return;
    }
    if (!filtered.get(choices.group)?.length) choices.group = [...filtered].find(([, rows]) => rows.length)?.[0] ?? null;
    const grid = node("div", undefined, "grouped"), list = node("nav", undefined, "group-list");
    list.setAttribute("aria-label", "Change groups");
    let selectedButton;
    for (const [key, group] of groups) {
      const count = filtered.get(key).length, selected = key === choices.group;
      const button = node("button", undefined, "group-button" + (selected ? " selected" : ""));
      button.type = "button";
      button.id = "group-" + key;
      button.disabled = !count;
      button.append(node("span", group.label), node("span", String(count), "group-count"));
      button.setAttribute("aria-label", group.label + ", " + countText(count));
      button.setAttribute("aria-pressed", String(selected));
      button.setAttribute("data-group", key);
      if (selected) selectedButton = button;
      button.addEventListener("click", () => {
        choices.group = key;
        if (scrollArea) scrollArea.scrollTop = 0;
        update(button.id);
      });
      list.append(button);
    }
    const selected = groups.get(choices.group), rows = node("section", undefined, "group-rows");
    rows.setAttribute("aria-label", selected.label);
    appendRows(rows, selected, filtered.get(choices.group));
    grid.append(list, rows);
    controls.results.append(grid);
    groupList = list;
    const focus = focusId && document.getElementById(focusId);
    revealGroup(list, focus?.parentElement === list && !focus.disabled ? focus : selectedButton, scrollLeft, scrollTop);
    if (focus && !focus.disabled) focus.focus({ preventScroll: true });
  }
  function inputDetails(message, side) {
    const label = titleCase(side), input = message[side];
    const model = node("details", undefined, "input-side"), heading = node("summary", undefined, "input-heading");
    heading.id = "input-" + side;
    const content = node("div", undefined, "input-details");
    const sourceLabel = message.inputNames?.[side] ?? basename(input.split(" · ")[0]);
    const summary = message.analysisSummaries?.[side], analysis = summary?.analysis;
    const configuration = analysis?.configuration ?? summary?.requested_analysis;
    const equation = configuration?.equation_selection, defines = configuration?.defines ?? [];
    const receipts = analysis?.overrides ?? [], overrides = configuration?.overrides ?? [];
    const preset = analysis?.selection?.name ?? configuration?.name;
    const hasNamedPreset = preset && !/^(?:Analysis preview|Macro inputs)(?: \(modified\))?$/.test(preset);
    const choiceCount = defines.length + (receipts.length || overrides.length);
    const compact = [];
    if (hasNamedPreset) compact.push(preset);
    else if (choiceCount) compact.push(choiceCount + (choiceCount === 1 ? " macro input" : " macro inputs"));
    else if (equation) compact.push("Equation selection");
    else if (configuration) compact.push("Source");
    const revision = message.inputRevisions?.[side];
    if (revision) compact.unshift(revision);
    heading.append(node("small", label), node("code", sourceLabel));
    if (compact.length) heading.append(node("span", compact.join(" · "), "input-summary"));

    content.append(node("p", input, "model-path"));
    for (const define of defines) content.append(node("p", "Macro: " + define.name + "=" + define.expression, "analysis-detail"));
    for (const value of equation?.values ?? []) content.append(node("p", "Equation: " + (equation.tag ?? "name") + "='" + value + "' (" + (equation.mode === "include" ? "included" : "excluded") + ")", "analysis-detail"));
    const names = new Map();
    for (const receipt of receipts) if (receipt.name) names.set(receipt.name, (names.get(receipt.name) ?? 0) + 1);
    const overrideLabel = (operation, name, branch) => {
      if (operation.kind === "definition") return "Macro: " + (name ? name + "=" : "") + operation.expression;
      if (operation.kind === "skip_definition") return "Skip " + (name ?? "macro") + " definition";
      return "Branch: " + (branch ?? (operation.branch_id === null ? "No branch" : "selected"));
    };
    if (receipts.length) {
      for (const receipt of receipts) {
        let choice = overrideLabel(receipt.operation, receipt.name, receipt.branch_label);
        if (names.get(receipt.name) > 1) choice += " · " + basename(receipt.operation.target.file ?? "unnamed model") + (receipt.source_line ? ":" + receipt.source_line : "");
        if (receipt.application !== "applied" || receipt.cause) {
          const state = receipt.application === "not_reached" ? "not reached" : receipt.application === "unknown" ? "application unknown" : "applied";
          choice += " (" + state + ")" + (receipt.cause ? " — " + receipt.cause : "");
        }
        content.append(node("p", choice, "analysis-detail"));
      }
    } else {
      for (const operation of overrides) {
        const file = operation.target.file === null ? "unnamed model" : basename(operation.target.file);
        content.append(node("p", overrideLabel(operation) + " · " + file + " · application unavailable", "analysis-detail"));
      }
    }
    if (summary?.search_paths?.length) content.append(node("p", "Include paths: " + summary.search_paths.join(", "), "analysis-detail"));
    if (summary?.counts) {
      const counts = value => value.n_endogenous + " endogenous · " + value.n_equations + (value.n_equations === 1 ? " equation" : " equations");
      const dimensions = summary.counts.heterogeneity_dimensions;
      content.append(node("p", (dimensions.length ? "Aggregate model: " : "") + counts(summary.counts), "analysis-detail"));
      for (const dimension of dimensions) content.append(node("p", "Heterogeneous model: " + dimension.dimension + " · " + counts(dimension), "analysis-detail"));
    } else if (summary) content.append(node("p", summary.counts_cause || "Model counts are unavailable for this captured input.", "analysis-detail"));
    if (summary?.cause?.message && summary.cause.message !== summary.counts_cause) content.append(node("p", summary.cause.message, "analysis-detail"));

    model.title = input;
    model.open = presentation.inputDetails.includes(side);
    model.append(heading, content);
    model.addEventListener("toggle", () => {
      if (!model.isConnected) return;
      const open = presentation.inputDetails;
      if (open.includes(side) === model.open) return;
      presentation.inputDetails = model.open ? [...open, side] : open.filter(value => value !== side);
      persist();
    });
    return model;
  }

  function comparisonLimits(message) {
    const semantic = message.semantic?.limits ?? [];
    const coverage = message.coverage;
    const workLimit = /_(?:tree_(?:node|materialization)|selector_(?:visit|token)|text_materialization)_limit$/;
    const hasWorkLimit = limits => limits.some(limit => workLimit.test(limit.code));
    const workLimited = hasWorkLimit(semantic) || hasWorkLimit(coverage?.limits ?? []) ||
      coverage?.families?.some(family => hasWorkLimit(family.limits));
    const omitted = codes => semantic.filter(limit => codes.includes(limit.code)).reduce((total, limit) => total + (limit.omitted ?? 0), 0);
    const rows = omitted(["serialized_output_limit"]);
    const refs = omitted(["reference_limit", "reference_output_limit", "reference_target_output_limit"]);
    let hunks = 0, sourceDetailLimited = false, pairedFiles = false, unpairedFiles = false;
    for (const file of message.sourceChanges?.files ?? []) {
      hunks += file.omitted_hunks ?? 0;
      sourceDetailLimited ||= file.limits.some(limit => ["source_materialization_limit", "source_alignment_limit"].includes(limit.code));
      if (file.before && file.after) pairedFiles = true;
      else unpairedFiles = true;
    }
    const clauses = [];
    if (rows) clauses.push(rows + (rows === 1 ? " model change detail" : " model change details"));
    if (refs) clauses.push(refs + (refs === 1 ? " equation reference" : " equation references"));
    if (hunks) clauses.push(hunks + (hunks === 1 ? " section of file differences" : " sections of file differences"));
    const notices = [];
    if (clauses.length) notices.push("Comparison limits omitted " + clauses.join(", ") + ".");
    else if (semantic.some(limit => ["token_alignment_limit", "occurrence_token_alignment_limit"].includes(limit.code))) notices.push("Some highlights were omitted because the comparison reached its limit.");
    else if (coverage?.limits?.some(limit => limit.code === "detail_output_partial")) notices.push("Some details were omitted because the comparison reached its limit.");
    if (sourceDetailLimited) notices.push("Some file difference details were omitted because the comparison reached its limit. Use Text diff to review the files.");
    if (workLimited) notices.push("Some model changes may be missing because the comparison reached its work limit.");
    return { incompleteRows: Boolean(rows || workLimited), pairedFiles, unpairedFiles, text: notices.join(" ") };
  }
  function resultMessage(message, limits) {
    if (message.status !== "ready") return message.message;
    const files = message.sourceChanges;
    const identical = files?.availability === "complete" && !files.files.length &&
      message.coverage?.source_boundary && message.coverage.source_boundary !== "parsed_models_only";
    if (message.rows.length) return identical ? "The captured file text is identical." : "";
    const model = limits.incompleteRows ? "No model changes shown." : "No model changes found.";
    return model + (limits.pairedFiles ? " The file text changed." : "") +
      (limits.unpairedFiles ? " Some files could not be matched between inputs." : "") +
      (identical ? " The captured file text is identical." : "");
  }
  function renderOutcome(message, limits) {
    controls.status.textContent = resultMessage(message, limits);
    controls.status.className = message.status;
    controls.status.hidden = !controls.status.textContent;
    controls.status.setAttribute("data-code", message.failureCode ?? "");
    controls.refresh.disabled = message.status === "loading";
    const limit = document.getElementById("limitsStatus");
    if (limit) {
      limit.textContent = limits.text;
      limit.hidden = !limit.textContent;
    }
    const choosePath = document.getElementById("choosePath");
    if (choosePath) choosePath.hidden = message.status !== "failure" || !message.canChoosePath;
    const boundary = document.getElementById("sourceBoundary");
    if (boundary) {
      const scope = message.coverage?.source_boundary;
      boundary.hidden = !scope;
      boundary.textContent = !scope ? "" : scope === "parsed_models_only" ? "File text is unavailable" : scope.startsWith("supplied_") ? "Provided model files and the includes they use" : "Model files and the includes they use";
    }
    const hasFiles = Boolean(message.sourceChanges?.files.length);
    for (const [id, hidden] of [["rootTextDiff", hasFiles], ["capturedTextDiff", !hasFiles]]) {
      const button = document.getElementById(id);
      if (!button) continue;
      button.hidden = hidden;
      button.disabled = message.status !== "ready";
      button.title = !button.disabled ? "" : message.status === "loading" ? "Wait for the comparison to finish." : "Refresh to compare the current file text.";
    }
  }

  controls.refresh.addEventListener("click", () => post("refresh"));
  for (const name of ["changeComparison", "swap", "rootTextDiff", "capturedTextDiff", "choosePath", "help"]) document.getElementById(name)?.addEventListener("click", () => post(name));
  controls.search.addEventListener("input", () => {
    choices.search = controls.search.value;
    if (scrollArea) scrollArea.scrollTop = 0;
    update();
  });
  controls.kinds.addEventListener("keydown", event => {
    if (event.key === "Escape") { controls.kinds.open = false; controls.kinds.children[0]?.focus(); }
  });
  controls.results.addEventListener("keydown", event => {
    const id = presentation.detailId;
    if (event.key !== "Escape" || !id) return;
    if (event.target.id !== "row-" + id && !document.getElementById("detail-" + id)?.contains(event.target)) return;
    event.preventDefault();
    toggleRow(id);
  });
  document.addEventListener?.("pointerdown", event => {
    if (!controls.kinds.contains(event.target)) controls.kinds.open = false;
  });
  window.addEventListener("message", event => {
    const message = event.data;
    if (!message || message.type !== "render" || !Array.isArray(message.rows)) return;

    const sameCapture = message.captureId && message.captureId === payload?.captureId;
    if (!message.captureId || message.captureId !== presentation.captureId || !presentation.referenceSides) {
      presentation = { captureId: message.captureId, detailId: null, openDependents: [], referenceSides: [], inputDetails: [], returnFocus: null };
      groupList = null;
      if (scrollArea) scrollArea.scrollTop = 0;
    }
    if (!sameCapture) {
      groups = indexGroups(message.rows);
      references = new Map((message.references ?? []).map(reference => [reference.pointer, reference]));
    }
    const limits = comparisonLimits(message);
    payload = { key: message.key, token: message.token, status: message.status, captureId: message.captureId,
      available: message.rows.length, semantic: Boolean(message.semantic), incompleteRows: limits.incompleteRows };
    choices = message.choices;
    const returning = message.active !== false && presentation.returnFocus;
    const focused = message.active === false ? "" : document.activeElement?.id;
    controls.models.replaceChildren(inputDetails(message, "before"), inputDetails(message, "after"));
    renderOutcome(message, limits);
    renderControls();
    renderRows(returning?.id || focused);
    if (returning) {
      if (scrollArea) scrollArea.scrollTop = returning.scrollTop;
      const target = document.getElementById(returning.id);
      if (!target || target.disabled) document.getElementById("row-" + returning.rowId)?.focus({ preventScroll: true });
      presentation.returnFocus = null;
    }
    remember();
  });
  api.postMessage({ type: "ready", key: saved?.key, choices: saved?.choices });
})();
