// Verify workspace folder grouping and saved include navigation in native VS Code.
const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const vscode = require("vscode");
const { nativeWorkbench } = require("./native_webview.cjs");

async function nativeValue(expression) {
  return nativeWorkbench(async send => {
    const response = await send("Runtime.evaluate", { expression, returnByValue: true });
    assert.equal(response.exceptionDetails, undefined, "native workspace tree inspection succeeds");
    return response.result?.value;
  });
}

const treeRowsExpression = `(() => {
  const panes = [...document.querySelectorAll('.pane')];
  const pane = panes.find(value => value.textContent.includes('Dynare workspace checks'));
  if (!pane) return [];
  return [...pane.querySelectorAll('.monaco-list-row:not(.monaco-tree-sticky-row)')].filter(row => row.getClientRects().length).map(row => ({
    label: row.querySelector('.label-name')?.textContent.trim() || row.textContent.trim(),
    ariaLabel: row.getAttribute('aria-label'),
    text: row.textContent.trim(), expanded: row.getAttribute('aria-expanded'), level: row.getAttribute('aria-level'),
    id: row.getAttribute('data-id'), title: row.getAttribute('title'),
  }));
})()`;

async function treeRows() { return nativeValue(treeRowsExpression); }
async function findRows(label) {
  const rows = await treeRows();
  return rows.filter(row => row.label === label || row.text === label || row.label.startsWith(`${label},`));
}
async function rowSelector(label, occurrence = 0) {
  const rows = await findRows(label);
  assert.ok(rows.length > occurrence, `native workspace tree contains ${label} row ${occurrence + 1}; saw ${JSON.stringify(await treeRows())}`);
  return `(() => { const pane=[...document.querySelectorAll('.pane')].find(value=>value.textContent.includes('Dynare workspace checks')); const rows=[...pane.querySelectorAll('.monaco-list-row:not(.monaco-tree-sticky-row)')].filter(row=>row.getClientRects().length); return rows.filter(row=>(row.querySelector('.label-name')?.textContent.trim()||row.textContent.trim())===${JSON.stringify(label)})[${occurrence}]; })()`;
}
async function clickRow(label, occurrence = 0, twistie = false) {
  const selector = await rowSelector(label, occurrence);
  const clicked = await nativeValue(`(() => { const row=${selector}; if(!row) return false; const target=${twistie ? "row.querySelector('.monaco-tl-twistie')" : "row"}; if(!target) return false; target.dispatchEvent(new MouseEvent('click',{bubbles:true,view:window})); return true; })()`);
  assert.equal(clicked, true, `native workspace tree can click ${label}`);
}
async function scrollWorkspaceTree(deltaY) {
  const point = await nativeValue(`(() => { const pane=[...document.querySelectorAll('.pane')].find(value=>value.textContent.includes('Dynare workspace checks')); const list=pane?.querySelector('.monaco-list'); if(!list) return undefined; const box=list.getBoundingClientRect(); return {x:box.x+box.width/2,y:box.y+box.height/2}; })()`);
  assert.ok(point, "native workspace tree has a scrollable viewport");
  await nativeWorkbench(send => send("Input.dispatchMouseEvent", { type: "mouseWheel", x: point.x, y: point.y, deltaX: 0, deltaY }));
  await new Promise(resolve => setTimeout(resolve, 120));
}
async function scrollWorkspaceTreeByViewport(direction = 1) {
  const height = await nativeValue(`(() => { const pane=[...document.querySelectorAll('.pane')].find(value=>value.textContent.includes('Dynare workspace checks')); return pane?.querySelector('.monaco-list')?.getBoundingClientRect().height ?? 0; })()`);
  const step = Math.max(1, Math.min(120, Math.floor(height / 2)));
  await scrollWorkspaceTree(step * direction);
}
async function revealRow(label, waitFor) {
  await scrollWorkspaceTreeToTop(waitFor);
  await waitFor(async () => {
    if ((await findRows(label)).length) return true;
    await scrollWorkspaceTreeByViewport();
    return (await findRows(label)).length > 0;
  }, `native workspace tree scrolls ${label} into view`);
}
async function enlargeWorkspaceTreePane() {
  const layout = await nativeValue(`(() => { const pane=[...document.querySelectorAll('.pane')].find(value=>value.textContent.includes('Dynare workspace checks')); if(!pane) return undefined; const box=pane.getBoundingClientRect(); const sashes=[...document.querySelectorAll('.monaco-sash')].map(sash=>{const rect=sash.getBoundingClientRect();return {rect:{x:rect.x,y:rect.y,width:rect.width,height:rect.height},classes:sash.className};}); return {pane:{x:box.x,y:box.y,width:box.width,height:box.height},sashes}; })()`);
  assert.ok(layout?.pane, "native workspace checks pane has a measurable layout");
  const sash = layout.sashes.map(value => ({ ...value, centerY: value.rect.y + value.rect.height / 2 }))
    .filter(value => Math.abs(value.centerY - layout.pane.y) < 24)
    .sort((left, right) => Math.abs(left.centerY - layout.pane.y) - Math.abs(right.centerY - layout.pane.y))[0];
  if (!sash) return;
  const x = sash.rect.x + Math.max(2, sash.rect.width / 2), fromY = sash.centerY;
  const toY = Math.max(80, fromY - 500);
  await nativeWorkbench(async send => {
    await send("Input.dispatchMouseEvent", { type: "mouseMoved", x, y: fromY });
    await send("Input.dispatchMouseEvent", { type: "mousePressed", x, y: fromY, button: "left", clickCount: 1 });
    await send("Input.dispatchMouseEvent", { type: "mouseMoved", x, y: toY, button: "left", buttons: 1 });
    await send("Input.dispatchMouseEvent", { type: "mouseReleased", x, y: toY, button: "left", clickCount: 1 });
  });
}
async function expandFolder(label, waitFor) {
  await revealRow(label, waitFor);
  if ((await findRows(label))[0]?.expanded === "false") await clickRow(label, 0, true);
  await waitFor(async () => (await findRows(label))[0]?.expanded === "true", `native workspace folder ${label} expands`);
}
async function scrollWorkspaceTreeToTop(waitFor) {
  const rootLabel = path.basename(vscode.workspace.workspaceFolders[0].uri.fsPath);
  await openExplorer(waitFor);
  // The virtualized list can retain scroll/focus after replacing its rows.
  // A status refresh can temporarily hide it. Native Home reveals its first
  // item after rendering, without changing folder expansion.
  await waitFor(() => nativeValue(`(() => { const pane=[...document.querySelectorAll('.pane')].find(value=>value.textContent.includes('Dynare workspace checks')); const list=pane?.querySelector('.monaco-list'); if(!list?.getClientRects().length||!list.querySelector('.monaco-list-row')) return false; list.focus(); return list.contains(document.activeElement); })()`),
    "native workspace tree becomes visible and receives keyboard focus");
  await nativeWorkbench(async send => {
    await send("Input.dispatchKeyEvent", { type: "keyDown", key: "Home", code: "Home", windowsVirtualKeyCode: 36, nativeVirtualKeyCode: 36 });
    await send("Input.dispatchKeyEvent", { type: "keyUp", key: "Home", code: "Home", windowsVirtualKeyCode: 36, nativeVirtualKeyCode: 36 });
  });
  for (let index = 0; index < 40; ++index) {
    const rows = await treeRows();
    if (rows[0]?.label === rootLabel) return;
    await scrollWorkspaceTreeByViewport(-1);
  }
  throw new Error(`Native workspace tree did not return to its workspace row: ${JSON.stringify(await treeRows())}`);
}
async function openExplorer(waitFor) {
  await vscode.commands.executeCommand("workbench.view.explorer");
  await vscode.commands.executeCommand("dygnosis.workspace.focus");
  await waitFor(async () => (await nativeValue("Boolean([...document.querySelectorAll('.pane')].some(value=>value.textContent.includes('Dynare workspace checks')))")), "workspace checks view is visible in Explorer");
  const paneState = await nativeValue(`(() => { const pane=[...document.querySelectorAll('.pane')].find(value=>value.textContent.includes('Dynare workspace checks')); const header=pane?.querySelector('.pane-header'); const box=header?.getBoundingClientRect(); return {expanded:pane?.classList.contains('expanded'),point:box?{x:box.x+box.width/2,y:box.y+box.height/2}:undefined}; })()`);
  if (!paneState.expanded) {
    assert.ok(paneState.point, "collapsed workspace checks pane has a native header");
    await nativeWorkbench(async send => {
      await send("Input.dispatchMouseEvent", { type: "mouseMoved", x: paneState.point.x, y: paneState.point.y });
      await send("Input.dispatchMouseEvent", { type: "mousePressed", x: paneState.point.x, y: paneState.point.y, button: "left", clickCount: 1 });
      await send("Input.dispatchMouseEvent", { type: "mouseReleased", x: paneState.point.x, y: paneState.point.y, button: "left", clickCount: 1 });
    });
    await waitFor(() => nativeValue(`(() => [...document.querySelectorAll('.pane')].some(value=>value.textContent.includes('Dynare workspace checks')&&value.classList.contains('expanded')))()`),
      "workspace checks pane expands");
  }
}
async function status(service) { return service.execute("dynare/workspaceStatus"); }
async function waitInventory(service, waitFor, predicate, description) {
  let current;
  await waitFor(async () => {
    current = await status(service);
    return current?.complete && Array.isArray(current.include_files) && predicate(current.include_files);
  }, description);
  return current;
}
function uriPath(uri) { return vscode.Uri.parse(uri).fsPath.toLowerCase(); }
function rowTextIncludes(rows, text) { return rows.some(row => row.label.includes(text) || row.text.includes(text)); }

async function ignoreFolderRow(label, workspaceRoot, waitFor, occurrence = 0) {
  await revealRow(label, waitFor);
  const selector = await rowSelector(label, occurrence);
  const rowPoint = await nativeValue(`(() => { const row=${selector}; if(!row) return undefined; const box=row.getBoundingClientRect(); return {x:box.x+box.width/2,y:box.y+box.height/2}; })()`);
  assert.ok(rowPoint, `native workspace row ${label} has a visible hit target`);
  await nativeWorkbench(send => send("Input.dispatchMouseEvent", { type: "mouseMoved", x: rowPoint.x, y: rowPoint.y }));
  const actionSelector = `(() => { const row=${selector}; return [...(row?.querySelectorAll('.action-label')??[])].find(item=>item.title==='Ignore this folder'||item.getAttribute('aria-label')==='Ignore this folder'); })()`;
  await waitFor(() => nativeValue(`(() => { const item=${actionSelector}; const box=item?.getBoundingClientRect(); return !!item&&box.width>0&&box.height>0; })()`),
    `visible inline Ignore this folder action appears on ${label}`).catch(async error => {
      const details = await nativeValue(`(() => { const row=${selector}; const rect=value=>{const box=value?.getBoundingClientRect();return box?{x:box.x,y:box.y,width:box.width,height:box.height}:undefined}; return JSON.stringify({row:{aria:row?.getAttribute('aria-label'),className:row?.className,rect:rect(row),html:row?.outerHTML},actions:[...(row?.querySelectorAll('.action-label')??[])].map(item=>({title:item.title,aria:item.getAttribute('aria-label'),className:item.className,rect:rect(item),html:item.outerHTML}))}); })()`);
      error.message += `; inline action DOM=${details}`;
      throw error;
    });
  const actionPoint = await nativeValue(`(() => { const item=${actionSelector}; if(!item) return undefined; const box=item.getBoundingClientRect(); return {x:box.x+box.width/2,y:box.y+box.height/2}; })()`);
  assert.ok(actionPoint, `inline Ignore action for ${label} has a visible hit target`);
  await nativeWorkbench(send => send("Input.dispatchMouseEvent", { type: "mouseMoved", x: actionPoint.x, y: actionPoint.y }));
  const clicked = await nativeValue(`(() => { const item=${actionSelector}; if(!item) return false; item.click(); return true; })()`);
  assert.equal(clicked, true, `native inline Ignore button for ${label} is clicked`);
  // This action writes its relative pattern directly; it must not open a picker.
  await waitFor(() => nativeValue("!document.querySelector('.quick-input-widget input')?.getClientRects().length"), "Ignore this folder finishes without a picker");
  const expected = label === path.basename(workspaceRoot) ? "**" : `${label}/**`;
  await waitFor(() => vscode.workspace.getConfiguration("dynare", vscode.workspace.workspaceFolders[0].uri).get("workspaceExcludePaths", []).includes(expected),
    `${label} exclusion is saved at workspace-folder scope`);
  return expected;
}

async function checkWorkspaceHost(service, workspaceRoot, evidence, { waitFor, currentSnapshot }) {
  assert.ok(process.env.DYGNOSIS_HOST_CDP_PORT, "Workspace acceptance requires the isolated native host CDP port");
  await waitFor(() => service.client && service.supportsModelInfo, "workspace status language client startup");

  const files = new Map([
    ["root.mod", "var root_y; model; root_y=1; end;\n"],
    ["root.dyn", "var root_x; model; root_x=1; end;\n"],
    ["alpha/same.mod", 'var a; model;\n@#include "../includes/shared.inc"\nend;\n'],
    ["beta/same.mod", 'var b; model;\n@#include "../includes/shared.inc"\nend;\n'],
    ["models/variants/policy.dyn", "var policy_y; model; policy_y=1; end;\n"],
    ["includes/shared.inc", "// Shared saved include.\n"],
    ["includes/calibration.inc", "// Unreferenced saved include.\n"],
    ["only-includes/standalone.inc", "// Include-only folder.\n"],
    ["+generated/skip.mod", "var skip; model; skip=1; end;\n"],
    ["+generated/skip.inc", "// Generated directory is skipped.\n"],
  ]);
  for (const [relative, contents] of files) {
    const filename = path.join(workspaceRoot, relative);
    await fs.mkdir(path.dirname(filename), { recursive: true });
    await fs.writeFile(filename, contents);
  }

  const expectedIncludes = [...files.keys()].filter(name => name.endsWith(".inc") && !name.startsWith("+")).map(name => path.join(workspaceRoot, name).toLowerCase()).sort();
  let workspaceStatus = await waitInventory(service, waitFor, values => expectedIncludes.length === values.length,
    "workspace status reports every saved .inc path");
  const includePaths = workspaceStatus.include_files.map(uriPath);
  assert.deepEqual([...includePaths].sort(), expectedIncludes, "saved and unreferenced includes have a unique URI inventory");
  assert.ok(!workspaceStatus.include_files.some(value => uriPath(value) === path.join(workspaceRoot, "+generated/skip.inc").toLowerCase()),
    "the saved include inventory skips generated + directories");
  assert.equal(workspaceStatus.roots.length, 5, "root .mod and .dyn files are checked without opening them");
  assert.equal(workspaceStatus.counts.checked, 5, "include files do not contribute to model coverage");
  for (const root of workspaceStatus.roots) assert.ok(!vscode.workspace.textDocuments.some(document => document.uri.fsPath.toLowerCase() === uriPath(root.root_uri)), "workspace model discovery does not open the model");
  evidence.workspace = { model_roots: workspaceStatus.roots.map(root => root.root_uri), include_files: workspaceStatus.include_files };

  await openExplorer(waitFor);
  await enlargeWorkspaceTreePane();
  await scrollWorkspaceTreeToTop(waitFor);
  await expandFolder(path.basename(workspaceRoot), waitFor);
  for (const folder of ["alpha", "beta", "includes", "models", "only-includes"])
    await expandFolder(folder, waitFor);
  await expandFolder("variants", waitFor);
  for (const name of ["root.mod", "root.dyn", "policy.dyn", "shared.inc", "calibration.inc", "standalone.inc"])
    await revealRow(name, waitFor);
  await scrollWorkspaceTreeToTop(waitFor);
  await waitFor(async () => {
    const rows = await treeRows();
    return rows.filter(row => row.label === "same.mod").length === 2;
  }, "native workspace tree renders both sibling model rows after expansion");
  let rows = await treeRows();
  for (const label of ["alpha", "beta", "same.mod"])
    assert.ok(rowTextIncludes(rows, label), `native workspace tree shows ${label}`);
  assert.equal(rows.filter(row => row.label === "same.mod").length, 2,
    "same-name models remain separate rows under sibling directories");
  assert.ok(!rowTextIncludes(rows, "alpha/same.mod"), "model labels use basenames while folders carry paths");
  for (const name of ["root.mod", "root.dyn", "policy.dyn", "shared.inc", "calibration.inc", "standalone.inc"]) {
    await revealRow(name, waitFor);
    const fileRow = (await findRows(name))[0];
    assert.ok(fileRow, `native workspace tree shows ${name}`);
    assert.ok(!fileRow.label.includes("/"), `${name} uses a basename label`);
    if (name.endsWith(".inc")) assert.match(fileRow.text, /Include file/, `${name} has the Include file description`);
    if (name.endsWith(".inc")) assert.equal((await treeRows()).filter(row => row.label === name).length, 1, `${name} appears once at its actual path`);
  }
  await scrollWorkspaceTreeToTop(waitFor);

  const alphaPattern = await ignoreFolderRow("alpha", workspaceRoot, waitFor);
  const alphaRootPath = path.join(workspaceRoot, "alpha", "same.mod").toLowerCase();
  await waitFor(async () => {
    workspaceStatus = await status(service);
    return workspaceStatus?.complete && workspaceStatus.roots.find(root => uriPath(root.root_uri) === alphaRootPath)?.state === "excluded";
  }, "Ignore this folder excludes the nested alpha model").catch(error => {
    error.message += `; latest workspaceStatus=${JSON.stringify(workspaceStatus)}`;
    throw error;
  });
  assert.equal(workspaceStatus.roots.length, 5, "ignoring one directory preserves every discovered model root");
  assert.ok(workspaceStatus.include_files.some(value => uriPath(value) === path.join(workspaceRoot, "includes/shared.inc").toLowerCase()),
    "nested model exclusion does not hide a shared include entry");
  await revealRow("shared.inc", waitFor);
  await vscode.workspace.getConfiguration("dynare", vscode.workspace.workspaceFolders[0].uri).update("workspaceExcludePaths", [], vscode.ConfigurationTarget.WorkspaceFolder);
  await waitFor(async () => {
    workspaceStatus = await status(service);
    return workspaceStatus?.complete && workspaceStatus.roots.every(root => root.state === "checked");
  }, "clear nested exclusion before continuing workspace tree checks");
  evidence.checks.push(`Ignore this folder on alpha writes ${alphaPattern}, excludes its model, and retains its shared include`);

  await revealRow("alpha", waitFor);
  await clickRow("alpha", 0, true);
  await waitFor(async () => {
    const current = await findRows("alpha");
    return current[0]?.expanded === "false" && (await treeRows()).filter(row => row.label === "same.mod" || row.text === "same.mod").length === 1;
  }, "collapsing alpha hides only its child files");
  rows = await treeRows();
  assert.equal(rows.filter(row => row.label === "same.mod" || row.text === "same.mod").length, 1,
    "collapsed alpha hides its own model while beta remains visible");
  for (const name of ["root.mod", "policy.dyn", "calibration.inc"]) {
    await revealRow(name, waitFor);
    assert.ok(rowTextIncludes(await treeRows(), name), `${name} remains visible while alpha is collapsed`);
  }
  await revealRow("alpha", waitFor);
  await clickRow("alpha", 0, true);
  await waitFor(async () => rowTextIncludes(await treeRows(), "same.mod"), "expanding alpha restores its file row");

  // Collapse one workspace subfolder, then perform a real status pass. Stable
  // item identities must keep the user's collapse choice through replacement.
  await revealRow("models", waitFor);
  await clickRow("models", 0, true);
  await waitFor(async () => (await findRows("models"))[0]?.expanded === "false", "models folder collapses");
  await vscode.commands.executeCommand("dygnosis.recheckWorkspace");
  await waitFor(async () => { workspaceStatus = await status(service); return workspaceStatus?.complete && workspaceStatus.pass_revision > 1; }, "native workspace Recheck completes a status update");
  await revealRow("models", waitFor);
  await waitFor(async () => (await findRows("models"))[0]?.expanded === "false", "status update preserves the collapsed models folder");
  evidence.checks.push("native workspace tree groups root and nested .mod/.dyn files, keeps independent folder state through status refresh, and displays saved includes once outside coverage counts");

  // Opening one of two equal basenames must use the selected row's URI.
  await revealRow("alpha", waitFor);
  if ((await findRows("alpha"))[0]?.expanded === "false") await clickRow("alpha", 0, true);
  await waitFor(async () => (await findRows("alpha"))[0]?.expanded === "true", "alpha folder is expanded before selecting its model");
  await revealRow("same.mod", waitFor);
  await clickRow("same.mod", 0);
  const alphaPath = path.join(workspaceRoot, "alpha", "same.mod").toLowerCase();
  await waitFor(() => vscode.window.activeTextEditor?.document.uri.fsPath.toLowerCase() === alphaPath, "native Open model action targets the selected alpha URI");
  evidence.checks.push("native Open model action on equal basenames targets the selected directory's exact URI");

  // Include creation, rename and deletion must refresh navigation inventory.
  const added = path.join(workspaceRoot, "only-includes", "created.inc");
  await fs.writeFile(added, "// Created after discovery.\n");
  await waitInventory(service, waitFor, values => values.some(value => uriPath(value) === added.toLowerCase()), "created include enters the inventory");
  await revealRow("created.inc", waitFor);
  const renamed = path.join(workspaceRoot, "only-includes", "renamed.inc");
  await fs.rename(added, renamed);
  await waitInventory(service, waitFor, values => values.some(value => uriPath(value) === renamed.toLowerCase()) && !values.some(value => uriPath(value) === added.toLowerCase()), "renamed include replaces its old inventory URI");
  await revealRow("renamed.inc", waitFor);
  await waitFor(async () => !rowTextIncludes(await treeRows(), "created.inc"), "renamed include removes its old native row");
  await fs.unlink(renamed);
  await waitInventory(service, waitFor, values => !values.some(value => uriPath(value) === renamed.toLowerCase()), "deleted include leaves the inventory");
  await revealRow("only-includes", waitFor);
  await waitFor(async () => !rowTextIncludes(await treeRows(), "renamed.inc"), "deleted include leaves the native tree");
  await revealRow("standalone.inc", waitFor);

  // Clicking a saved include is a navigation action and must preserve the URI.
  await revealRow("standalone.inc", waitFor);
  await clickRow("standalone.inc");
  const includePath = path.join(workspaceRoot, "only-includes", "standalone.inc").toLowerCase();
  await waitFor(() => vscode.window.activeTextEditor?.document.uri.fsPath.toLowerCase() === includePath, "native Open file action targets the exact include URI");
  evidence.checks.push("saved include creation, rename and deletion refresh the tree; opening an include targets its exact URI");

  // The filter must reveal a model under a collapsed branch and restore that branch afterwards.
  const policyPath = path.join(workspaceRoot, "models/variants/policy.dyn");
  await fs.writeFile(policyPath, '@#include "missing-workspace-check.inc"\n');
  await waitFor(async () => {
    workspaceStatus = await status(service);
    return workspaceStatus?.complete && workspaceStatus.counts.incomplete === 1;
  }, "missing include produces one model that is not fully checked");
  const incompletePolicy = workspaceStatus.roots.find(root => vscode.Uri.parse(root.root_uri).fsPath.toLowerCase() === policyPath.toLowerCase());
  assert.ok(incompletePolicy, "the incomplete root uses the policy file identity");
  assert.match(incompletePolicy.cause, /policy\.dyn:1.*missing-workspace-check\.inc/);
  const clickFilter = async title => {
    const clicked = await nativeValue(`(() => { const pane=[...document.querySelectorAll('.pane')].find(value=>value.textContent.includes('Dynare workspace checks')); const action=[...pane.querySelectorAll('.pane-header .action-label')].find(value=>(value.getAttribute('aria-label')||value.title||'').includes(${JSON.stringify(title)})); if(!action) return false; action.click(); return true; })()`);
    assert.equal(clicked, true, `native workspace toolbar contains ${title}`);
  };
  await clickFilter("Show not fully checked models");
  await scrollWorkspaceTreeToTop(waitFor);
  await waitFor(async () => rowTextIncludes(await treeRows(), "policy.dyn"), "filter expands the path to the affected model");
  const filteredRows = await treeRows();
  assert.ok(rowTextIncludes(filteredRows, "missing-workspace-check.inc"), "the affected row identifies the failed include without another model query");
  assert.ok(rowTextIncludes(filteredRows, "models") && rowTextIncludes(filteredRows, "variants"));
  for (const name of ["root.mod", "root.dyn", "same.mod", "shared.inc", "Open Dygnosis Help"])
    assert.ok(!rowTextIncludes(filteredRows, name), `${name} is absent from the affected-model list`);
  assert.equal(await nativeValue(`(() => { const pane=[...document.querySelectorAll('.pane')].find(value=>value.textContent.includes('Dynare workspace checks')); return pane.textContent.includes('Coverage is incomplete. Open affected models for details.'); })()`), false);
  const firstPreview = await vscode.window.showTextDocument(vscode.Uri.file(path.join(workspaceRoot, "root.mod")), { preview: true, viewColumn: vscode.ViewColumn.One });
  assert.equal(vscode.window.tabGroups.activeTabGroup.activeTab.isPreview, true, "first model is a preview tab");
  await currentSnapshot(service, firstPreview.document, () => service.modelInfo(firstPreview.document.uri), "first preview model reaches the language server");
  await waitFor(async () => (await status(service))?.complete, "first preview model finishes updating");
  await waitFor(async () => rowTextIncludes(await treeRows(), "policy.dyn"), "affected model remains visible before opening its preview");
  const closedPreviews = [];
  const closeSubscription = vscode.window.tabGroups.onDidChangeTabs(event => {
    for (const tab of event.closed) if (tab.input?.uri) closedPreviews.push(tab.input.uri.fsPath.toLowerCase());
  });
  await nativeValue(`(() => {
    const pane=[...document.querySelectorAll('.pane')].find(value=>value.textContent.includes('Dynare workspace checks'));
    const trace={removed:false};
    trace.observer=new MutationObserver(()=>{
      const names=[...pane.querySelectorAll('.monaco-list-row .label-name')].map(value=>value.textContent.trim());
      if(!names.includes('policy.dyn')) trace.removed=true;
    });
    trace.observer.observe(pane,{childList:true,subtree:true});
    globalThis.dygnosisWorkspacePreviewTrace=trace;
  })()`);
  try {
    const affectedPreview = await vscode.window.showTextDocument(vscode.Uri.file(policyPath), { preview: true, viewColumn: vscode.ViewColumn.One });
    await waitFor(() => vscode.window.activeTextEditor?.document.uri.fsPath.toLowerCase() === policyPath.toLowerCase(), "affected model opens in the editor");
    await waitFor(() => closedPreviews.includes(path.join(workspaceRoot, "root.mod").toLowerCase()), "opening the model closes its previous preview tab");
    await currentSnapshot(service, affectedPreview.document, () => service.modelInfo(affectedPreview.document.uri), "affected preview reaches the language server");
    await waitFor(async () => (await status(service))?.complete, "opened model finishes its replacement check");
    assert.equal(await nativeValue("globalThis.dygnosisWorkspacePreviewTrace.removed"), false, "opening a model never removes it from the affected-model view during refresh");
    for (const name of [path.basename(workspaceRoot), "models", "variants"])
      assert.equal((await findRows(name))[0]?.expanded, "true", `${name} stays expanded after opening a model`);
  } finally {
    closeSubscription.dispose();
    await nativeValue("globalThis.dygnosisWorkspacePreviewTrace.observer.disconnect(); delete globalThis.dygnosisWorkspacePreviewTrace;");
  }
  evidence.checks.push("opening an affected model replaces its previous preview tab without clearing the filtered view or its expanded paths during pending and running checks");
  await clickFilter("Show all workspace files");
  await revealRow("models", waitFor);
  await waitFor(async () => (await findRows("models"))[0]?.expanded === "false", "clearing the filter restores the collapsed models folder");
  await fs.writeFile(policyPath, files.get("models/variants/policy.dyn"));
  await waitFor(async () => {
    workspaceStatus = await status(service);
    return workspaceStatus?.coverage_complete;
  }, "restoring the include-free model completes workspace coverage");
  evidence.checks.push("native filter buttons reveal incomplete models and their expanded paths, omit other files and duplicate Help rows, and restore full-tree collapse choices without the coverage banner");

  await ignoreFolderRow(path.basename(workspaceRoot), workspaceRoot, waitFor);
  await waitFor(async () => {
    workspaceStatus = await status(service);
    return workspaceStatus?.complete && workspaceStatus.roots.every(root => root.state === "excluded");
  }, "Ignore this folder excludes all workspace model roots");
  assert.equal(workspaceStatus.counts.excluded, 5, "workspace ignore excludes all selected model roots");
  assert.deepEqual(workspaceStatus.include_files.map(uriPath).sort(), expectedIncludes,
    "workspace model exclusion keeps the saved include inventory unchanged");
  await expandFolder("only-includes", waitFor);
  await revealRow("standalone.inc", waitFor);
  evidence.checks.push(`Ignore this folder on alpha writes ${alphaPattern} at workspace-folder scope, excludes its model, and retains include entries`);
  evidence.checks.push("Ignore this folder on the workspace row writes **, excludes every selected model root, and retains the complete include inventory");
}

module.exports = { checkWorkspaceHost };
