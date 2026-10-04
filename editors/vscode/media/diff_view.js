/* global acquireVsCodeApi, document, window */
(() => {
  "use strict";
  const api = acquireVsCodeApi();
  const saved = api.getState();
  const names = {
    symbols: "Symbols", parameters: "Parameter values", aggregateEquations: "Aggregate equations",
    heterogeneousEquations: "Equations by dimension", shockSetup: "Shock setup", shockAnalysisSetup: "Shock analysis setup",
  };
  const kinds = ["added", "removed", "changed", "unpaired"];
  const controls = Object.fromEntries(["models", "status", "search", "scope", "layout", "expansion", "kinds", "sections", "counts", "results", "refresh"].map(id => [id, document.getElementById(id)]));
  let payload, choices;
  const node = (tag, text, className) => {
    const element = document.createElement(tag);
    if (text !== undefined) element.textContent = text;
    if (className) element.className = className;
    return element;
  };
  const remember = () => {
    api.setState({ key: payload.key, choices });
    api.postMessage({ type: "choices", key: payload.key, choices });
  };
  function update() { remember(); renderRows(); }
  function checklist(container, entries, selected, property) {
    container.replaceChildren(node("legend", property === "sections" ? "Sections" : "Change kinds"));
    for (const [value, text] of entries) {
      const label = node("label"), input = node("input"); input.type = "checkbox"; input.checked = selected.includes(value);
      input.addEventListener("change", () => {
        choices[property] = input.checked ? [...choices[property], value] : choices[property].filter(item => item !== value); update();
      });
      label.append(input, node("span", text)); container.append(label);
    }
  }
  function renderControls() {
    controls.search.value = choices.search; controls.layout.value = choices.layout; controls.expansion.value = choices.expansion;
    const scopes = [...new Set(payload.rows.flatMap(row => row.scopes))].sort();
    controls.scope.replaceChildren();
    for (const value of ["all", ...scopes]) {
      const option = node("option", value === "all" ? "All scopes" : value === "aggregate" ? "Aggregate" : `Dimension: ${value}`); option.value = value; controls.scope.append(option);
    }
    if (choices.scope !== "all" && !scopes.includes(choices.scope)) {
      const option = node("option", `Dimension: ${choices.scope} (no rows)`); option.value = choices.scope; controls.scope.append(option);
    }
    controls.scope.value = choices.scope;
    checklist(controls.kinds, kinds.map(kind => [kind, kind[0].toUpperCase() + kind.slice(1)]), choices.changeKinds, "changeKinds");
    checklist(controls.sections, Object.entries(names), choices.sections, "sections");
  }
  function sidePanel(row, side) {
    const value = row[side], target = row.navigation[side], panel = node("div", undefined, `side ${side}`);
    const title = node("div", undefined, "side-title");
    title.append(node("strong", side === "before" ? "Before" : "After"));
    const dimension = row.sideScopes[side];
    const scope = dimension === null ? "" : dimension === "aggregate" ? "Aggregate" : `Dimension: ${dimension}`;
    if (scope) title.append(node("span", scope, "scope-label"));
    const button = node("button", "Open source"); button.type = "button";
    const reason = value === null ? "This row is absent on this side." : payload.status !== "ready" ? "Refresh the comparison to enable source actions." : !target?.written_locations.length ? "No verified written location is available for this row." : "";
    button.disabled = !!reason;
    if (reason) { button.title = reason; button.setAttribute("aria-label", `Open ${side} source unavailable: ${reason}`); }
    else button.setAttribute("aria-label", `Open ${side} source for ${row.label}`);
    button.addEventListener("click", () => api.postMessage({ type: "openSource", token: payload.token, rowId: row.id, side }));
    title.append(button); panel.append(title, node("pre", value === null ? "Not present" : value));
    if (reason && value !== null) panel.append(node("p", reason, "source-note"));
    return panel;
  }
  function renderRows() {
    if (!payload || !choices) return;
    controls.results.className = `results layout-${choices.layout}`;
    controls.results.replaceChildren();
    const available = payload.rows.filter(row => choices.sections.includes(row.section));
    const query = choices.search.trim().toLocaleLowerCase();
    const shown = available.filter(row => choices.changeKinds.includes(row.kind) &&
      (choices.scope === "all" || row.scopes.includes(choices.scope)) &&
      (!query || [row.label, row.group, row.before ?? "", row.after ?? ""].join("\n").toLocaleLowerCase().includes(query)));
    const breakdown = kinds.map(kind => `${shown.filter(row => row.kind === kind).length} ${kind}`).join(" · ");
    controls.counts.textContent = `${shown.length} of ${available.length} rows shown · ${breakdown}`;
    if (!shown.length) {
      const message = !choices.sections.length ? "All sections are hidden. Choose a section to show its rows." :
        ["failure", "incomplete", "loading"].includes(payload.status) ? "" : payload.rows.length ? "No rows match these filters." : payload.status === "ready" ? "No structural changes." : "Refresh to compare current inputs.";
      if (message) controls.results.append(node("p", message));
      return;
    }
    const groups = new Map();
    for (const row of shown) {
      const key = `${row.section}:${row.group}`;
      if (!groups.has(key)) groups.set(key, { label: row.group, rows: [] });
      groups.get(key).rows.push(row);
    }
    for (const [key, group] of groups) {
      const details = node("details"), summary = node("summary", `${group.label} (${group.rows.length})`);
      details.open = choices.expanded[key] ?? choices.expansion !== "none";
      details.append(summary);
      details.addEventListener("toggle", () => {
        if (choices.expanded[key] === details.open) return;
        choices.expanded[key] = details.open; remember();
      });
      for (const row of group.rows) {
        const article = node("article", undefined, `change ${row.kind}`), header = node("h2");
        header.append(node("span", row.kind, "badge"), node("span", row.label)); article.append(header);
        const sides = node("div", undefined, "sides"); sides.append(sidePanel(row, "before"), sidePanel(row, "after")); article.append(sides); details.append(article);
      }
      controls.results.append(details);
    }
  }
  controls.refresh.addEventListener("click", () => api.postMessage({ type: "refresh" }));
  document.getElementById("help").addEventListener("click", () => api.postMessage({ type: "help" }));
  controls.search.addEventListener("input", () => { choices.search = controls.search.value; update(); });
  controls.scope.addEventListener("change", () => { choices.scope = controls.scope.value; update(); });
  controls.layout.addEventListener("change", () => { choices.layout = controls.layout.value; update(); });
  controls.expansion.addEventListener("change", () => { choices.expansion = controls.expansion.value; choices.expanded = {}; update(); });
  window.addEventListener("message", event => {
    const message = event.data;
    if (!message || message.type !== "render" || !Array.isArray(message.rows)) return;
    payload = message; choices = message.choices;
    controls.models.replaceChildren();
    for (const [label, uri] of [["Before", message.before], ["After", message.after]]) {
      const model = node("p"); model.append(node("strong", `${label}: `), node("code", uri)); controls.models.append(model);
    }
    controls.status.textContent = message.message; controls.status.className = message.status;
    controls.refresh.disabled = message.status === "loading";
    renderControls(); renderRows(); api.setState({ key: payload.key, choices });
  });
  api.postMessage({ type: "ready", key: saved?.key, choices: saved?.choices });
})();
