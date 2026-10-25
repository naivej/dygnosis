/* global acquireVsCodeApi, document, window */
(() => {
  "use strict";
  const api = acquireVsCodeApi(), saved = api.getState();
  const names = { symbols: "Symbols", parameters: "Parameters", equations: "Equations", shockSetup: "Shock setup", shockAnalysisSetup: "Shock analysis setup", steadyState: "State", forecast: "Forecast", priors: "Priors", commands: "Commands", data: "Data", occbin: "OccBin", policy: "Policy", semiStructural: "Semi-structural", moments: "Method of moments", msSbvar: "MS-SBVAR", heterogeneity: "Heterogeneity", externalFunctions: "External functions", trends: "Trends", operations: "Operations", macroContext: "Macro context" };
  const kinds = ["added", "removed", "changed"], marks = { added: "+", removed: "−", changed: "~" };
  const changeLabel = kind => kind === "changed" ? "Replaced" : titleCase(kind);
  const controls = Object.fromEntries(["models", "status", "search", "kinds", "counts", "results", "refresh"].map(id => [id, document.getElementById(id)]));
  let payload, choices, references = new Map();
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
  function action(label, type, args, reason) {
    const button = node("button", label); button.type = "button"; button.disabled = !!reason;
    button.setAttribute("aria-label", reason ? label + " unavailable: " + reason : label); if (reason) button.title = reason;
    button.addEventListener("click", () => { if (!button.disabled) post(type, args); }); return button;
  }
  function sourceAction(row, side) {
    const target = row.navigation[side] ?? row.navigation.block_openers?.[side], files = row.sourceFiles?.[side] ?? [];
    const locations = target?.written_locations ?? [];
    const reason = payload.status !== "ready" ? "Refresh to open source links." : !locations.length && !files.length ? "The source for this side is unavailable." : "";
    const button = action("Open " + titleCase(side) + " source", "openSource", { rowId: row.id, side }, reason);
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
    if (row.semantic) {
      const ownRefs = row.semantic.references.map(pointer => references.get(pointer)).filter(ref => ref?.side === side);
      if (ownRefs.length) {
        const refs = node("section", undefined, "refs"); refs.append(node("h3", "Direct equation references"));
        const seenEquations = new Set();
        for (const ref of ownRefs) {
          // The wire retains each symbol use; the card lists its equations.
          const equationKey = JSON.stringify([ref.scope.domain, ref.scope.dimension, ref.equation_index]);
          if (seenEquations.has(equationKey)) continue;
          seenEquations.add(equationKey);
          const line = node("p", undefined, "reference"), target = ref.navigation[side];
          const button = action(ref.label, "openReference", { pointer: ref.pointer, side }, payload.status !== "ready" ? "Refresh to open this equation." : !target?.written_locations.length ? "The source line for this equation is unavailable." : "");
          button.setAttribute("aria-label", "Open " + titleCase(side) + " reference: " + ref.label + (button.disabled ? ". " + button.title : ""));
          const context = ref.scope.dimension && (!scopeShown || ref.scope.dimension !== scope) ? scopeLabel(ref.scope.dimension) : "";
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
    if (row.semantic?.family === "operations" && instruction?.before && instruction.after && instruction.before.text === instruction.after.text && row.semantic.fields.some(field => field.changed && !["role", "order"].includes(field.name))) article.append(node("p", "The text is the same, but the resulting changes to the model differ.", "source-note"));
    const notes = rowNotes(row); if (notes) article.append(notes);
    return article;
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
        const card = { ...row, semantic: { ...detail, expressions: [{ ...statement, ...Object.fromEntries(["before", "after"].map(side => [side, statement[side] && { ...statement[side], runs: [...statement[side].runs] }])) }] } };
        groups.set(key, { card, parts }); cards.push(card); continue;
      }
      existing.card.label = target;
      const combined = existing.card.semantic.expressions[0];
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
    references_partial: "Equation reference list is partial.",
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
      for (const row of historyCards(selected.rows)) rows.append(rowDetail(row));
    } else {
      const text = ["failure", "incomplete", "loading"].includes(payload.status) ? "" : payload.rows.length ? "No changes match these filters." : payload.sourceChanges?.files.length ? "No model changes. File text differs; use Text diff." : payload.coverage?.availability !== "complete" && payload.semantic ? "No model changes shown. Some details could not be compared." : payload.status === "ready" ? "No model changes found. File text may still differ." : "Refresh to compare current files.";
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
    controls.models.replaceChildren();
    for (const [side, label, input] of [["before", "Before", message.before], ["after", "After", message.after]]) {
      const model = node("div", undefined, "input-side"), parts = input.split(" · ");
      const heading = node("div", undefined, "input-heading");
      const sourceLabel = message.inputNames?.[side] ?? basename(parts[0]);
      heading.append(node("small", label), node("code", sourceLabel)); model.append(heading);
      const summary = message.analysisSummaries?.[side], analysis = summary?.analysis;
      if (summary) {
        const configuration = analysis?.configuration ?? summary.requested_analysis;
        const names = new Map();
        for (const receipt of analysis?.overrides ?? []) if (receipt.name) names.set(receipt.name, (names.get(receipt.name) ?? 0) + 1);
        const overrideChoice = (operation, name, branchLabel) => {
          const label = operation.kind === "definition" ? `Macro: ${name ? `${name}=` : ""}${operation.expression}`
            : operation.kind === "skip_definition" ? `Skip ${name ?? "macro"} definition` : `Branch: ${branchLabel ?? (operation.branch_id === null ? "No branch" : "selected")}`;
          if (analysis) return label;
          const file = operation.target.file === null ? "unnamed model" : basename(operation.target.file.replaceAll("\\", "/"));
          return `${label} · ${file} · application unavailable`;
        };
        const overrideChoices = analysis?.overrides?.length
          ? analysis.overrides.map((receipt, index) => {
            let choice = overrideChoice(receipt.operation, receipt.name, receipt.branch_label ?? analysis.override_branch_labels?.[index]);
            if (receipt.name && names.get(receipt.name) > 1) choice += ` · ${basename(receipt.operation.target.file ?? "unnamed model")}${receipt.source_line ? `:${receipt.source_line}` : ""}`;
            if (receipt.application === "applied" && !receipt.cause) return choice;
            const state = receipt.application === "not_reached" ? "not reached" : receipt.application === "unknown" ? "application unknown" : "applied";
            return `${choice} (${state})${receipt.cause ? ` — ${receipt.cause}` : ""}`;
          })
          : (configuration?.overrides ?? []).map((operation, index) => overrideChoice(operation, analysis?.override_names?.[index], analysis?.override_branch_labels?.[index]));
        const choiceLines = [
          ...(configuration?.defines ?? []).map(input => `Macro: ${input.name}=${input.expression}`),
          ...(configuration?.equation_selection ? configuration.equation_selection.values.map(value => `Equation: ${configuration.equation_selection.tag ?? "name"}='${value}' (${configuration.equation_selection.mode === "include" ? "included" : "excluded"})`) : []),
          ...overrideChoices,
        ];
        const presetName = analysis?.selection?.name ?? configuration?.name;
        const usefulPresetName = presetName && !/^(?:Analysis preview|Macro inputs)(?: \(modified\))?$/.test(presetName) ? presetName : undefined;
        const hoverLines = [input];
        hoverLines.push(...choiceLines);
        if (choiceLines.length) for (const choice of choiceLines) model.append(node("p", choice, "analysis-detail"));
        else if (configuration) model.append(node("p", "Source", "analysis-detail"));
        else if (!analysis) model.append(node("p", "Analysis unavailable", "analysis-detail"));
        if (usefulPresetName) hoverLines.push(usefulPresetName);
        if (summary.search_paths?.length) model.append(node("p", `Include paths: ${summary.search_paths.join(", ")}`, "analysis-detail"));
        if (summary.search_paths?.length) hoverLines.push(`Include paths: ${summary.search_paths.join(", ")}`);
        if (summary.counts) {
          const counts = value => `${value.n_endogenous} endogenous · ${value.n_equations} ${value.n_equations === 1 ? "equation" : "equations"}`;
          hoverLines.push(`${summary.counts.heterogeneity_dimensions.length ? "Aggregate model: " : ""}${counts(summary.counts)}`);
          for (const dimension of summary.counts.heterogeneity_dimensions) hoverLines.push(`Heterogeneous model: ${dimension.dimension} · ${counts(dimension)}`);
        } else model.append(node("p", summary.counts_cause || "Model counts are unavailable for this captured input.", "analysis-detail"));
        if (summary.cause?.message && summary.cause.message !== summary.counts_cause) model.append(node("p", summary.cause.message, "analysis-detail"));
        model.title = hoverLines.join("\n");
      } else model.title = input;
      controls.models.append(model);
    }
    controls.status.textContent = message.message; controls.status.className = message.status; controls.refresh.disabled = message.status === "loading";
    controls.status.setAttribute("data-code", message.failureCode ?? "");
    controls.status.hidden = message.status === "ready" && message.message === "Current comparison";
    const path = document.getElementById("choosePath"); if (path) path.hidden = message.status !== "failure" || !message.canChoosePath;
    const boundary = document.getElementById("sourceBoundary"); if (boundary) { boundary.hidden = !message.coverage; boundary.textContent = !message.coverage ? "" : message.coverage.source_boundary === "parsed_models_only" ? "File text is unavailable" : message.coverage.source_boundary.startsWith("supplied_") ? "Provided model files and the includes they use" : "Model files and the includes they use"; }
    const hasFiles = !!message.sourceChanges?.files.length;
    const root = document.getElementById("rootTextDiff"); if (root) { root.disabled = message.status !== "ready"; root.hidden = hasFiles; }
    const captured = document.getElementById("capturedTextDiff"); if (captured) { captured.disabled = message.status !== "ready" || !hasFiles; captured.hidden = !hasFiles; }
    renderControls(); renderRows(); remember();
  });
  api.postMessage({ type: "ready", key: saved?.key, choices: saved?.choices });
})();
