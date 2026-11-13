/* global acquireVsCodeApi, document, window */
(() => {
  "use strict";
  const api = acquireVsCodeApi(), saved = api.getState();
  const names = { symbols: "Symbols", parameters: "Parameters", equations: "Equations", shockSetup: "Shock setup", shockAnalysisSetup: "Shock analysis setup", steadyState: "State", forecast: "Forecast", priors: "Priors", commands: "Commands", data: "Data", occbin: "OccBin", policy: "Policy", semiStructural: "Semi-structural", moments: "Method of moments", msSbvar: "MS-SBVAR", heterogeneity: "Heterogeneity", externalFunctions: "External functions", trends: "Trends", operations: "Operations", macroContext: "Macro context" };
  const kinds = ["added", "removed", "changed"], marks = { added: "+", removed: "−", changed: "~" };
  const changeLabel = kind => titleCase(kind);
  const controls = Object.fromEntries(["models", "status", "search", "kinds", "kindToggles", "counts", "results"].map(id => [id, document.getElementById(id)]));
  let payload, choices, groupList, references = new Map(), groups = new Map();
  let kindCounts = {}, rowFocus, refreshFocus;
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
  const ownerIdentity = row => JSON.stringify([row.semantic?.family, row.kind, row.label, row.sideScopes,
    ...["before", "after"].map(side => row.navigation[side]?.occurrence_id ?? row.semantic?.[side]?.occurrence)]);
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
    const selection = selected.length === kinds.length ? "All kinds" : selected.map(changeLabel).join(", ") || "No kinds";
    const summary = node("summary", selection), options = node("div", undefined, "change-type-options");
    summary.setAttribute("aria-label", "Change kinds. " + selection);
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
      label.append(checkbox, node("span", kind === "all" ? "All" : changeLabel(kind) + " " + (kindCounts[kind] ?? 0))); options.append(label);
    }
    controls.kinds.replaceChildren(summary, options);
    controls.kindToggles?.replaceChildren(...kinds.map(kind => {
      const button = node("button", changeLabel(kind) + " " + (kindCounts[kind] ?? 0), "kind-toggle " + kind);
      button.type = "button";
      button.id = "kind-toggle-" + kind;
      button.setAttribute("aria-pressed", String(selected.includes(kind)));
      button.addEventListener("click", () => {
        choices.changeKinds = kinds.filter(value => value === kind ? !selected.includes(value) : selected.includes(value));
        choices.customChangeKinds = [...choices.changeKinds];
        renderControls(); update(button.id);
      });
      return button;
    }));
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
  function sourceAction(row, side, owner = row.id) {
    const target = row.navigation[side] ?? row.navigation.block_openers?.[side], files = row.sourceFiles?.[side] ?? [];
    const locations = target?.written_locations ?? [];
    const reason = payload.status !== "ready" ? "Refresh to open source links." : !locations.length && !files.length ? "The source for this side is unavailable." : "";
    const label = row.sourceLabels?.[side];
    const button = action("Open " + titleCase(side) + " " + (label ?? "source"), "openSource", { rowId: row.id, side }, reason, owner);
    button.id = "source-" + side + "-" + row.id;
    button.textContent = label ?? (locations.length === 1 ? basename(locations[0].uri) + ":" + (locations[0].range.start.line + 1) : files.length === 1 ? basename(files[0]) : "Open source" + (locations.length > 1 || files.length > 1 ? "…" : ""));
    button.className = "source-link";
    if (!reason) button.title = [...locations.map(location => location.uri), ...files].map(uri => payload.sourcePaths?.[uri] ?? uri).join("\n");
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
  function codeLines(parent, expression, side, writtenMacro = false) {
    if (!expression) return;
    const display = writtenMacro ? { runs: expression.runs, compact: false } : displayedCode(expression.runs);
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
  const observableDeclaration = detail => !detail.statement_group && detail.family === "observables" && ["before", "after"].every(side => !detail[side] || observableKeyword(detail, side));
  function declaration(detail, side) {
    if (observableDeclaration(detail)) return observableKeyword(detail, side);
    const written = textField(detail, "written_kind", side);
    return detail[side] && ["var", "varexo", "varexo_det", "parameters"].includes(written) ? written : localRole(detail) === "model_local_declaration" && detail[side] ? "model_local_variable" : null;
  }
  const writtenStatement = detail => detail.expressions.find(expression => expression.field === "statement_text" && ["before", "after"].every(side => !detail[side] || expression[side]));
  const pairedCommand = group => /^\d+:\d+$/.test(group);
  const writtenEdit = statement => statement && statement.before?.text !== statement.after?.text && statement.highlight_basis !== "none";
  // Interpolation can change a retained value without changing its written tokens.
  const writtenEditExplainsFields = statement => writtenEdit(statement) && ![statement.before, statement.after].some(side => side && /@(?:\{|#)/.test(side.text));
  function declarationKeywordRole(detail, side, keyword) {
    if (!observableKeyword(detail, side)) return "unchanged";
    const text = writtenStatement(detail)?.[side];
    if (!text) return "unchanged";
    const start = text.text.length - text.text.trimStart().length;
    if (text.text.slice(start, start + keyword.length).toLowerCase() !== keyword) return "unchanged";
    const roles = new Set(sliceRuns(text, start, start + keyword.length).map(run => run.role));
    return roles.size === 1 ? [...roles][0] : "unchanged";
  }
  function visibleFields(detail, expressions) {
    const assignment = detail.expressions.find(expression => expression.field === "expression");
    const evaluatedValueNeeded = ["before", "after"].some(side => {
      const text = assignment?.[side]?.text ?? textField(detail, "expression", side);
      return text !== null && text.trim() !== "" && !/^(?:[+-]\s*)?(?:\d+(?:\.\d*)?|\.\d+)(?:[eEdD][+-]?\d+)?$/.test(text.trim());
    });
    const statement = writtenStatement(detail);
    const writtenExplainsFields = writtenEditExplainsFields(statement);
    return detail.fields.filter(field => {
      if (field.name === "dependent_on") return false;
      if (field.name === "written_directives" && expressions.some(expression => expression.field === field.name)) return false;
      if (observableDeclaration(detail) && field.comparison_availability === "complete") {
        if (field.name === "role" || field.name === "target" && ["before", "after"].every(side => !detail[side] || textField(detail, "target", side) === detail[side].name)) return false;
        if (field.name === "captured_kind" && ["before", "after"].every(side => !detail[side] || textField(detail, "captured_kind", side) === (observableKeyword(detail, side) === "varobs" ? "var" : "varexo"))) return false;
      }
      if (field.name === "evaluated_value") return evaluatedValueNeeded;
      if (writtenExplainsFields && field.comparison_availability === "complete" && !["order", "execution_order"].includes(field.name)
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
    if (writtenEditExplainsFields(written)) return [written];
    const values = detail.expressions.filter(expression => expression !== written &&
      (expression.before?.text !== expression.after?.text || expression.availability !== "complete" || !written && expression.field === "expression"));
    return written ? [written, ...values] : values;
  }
  function sidePanel(row, side, owner) {
    const panel = node("div", undefined, "side " + side), heading = node("div", undefined, "side-title"), own = row.semantic?.[side];
    const scope = row.sideScopes[side], scopeShown = scope !== null && contextScopes(row).includes(scope);
    heading.append(node("strong", titleCase(side)));
    if (scopeShown) heading.append(node("span", scopeLabel(scope), "scope-label"));
    if (!row.includeFile || row[side] !== null) heading.append(sourceAction(row.commandSources?.[side] ?? row, side, owner));
    panel.append(heading);
    if (row[side] === null) panel.append(node("p", row.includeFile ? "Not included" : "Not present", "source-note"));
    else if (row.includeFile) return panel;
    else if (!row.semantic) panel.append(node("pre", row[side]));
    else if (own) {
      const keyword = declaration(row.semantic, side), role = localRole(row.semantic);
      if (keyword) {
        codeLines(panel, { runs: [{ text: keyword, role: declarationKeywordRole(row.semantic, side, keyword) }, { text: " ", role: "unchanged" }, { text: own.name, role: row.kind === "added" ? "added" : row.kind === "removed" ? "removed" : "unchanged" }, { text: ";", role: "unchanged" }] }, side);
      }
      const expressions = visibleExpressions(row.semantic);
      if (!keyword && !["symbols", "parameters", "equations", "shocks", "macro_context"].includes(row.semantic.family) && !expressions.some(expression => expression.field === "statement_text" && expression[side])) panel.append(node("p", "The file text for this instruction is unavailable. Use Text diff to review the files.", "source-note"));
      for (const expression of expressions) {
        if (!["expression", "statement_text"].includes(expression.field) || expressions.length > 1) panel.append(node("h3", expression.field === "statement_text" ? "Written instruction" : expression.field === "statement_tokens" ? "Expanded instruction" : row.semantic.fields.find(field => field.name === expression.field)?.label ?? titleCase(expression.field), "field-label"));
        const value = expression[side];
        const definitionRole = ["added", "removed"].includes(row.kind) ? row.kind : "unchanged";
        if (expression.field !== "written_directives" || value?.text) {
          codeLines(panel, value && role === "model_local_definition" && expression.field === "expression" ? { runs: [{ text: "# " + own.name + " = ", role: definitionRole }, ...value.runs, { text: ";", role: definitionRole }] } : value, side, expression.field === "written_directives");
        }
      }
      for (const field of visibleFields(row.semantic, expressions)) {
        if (field.name === "dimension" && field.comparison_availability === "complete" && scopeShown && field[side].value?.kind === "text" && field[side].value.value === scope) continue;
        const value = node("div", undefined, "field"), available = field.comparison_availability === "complete";
        value.setAttribute("data-field", field.name);
        value.append(node("h3", field.label, "field-label"));
        const written = field.name === "written_directives" && field[side].value?.kind === "list" ? field[side].value.value : null;
        if (written) {
          const opposite = field[side === "before" ? "after" : "before"].value;
          const role = side === "before" ? "removed" : "added";
          const runs = written.map((item, index) => ({ text: (index ? "\n" : "") + valueText(item),
            role: available && !(opposite?.kind === "list" && valueText(opposite.value[index]) === valueText(item)) ? role : "unchanged" }));
          if (runs.length) codeLines(value, { runs }, side, true);
        } else {
          const text = node("div", undefined, "field-content");
          text.append(fieldValue(field[side], field.changed && available ? side === "before" ? "token removed" : "token added" : ""));
          value.append(text);
        }
        if (!available) value.append(node("p", "This value could not be compared.", "source-note"));
        panel.append(value);
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
      const open = presentation.expandedDetails[row.id].includes(side);
      toggle.setAttribute("aria-expanded", String(open));
      list.hidden = !open;
      list.replaceChildren();
      if (!open) return;
      for (const ref of equations.values()) {
        const line = node("p", undefined, "reference"), target = ref.navigation[side];
        const reason = payload.status !== "ready" ? "Refresh to open this equation." : !target?.written_locations.length ? "The source line for this equation is unavailable." : "";
        const button = action("Open " + titleCase(side) + " reference: " + ref.label, "openReference", { pointer: ref.pointer, side }, reason, row.id);
        button.id = "reference-" + row.id + "-" + ref.pointer;
        button.textContent = ref.label;
        line.append(button);
        if (ref.scope.dimension && ref.scope.dimension !== shownScope) line.append(node("span", scopeLabel(ref.scope.dimension), "source-note"));
        list.append(line);
      }
    };
    toggle.addEventListener("click", () => {
      const sides = presentation.expandedDetails[row.id];
      presentation.expandedDetails[row.id] = sides.includes(side) ? sides.filter(value => value !== side) : [...sides, side];
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
  function renderRowDetail(row, detail) {
    if (!row.macroRows) { renderChangeDetail(row, detail, row.id); return; }
    for (const entry of row.macroRows) {
      const block = node("div", undefined, "macro-entry");
      if (entry.includeFile) block.append(node("h3", "Included file " + entry.kind, "field-label"));
      renderChangeDetail(entry, block, row.id);
      detail.append(block);
    }
  }
  function renderChangeDetail(row, detail, owner) {
    const sides = node("div", undefined, "sides");
    const card = row.commandRows ? combinedCommand(row) : row.historyRows?.length > 1 ? combinedHistory(row) : row;
    sides.append(sidePanel(card, "before", owner), sidePanel(card, "after", owner));
    detail.append(sides);
    if (card.includeFile) return;
    const instruction = card.semantic && writtenStatement(card.semantic);
    if (card.semantic?.family === "operations" && instruction?.before && instruction.after && instruction.before.text === instruction.after.text && card.semantic.fields.some(field => field.changed && !["role", "order"].includes(field.name))) detail.append(node("p", "The same instruction changes the model in different ways.", "source-note"));
    const notes = rowNotes(card);
    if (notes) detail.append(notes);
  }
  function commandName(row) {
    if (!row.semantic?.statement_group) return null;
    const context = row.semantic?.after?.context ?? row.semantic?.before?.context;
    return context?.kind === "command" ? context.name : null;
  }
  function combinedCommand(card) {
    const { commandRows, semantic } = card;
    const sides = ["before", "after"];
    const paired = pairedCommand(semantic.statement_group);
    const sideRows = Object.fromEntries(sides.map(side => [side, commandRows.find(row => row.semantic[side])]));
    const commandSides = Object.fromEntries(sides.map(side => [side, sideRows[side]?.semantic[side] ?? (paired ? semantic.before ?? semantic.after : null)]));
    const statements = commandRows.map(row => writtenStatement(row.semantic)).filter(Boolean);
    const statement = statements.find(expression => sides.every(side => !commandSides[side] || expression[side]));
    const fields = new Map(), expressions = new Map();
    for (const row of commandRows) {
      for (const field of row.semantic.fields) fields.set(JSON.stringify([field.name, field.before, field.after]), field);
      for (const expression of row.semantic.expressions) {
        if (expression.field === "statement_text") continue;
        expressions.set(JSON.stringify([expression.field, expression.before?.text, expression.after?.text]), expression);
      }
    }
    if (statement) expressions.set("statement_text", statement);
    else if (statements.length) {
      // These snippets describe one-sided facts inside a command pair. Only a
      // paired statement expression can highlight changes to the full command.
      const context = side => {
        const text = statements.find(item => item[side])?.[side].text;
        return text === undefined ? null : { text, runs: [{ text, role: "unchanged" }] };
      };
      expressions.set("statement_text", { field: "statement_text", before: context("before"), after: context("after"), highlight_basis: "none", availability: "complete" });
    }
    return {
      ...card,
      before: commandSides.before?.name ?? null, after: commandSides.after?.name ?? null,
      commandSources: Object.fromEntries(sides.map(side => [side, card.navigation[side]?.written_locations.length ? card : commandRows.find(row => row.navigation[side]?.written_locations.length) ??
        commandRows.find(row => row.navigation.block_openers?.[side]?.written_locations.length) ?? card])),
      semantic: { ...semantic, change: card.kind, ...commandSides,
        fields: [...fields.values()], expressions: [...expressions.values()], limits: commandRows.flatMap(row => row.semantic.limits),
        references: commandRows.flatMap(row => row.semantic.references) },
    };
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
      if (!history || !target || !statement || detail.limits.some(limit => limit.code === "occurrence_correspondence_unpaired") || detail.fields.some(field => field.changed && field.comparison_availability === "complete" && !["expression", "role", "target", "lag"].includes(field.name))) { cards.push(row); continue; }
      const parts = Object.fromEntries(["before", "after"].map(side => {
        const text = statement[side]?.text, match = text?.match(/^([^\n]*;\r?\n)([\s\S]*)(\r?\nend;)$/);
        return [side, match && { start: match[1].length, end: text.length - match[3].length, header: match[1], footer: match[3] }];
      }));
      if (["before", "after"].some(side => detail[side] && !parts[side])) { cards.push(row); continue; }
      const key = JSON.stringify([target, row.kind, ...["before", "after"].map(side => [detail[side]?.context, row.navigation[side]?.occurrence_id, row.navigation[side]?.written_locations.map(location => location.uri), parts[side]?.header, parts[side]?.footer])]);
      const existing = groups.get(key);
      if (!existing) {
        const card = { ...row, historyRows: [{ parts, statement }] };
        groups.set(key, card);
        cards.push(card);
        continue;
      }
      existing.label = target;
      existing.historyRows.push({ parts, statement });
    }
    return cards;
  }
  function combinedHistory(card) {
    const [first, ...periods] = card.historyRows, combined = { ...first.statement };
    for (const side of ["before", "after"]) {
      const expression = first.statement[side];
      if (!expression) continue;
      const end = first.parts[side].end;
      const runs = [
        sliceRuns(expression, 0, end),
        ...periods.map(({ parts, statement }) => [{ text: "\n", role: "unchanged" }, ...sliceRuns(statement[side], parts[side].start, parts[side].end)]),
        sliceRuns(expression, end, expression.text.length),
      ].flat();
      combined[side] = { text: runs.map(run => run.text).join(""), runs };
    }
    return { ...card, semantic: { ...card.semantic, expressions: [combined] } };
  }
  // Engine reasons describe the data contract. The view explains only what a
  // reader needs to know about the displayed changes, with no raw reason fallback.
  const comparisonNotes = {
    identifier_correspondence_unpaired: "Some symbol uses in this equation could not be matched between Before and After.",
    timing_correspondence_limit: "This equation exceeded the limit for comparing leads and lags.",
    statement_options_text_only: null, matched_irf_expression_positions: null,
    command_list_empty_occurrences_not_retained: null, type_result_history_association: null,
    prior_function_history_unavailable: null,
    equation_correspondence_unpaired: "This equation could not be matched between Before and After.",
    condition_correspondence_unpaired: "This condition could not be matched between Before and After.",
    occurrence_correspondence_unpaired: "This entry could not be matched between Before and After.",
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
    if (code === "identifier_correspondence_unpaired" && detail.limits.some(limit => limit.code === "timing_correspondence_limit")) return null;
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
    return [rowLabel(row), commandName(row), row.group, row.before ?? "", row.after ?? "", ...contextScopes(row).map(scopeLabel),
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
    kindCounts = {};
    for (const row of rows) {
      const key = row.section + ":" + row.group;
      kindCounts[row.kind] = (kindCounts[row.kind] ?? 0) + 1;
      row.summary = commandName(row) || ["commands", "macroContext"].includes(row.section) ? null : changeSummary(row);
      row.search = searchText(row);
      row.owner = ownerIdentity(row);
      if (!indexed.has(key)) indexed.set(key, { key, label: row.group, section: row.section, rows: [], commands: new Map() });
      const group = indexed.get(key);
      group.rows.push(row);
      const statement = row.semantic?.statement_group;
      if (statement && commandName(row)) {
        if (!group.commands.has(statement)) group.commands.set(statement, []);
        group.commands.get(statement).push(row);
      }
    }
    const ordered = [...indexed.values()].sort((left, right) => sections.indexOf(left.section) - sections.indexOf(right.section));
    for (const group of ordered) {
      group.parents = group.section === "parameters" ? parameterParents(group.rows) : new Map();
      for (const [key, commandRows] of group.commands) {
        const paired = pairedCommand(key);
        if (commandRows.length < 2 && !paired) { group.commands.delete(key); continue; }
        const primary = commandRows.find(row => row.kind === "changed" && writtenStatement(row.semantic)) ??
          commandRows.find(row => row.kind === "changed") ?? commandRows.find(row => writtenStatement(row.semantic)) ?? commandRows[0];
        const kind = paired || commandRows.some(row => row.kind !== primary.kind) ? "changed" : primary.kind;
        group.commands.set(key, { ...primary, kind, commandRows });
      }
    }
    return new Map(ordered.map(group => [group.key, group]));
  }

  function changeSummary(row) {
    const preview = text => text ? { text, valuePair: false } : null;
    const values = (field, label = "") => ({ text: label + fieldPair(field), valuePair: true });
    if (pairedParameter(row)) {
      const value = parameterField(row, "evaluated_value");
      if (value?.changed) return values(value);
      const expression = parameterField(row, "expression");
      if (expression?.changed) return values(expression);
    }
    const fact = row.semantic;
    if (!fact) return null;
    if (fact.family === "equations") {
      const expression = fact.expressions.find(expression => expression.field === "expression");
      if (row.kind !== "changed") {
        const side = row.kind === "added" ? "after" : "before";
        const field = fact.fields.find(field => field.name === "expression")?.[side];
        return preview(excerpt(expression?.[side] ?? (field && { text: fieldText(field), runs: [] }), false));
      }
      const timing = fact.timing.find(({ before, after }) => before && after && before.name === after.name &&
        before.written_offset !== after.written_offset);
      if (timing) {
        const use = item => item.name + (item.written_offset ? "(" + (item.written_offset > 0 ? "+" : "") + item.written_offset + ")" : "");
        return preview(use(timing.before) + " → " + use(timing.after));
      }
      const before = excerpt(expression?.before), after = excerpt(expression?.after);
      if (before && after && before !== after) return preview(before + " → " + after);
      if (fact.facets.includes("expression")) return preview("Expression changed");
      if (fact.facets.includes("timing")) return preview("Timing changed");
      if (fact.facets.includes("tags")) return preview("Tags changed");
    }
    if (row.kind !== "changed") return null;
    const fields = fact.fields.filter(field => field.changed && field.comparison_availability === "complete" &&
      !["form", "role", "target", "block", "domain", "order"].includes(field.name));
    const field = fields.find(field => ["irf", "values", "expression", "mean"].includes(field.name)) ?? fields[0];
    if (field) {
      if (field.name === "expression" && fact.family === "symbols" && localRole(fact)) return values(field);
      const measure = fact.family === "shocks" && (textField(fact, "measure", "after") || textField(fact, "measure", "before"));
      return values(field, (measure || field.label) + " ");
    }
    return null;
  }
  function fieldPair(field) {
    const display = side => side.state === "absent" ? "Not set" : side.state === "empty" ? "Empty" : fieldText(side);
    const before = display(field.before);
    return field.changed ? before + " → " + display(field.after) : before;
  }
  function excerpt(side, aroundChange = true) {
    if (!side?.text) return "";
    let first = 0;
    for (const run of aroundChange ? side.runs : []) {
      if (run.role !== "unchanged") break;
      first += run.text.length;
    }
    const text = side.text;
    let start = Math.max(0, (first === text.length ? 0 : first) - 12);
    if (start > 0 && /[\uDC00-\uDFFF]/.test(text[start])) start -= 1;
    let end = Math.min(text.length, start + 52);
    if (end < text.length && /[\uDC00-\uDFFF]/.test(text[end])) end += 1;
    return (start ? "…" : "") + side.text.slice(start, end).trim().replace(/\s+/g, " ") + (end < side.text.length ? "…" : "");
  }
  function compactRow(row, { child = false, context = false, dependents = [] } = {}) {
    const article = node("article", undefined, "change-item" + (child ? " dependent-row" : ""));
    article.setAttribute("data-row-id", row.id);
    const title = commandName(row) ?? rowLabel(row), summary = row.summary;
    const line = node("div", undefined, "change-line"), heading = node("h2", undefined, "row-heading");
    const hasValuePair = summary?.valuePair;
    const toggle = node("button", undefined, "change-row" + (hasValuePair ? " has-value-pair" : ""));
    toggle.type = "button";
    toggle.id = "row-" + row.id;
    toggle.title = title;
    toggle.tabIndex = -1;
    toggle.setAttribute("data-row-owner", row.id);
    toggle.setAttribute("aria-controls", "detail-" + row.id);
    const mark = node("span", marks[row.kind], "change-mark " + row.kind);
    mark.setAttribute("aria-hidden", "true");
    const twisty = node("span", undefined, "twisty");
    twisty.setAttribute("aria-hidden", "true");
    toggle.append(twisty, mark, node("span", changeLabel(row.kind) + " ", "sr-only"), node("span", title, "row-label"));
    const scope = contextScopes(row).map(scopeLabel).join(" · ");
    if (scope) toggle.append(node("span", scope, "row-scope"));
    if (row.historyRows?.length > 1) toggle.append(node("span", row.historyRows.length + " changed periods", "row-summary"));
    else if (summary) toggle.append(node("span", summary.text, "row-summary" + (hasValuePair ? " value-pair" : "")));
    if (!child && valueOnly(row)) toggle.append(node("span", "Value only", "row-note"));
    heading.append(toggle);
    line.append(heading);
    article.append(line);
    const detail = node("div");
    detail.id = "detail-" + row.id;
    article.append(detail);
    const showDetail = () => {
      const open = Object.hasOwn(presentation.expandedDetails, row.id);
      toggle.setAttribute("aria-expanded", String(open));
      detail.hidden = !open;
      if (open) {
        detail.className = "detail";
        renderRowDetail(row, detail);
      } else if (detail.className) {
        detail.className = "";
        detail.replaceChildren();
      }
    };
    const toggleDetail = () => {
      if (Object.hasOwn(presentation.expandedDetails, row.id)) delete presentation.expandedDetails[row.id];
      else presentation.expandedDetails[row.id] = [];
      presentation.returnFocus = null;
      showDetail();
      toggle.focus({ preventScroll: true });
      persist();
    };
    toggle.addEventListener("click", toggleDetail);
    showDetail();
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
    if (group.section === "macroContext") {
      const primary = group.rows[0], kind = shown.every(row => row.kind === shown[0].kind) ? shown[0].kind : "changed";
      const includes = group.rows.some(row => row.includeFile), macros = group.rows.some(row => !row.includeFile);
      const label = includes && macros ? "Macro and include changes" : includes ? "Include changes" : "Macro code changes";
      container.append(compactRow({ ...primary, label, kind, macroRows: shown }));
      return;
    }
    if (!group.parents.size) {
      const statements = new Set();
      for (const row of historyCards(shown)) {
        const key = row.semantic?.statement_group;
        const command = commandName(row) && group.commands.get(key);
        if (!command) { container.append(compactRow(row)); continue; }
        if (statements.has(key)) continue;
        statements.add(key);
        container.append(compactRow(command));
      }
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
      const rows = group.rows.filter(row => choices.changeKinds.includes(row.kind) && (!query || row.search.includes(query)));
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
      button.tabIndex = selected ? 0 : -1;
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
    const buttons = [...rows.querySelectorAll(".change-row")];
    const row = buttons.find(button => button.id === focusId || button.id === rowFocus) ?? buttons[0];
    if (row) row.tabIndex = 0;
    if (focus?.parentElement === list && !focus.disabled && focus !== selectedButton) {
      selectedButton.tabIndex = -1;
      focus.tabIndex = 0;
    }
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
    const compact = [message.inputRevisions?.[side], message.configurationLabels?.[side]].filter(Boolean);
    heading.append(node("small", label), node("code", sourceLabel));
    if (compact.length) heading.append(node("span", compact.join(" · "), "input-summary"));

    content.append(node("p", input, "model-path"));
    const includes = message.executedIncludes?.[side];
    if (includes?.length) {
      const list = node("ul", undefined, "executed-includes");
      for (const file of includes) list.append(node("li", file));
      content.append(node("p", "Executed includes", "analysis-detail"), list);
    }
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
    const summary = message.summary;
    const notices = [];
    for (const issue of summary?.issues ?? []) {
      if (["text_hunks", "pairing"].includes(issue.effect) || ["identifier_correspondence_unpaired", "timing_correspondence_limit"].includes(issue.code)) continue;
      const family = issue.family ? titleCase(issue.family) : "Model";
      if (issue.effect === "model_rows") notices.push(issue.omitted !== null && issue.code === "serialized_output_limit" ?
        countText(issue.omitted) + " omitted by the comparison limit." : family + " comparison is incomplete. " + issue.reason);
      else if (issue.effect === "references") notices.push("Some equation references were omitted.");
      else if (issue.effect === "highlights") notices.push(issue.family ? family + ": some changed text could not be highlighted." : "Some changed text could not be highlighted.");
      else if (issue.code === "captured_sources_required") notices.push("File text is unavailable.");
      else notices.push(issue.reason);
    }
    return { incompleteRows: summary?.semantic_rows === null, text: [...new Set(notices)].join(" ") };
  }
  function resultMessage(message, limits) {
    if (message.status !== "ready") return message.message;
    if (message.rows.length) return "";
    if (limits.incompleteRows) return "No model changes shown.";
    if (message.rootText === "changed") return "No model changes found. Open Text diff to review file edits.";
    return "No model changes found.";
  }
  function renderOutcome(message, limits) {
    controls.status.textContent = resultMessage(message, limits);
    controls.status.className = message.status;
    controls.status.hidden = !controls.status.textContent;
    controls.status.setAttribute("data-code", message.failureCode ?? "");
    const limit = document.getElementById("limitsStatus");
    if (limit) {
      limit.textContent = limits.text;
      limit.hidden = !limit.textContent;
    }
    const choosePath = document.getElementById("choosePath");
    if (choosePath) choosePath.hidden = message.status !== "failure" || !message.canChoosePath;
  }

  for (const name of ["choosePath", "help"]) document.getElementById(name)?.addEventListener("click", () => post(name));
  controls.search.addEventListener("input", () => {
    choices.search = controls.search.value;
    if (scrollArea) scrollArea.scrollTop = 0;
    update();
  });
  controls.kinds.addEventListener("keydown", event => {
    if (event.key === "Escape") { controls.kinds.open = false; controls.kinds.children[0]?.focus(); }
  });
  controls.results.addEventListener("keydown", event => {
    const movement = { ArrowLeft: -1, ArrowUp: -1, ArrowRight: 1, ArrowDown: 1 };
    const group = event.target.getAttribute("data-group"), owner = event.target.getAttribute("data-row-owner");
    if ((group || owner) && (Object.hasOwn(movement, event.key) || event.key === "Home" || event.key === "End")) {
      const buttons = [...controls.results.querySelectorAll(group ? ".group-button" : ".change-row")].filter(button => !button.disabled);
      const index = buttons.indexOf(event.target);
      const next = event.key === "Home" ? buttons[0] : event.key === "End" ? buttons.at(-1) : buttons[index + movement[event.key]];
      if (next) {
        event.target.tabIndex = -1;
        next.tabIndex = 0;
        next.focus();
      }
      event.preventDefault();
      return;
    }
    if (event.key !== "Escape") return;
    const id = owner ?? event.target.closest(".detail")?.parentElement?.getAttribute("data-row-id");
    if (!id || !Object.hasOwn(presentation.expandedDetails, id)) return;
    event.preventDefault();
    document.getElementById("row-" + id)?.click();
  });
  controls.results.addEventListener("focusin", event => {
    if (event.target.getAttribute("data-row-owner")) {
      const previous = rowFocus && document.getElementById(rowFocus);
      if (previous && previous !== event.target) previous.tabIndex = -1;
      rowFocus = event.target.id;
      event.target.tabIndex = 0;
    }
  });
  document.addEventListener?.("pointerdown", event => {
    if (!controls.kinds.contains(event.target)) controls.kinds.open = false;
  });
  window.addEventListener("message", event => {
    const message = event.data;
    if (message?.type === "findChanges") { controls.search.focus(); controls.search.select(); return; }
    if (!message || message.type !== "render" || !Array.isArray(message.rows)) return;

    const sameCapture = message.captureId && message.captureId === payload?.captureId;
    if (message.status === "loading" && payload?.captureId) {
      const focused = document.activeElement?.closest("[data-row-id]")?.getAttribute("data-row-id");
      refreshFocus = focused && groups.get(choices?.group)?.rows.find(row => row.id === focused)?.owner;
    }
    if (!message.captureId || message.captureId !== presentation.captureId) {
      presentation = { captureId: message.captureId, expandedDetails: {}, openDependents: [], inputDetails: [], returnFocus: null };
      groupList = null;
      if (scrollArea) scrollArea.scrollTop = 0;
    } else if (!presentation.expandedDetails) {
      const { detailId, referenceSides, ...rest } = presentation;
      presentation = { ...rest, expandedDetails: detailId ? { [detailId]: referenceSides ?? [] } : {} };
    }
    if (!sameCapture) {
      groups = indexGroups(message.rows);
      references = new Map((message.references ?? []).map(reference => [reference.pointer, reference]));
    }
    const limits = comparisonLimits(message);
    payload = { key: message.key, token: message.token, status: message.status, captureId: message.captureId, sourcePaths: message.sourcePaths,
      available: message.rows.length, semantic: Boolean(message.semantic), incompleteRows: limits.incompleteRows };
    choices = message.choices;
    const returning = message.active !== false && presentation.returnFocus;
    const focused = message.active === false ? "" : document.activeElement?.id;
    let refreshed;
    if (refreshFocus && message.status === "ready" && message.active !== false) {
      const matches = message.rows.filter(row => row.owner === refreshFocus);
      if (matches.length === 1) {
        const row = matches[0], group = groups.get(row.section + ":" + row.group);
        choices.group = row.section + ":" + row.group;
        const parent = group?.parents.get(row.id);
        if (parent) presentation.openDependents.push(parent);
        refreshed = "row-" + row.id;
      }
      refreshFocus = undefined;
    }
    controls.models.replaceChildren(inputDetails(message, "before"), inputDetails(message, "after"));
    renderOutcome(message, limits);
    renderControls();
    renderRows(returning?.id || refreshed || focused);
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
