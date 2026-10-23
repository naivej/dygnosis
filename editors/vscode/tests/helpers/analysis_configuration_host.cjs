// Exercise shared analysis choices through native VS Code pickers and immutable macro views.
const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const { Buffer } = require("node:buffer");
const path = require("node:path");
const vscode = require("vscode");
const { nativeWorkbench, nativeChanges } = require("./native_webview.cjs");

async function press(key, code, number, modifiers = 0) {
  await nativeWorkbench(async send => {
    await send("Input.dispatchKeyEvent", { type: "keyDown", key, code, windowsVirtualKeyCode: number, nativeVirtualKeyCode: number, modifiers });
    await send("Input.dispatchKeyEvent", { type: "keyUp", key, code, windowsVirtualKeyCode: number, nativeVirtualKeyCode: number, modifiers });
  });
}
async function choose(label, waitFor, keyboard = false, placeholder = "Select the analysis configuration for this model root") {
  let point;
  await waitFor(async () => {
    point = await nativeWorkbench(async send => {
      const result = await send("Runtime.evaluate", { returnByValue: true, expression:
        `(() => { const input=document.querySelector('.quick-input-widget input'); if(input!==document.activeElement || input?.placeholder!==${JSON.stringify(placeholder)}) return null; const row=[...document.querySelectorAll('.quick-input-list .monaco-list-row')].find(row=>row.querySelector('.label-name')?.textContent===${JSON.stringify(label)} && row.getBoundingClientRect().width && row.getBoundingClientRect().height); const box=row?.getBoundingClientRect(); return box ? {x:box.x+box.width/2,y:box.y+box.height/2} : null; })()` });
      return result.result.value;
    });
    return point;
  }, `native analysis picker offers ${label}`).catch(async error => {
    const state = await nativeWorkbench(async send => {
      const result = await send("Runtime.evaluate", { returnByValue: true, expression:
        "({rows:[...document.querySelectorAll('.quick-input-list .monaco-list-row')].map(row=>row.textContent),placeholder:document.querySelector('.quick-input-widget input')?.placeholder,notifications:[...document.querySelectorAll('.notification-list-item-message')].map(row=>row.textContent)})" });
      return result.result.value;
    });
    error.message += `; native state=${JSON.stringify(state)}`; throw error;
  });
  if (keyboard) {
    await press("a", "KeyA", 65, 2);
    await nativeWorkbench(send => send("Input.insertText", { text: label }));
    await press("Enter", "Enter", 13); return;
  }
  await nativeWorkbench(async send => {
    await send("Input.dispatchMouseEvent", { type: "mousePressed", button: "left", clickCount: 1, ...point });
    await send("Input.dispatchMouseEvent", { type: "mouseReleased", button: "left", clickCount: 1, ...point });
  });
}
async function input(text, waitFor) {
  await waitFor(async () => nativeWorkbench(async send => {
    const result = await send("Runtime.evaluate", { expression: "(() => { const widget=document.querySelector('.quick-input-widget'), input=widget?.querySelector('input'), box=input?.getBoundingClientRect(); return Boolean(box?.width && box.height && input===document.activeElement && widget.textContent.includes('Save Analysis Configuration')); })()", returnByValue: true });
    return result.result.value;
  }), "native analysis input is visible");
  await press("a", "KeyA", 65, 2);
  await nativeWorkbench(send => send("Input.insertText", { text }));
  await press("Enter", "Enter", 13);
}
async function openConfigurationFromStatus(waitFor) {
  let point;
  await waitFor(async () => {
    point = await nativeWorkbench(async send => {
      const result = await send("Runtime.evaluate", { returnByValue: true, expression:
        "(() => { const item=[...document.querySelectorAll('.statusbar-item')].find(item=>item.id.endsWith('dygnosis.modelCounts')); const box=item?.getBoundingClientRect(); return box?.width && item.textContent.startsWith('Current config:') ? {x:box.x+box.width/2,y:box.y+box.height/2} : null; })()" });
      return result.result.value;
    });
    return point;
  }, "configured counts use Current config in the native status bar");
  await nativeWorkbench(async send => {
    await send("Input.dispatchMouseEvent", { type: "mouseMoved", x: 10, y: 10 });
    await send("Input.dispatchMouseEvent", { type: "mouseMoved", ...point });
  });
  await waitFor(async () => nativeWorkbench(async send => {
    const result = await send("Runtime.evaluate", { returnByValue: true, expression:
      "(() => { const hover=[...document.querySelectorAll('.monaco-hover .rendered-markdown')].find(item=>item.getClientRects().length && item.textContent.replaceAll(String.fromCharCode(160),' ').includes('Forced branch preview')); const link=[...hover?.querySelectorAll('a')??[]].find(item=>item.textContent==='Current config'); if(!link) return false; link.click(); return true; })()" });
    return result.result.value;
  }), "Current config hover link opens the configuration selector").catch(async error => {
    const state = await nativeWorkbench(async send => {
      const result = await send("Runtime.evaluate", { returnByValue: true, expression:
        "({status:[...document.querySelectorAll('.statusbar-item')].filter(item=>item.id.endsWith('dygnosis.modelCounts')).map(item=>item.outerHTML),hovers:[...document.querySelectorAll('.monaco-hover')].map(item=>item.outerHTML),point:document.elementFromPoint(" + point.x + "," + point.y + ")?.outerHTML})" });
      return result.result.value;
    });
    error.message += `; native hover state=${JSON.stringify(state)}`;
    throw error;
  });
}
async function dismissOutcome(cause, waitFor) {
  await vscode.commands.executeCommand("notifications.showList");
  let outcome;
  await waitFor(async () => {
    outcome = await nativeWorkbench(async send => {
      const result = await send("Runtime.evaluate", { returnByValue: true, expression:
        `(() => { const row=[...document.querySelectorAll('.notification-list-item')].find(row=>row.querySelector('.notification-list-item-message')?.textContent.includes(${JSON.stringify(cause)})); const button=row?.querySelector('.codicon-notifications-clear'), box=button?.getBoundingClientRect(); return box?.width && box.height ? {message:row.querySelector('.notification-list-item-message').textContent,x:box.x+box.width/2,y:box.y+box.height/2} : null; })()` });
      return result.result.value;
    });
    return outcome;
  }, "native stale-refresh outcome states its cause");
  await nativeWorkbench(async send => {
    for (const type of ["mousePressed", "mouseReleased"])
      await send("Input.dispatchMouseEvent", { type, button: "left", clickCount: 1, x: outcome.x, y: outcome.y });
  });
  await press("Escape", "Escape", 27);
  return outcome.message;
}
async function checkAnalysisConfigurations(service, workspaceRoot, evidence, helpers) {
  const { waitFor, writeObservedInput } = helpers;
  assert.ok(process.env.DYGNOSIS_HOST_CDP_PORT, "Analysis acceptance uses native controls");
  await waitFor(() => service.supportsAnalysisConfiguration, "analysis configuration capability");
  const rootPath = path.join(workspaceRoot, "analysis.mod"), helperPath = path.join(workspaceRoot, "analysis-active.inc");
  const source = '@#define FLAG = 1\nvar y $Y$ (long_name=\'Output\');\n@#if 0\nparameters dormant_p $P$ (long_name=\'Dormant\');\n@#endif\nmodel;\n@#if FLAG\n@#include "analysis-active.inc"\n@#else\ny=2;\n@#endif\nend;\n';
  await writeObservedInput(service, helperPath, "y=1;\n");
  await writeObservedInput(service, rootPath, source);
  const document = await vscode.workspace.openTextDocument(rootPath);
  await vscode.window.showTextDocument(document);
  const context = () => service.analysisContext(document.uri);
  await waitFor(async () => (await context())?.analysis.state === "selected", "Source analysis startup");
  assert.equal((await context()).analysis.selection.preview, false);
  const hasExpansion = () => vscode.workspace.textDocuments.some(document => document.uri.scheme === "dygnosis-effective");
  assert.equal(hasExpansion(), false, "written source intelligence starts without an expanded view");
  const dormantPosition = document.positionAt(source.indexOf("dormant_p"));
  const descendants = rows => (rows ?? []).flatMap(row => [row, ...descendants(row.children)]);
  let dormant;
  async function assertBranchActivity(selected) {
    let rows;
    await waitFor(async () => {
      rows = descendants(await vscode.commands.executeCommand("vscode.executeDocumentSymbolProvider", document.uri));
      return rows.find(row => row.name === "@#if 0")?.detail === "inactive" &&
        rows.find(row => row.name === "@#if FLAG")?.detail === (selected ? "active" : "inactive") &&
        rows.find(row => row.name === "@#else")?.detail === (selected ? "inactive" : "active");
    }, "native Outline refreshes arm activity for the accepted configuration").catch(async error => {
      error.message += `; analysis=${JSON.stringify((await context())?.analysis)}; arms=${JSON.stringify(rows?.filter(row => row.name.startsWith("@#")).map(row => ({name:row.name,detail:row.detail,range:row.selectionRange})))}`;
      throw error;
    });
    for (const row of rows.filter(row => !row.name.startsWith("@#")))
      assert.doesNotMatch(row.detail ?? "", /\b(?:active|inactive|mixed|unknown)\b/, `${row.name} repeats macro activity`);
  }
  await waitFor(async () => {
    const rows = await vscode.commands.executeCommand("vscode.executeDocumentSymbolProvider", document.uri);
    dormant = descendants(rows).find(row => row.name === "dormant_p");
    return dormant?.detail === "Dormant · $P$";
  }, "native written Outline exposes the dormant declaration without a repeated activity label");
  await assertBranchActivity(true);
  const startupRows = descendants(await vscode.commands.executeCommand("vscode.executeDocumentSymbolProvider", document.uri));
  assert.equal(startupRows.find(row => row.name === "dormant_p").kind, vscode.SymbolKind.Number);
  assert.equal(startupRows.find(row => row.name === "y").detail, "Output · $Y$", "endogenous Outline metadata has no timing class");
  assert.equal(dormant.selectionRange.contains(dormantPosition), true);
  let dormantHover;
  await waitFor(async () => {
    const hovers = await vscode.commands.executeCommand("vscode.executeHoverProvider", document.uri, dormantPosition);
    dormantHover = (hovers ?? []).flatMap(hover => hover.contents.map(content => typeof content === "string" ? content : content.value)).join("\n");
    return dormantHover.includes("inactive");
  }, "native written hover reports the dormant declaration as inactive");
  assert.match(dormantHover, /written source information/);
  assert.doesNotMatch(dormantHover, /(?:Equation number|Effective equation|Binding|Timing)\s*[:#]\s*\S/i);
  const dormantDefinitions = await vscode.commands.executeCommand("vscode.executeDefinitionProvider", document.uri, dormantPosition);
  assert.deepEqual(dormantDefinitions ?? [], [], "an inactive written declaration does not invent a semantic binding");
  assert.equal(hasExpansion(), false, "source-only provider requests do not open an expanded view");
  evidence.analysis_source_only = { outline: dormant.detail, hover: dormantHover, definitions: 0 };
  evidence.checks.push("native source-only Outline labels controlling macro arms; dormant declaration hover keeps its inactive state without inventing a binding or equation number");
  await vscode.commands.executeCommand("outline.focus");
  const outlineHeader = await nativeWorkbench(async send => {
    const result = await send("Runtime.evaluate", { returnByValue: true, expression:
      "(() => { const header=[...document.querySelectorAll('.pane-header')].find(item=>item.textContent==='Outline'); const box=header?.getBoundingClientRect(); return box?.width && box.height ? {x:box.x+box.width/2,y:box.y+box.height/2,collapsed:header.getAttribute('aria-expanded')==='false'} : null; })()" });
    return result.result.value;
  });
  assert.ok(outlineHeader, "native Outline header is visible");
  await nativeWorkbench(async send => {
    await send("Input.dispatchMouseEvent", { type: "mouseMoved", x: outlineHeader.x, y: outlineHeader.y });
    if (outlineHeader.collapsed) for (const type of ["mousePressed", "mouseReleased"])
      await send("Input.dispatchMouseEvent", { type, button: "left", clickCount: 1, x: outlineHeader.x, y: outlineHeader.y });
  });
  let filterPoint;
  await waitFor(async () => {
    filterPoint = await nativeWorkbench(async send => {
      const result = await send("Runtime.evaluate", { returnByValue: true, expression:
        "(() => { const item=[...document.querySelectorAll('.action-label')].find(item=>item.getAttribute('aria-label')?.includes('Filter Outline by macro activity') && item.getClientRects().length); const box=item?.getBoundingClientRect(); return box?.width && box.height ? {x:box.x+box.width/2,y:box.y+box.height/2} : null; })()" });
      return result.result.value;
    });
    return filterPoint;
  }, "native Outline toolbar offers the activity filter").catch(async error => {
    const state = await nativeWorkbench(async send => {
      const result = await send("Runtime.evaluate", { returnByValue: true, expression:
        "({headers:[...document.querySelectorAll('.pane-header')].filter(item=>item.textContent.includes('Outline')).map(item=>item.outerHTML)})" });
      return result.result.value;
    });
    error.message += `; native Outline state=${JSON.stringify(state)}`; throw error;
  });
  await waitFor(async () => nativeWorkbench(async send => {
    const result = await send("Runtime.evaluate", { returnByValue: true, expression:
      "(() => { const item=[...document.querySelectorAll('.action-label')].find(item=>item.getAttribute('aria-label')?.includes('Filter Outline by macro activity') && item.getClientRects().length); if(!item || item.getAttribute('aria-disabled')==='true') return false; item.click(); return true; })()" });
    return result.result.value;
  }), "native Outline filter button opens its picker after layout settles");
  await choose("Inactive", waitFor, false, "Show active, inactive, or both");
  await waitFor(async () => {
    const rows = descendants(await vscode.commands.executeCommand("vscode.executeDocumentSymbolProvider", document.uri));
    return rows.some(row => row.name === "dormant_p") && !rows.some(row => row.name === "@#if FLAG");
  }, "Inactive filter removes active rows and retains dormant source");
  for (const [label, matches] of [
    ["Active", rows => !rows.some(row => row.name === "dormant_p") && rows.some(row => row.name === "@#if FLAG") && !rows.some(row => row.name === "@#else")],
    ["Both", rows => rows.some(row => row.name === "dormant_p") && rows.some(row => row.name === "@#if FLAG") && rows.some(row => row.name === "@#else")],
  ]) {
    const selecting = vscode.commands.executeCommand("dygnosis.filterOutline");
    await choose(label, waitFor, false, "Show active, inactive, or both"); await selecting;
    await waitFor(async () => matches(descendants(await vscode.commands.executeCommand("vscode.executeDocumentSymbolProvider", document.uri))), `${label} filter refreshes the native Outline provider`);
  }
  await assertBranchActivity(true);
  evidence.checks.push("native Outline toolbar filter selects Inactive, Active and Both and refreshes the source provider without an editor change");
  const helper = await vscode.workspace.openTextDocument(helperPath);
  await vscode.window.showTextDocument(helper);
  await waitFor(async () => (await service.rootForDocument(helper))?.toString() === document.uri.toString(), "helper initially has its executed root");
  const editor = await vscode.window.showTextDocument(document);
  const conditionalPosition = document.positionAt(source.indexOf("@#if FLAG"));
  editor.selection = new vscode.Selection(conditionalPosition, conditionalPosition);
  const branch = vscode.commands.executeCommand("dygnosis.previewMacroBranch");
  await choose("@#else", waitFor, false, "Preview one written branch; enclosing loops and conditions still apply"); await branch;
  let selected = await context();
  assert.equal(selected.analysis.selection.preview, true);
  assert.equal(selected.analysis.configuration.overrides[0].kind, "branch");
  await assertBranchActivity(false);
  assert.equal(document.getText(), source, "analysis preview does not edit written macros");
  await vscode.window.showTextDocument(helper);
  assert.equal((await service.rootForDocument(helper))?.toString(), document.uri.toString(), "a dormant helper retains the chosen root");
  await vscode.window.showTextDocument(document);
  const saving = vscode.commands.executeCommand("dygnosis.saveAnalysisConfiguration");
  await input("Else preview", waitFor); await saving;
  const folder = vscode.workspace.getWorkspaceFolder(document.uri);
  const saved = vscode.workspace.getConfiguration("dynare", folder.uri).inspect("analysisConfigurations").workspaceFolderValue;
  assert.equal(saved.length, 1); assert.equal(saved[0].bindings.length, 1);
  assert.equal(saved[0].bindings[0].target_text.includes("@#endif"), true);
  evidence.checks.push("native branch preview and explicit folder Save preserve exact guards, written source, and dormant helper root");

  await vscode.commands.executeCommand("dygnosis.showEffectiveModel");
  let preview = vscode.window.activeTextEditor.document;
  assert.equal(preview.uri.scheme, "dygnosis-effective");
  const originalPreview = preview, originalText = preview.getText();
  assert.match(originalText, /y=2/);
  const events = [];
  const inputEvents = service.onDidInvalidate(event => events.push({ kind: "input", ...event }));
  const analysisEvents = service.onDidChangeAnalysis(event => events.push({ kind: "analysis", generation: event.context.analysis.selection_generation }));
  await openConfigurationFromStatus(waitFor);
  await choose("Source", waitFor);
  await waitFor(() => vscode.window.activeTextEditor?.document.uri.scheme === "dygnosis-effective" &&
    vscode.window.activeTextEditor.document.uri.toString() !== preview.uri.toString(), "successful Source refresh creates a new immutable URI").catch(async error => {
      error.message += `; events=${JSON.stringify(events)}; source=${JSON.stringify(await context())}; editors=${JSON.stringify(vscode.window.visibleTextEditors.map(editor=>({uri:editor.document.uri.toString(),text:editor.document.getText()})))}`;
      throw error;
    });
  inputEvents.dispose(); analysisEvents.dispose();
  preview = vscode.window.activeTextEditor.document;
  assert.match(preview.uri.path, /Source/); assert.match(preview.getText(), /y=1/);
  assert.match(originalPreview.uri.path, /Analysis preview/);
  assert.equal((await context()).analysis.selection.preview, false);
  await assertBranchActivity(true);
  const loading = vscode.commands.executeCommand("dygnosis.selectAnalysisConfiguration");
  await choose("Else preview", waitFor); await loading;
  await waitFor(() => vscode.window.activeTextEditor?.document.uri.scheme === "dygnosis-effective" &&
    vscode.window.activeTextEditor.document.uri.toString() !== preview.uri.toString(), "saved choice selected from expanded view refreshes that same root");
  const savedPreview = vscode.window.activeTextEditor.document;
  assert.equal((await context()).analysis.selection.name, "Else preview");
  assert.match(savedPreview.getText(), /y=2/);
  assert.ok((await vscode.commands.executeCommand("vscode.executeDocumentSymbolProvider", savedPreview.uri))?.length,
    "the current captured expansion has a nonempty Outline before it becomes stale");
  evidence.checks.push("Source and expanded native selectors share one root and successful refresh replaces the URI and configuration title");

  const frozenGeneration = (await context()).analysis.selection_generation;
  const comparing = vscode.commands.executeCommand("dygnosis.compareAnalysisConfigurations");
  await choose("Source", waitFor, false, "Choose configuration A");
  await choose("Else preview", waitFor, false, "Choose configuration B"); await comparing;
  await waitFor(() => vscode.window.tabGroups.all.some(group => group.tabs.some(tab => tab.input?.viewType === "dygnosis.changes")), "analysis comparison opens existing Model Diff");
  await waitFor(async () => {
    try {
      const state = await nativeChanges(async ({ evaluate }) => evaluate('({status:document.getElementById("status").className,names:[...document.querySelectorAll(".analysis-name")].map(row=>row.textContent),details:[...document.querySelectorAll(".analysis-detail")].map(row=>row.textContent),counts:[...document.querySelectorAll(".analysis-counts")].map(row=>row.textContent)})'));
      evidence.analysis_comparison = state;
      return state.status === "ready";
    } catch (error) { if (error.code === "NATIVE_FRAME_PENDING") return false; throw error; }
  }, "frozen comparison summaries are rendered");
  assert.deepEqual(evidence.analysis_comparison.names, ["Source · Complete", "Else preview (preview) · Complete"]);
  assert.ok(evidence.analysis_comparison.details.some(text => /Selected branch.*applied 1 time/.test(text)));
  assert.deepEqual(evidence.analysis_comparison.counts, ["1 equation · 1 endogenous variable", "1 equation · 1 endogenous variable"]);
  assert.equal((await context()).analysis.selection_generation, frozenGeneration, "comparison never sets a live choice");
  evidence.checks.push("native two-side analysis comparison uses the existing frozen Model Diff without changing live selection");

  await vscode.window.showTextDocument(document);
  const edit = new vscode.WorkspaceEdit();
  edit.insert(document.uri, new vscode.Position(0, 0), "// unrelated source edit\n");
  await vscode.workspace.applyEdit(edit);
  await waitFor(async () => (await context())?.analysis.state === "stale", "exact target guard marks edited source stale");
  await vscode.window.showTextDocument(savedPreview);
  assert.match(savedPreview.getText(), /OUTDATED EXPANSION/);
  const refreshing = vscode.commands.executeCommand("dygnosis.refreshEffectiveModel");
  evidence.analysis_stale_refusal = await dismissOutcome("override's source changed", waitFor);
  await refreshing;
  assert.match(evidence.analysis_stale_refusal, /displayed text keeps its old configuration/);
  assert.equal(vscode.window.activeTextEditor.document.uri.toString(), savedPreview.uri.toString(), "failed refresh retains its old immutable display and label");
  assert.match(savedPreview.uri.path, /Else preview/);
  assert.deepEqual((await vscode.commands.executeCommand("vscode.executeDocumentSymbolProvider", savedPreview.uri)) ?? [], [], "stale expansion has no current Outline");
  const rebinding = vscode.commands.executeCommand("dygnosis.selectAnalysisConfiguration");
  await choose("Else preview", waitFor); await rebinding;
  await waitFor(async () => (await context())?.analysis.state === "selected", "explicit exact saved evidence rebinds unchanged moved directive");
  evidence.checks.push("stale targets retain the old named display with no current Outline; explicit unchanged saved evidence restores the selection");

  await vscode.window.showTextDocument(document);
  await service.restart();
  await waitFor(async () => (await context())?.analysis.state === "selected", "restarted Source context");
  selected = await context();
  assert.equal(selected.analysis.selection.preview, false);
  assert.equal(selected.analysis.configuration.overrides.length, 0, "saved presets do not activate on server startup");
  const picker = vscode.commands.executeCommand("dygnosis.selectAnalysisConfiguration");
  await choose("Source", waitFor, true); await picker;
  evidence.checks.push("server restart returns to Source and leaves explicit saved presets inactive");
  const appearance = vscode.workspace.getConfiguration("workbench"), previousTheme = appearance.get("colorTheme");
  const availableThemes = vscode.extensions.getExtension("vscode.theme-defaults")?.packageJSON.contributes?.themes;
  assert.ok(Array.isArray(availableThemes), "native host declares its built-in themes");
  evidence.analysis_screenshots = [];
  try {
    for (const [ids, bodyClass, name] of [
      [["Default Dark Modern", "Dark Modern"], "vs-dark", "dark"],
      [["Default Light Modern", "Light Modern"], "vs", "light"],
      [["Default High Contrast"], "hc-black", "high-contrast"],
      [["Default High Contrast Light"], "hc-light", "high-contrast-light"],
    ]) {
      const theme = availableThemes.find(candidate => ids.includes(candidate.id))?.id;
      assert.ok(theme, `native host declares ${name} theme`);
      await appearance.update("colorTheme", theme, vscode.ConfigurationTarget.Workspace);
      await waitFor(async () => nativeWorkbench(async send => {
        const state = await send("Runtime.evaluate", { returnByValue: true, expression: `Boolean(document.querySelector('.monaco-workbench')?.classList.contains(${JSON.stringify(bodyClass)}))` });
        return state.result.value;
      }), `analysis controls apply ${theme}`);
      const opening = vscode.commands.executeCommand("dygnosis.selectAnalysisConfiguration");
      await waitFor(async () => nativeWorkbench(async send => {
        const state = await send("Runtime.evaluate", { returnByValue: true, expression: "(() => { const widget=document.querySelector('.quick-input-widget'), box=widget?.getBoundingClientRect(); if (!box?.width || !box.height || widget.getAnimations().some(animation=>animation.playState==='running')) return false; for(let element=widget;element;element=element.parentElement) if(Number(getComputedStyle(element).opacity)<0.99) return false; return [...widget.querySelectorAll('.label-name')].some(row=>row.textContent==='Else preview'); })()" });
        return state.result.value;
      }), "native selector retains Source and saved choice under the current theme");
      const filename = path.join(path.dirname(process.env.DYGNOSIS_HOST_RESULT), `analysis-${vscode.version}-${name}.png`);
      await nativeWorkbench(async send => {
        const screenshot = await send("Page.captureScreenshot", { format: "png", captureBeyondViewport: false });
        await fs.writeFile(filename, Buffer.from(screenshot.data, "base64"));
      });
      evidence.analysis_screenshots.push(filename);
      await press("Escape", "Escape", 27); await opening;
    }
  } finally { await appearance.update("colorTheme", previousTheme, vscode.ConfigurationTarget.Workspace); }
  evidence.checks.push("native configuration controls support keyboard selection, cancellation, and dark, light, high-contrast dark/light themes");
  const splitPath = path.join(workspaceRoot, "outline-split.mod"), expressionPath = path.join(workspaceRoot, "outline-expression.inc");
  await writeObservedInput(service, expressionPath, "y(-1)\n");
  await writeObservedInput(service, splitPath, 'var y;\nmodel;\n[name=\'joined\'] y=1+(\n@#include "outline-expression.inc"\n);\nend;\n');
  const splitDocument = await vscode.workspace.openTextDocument(splitPath);
  await vscode.window.showTextDocument(splitDocument);
  await waitFor(async () => (await service.modelInfo(splitDocument.uri))?.complete === true, "included-expression model is complete");
  const splitRows = descendants(await vscode.commands.executeCommand("vscode.executeDocumentSymbolProvider", splitDocument.uri));
  assert.equal(splitRows.filter(row => row.name === "model").length, 1);
  assert.equal(splitRows.filter(row => row.name === "1 · joined").length, 1);
  assert.ok(splitRows.every(row => !/fragment|source candidate|source template/.test(row.name + " " + row.detail)));
  evidence.checks.push("native Outline keeps inactive literal metadata and type icons, omits endogenous timing, and shows one model/equation entry across included expressions without fragments");
}

module.exports = { checkAnalysisConfigurations };
