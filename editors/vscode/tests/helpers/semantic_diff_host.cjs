// Verify semantic comparison controls on the rendered native VS Code webview.
const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const { Buffer } = require("node:buffer");
const { URL } = require("node:url");
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
  const saved = Object.fromEntries(["diff.defaultChangeKinds", "diff.sections"].map(key => [key, config.inspect(key)?.workspaceValue]));
  try {
    await config.update("diff.defaultChangeKinds", ["added", "changed", "unpaired"], vscode.ConfigurationTarget.Workspace);
    await config.update("diff.sections", ["symbols", "parameters", "aggregateEquations", "priors", "commands"], vscode.ConfigurationTarget.Workspace);
    await runSemanticDiff(service, workspaceRoot, evidence, waitFor);
  } finally {
    for (const [key, value] of Object.entries(saved)) await config.update(key, value, vscode.ConfigurationTarget.Workspace);
  }
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
          const state = await evaluate('({rows:Boolean(document.querySelector("button.list-row")),status:document.getElementById("status").className,message:document.getElementById("status").textContent})');
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
      try { await refreshContext(); delete evidence.semantic_context_discovery; return true; }
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
    const select = (selector, value) => evaluate(`(() => {
      const control = document.querySelector(${JSON.stringify(selector)});
      if (!control) throw new Error("Missing native control");
      control.value = ${JSON.stringify(value)};
      control.dispatchEvent(new Event("change", { bubbles: true }));
    })()`);
    const click = selector => evaluate(`(() => {
      const control = document.querySelector(${JSON.stringify(selector)});
      if (!control || !control.getClientRects().length) throw new Error("Missing visible native control");
      control.click();
    })()`);
    assert.equal(await evaluate('document.getElementById("presentation").value'), "focusedReview");
    const header = await evaluate('({cards:[...document.querySelectorAll("#models .input-side")].map(card=>({label:card.querySelector("small").textContent,name:card.querySelector("code").textContent,title:card.title})),kinds:document.getElementById("kinds").tagName,sections:document.getElementById("sections").tagName,checkboxes:document.querySelectorAll("input[type=checkbox],fieldset").length,legend:[...document.querySelectorAll("footer .change-mark")].map(mark=>mark.parentElement.textContent.trim())})');
    assert.deepEqual(header.cards.map(card => [card.label, card.name]), [["Before", "root.mod"], ["After", "root.mod"]]);
    assert.ok(header.cards[0].title.includes(decodeURIComponent(new URL(roots.before.toString()).pathname)));
    assert.ok(header.cards[1].title.includes(decodeURIComponent(new URL(roots.after.toString()).pathname)));
    assert.equal(header.kinds, "SELECT"); assert.equal(header.sections, "SELECT");
    assert.equal(header.checkboxes, 0, "compact controls have no checkbox panels");
    assert.deepEqual(header.legend, ["+ Added", "− Removed", "~ Changed", "? Unpaired"]);
    assert.equal(await evaluate('document.getElementById("kinds").value'), "saved");
    assert.equal(await evaluate('document.getElementById("sections").value'), "saved");
    await select("#kinds", "changed");
    assert.equal(await evaluate('[...document.querySelectorAll("button.list-row")].every(row=>row.classList.contains("changed"))'), true);
    await select("#kinds", "saved");
    assert.equal(await evaluate('Boolean(document.querySelector("button.list-row.removed"))'), false);
    await select("#kinds", "all");
    await select("#sections", "parameters");
    assert.equal(await evaluate('document.querySelectorAll("button.list-row").length'), 1);
    await select("#sections", "saved");
    assert.equal(await evaluate('document.getElementById("sections").value'), "saved");
    await select("#sections", "all");
    assert.equal(await evaluate('document.getElementById("modelCount").textContent'), "15", "the rich accepted producer fixture retains all model rows");
    assert.equal(await evaluate('document.getElementById("sourceCount").textContent'), "3");
    evidence.semantic_compact_controls = header;
    evidence.semantic_saved_filters = true;
    for (const presentation of ["focusedReview", "changeList"]) {
      await select("#presentation", presentation);
      assert.equal(await evaluate('document.activeElement.id'), "presentation", "switching presentation keeps keyboard focus visible");
      for (const tab of ["modelTab", "sourceTab", "coverageTab"]) {
        await click(`#${tab}`);
        assert.equal(await evaluate(`document.getElementById(${JSON.stringify(tab)}).getAttribute("aria-selected")`), "true");
        assert.equal(await evaluate('Boolean(document.getElementById("presentation").getClientRects().length)'), true);
        assert.equal(await evaluate('Boolean(document.getElementById("results").textContent.trim())'), true);
        const filters = await evaluate('({toolbar:document.getElementById("filterTools").hidden,scope:document.getElementById("scopeFilter").hidden,section:document.getElementById("sectionFilter").hidden,tabsBottom:document.getElementById("tabs").getBoundingClientRect().bottom,toolsTop:document.getElementById("filterTools").getBoundingClientRect().top})');
        assert.equal(filters.toolbar, tab === "coverageTab");
        assert.equal(filters.scope, tab !== "modelTab"); assert.equal(filters.section, tab !== "modelTab");
        if (tab !== "coverageTab") assert.ok(filters.toolsTop >= filters.tabsBottom, "tabs precede the compact filters");
        checks.push(`${presentation}/${tab}`);
      }
    }
    await select("#presentation", "focusedReview");
    await click("#modelTab");
    assert.equal(await evaluate('Boolean(document.querySelector("nav.change-tree button[data-row-id]"))'), true);
    assert.equal(await evaluate('Boolean(document.querySelector("section.selected-detail"))'), true);
    await evaluate('document.querySelector("nav.change-tree .tree-group > summary").focus()');
    await key("Enter", "Enter", 13);
    assert.equal(await evaluate('document.querySelector("nav.change-tree .tree-group").open'), false, "native keyboard collapses a counted group");
    await key("Enter", "Enter", 13);
    assert.equal(await evaluate('document.querySelector("nav.change-tree .tree-group").open'), true);
    await select("#layout", "stacked");
    await select("#presentation", "changeList");
    await select("#layout", "sideBySide");
    await select("#presentation", "focusedReview");
    assert.equal(await evaluate('document.getElementById("layout").value'), "stacked", "presentations retain their own detail layout");
    await evaluate('document.getElementById("presentation").focus()');
    await key("Tab", "Tab", 9);
    assert.notEqual(await evaluate('document.activeElement.id'), "presentation", "native Tab moves to another visible control");
    const focus = await evaluate('({style:getComputedStyle(document.activeElement).outlineStyle,width:getComputedStyle(document.activeElement).outlineWidth})');
    assert.notEqual(focus.style, "none"); assert.notEqual(focus.width, "0px");
    await evaluate('document.getElementById("modelTab").focus()');
    await key("ArrowRight", "ArrowRight", 39);
    assert.equal(await evaluate('document.getElementById("sourceTab").getAttribute("aria-selected")'), "true", "native arrow key changes the selected tab");
    await click("#sourceTab");
    assert.equal(await evaluate('Boolean(document.querySelector("[data-source-pointer]"))'), true);
    assert.equal(await evaluate('Boolean(document.querySelector("table.source-hunk .gutter"))'), true);
    assert.ok(await evaluate('document.querySelectorAll("table.source-hunk .token.removed").length') > 0, "the deletion style check uses actual retained source lines");
    assert.equal(await evaluate('Array.from(document.querySelectorAll("table.source-hunk .token.removed")).every(node => !getComputedStyle(node).textDecorationLine.includes("line-through"))'), true);
    assert.equal(await evaluate('document.getElementById("moreActions").open'), false);
    assert.equal(await evaluate('document.getElementById("rootTextDiff").disabled'), false, "ready root action is available inside the closed overflow");
    await evaluate('document.querySelector("#moreActions > summary").focus()');
    await key("Enter", "Enter", 13);
    assert.equal(await evaluate('document.getElementById("moreActions").open'), true, "native keyboard opens More actions");
    assert.equal(await evaluate('document.getElementById("rootTextDiff").getClientRects().length > 0'), true);
    await click("#rootTextDiff");
    await waitFor(() => vscode.window.tabGroups.activeTabGroup.activeTab?.input instanceof vscode.TabInputTextDiff, "native overflow opens the root text diff");
    const rootDiffTab = vscode.window.tabGroups.activeTabGroup.activeTab;
    const rootDiff = rootDiffTab.input;
    assert.equal((await vscode.workspace.openTextDocument(rootDiff.original)).getText(), await fs.readFile(roots.before.fsPath, "utf8"));
    assert.equal((await vscode.workspace.openTextDocument(rootDiff.modified)).getText(), await fs.readFile(roots.after.fsPath, "utf8"));
    await vscode.window.tabGroups.close(rootDiffTab);
    await vscode.commands.executeCommand("vscode.openWith", uri, changesViewType, { viewColumn: vscode.ViewColumn.One, preview: false });
    await regainChanges();
    if (!await evaluate('document.getElementById("moreActions").open')) await click("#moreActions > summary");
    await evaluate('document.querySelector("#moreActions > summary").focus()');
    await key("Enter", "Enter", 13);
    assert.equal(await evaluate('document.getElementById("moreActions").open'), false, "native keyboard closes More actions");
    evidence.semantic_overflow_root_diff = true;
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
        for (const presentation of ["focusedReview", "changeList"]) {
          await select("#presentation", presentation);
          for (const tab of ["modelTab", "sourceTab", "coverageTab"]) {
            await click(`#${tab}`);
            const control = await evaluate('({top:document.getElementById("presentation").getBoundingClientRect().top,tabs:document.getElementById("modelTab").getBoundingClientRect().top,labels:Array.from(document.querySelectorAll("button,input,select")).every(element => element.getAttribute("aria-label") || element.labels?.length || element.textContent.trim())})');
            assert.ok(control.top < control.tabs, "Presentation stays in the top comparison controls");
            assert.equal(control.labels, true, "native controls have accessible names");
          }
        }
        await click("#sourceTab");
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
      evidence.semantic_custom_configuration = { ...appearance.get("colorCustomizations") };
      assert.equal(evidence.semantic_custom_configuration["dynare.diff.changedForeground"], "#2266aa");
      const fills = await evaluate('({added:[...document.querySelectorAll("table.source-hunk .token.added")].map(token=>getComputedStyle(token).backgroundColor),removed:[...document.querySelectorAll("table.source-hunk .token.removed")].map(token=>getComputedStyle(token).backgroundColor)})');
      assert.ok(fills.added.length > 0 && fills.removed.length > 0, "native diff fill checks use actual added and removed source tokens");
      assert.ok(fills.added.every(color => color.startsWith("rgba(34, 136, 68,")));
      assert.ok(fills.removed.every(color => color.startsWith("rgba(170, 51, 68,")));
      evidence.semantic_native_diff_fills = fills;
      await click("#modelTab");
      const changedColor = await evaluate('getComputedStyle(document.querySelector("footer .change-mark.changed")).color');
      assert.equal(changedColor, "rgb(34, 102, 170)", "Changed glyph uses the customized native semantic color");
      evidence.semantic_changed_color = changedColor;
      await appearance.update("activityBar.location", "hidden", vscode.ConfigurationTarget.Workspace);
      await send("Emulation.setDeviceMetricsOverride", { width: 320, height: 1000, deviceScaleFactor: 1, mobile: false });
      await waitFor(() => evaluate('window.innerWidth <= 320 && window.innerWidth > 250'), "real native narrow webview viewport");
      for (const presentation of ["focusedReview", "changeList"]) {
        await select("#presentation", presentation);
        for (const tab of ["modelTab", "sourceTab", "coverageTab"]) {
          await click(`#${tab}`);
          assert.equal(await evaluate('document.documentElement.scrollWidth <= window.innerWidth + 1'), true, "narrow controls keep overflow inside their detail tables");
        }
      }
      evidence.semantic_narrow = await evaluate('({width:window.innerWidth,height:window.innerHeight})');
      const viewports = [];
      for (const width of [1024, 1600]) {
        await send("Emulation.setDeviceMetricsOverride", { width, height: 1000, deviceScaleFactor: 1, mobile: false });
        await waitFor(() => evaluate(`window.innerWidth > ${width - 100} && window.innerWidth <= ${width}`), `native ${width}-pixel viewport`);
        for (const presentation of ["focusedReview", "changeList"]) {
          await select("#presentation", presentation);
          for (const tab of ["modelTab", "sourceTab", "coverageTab"]) {
            await click(`#${tab}`);
            assert.equal(await evaluate('document.documentElement.scrollWidth <= window.innerWidth + 1'), true);
            if (presentation === "focusedReview" && tab !== "coverageTab") {
              const treeWidth = await evaluate('document.querySelector("nav.change-tree").getBoundingClientRect().width');
              assert.ok(Math.abs(treeWidth - 245) < 0.6, "wide focused review keeps its compact 245-pixel tree");
            }
          }
        }
        await select("#presentation", "focusedReview"); await click("#modelTab");
        viewports.push(await evaluate('({width:window.innerWidth,height:window.innerHeight,tree:document.querySelector("nav.change-tree").getBoundingClientRect().width})'));
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
    await select("#presentation", "focusedReview");
    await click("#modelTab");
    await select("#layout", "auto");
    await evaluate('(() => { const row=Array.from(document.querySelectorAll("button.list-row")).find(element => element.textContent.includes("forecast")); if (!row) throw new Error("Missing native local definition"); row.click(); })()');
    assert.equal(await evaluate('Boolean(document.querySelector(".selected-detail .reference"))'), true, "local detail retains direct counted-equation references");
    await vscode.commands.executeCommand("notifications.clearAll");
    await evaluate('window.scrollTo(0,0)');
    const screenshot = await send("Page.captureScreenshot", { format: "png", captureBeyondViewport: false });
    await fs.writeFile(filename, Buffer.from(screenshot.data, "base64"));
    evidence.semantic_diff = { checks, screenshot: filename, required_visual: true };
    const referencedFile = path.join(path.dirname(roots.after.fsPath), "body.mod");
    await click('.selected-detail .reference button[aria-label^="Open After reference:"]');
    await waitFor(() => vscode.window.activeTextEditor?.document.uri.fsPath === referencedFile, "native direct reference opens the captured include target");
    assert.equal(vscode.window.activeTextEditor.selection.start.line, 2, "the accepted direct reference maps to the counted Output equation");
    await vscode.commands.executeCommand("vscode.openWith", uri, changesViewType, { viewColumn: vscode.ViewColumn.One, preview: false });
    await regainChanges();
    await click("#sourceTab");
    await evaluate('(() => { const row=Array.from(document.querySelectorAll("button[data-source-pointer]")).find(element => element.textContent.includes("semantic-after") && element.textContent.includes("body.mod")); if (!row) throw new Error("Missing After captured include"); row.click(); })()');
    await click('.selected-detail button[aria-label="Open After captured text"]');
    await waitFor(() => vscode.window.activeTextEditor?.document.uri.scheme === "dygnosis-captured", "native captured include opens read-only retained text");
    assert.equal(vscode.window.activeTextEditor.document.getText(), await fs.readFile(referencedFile, "utf8"));
    await vscode.commands.executeCommand("vscode.openWith", uri, changesViewType, { viewColumn: vscode.ViewColumn.One, preview: false });
    await regainChanges();
    await click("#modelTab");
    const rootDocument = await vscode.workspace.openTextDocument(roots.after);
    const edit = new vscode.WorkspaceEdit();
    edit.insert(roots.after, rootDocument.positionAt(rootDocument.getText().length), "// Native refresh control\n");
    assert.equal(await vscode.workspace.applyEdit(edit), true);
    await waitFor(() => evaluate('document.getElementById("status").classList.contains("stale")'), "native comparison becomes out of date");
    assert.equal(await evaluate('document.getElementById("rootTextDiff").disabled'), true);
    assert.equal(await evaluate('document.getElementById("moreActions").open'), false, "stale root action is disabled while overflow remains closed");
    assert.ok(await evaluate('document.querySelectorAll(".selected-detail .reference button").length') > 0, "the stale reference check has real accepted references");
    assert.equal(await evaluate('Array.from(document.querySelectorAll(".selected-detail button")).filter(button => /Open .*source|Open .*reference/.test(button.getAttribute("aria-label"))).every(button => button.disabled)'), true);
    await click("#sourceTab");
    assert.equal(await evaluate('Array.from(document.querySelectorAll(".selected-detail button")).every(button => button.disabled)'), true, "stale capture disables every captured Source action");
    await click("#refresh");
    await waitFor(() => evaluate('document.getElementById("status").classList.contains("ready")'), "native refreshed comparison");
    await click("#modelTab");
    assert.equal(await evaluate('Boolean(document.querySelector("button.list-row[aria-pressed=true]"))'), true);
    assert.equal(await evaluate('document.querySelector("button.list-row[aria-pressed=true]").textContent.includes("forecast")'), false, "Refresh does not reuse selection without cross-capture proof");
    evidence.semantic_stale_refresh = true;
  }).catch(async error => { await captureFailure(evidence); throw error; });
  evidence.checks.push("native compact semantic Model/Source/Coverage, saved select filters, both presentations, per-presentation layout, native keyboard focus and More actions, 245px tree, custom Changed color, root text diff, captured include and stale Refresh actions");
}

module.exports = { checkSemanticDiff };
