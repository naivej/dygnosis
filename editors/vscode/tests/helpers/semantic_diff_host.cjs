// Verify semantic comparison controls on the rendered native VS Code webview.
const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const { Buffer } = require("node:buffer");
const { URL } = require("node:url");
const { execFile } = require("node:child_process");
const { promisify } = require("node:util");
const vscode = require("vscode");
const { nativeChanges, nativeWorkbench } = require("./native_webview.cjs");
const { resourceName, resourceQuery, changesViewType } = require("../../out/changes_resource");

async function captureFailure(evidence) {
  evidence.semantic_tabs = vscode.window.tabGroups.all.map(group => ({
    active: group.activeTab?.label,
    tabs: group.tabs.map(tab => ({ label: tab.label, viewType: tab.input.viewType, path: tab.input.uri?.path })),
  }));
  try {
    await nativeWorkbench(async send => {
      const state = await send("Runtime.evaluate", {
        expression: '({width:innerWidth,height:innerHeight,visible:document.visibilityState,iframes:[...document.querySelectorAll("iframe")].map(f=>({src:f.src,title:f.title,rect:JSON.stringify(f.getBoundingClientRect()),display:getComputedStyle(f).display}))})', returnByValue: true,
      });
      evidence.semantic_workbench = state.result?.value;
      const screenshot = await send("Page.captureScreenshot", { format: "png", captureBeyondViewport: false });
      const filename = path.join(path.dirname(process.env.DYGNOSIS_HOST_RESULT), `semantic-failure-${vscode.version}.png`);
      await fs.writeFile(filename, Buffer.from(screenshot.data, "base64"));
      evidence.semantic_failure_screenshot = filename;
    });
  } catch (error) { evidence.semantic_failure_capture_error = String(error.message); }
}

async function checkSemanticDiff(service, workspaceRoot, evidence, waitFor) {
  if (process.env.DYGNOSIS_HOST_REQUIRED_VISUAL !== "1") return;
  const config = vscode.workspace.getConfiguration("dynare");
  const saved = Object.fromEntries(["diff.defaultChangeKinds"].map(key => [key, config.inspect(key)?.workspaceValue]));
  try {
    await config.update("diff.defaultChangeKinds", ["added", "changed", "unpaired"], vscode.ConfigurationTarget.Workspace);
    await checkEquationSurgerySource(service, workspaceRoot, evidence, waitFor);
    await checkForecastSource(service, workspaceRoot, evidence, waitFor);
    await runSemanticDiff(service, workspaceRoot, evidence, waitFor);
    await checkHistoryCardSource(service, workspaceRoot, evidence, waitFor);
  } finally {
    for (const [key, value] of Object.entries(saved)) await config.update(key, value, vscode.ConfigurationTarget.Workspace);
  }
}

async function checkEquationSurgerySource(service, workspaceRoot, evidence, waitFor) {
  const directory = path.join(workspaceRoot, "equation-surgery-source");
  await fs.mkdir(directory, { recursive: true });
  const beforeFile = path.join(directory, "equation_surgery_1.mod"), afterFile = path.join(directory, "equation_surgery_2.mod");
  const beforeText = "var c;\nmodel;\n[name='Consumption'] c=.8;\nend;\n";
  const instruction = "model_replace( 'Consumption' );\n  // retain original spacing and comments\n  [name='Consumption'] c = .8;\nend;";
  await fs.writeFile(beforeFile, beforeText);
  await fs.writeFile(afterFile, beforeText.replace("c=.8", "c=1") + instruction + "\n");
  const after = { kind: "working", root_uri: vscode.Uri.file(afterFile).toString() };
  const document = await vscode.workspace.openTextDocument(vscode.Uri.file(afterFile));
  await vscode.window.showTextDocument(document);
  await waitFor(async () => (await service.modelInfo(document.uri))?.complete, "replacement model facts");
  const resource = { schema_version: 1, before: { kind: "working", root_uri: vscode.Uri.file(beforeFile).toString() }, after, anchor: after, context_uri: after.root_uri };
  const uri = vscode.Uri.from({ scheme: "dygnosis-changes", path: `/${resourceName(resource)}`, query: resourceQuery(resource) });
  await vscode.commands.executeCommand("vscode.openWith", uri, changesViewType, { viewColumn: vscode.ViewColumn.One, preview: false });
  await waitFor(async () => {
    try { return await nativeChanges(async ({ evaluate }) => { evidence.semantic_surgery_state = await evaluate('({status:document.getElementById("status").className,message:document.getElementById("status").textContent,groups:[...document.querySelectorAll(".group-button")].map(node=>node.textContent),code:document.querySelector(".side.after .expression")?.textContent,models:document.getElementById("models").textContent})'); return evidence.semantic_surgery_state.status === "ready" && Boolean(evidence.semantic_surgery_state.code); }); }
    catch (error) { if (error.code === "NATIVE_FRAME_PENDING") { evidence.semantic_surgery_discovery = error.discovery; return false; } throw error; }
  }, "written replacement card");
  await nativeChanges(async ({ evaluate, send }) => {
    assert.deepEqual(await evaluate('[...document.querySelectorAll(".group-button > span:first-child")].map(node=>node.textContent)'), ["Operations"]);
    assert.equal(await evaluate('document.querySelector(".side.after .expression").textContent'), instruction);
    assert.equal(await evaluate('document.querySelectorAll("article [data-field]").length'), 0, "operation records do not become displayed model syntax");
    const screenshot = await send("Page.captureScreenshot", { format: "png", captureBeyondViewport: false });
    const filename = path.join(path.dirname(process.env.DYGNOSIS_HOST_RESULT), `semantic-equation-surgery-${vscode.version}.png`);
    await fs.writeFile(filename, Buffer.from(screenshot.data, "base64")); evidence.semantic_equation_surgery_screenshot = filename;
  });
  evidence.semantic_equation_surgery_written_source = true;
}

async function checkForecastSource(service, workspaceRoot, evidence, waitFor) {
  await vscode.commands.executeCommand("workbench.action.closeAllEditors");
  const directory = path.join(workspaceRoot, "forecast-source"); await fs.mkdir(directory, { recursive: true });
  const beforeFile = path.join(directory, "forecast_groups_1.mod"), afterFile = path.join(directory, "forecast_groups_2.mod");
  const beforeText = "var y pi; varexo e u; model; y=e; pi=u; end;\nshock_groups(name=drivers);\ndemand=e;\nsupply=u;\nend;\nconditional_forecast_paths;\nvar y;\nperiods 1 2 3;\nvalues 0.1 0.25 0.1;\nvar pi;\nperiods 1 2 3;\nvalues 0.5 0.5 0.5;\nend;";
  await fs.writeFile(beforeFile, beforeText);
  await fs.writeFile(afterFile, beforeText.replace("demand=e;\nsupply=u;", "joint=e u;").replace("values 0.1 0.25 0.1;", "values 0.2 0.25 0.1;"));
  const beforeDocument = await vscode.workspace.openTextDocument(vscode.Uri.file(beforeFile));
  await vscode.window.showTextDocument(beforeDocument);
  await waitFor(async () => { const facts = await service.modelInfo(beforeDocument.uri); evidence.semantic_forecast_before = facts ? { complete: facts.complete, root_uri: facts.root_uri } : null; return facts?.complete; }, "forecast Before model facts");
  const after = { kind: "working", root_uri: vscode.Uri.file(afterFile).toString() }, document = await vscode.workspace.openTextDocument(vscode.Uri.file(afterFile));
  await vscode.window.showTextDocument(document);
  await waitFor(async () => (await service.modelInfo(document.uri))?.complete, "forecast model facts");
  const resource = { schema_version: 1, before: { kind: "working", root_uri: vscode.Uri.file(beforeFile).toString() }, after, anchor: after, context_uri: after.root_uri };
  const uri = vscode.Uri.from({ scheme: "dygnosis-changes", path: `/${resourceName(resource)}`, query: resourceQuery(resource) });
  await vscode.commands.executeCommand("vscode.openWith", uri, changesViewType, { viewColumn: vscode.ViewColumn.One, preview: false });
  await waitFor(async () => {
    try { return await nativeChanges(async ({ evaluate }) => { evidence.semantic_forecast_state = await evaluate('({status:document.getElementById("status").className,message:document.getElementById("status").textContent,groups:[...document.querySelectorAll(".group-button")].map(node=>node.textContent),models:document.getElementById("models").textContent})'); return evidence.semantic_forecast_state.status === "ready" && evidence.semantic_forecast_state.groups.length > 0; }); }
    catch (error) { if (error.code === "NATIVE_FRAME_PENDING") return false; throw error; }
  }, "forecast source cards");
  await nativeChanges(async ({ evaluate, send }) => {
    assert.equal(await evaluate('document.querySelector("#kinds summary").textContent'), "Type of changes");
    assert.equal(await evaluate('[...document.querySelectorAll(".token.added,.token.removed")].some(node=>node.textContent.includes("name=drivers"))'), false);
    await evaluate('[...document.querySelectorAll(".group-button")].find(button=>button.querySelector("span").textContent==="MS-SBVAR").click()');
    assert.equal(await evaluate('document.querySelector(".side.after .expression").textContent'), "conditional_forecast_paths;\nvar y;\nperiods 1 2 3;\nvalues 0.2 0.25 0.1;\nend;");
    assert.deepEqual(await evaluate('[...document.querySelectorAll(".side.after .token.added")].map(node=>node.textContent)'), ["0.2"]);
    assert.equal(await evaluate('document.querySelectorAll("article [data-field]").length'), 0);
    const screenshot = await send("Page.captureScreenshot", { format: "png", captureBeyondViewport: false });
    const filename = path.join(path.dirname(process.env.DYGNOSIS_HOST_RESULT), `semantic-forecast-source-${vscode.version}.png`);
    await fs.writeFile(filename, Buffer.from(screenshot.data, "base64")); evidence.semantic_forecast_source_screenshot = filename;
  });
  evidence.semantic_forecast_written_source = true;
}

async function checkHistoryCardSource(service, workspaceRoot, evidence, waitFor) {
  const directory = path.join(workspaceRoot, "history-card-source");
  await fs.mkdir(directory, { recursive: true });
  const git = async (...args) => (await promisify(execFile)("git", args, { cwd: directory, windowsHide: true })).stdout.trim();
  await git("init", "--quiet");
  await git("config", "user.name", "Dygnosis test"); await git("config", "user.email", "test@example.invalid");
  const filename = path.join(directory, "root.mod"), beforeText = "var y;\nmodel;\ny=1;\nend;\nvarexo e u;\nshocks; var e=.1; var u=.2; end;\n";
  await fs.writeFile(filename, beforeText); await git("add", "root.mod"); await git("commit", "-qm", "Before source");
  const commit = await git("rev-parse", "HEAD");
  const afterText = beforeText.replace("var y;", "var y xxx;\npredetermined_variables xxx;").replace("model;", "model;\n# helper=2;").replace("y=1", "y=xxx+2").replace("var e=.1; var u=.2;", "var e=.3;");
  await fs.writeFile(filename, afterText);
  await vscode.commands.executeCommand("workbench.action.closeAllEditors");
  const root = vscode.Uri.file(filename), document = await vscode.workspace.openTextDocument(root);
  await vscode.window.showTextDocument(document);
  await waitFor(async () => { const info = await service.modelInfo(root); return info?.complete && info.document_version === document.version; }, "HEAD/Working model capture");
  const after = { kind: "working", root_uri: root.toString() };
  const resource = { schema_version: 1, before: { kind: "git", repository_uri: vscode.Uri.file(directory).toString(), root_file: "root.mod", commit, requested_ref: "HEAD" }, after, anchor: after, context_uri: root.toString() };
  const uri = vscode.Uri.from({ scheme: "dygnosis-changes", path: `/${resourceName(resource)}`, query: resourceQuery(resource) });
  await vscode.commands.executeCommand("vscode.openWith", uri, changesViewType, { viewColumn: vscode.ViewColumn.One, preview: false });
  const ready = () => waitFor(async () => {
    try { return await nativeChanges(async ({ evaluate }) => evaluate('document.getElementById("status").className === "ready" && Boolean(document.querySelector(".side.before .source-link:not(:disabled)"))')); }
    catch (error) { if (error.code === "NATIVE_FRAME_PENDING") return false; throw error; }
  }, "HEAD/Working Before source is enabled");
  try {
    await ready();
    await nativeChanges(async ({ evaluate }) => {
      await evaluate('document.querySelector(".side.before .source-link").click()');
    });
    await waitFor(() => vscode.window.activeTextEditor?.document.uri.scheme === "dygnosis-history" && vscode.window.activeTextEditor.selection.start.line === 0, "added variable Before card opens HEAD file");
    assert.equal(vscode.window.activeTextEditor.document.getText(), beforeText);
    assert.equal(vscode.window.activeTextEditor.document.languageId, "dynare");
    await vscode.commands.executeCommand("vscode.openWith", uri, changesViewType, { viewColumn: vscode.ViewColumn.One, preview: false });
    await ready();
    await nativeChanges(async ({ evaluate, send }) => {
      assert.equal(await evaluate('document.querySelector(".side.before").textContent.includes("Not present")'), true);
      assert.equal(await evaluate('document.querySelector(".side.after").textContent.includes("var xxx;")'), true);
      assert.equal(await evaluate('document.querySelector(".side.after").textContent.includes("Written declaration kind")'), false);
      assert.equal(await evaluate('document.querySelectorAll("article[data-row-id]").length'), 1, "the convention belongs to the symbol card");
      assert.equal(await evaluate('document.querySelector(".side.after").textContent.includes("Predetermined convention")'), true);
      assert.equal(await evaluate('document.querySelector(".side.after .refs").textContent.includes("after convention")'), false);
      assert.equal(await evaluate('[...document.querySelectorAll(".side.after .expression .token.added")].map(token=>token.textContent).join("")'), "xxx");
      assert.equal(await evaluate('[...document.querySelectorAll(".group-button")].some(button=>button.textContent.includes("Model-local variables"))'), true);
      assert.equal(await evaluate('[...document.querySelectorAll(".group-button")].some(button=>button.textContent.includes("Commands"))'), false, "local insertion leaves no separator-only Commands row");
      const screenshot = await send("Page.captureScreenshot", { format: "png", captureBeyondViewport: false });
      const image = path.join(path.dirname(process.env.DYGNOSIS_HOST_RESULT), `semantic-added-symbol-${vscode.version}.png`);
      await fs.writeFile(image, Buffer.from(screenshot.data, "base64")); evidence.semantic_added_symbol_screenshot = image;
      await evaluate('document.querySelector(".side.after .source-link").click()');
    });
    await waitFor(() => vscode.window.activeTextEditor?.document.uri.toString() === root.toString() && vscode.window.activeTextEditor.selection.start.line === 0, "added variable After card opens Working declaration");
    assert.equal(vscode.window.activeTextEditor.document.getText(), afterText);
    await vscode.commands.executeCommand("vscode.openWith", uri, changesViewType, { viewColumn: vscode.ViewColumn.One, preview: false });
    await ready();
    await nativeChanges(async ({ evaluate, send }) => {
      await evaluate('const all=document.querySelector("[data-change-type=all]"); if (!all.checked) all.click();');
      await evaluate('[...document.querySelectorAll(".group-button")].find(button=>button.querySelector("span").textContent==="Model-local variables").click()');
      assert.equal(await evaluate('[...document.querySelectorAll(".side.after .expression .token.added")].map(token=>token.textContent).join("")'), "# helper = 2;");
      await evaluate('[...document.querySelectorAll(".group-button")].find(button=>button.querySelector("span").textContent==="Equations").click()');
      assert.equal(await evaluate('document.querySelector(".group-rows").textContent.includes("after convention")'), false);
      await evaluate('[...document.querySelectorAll(".group-button")].find(button=>button.querySelector("span").textContent==="Shock setup").click()');
      assert.equal(await evaluate('document.querySelectorAll("article[data-row-id]").length'), 2);
      assert.equal(await evaluate('document.querySelectorAll("article [data-field]").length'), 0);
      assert.equal(await evaluate('document.querySelector(".side.before .expression").textContent.includes("var e=.1;")'), true);
      assert.equal(await evaluate('document.querySelector(".side.after .expression").textContent.includes("var e=.3;")'), true);
      assert.equal(await evaluate('[...document.querySelectorAll(".expression .token.added, .expression .token.removed")].some(token=>/shocks|\\bend\\b/.test(token.textContent))'), false);
      assert.equal(await evaluate('[...document.querySelectorAll(".expression .token.removed")].some(token=>token.textContent.includes("var u=.2;"))'), true);
      const screenshot = await send("Page.captureScreenshot", { format: "png", captureBeyondViewport: false });
      const image = path.join(path.dirname(process.env.DYGNOSIS_HOST_RESULT), `semantic-shocks-${vscode.version}.png`);
      await fs.writeFile(image, Buffer.from(screenshot.data, "base64")); evidence.semantic_shock_screenshot = image;
    });
    evidence.semantic_shock_cards = true;
    evidence.semantic_head_source = true;
  } catch (error) { await captureFailure(evidence); throw error; }
}

async function runSemanticDiff(service, workspaceRoot, evidence, waitFor) {
  const roots = {};
  for (const [side, symbol, value, mean, order, irf, local, equations] of [
    ["before", "x", ".5", ".8", 1, 0, "a*y(-1)", "k=y;\n[name='Ambiguous'] x=1;"],
    ["after", "z", ".6", ".9", 2, 10, "a*y", "k=y+1;\n[name='Ambiguous'] z=2;"],
  ]) {
    const directory = path.join(workspaceRoot, `semantic-${side}`);
    await fs.mkdir(directory, { recursive: true });
    const root = path.join(directory, "root.mod"), body = path.join(directory, "body.mod");
    await fs.writeFile(root, `var y k ${symbol}; varexo e; parameters a; a=${value};\nmodel;\n@#include "body.mod"\nend;\nestimated_params; a,beta_pdf,${mean},.1; end;\nstoch_simul(order=${order},irf=${irf});\n`);
    await fs.writeFile(body, `// Captured ${side} include 🧭\n# forecast=${local};\n[name='Output'] y=forecast+e;\n[name='Ambiguous'] ${equations}\n`);
    const document = await vscode.workspace.openTextDocument(root);
    await vscode.window.showTextDocument(document);
    await waitFor(async () => {
      const info = await service.modelInfo(document.uri);
      return info?.complete && info.document_version === document.version;
    }, `native semantic ${side} input capture`);
    roots[side] = document.uri;
  }
  const before = { kind: "working", root_uri: roots.before.toString() };
  const after = { kind: "working", root_uri: roots.after.toString() };
  const resource = { schema_version: 1, before, after, anchor: after, context_uri: roots.after.toString() };
  const uri = vscode.Uri.from({ scheme: "dygnosis-changes", path: `/${resourceName(resource)}`, query: resourceQuery(resource) });
  await vscode.commands.executeCommand("workbench.action.joinAllGroups");
  await vscode.commands.executeCommand("workbench.action.closeSidebar");
  if ((await vscode.commands.getCommands(true)).includes("workbench.action.closeAuxiliaryBar")) {
    await vscode.commands.executeCommand("workbench.action.closeAuxiliaryBar");
  }
  await vscode.commands.executeCommand("vscode.openWith", uri, changesViewType, { viewColumn: vscode.ViewColumn.One, preview: false });
  try {
    await waitFor(async () => {
      try {
        return await nativeChanges(async ({ evaluate }) => {
          const state = await evaluate('({rows:Boolean(document.querySelector("article[data-row-id]")),status:document.getElementById("status").className,message:document.getElementById("status").textContent})');
          assert.notEqual(state.status, "failure", `Native Changes failed: ${state.message}`);
          return state.rows;
        });
      }
      catch (error) { if (error.code === "NATIVE_FRAME_PENDING") { evidence.semantic_context_discovery = error.discovery; return false; } throw error; }
    }, "native semantic Changes rows");
    delete evidence.semantic_context_discovery;
  } catch (error) {
    await captureFailure(evidence);
    throw error;
  }
  const checks = [];
  await nativeChanges(async ({ send, evaluate, refreshContext }) => {
    const regainChanges = () => waitFor(async () => {
      try {
        await refreshContext(); delete evidence.semantic_context_discovery;
        return await evaluate('document.getElementById("status").className === "ready" && Boolean(document.querySelector("article[data-row-id]"))');
      }
      catch (error) {
        if (error.code !== "NATIVE_FRAME_PENDING") throw error;
        evidence.semantic_context_discovery = error.discovery; return false;
      }
    }, "native Changes frame after editor action");
    const key = async (name, code, virtualKey) => {
      const text = name === "Enter" ? "\r" : name === " " ? " " : undefined;
      await send("Input.dispatchKeyEvent", { type: "keyDown", key: name, code, windowsVirtualKeyCode: virtualKey, ...(text ? { text, unmodifiedText: text } : {}) });
      await send("Input.dispatchKeyEvent", { type: "keyUp", key: name, code, windowsVirtualKeyCode: virtualKey });
    };
    const click = selector => evaluate(`(() => {
      const control = document.querySelector(${JSON.stringify(selector)});
      if (!control || !control.getClientRects().length || control.disabled) throw new Error("Missing enabled visible native control: " + ${JSON.stringify(selector)});
      control.click();
    })()`);
    const textDiff = async filename => {
      await click("#capturedTextDiff");
      let picker;
      await waitFor(async () => nativeWorkbench(async send => {
        const state = await send("Runtime.evaluate", { expression: 'Array.from(document.querySelectorAll(".quick-input-list .monaco-list-row")).filter(row=>row.getClientRects().length).map(row=>row.textContent)', returnByValue: true });
        picker = state.result?.value ?? [];
        return picker.some(label => label.includes("semantic-after") && label.includes(filename));
      }), "native captured-file picker");
      const choice = picker.findIndex(label => label.includes("semantic-after") && label.includes(filename));
      await nativeWorkbench(async send => {
        const result = await send("Runtime.evaluate", { expression: `(() => { const row=Array.from(document.querySelectorAll(".quick-input-list .monaco-list-row")).filter(row=>row.getClientRects().length)[${choice}]; const rect=row.getBoundingClientRect(); return {x:rect.x + rect.width / 2,y:rect.y + rect.height / 2}; })()`, returnByValue: true });
        const point = result.result.value;
        await send("Input.dispatchMouseEvent", { type: "mousePressed", button: "left", clickCount: 1, ...point });
        await send("Input.dispatchMouseEvent", { type: "mouseReleased", button: "left", clickCount: 1, ...point });
      });
      await waitFor(() => vscode.window.tabGroups.activeTabGroup.activeTab?.input instanceof vscode.TabInputTextDiff, "native captured text diff");
      return vscode.window.tabGroups.activeTabGroup.activeTab;
    };

    assert.equal(await evaluate('Boolean(document.getElementById("presentation") || document.getElementById("layout") || document.getElementById("expansion") || document.querySelector("[role=tablist], .change-tree, .selected-detail"))'), false);
    assert.equal(await evaluate('Boolean(document.getElementById("comparisonLimits"))'), false);
    const header = await evaluate('({cards:[...document.querySelectorAll("#models .input-side")].map(card=>({label:card.querySelector("small").textContent,name:card.querySelector("code").textContent,title:card.title})),kinds:document.getElementById("kinds").tagName,checkboxes:document.querySelectorAll("input[type=checkbox],fieldset").length,legend:[...document.querySelectorAll("footer .change-mark")].map(mark=>mark.parentElement.textContent.trim())})');
    assert.deepEqual(header.cards.map(card => [card.label, card.name]), [["Before", "root.mod"], ["After", "root.mod"]]);
    assert.ok(header.cards[0].title.includes(decodeURIComponent(new URL(roots.before.toString()).pathname)));
    assert.ok(header.cards[1].title.includes(decodeURIComponent(new URL(roots.after.toString()).pathname)));
    assert.equal(header.kinds, "DETAILS"); assert.equal(header.checkboxes, 5);
    const placement = await evaluate('({models:document.getElementById("models").getBoundingClientRect().right,change:document.getElementById("changeComparison").getBoundingClientRect().x,swap:document.getElementById("swap").getBoundingClientRect().right,refresh:document.getElementById("refresh").getBoundingClientRect().x})');
    assert.ok(placement.change >= placement.models && placement.refresh > placement.swap);
    assert.deepEqual(header.legend, ["+ Added", "− Removed", "~ Replaced", "? Unpaired"]);
    assert.equal(await evaluate('document.querySelector("[data-change-type=all]").indeterminate'), true);
    const selectTypes = selected => evaluate(`(() => { const selected=${JSON.stringify(selected)}; for (const kind of ["added","removed","changed","unpaired"]) { const input=document.querySelector('[data-change-type="'+kind+'"]'); if (input.checked !== selected.includes(kind)) input.click(); } })()`);
    await selectTypes(["changed"]);
    assert.equal(await evaluate('[...document.querySelectorAll("article[data-row-id]")].every(row=>row.classList.contains("changed"))'), true);
    await selectTypes(["added", "changed", "unpaired"]);
    assert.equal(await evaluate('Boolean(document.querySelector("article.removed"))'), false);
    await evaluate('document.querySelector("[data-change-type=all]").click()');
    await evaluate('[...document.querySelectorAll(".group-button")].find(button=>button.querySelector("span").textContent==="Parameters").click()');
    assert.equal(await evaluate('document.querySelectorAll("article[data-row-id]").length'), 1);
    assert.equal(await evaluate('Boolean(document.getElementById("scope") || document.getElementById("sectionFilter") || document.getElementById("section-all"))'), false);
    assert.match(await evaluate('document.getElementById("counts").textContent'), /15 of 15 model rows match filters/);
    const pairedCards = () => evaluate('[...document.querySelectorAll("article[data-row-id]")].map(row=>{const before=row.querySelector(".side.before").getBoundingClientRect(),after=row.querySelector(".side.after").getBoundingClientRect();return {bx:before.x,by:before.y,bw:before.width,ax:after.x,ay:after.y,aw:after.width};})');
    const checkPairedCards = async () => { const cards = await pairedCards(), count = await evaluate('Number(document.querySelector(".group-button.selected .group-count").textContent)'); assert.equal(cards.length, count); for (const card of cards) { assert.equal(card.by, card.ay); assert.ok(card.ax > card.bx && card.bw > 0 && card.aw > 0); } return cards; };
    const group = label => evaluate(`(() => {const button=[...document.querySelectorAll(".group-button")].find(button=>button.querySelector("span").textContent===${JSON.stringify(label)});if (!button) throw new Error("Missing change group");button.click();})()`);
    const groupLabels = await evaluate('[...document.querySelectorAll(".group-button")].map(button=>button.querySelector("span").textContent)');
    const visited = [];
    for (const label of groupLabels) {
      await group(label); await checkPairedCards(); visited.push(...await evaluate('[...document.querySelectorAll("article[data-row-id]")].map(row=>row.getAttribute("data-row-id"))'));
      const notes = await evaluate('({withinRows:document.querySelectorAll("article .limits").length,text:[...document.querySelectorAll(".group-notes li")].map(note=>note.textContent),last:document.querySelector(".group-rows").lastElementChild.className})');
      assert.equal(notes.withinRows, 0); assert.equal(notes.text.length, new Set(notes.text).size);
      if (notes.text.length) assert.equal(notes.last, "group-notes");
      if (label === "Commands") assert.equal(await evaluate('document.querySelector(".group-rows").textContent.includes("Unclaimed accepted tokens")'), false);
    }
    assert.equal(new Set(visited).size, 15, "all filtered changes are available as full rows in their groups");
    evidence.semantic_group_rows = { groups: groupLabels, rows: visited.length };
    await group("Equations");
    evidence.semantic_compact_controls = header;
    evidence.semantic_saved_filters = true;
    const legend = await evaluate('(() => {const content=document.getElementById("reviewContent"),footer=document.querySelector("footer"),before=footer.getBoundingClientRect().top;content.scrollTop=content.scrollHeight;return {before,after:footer.getBoundingClientRect().top,bottom:footer.getBoundingClientRect().bottom,height:innerHeight,scroll:content.scrollTop};})()');
    assert.equal(legend.before, legend.after);
    assert.ok(legend.scroll > 0, "the legend check scrolls real change rows");
    assert.ok(Math.abs(legend.bottom - legend.height) < 1, "the legend stays at the viewport bottom");
    await evaluate('document.getElementById("reviewContent").scrollTop=0');
    evidence.semantic_frozen_legend = legend;
    checks.push("single model view");
    await group("Symbols");
    await evaluate('document.querySelector(".group-button").focus()');
    await key("Tab", "Tab", 9);
    await key("Enter", "Enter", 13);
    assert.equal(await evaluate('document.querySelector(".group-button.selected > span").textContent'), "Parameters", "native keyboard selects a whole group");
    await group("Model-local variables");
    const localId = await evaluate('(() => {const row=[...document.querySelectorAll("article[data-row-id]")].find(row=>row.querySelector("h2").textContent === "forecast");if (!row) throw new Error("Missing local definition");return row.getAttribute("data-row-id");})()');
    const localSelector = `article[data-row-id=${JSON.stringify(localId)}]`;
    const localQuery = selector => JSON.stringify(localSelector + " " + selector);
    const sideBySide = await checkPairedCards();
    await evaluate('document.getElementById("search").focus()'); await key("Tab", "Tab", 9);
    const focus = await evaluate('({style:getComputedStyle(document.activeElement).outlineStyle,width:getComputedStyle(document.activeElement).outlineWidth})');
    assert.notEqual(focus.style, "none"); assert.notEqual(focus.width, "0px");
    assert.ok(await evaluate(`document.querySelectorAll(${localQuery(".token.removed")}).length`) > 0);
    assert.equal(await evaluate('Array.from(document.querySelectorAll(".token.removed")).every(node => !getComputedStyle(node).textDecorationLine.includes("line-through"))'), true);
    evidence.semantic_expression_layout = { sideBySide };
    assert.equal(await evaluate('Boolean(document.getElementById("moreActions") || document.getElementById("details"))'), false);
    assert.equal(await evaluate(`document.querySelectorAll(${localQuery(".side-title button.source-link")}).length`), 2);
    assert.equal(await evaluate('Boolean(document.querySelector(".detail table, .detail details, .detail .gutter, .detail .facet"))'), false);
    assert.equal(await evaluate(`document.querySelectorAll(${localQuery(".expression")}).length`), 2, "each expression appears once per side");
    assert.equal(await evaluate('document.getElementById("capturedTextDiff").getClientRects().length > 0'), true);
    assert.equal(await evaluate('Boolean(document.getElementById("updateRevision"))'), false);
    const rootDiffTab = await textDiff("root.mod");
    const rootDiff = rootDiffTab.input;
    assert.equal((await vscode.workspace.openTextDocument(rootDiff.original)).getText(), await fs.readFile(roots.before.fsPath, "utf8"));
    assert.equal((await vscode.workspace.openTextDocument(rootDiff.modified)).getText(), await fs.readFile(roots.after.fsPath, "utf8"));
    await vscode.window.tabGroups.close(rootDiffTab);
    await vscode.commands.executeCommand("vscode.openWith", uri, changesViewType, { viewColumn: vscode.ViewColumn.One, preview: false });
    await regainChanges();
    evidence.semantic_direct_root_diff = true;
    const appearance = vscode.workspace.getConfiguration("workbench");
    const previousTheme = appearance.get("colorTheme"), previousColors = appearance.get("colorCustomizations");
    const previousActivity = appearance.get("activityBar.location");
    const themes = [];
    const themeColors = [];
    try {
      for (const [theme, bodyClass] of [
        ["Default Light Modern", "vscode-light"],
        ["Default Dark Modern", "vscode-dark"],
        ["Default High Contrast", "vscode-high-contrast"],
        ["Default High Contrast Light", "vscode-high-contrast-light"],
      ]) {
        await appearance.update("colorTheme", theme, vscode.ConfigurationTarget.Workspace);
        await waitFor(() => evaluate(`document.body.classList.contains(${JSON.stringify(bodyClass)})`), `rendered native ${theme}`);
        assert.equal(await evaluate('Array.from(document.querySelectorAll("button,input,select")).every(element => element.getAttribute("aria-label") || element.labels?.length || element.textContent.trim())'), true);
        assert.equal(await evaluate('Array.from(document.querySelectorAll(".token.removed")).every(node => !getComputedStyle(node).textDecorationLine.includes("line-through"))'), true);
        themes.push(theme);
        evidence.semantic_themes = [...themes];
        themeColors.push(await evaluate('({theme:document.body.className,roles:[...document.querySelectorAll("footer .change-mark")].map(mark=>({role:mark.className,color:getComputedStyle(mark).color}))})'));
        evidence.semantic_theme_colors = [...themeColors];
      }
      await appearance.update("colorCustomizations", {
        ...(previousColors ?? {}), "dynare.diff.changedForeground": "#2266aa",
        "diffEditor.insertedTextBackground": "#22884455", "diffEditor.removedTextBackground": "#aa334455",
      }, vscode.ConfigurationTarget.Workspace);
      await waitFor(async () => {
        const color = await evaluate('(() => { const style=getComputedStyle(document.body); return {variables:[...style].filter(name=>name.startsWith("--")&&(name.includes("dynare")||name.includes("diff"))).map(name=>({name,value:style.getPropertyValue(name).trim()})),dotted:style.getPropertyValue("--vscode-dynare-diff.changedForeground").trim().toLowerCase(),hyphens:style.getPropertyValue("--vscode-dynare-diff-changedForeground").trim().toLowerCase(),glyph:getComputedStyle(document.querySelector("footer .change-mark.changed")).color}; })()');
        evidence.semantic_custom_colors = color;
        return color.glyph === "rgb(34, 102, 170)";
      }, "native customized comparison color");
      evidence.semantic_custom_configuration = { ...vscode.workspace.getConfiguration("workbench").get("colorCustomizations") };
      assert.equal(evidence.semantic_custom_configuration["dynare.diff.changedForeground"], "#2266aa");
      await group("Priors");
      const fills = await evaluate('({added:[...document.querySelectorAll(".detail .token.added")].map(token=>getComputedStyle(token).backgroundColor),removed:[...document.querySelectorAll(".detail .token.removed")].map(token=>getComputedStyle(token).backgroundColor)})');
      assert.ok(fills.added.length > 0 && fills.removed.length > 0, "native diff fill checks use actual added and removed expression tokens");
      assert.ok(fills.added.every(color => color.startsWith("rgba(34, 136, 68,")));
      assert.ok(fills.removed.every(color => color.startsWith("rgba(170, 51, 68,")));
      evidence.semantic_native_diff_fills = fills;
      const changedColor = await evaluate('getComputedStyle(document.querySelector("footer .change-mark.changed")).color');
      assert.equal(changedColor, "rgb(34, 102, 170)", "Replaced glyph uses the customized native semantic color");
      evidence.semantic_changed_color = changedColor;
      await appearance.update("activityBar.location", "hidden", vscode.ConfigurationTarget.Workspace);
      await send("Emulation.setDeviceMetricsOverride", { width: 320, height: 1000, deviceScaleFactor: 1, mobile: false });
      await waitFor(() => evaluate('window.innerWidth <= 320 && window.innerWidth > 250'), "real native narrow webview viewport");
      assert.equal(await evaluate('document.getElementById("reviewContent").scrollWidth <= document.getElementById("reviewContent").clientWidth + 1'), true, "narrow controls and cards keep text readable");
      evidence.semantic_narrow = await evaluate('({width:window.innerWidth,height:window.innerHeight})');
      for (const label of groupLabels) { await group(label); await checkPairedCards(); }
      const viewports = [];
      for (const width of [1024, 1600]) {
        await send("Emulation.setDeviceMetricsOverride", { width, height: 1000, deviceScaleFactor: 1, mobile: false });
        await waitFor(() => evaluate(`window.innerWidth > ${width - 100} && window.innerWidth <= ${width}`), `native ${width}-pixel viewport`);
        assert.equal(await evaluate('document.getElementById("reviewContent").scrollWidth <= document.getElementById("reviewContent").clientWidth + 1'), true);
        for (const label of groupLabels) { await group(label); await checkPairedCards(); }
        const navigation = await evaluate('document.querySelector(".group-list").getBoundingClientRect().width');
        assert.ok(Math.abs(navigation - 190) < 0.6, "the groups stay in a compact left panel");
        viewports.push(await evaluate('({width:window.innerWidth,height:window.innerHeight})'));
      }
      evidence.semantic_viewports = viewports;
      evidence.semantic_themes = themes;
      evidence.semantic_custom_theme = true;
    } finally {
      await send("Emulation.clearDeviceMetricsOverride");
      await appearance.update("activityBar.location", previousActivity, vscode.ConfigurationTarget.Workspace);
      await appearance.update("colorCustomizations", previousColors, vscode.ConfigurationTarget.Workspace);
      await appearance.update("colorTheme", previousTheme, vscode.ConfigurationTarget.Workspace);
    }
    const filename = path.join(path.dirname(process.env.DYGNOSIS_HOST_RESULT), `semantic-${vscode.version}.png`);
    await group("Model-local variables");
    assert.equal(await evaluate(`Boolean(document.querySelector(${localQuery(".reference")}))`), true, "local row retains direct counted-equation references");
    assert.equal(await evaluate('Array.from(document.querySelectorAll(".reference")).every(ref=>Boolean(ref.closest(".side")))'), true);
    await vscode.commands.executeCommand("notifications.clearAll");
    await group("Equations");
    await evaluate('document.getElementById("reviewContent").scrollTop=0');
    const screenshot = await send("Page.captureScreenshot", { format: "png", captureBeyondViewport: false });
    await fs.writeFile(filename, Buffer.from(screenshot.data, "base64"));
    evidence.semantic_diff = { checks, screenshot: filename, required_visual: true };
    await group("Model-local variables");
    const referencedFile = path.join(path.dirname(roots.after.fsPath), "body.mod");
    for (const side of ["before", "after"]) {
      const writtenFile = path.join(path.dirname(roots[side].fsPath), "body.mod");
      await click(`${localSelector} .side.${side} .side-title button.source-link`);
      await waitFor(() => vscode.window.activeTextEditor?.document.uri.fsPath === writtenFile && vscode.window.activeTextEditor.selection.start.line === 1, `native ${side} card opens its own written include`);
      await vscode.commands.executeCommand("vscode.openWith", uri, changesViewType, { viewColumn: vscode.ViewColumn.One, preview: false });
      await regainChanges();
    }
    evidence.semantic_card_sources = true;
    await click(`${localSelector} .reference button[aria-label^="Open After reference:"]`);
    await waitFor(() => vscode.window.activeTextEditor?.document.uri.fsPath === referencedFile && vscode.window.activeTextEditor.selection.start.line === 2, "native direct reference opens the captured include target and selects the counted equation");
    assert.equal(vscode.window.activeTextEditor.selection.start.line, 2, "the accepted direct reference maps to the counted Output equation");
    await vscode.commands.executeCommand("vscode.openWith", uri, changesViewType, { viewColumn: vscode.ViewColumn.One, preview: false });
    await regainChanges();
    await waitFor(() => evaluate('document.getElementById("status").className === "ready" && !document.getElementById("capturedTextDiff").disabled'), "rendered captured-text action is ready");
    const capturedDiffTab = await textDiff("body.mod"), capturedDiff = capturedDiffTab.input;
    assert.equal(capturedDiffTab.label, "body.mod (Working Tree) ↔ body.mod (Working Tree)");
    assert.equal(capturedDiff.modified.scheme, "dygnosis-captured");
    assert.equal((await vscode.workspace.openTextDocument(capturedDiff.modified)).getText(), await fs.readFile(referencedFile, "utf8"));
    assert.equal((await vscode.workspace.openTextDocument(capturedDiff.original)).getText(), "", "an unpaired added include has an empty Before side");
    await vscode.window.tabGroups.close(capturedDiffTab);
    await vscode.commands.executeCommand("vscode.openWith", uri, changesViewType, { viewColumn: vscode.ViewColumn.One, preview: false });
    await regainChanges();
    const rootDocument = await vscode.workspace.openTextDocument(roots.after);
    const edit = new vscode.WorkspaceEdit();
    edit.insert(roots.after, rootDocument.positionAt(rootDocument.getText().length), "// Native refresh control\n");
    assert.equal(await vscode.workspace.applyEdit(edit), true);
    await waitFor(() => evaluate('document.getElementById("status").classList.contains("stale")'), "native comparison becomes out of date");
    assert.equal(await evaluate('document.getElementById("rootTextDiff").disabled'), true);
    assert.ok(await evaluate('document.querySelectorAll(".detail .reference button").length') > 0, "the stale reference check has real accepted references");
    assert.equal(await evaluate('Array.from(document.querySelectorAll(".detail button")).filter(button => /Open .*source|Open .*reference/.test(button.getAttribute("aria-label"))).every(button => button.disabled)'), true);
    assert.equal(await evaluate('document.getElementById("capturedTextDiff").disabled'), true);
    await click("#refresh");
    await waitFor(() => evaluate('document.getElementById("status").classList.contains("ready")'), "native refreshed comparison");
    await checkPairedCards();
    assert.equal(await evaluate(`document.querySelector(${localQuery(".side.after .source-link")}).disabled`), false, "Refresh restores the current row's source action");
    evidence.semantic_stale_refresh = true;
    await vscode.window.showTextDocument(rootDocument, { viewColumn: vscode.ViewColumn.Active, preview: false });
    await vscode.commands.executeCommand("dygnosis.showEffectiveModel");
    await waitFor(() => vscode.window.activeTextEditor?.document.uri.scheme === "dygnosis-effective", "Expand macros opens the read-only include expansion");
    const expansionScreenshot = path.join(path.dirname(process.env.DYGNOSIS_HOST_RESULT), `macro-expansion-${vscode.version}.png`);
    await nativeWorkbench(async workbenchSend => {
      const picture = await workbenchSend("Page.captureScreenshot", { format: "png", captureBeyondViewport: false });
      await fs.writeFile(expansionScreenshot, Buffer.from(picture.data, "base64"));
    });
    evidence.macro_expansion_screenshot = expansionScreenshot;
  }).catch(async error => { await captureFailure(evidence); throw error; });
  evidence.checks.push("native grouped rows with Before left and After right, references within their own card, source links for both includes and HEAD, saved filters, keyboard focus, themes, direct text diff and stale Refresh actions");
}

module.exports = { checkSemanticDiff };
