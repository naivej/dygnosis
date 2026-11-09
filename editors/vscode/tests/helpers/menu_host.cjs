// Check the rendered editor context menu in an isolated native VS Code host.
const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const vscode = require("vscode");
const { nativeWorkbench } = require("./native_webview.cjs");

async function evaluate(expression) {
  return nativeWorkbench(async send => {
    const result = await send("Runtime.evaluate", { expression, returnByValue: true });
    assert.equal(result.exceptionDetails, undefined, "native menu inspection succeeds");
    return result.result?.value;
  });
}

async function openMenu(line, waitFor) {
  let point;
  await waitFor(async () => {
    point = await evaluate(`(() => { const e = [...document.querySelectorAll('.monaco-editor.focused .view-line')].find(e => e.textContent.includes(${JSON.stringify(line)})); if (!e) return null; const r = e.getBoundingClientRect(); return {x:r.left + Math.min(35, r.width / 2), y:r.top + r.height / 2}; })()`);
    return !!point;
  }, `native editor renders ${line}`);
  await nativeWorkbench(async send => {
    await send("Input.dispatchMouseEvent", { type: "mouseMoved", ...point });
    await send("Input.dispatchMouseEvent", { type: "mousePressed", button: "right", buttons: 2, clickCount: 1, ...point });
    await send("Input.dispatchMouseEvent", { type: "mouseReleased", button: "right", buttons: 0, clickCount: 1, ...point });
  });
  let rows;
  await waitFor(async () => {
    rows = await evaluate(`(() => { const roots = [document, ...[...document.querySelectorAll('.shadow-root-host')].map(e => e.shadowRoot).filter(Boolean)]; const menu = roots.flatMap(root => [...root.querySelectorAll('.monaco-menu')]).find(e => e.getClientRects().length); return menu && [...menu.querySelectorAll('.action-item')].map(e => ({text:e.innerText.trim().replace(/\\s+/g, ' '), separator:e.classList.contains('separator'), aria:e.getAttribute('aria-label')})); })()`);
    return rows?.length;
  }, "native editor context menu opens").catch(async error => {
    error.message += `; point ${JSON.stringify(point)} hit ${JSON.stringify(await evaluate(`document.elementFromPoint(${point.x},${point.y})?.outerHTML.slice(0,1000)`))}; menu DOM: ${JSON.stringify(await evaluate(`([...document.querySelectorAll('[role="menu"], .context-view')].map(e => ({html:e.outerHTML.slice(0,3000), rect:e.getBoundingClientRect().toJSON()})))`))}`;
    throw error;
  });
  return rows;
}

async function closeMenu() {
  await nativeWorkbench(async send => {
    await send("Input.dispatchKeyEvent", { type: "keyDown", key: "Escape", code: "Escape", windowsVirtualKeyCode: 27, nativeVirtualKeyCode: 27 });
    await send("Input.dispatchKeyEvent", { type: "keyUp", key: "Escape", code: "Escape", windowsVirtualKeyCode: 27, nativeVirtualKeyCode: 27 });
  });
}

// Guard discovery of saved model owners before any parent is opened in VS Code.
async function checkColdOwners(service, workspaceRoot, evidence, waitFor) {
  const settings = vscode.workspace.getConfiguration("dynare");
  const previous = settings.inspect("workspaceDiagnostics")?.workspaceValue;
  const folder = path.join(workspaceRoot, "menu-cold-owners");
  const parentNames = ["cold-a.mod", "cold-b.dyn", "cold-single.mod"];
  const parentPaths = parentNames.map(name => path.join(folder, name));
  const sharedPath = path.join(folder, "cold-shared.mod");
  const incPath = path.join(folder, "cold-shared.inc");
  const singlePath = path.join(folder, "cold-single-include.mod");
  let menuOpen = false;
  try {
    await settings.update("workspaceDiagnostics", false, vscode.ConfigurationTarget.Workspace);
    await fs.mkdir(folder, { recursive: true });
    await Promise.all([
      fs.writeFile(sharedPath, "y=1; // cold-mod-site\n"),
      fs.writeFile(incPath, "z=1; // cold-inc-site\n"),
      fs.writeFile(singlePath, "q=1; // cold-single-site\n"),
      ...parentPaths.slice(0, 2).map(file => fs.writeFile(file, 'var y z;\nmodel;\n@#include "cold-shared.mod"\n@#include "cold-shared.inc"\nend;\n')),
      fs.writeFile(parentPaths[2], 'var q;\nmodel;\n@#include "cold-single-include.mod"\nend;\n'),
    ]);
    await settings.update("workspaceDiagnostics", true, vscode.ConfigurationTarget.Workspace);
    await waitFor(async () => {
      const status = await service.execute("dynare/workspaceStatus");
      return status?.complete && parentPaths.every(file => status.roots.some(root => vscode.Uri.parse(root.root_uri).fsPath.toLowerCase() === file.toLowerCase() && root.state === "checked"));
    }, "workspace checks complete for saved unopened parents");
    const assertParentsClosed = () => assert.ok(parentPaths.every(file => !vscode.workspace.textDocuments.some(doc => doc.uri.fsPath.toLowerCase() === file.toLowerCase())), "cold owner parents remain unopened");
    assertParentsClosed();
    let incDocument;
    for (const [filename, marker] of [[sharedPath, "cold-mod-site"], [incPath, "cold-inc-site"]]) {
      const document = await vscode.workspace.openTextDocument(filename);
      if (filename === incPath) incDocument = document;
      await vscode.window.showTextDocument(document);
      await waitFor(() => service.knownOwners(document.uri).filter(owner => parentPaths.slice(0, 2).some(file => owner.fsPath.toLowerCase() === file.toLowerCase())).length === 2, `${marker} receives two workspace owners`);
      assertParentsClosed();
      await vscode.commands.executeCommand("workbench.action.focusActiveEditorGroup");
      await waitFor(async () => {
        const rows = await openMenu(marker, waitFor); menuOpen = true;
        const found = rows.some(row => row.text.includes("Choose model owner"));
        await closeMenu(); menuOpen = false;
        return found;
      }, `${marker} native menu offers Choose model owner`);
      // The isolated hidden host has unreliable menu-item pointer activation.
      // Keep the real text right-click and use the command's documented URI.
      let commandState = "pending", candidates, observedUi;
      void vscode.commands.executeCommand("dygnosis.chooseModelOwner", document.uri).then(() => { commandState = "resolved"; }, error => { commandState = String(error); });
      await waitFor(async () => {
        observedUi = await evaluate(`(() => { const roots=[document,...[...document.querySelectorAll('.shadow-root-host')].map(e=>e.shadowRoot).filter(Boolean)]; return roots.flatMap(root=>[...root.querySelectorAll('.quick-input-widget,.notification-toast')]).filter(e=>e.getClientRects().length && getComputedStyle(e).display!=='none').map(e=>({text:e.innerText,placeholder:e.querySelector('input')?.placeholder,rows:[...e.querySelectorAll('.monaco-list-row')].map(r=>r.innerText)})); })()`);
        candidates = observedUi.find(item => item.placeholder === "Choose the model that owns this include")?.rows;
        return candidates?.some(row => row.includes("cold-a.mod")) && candidates?.some(row => row.includes("cold-b.dyn"));
      }, `${marker} native owner picker lists both unopened parents`).catch(async error => {
        error.message += `; command=${commandState}; candidates=${JSON.stringify(candidates)}; UI=${JSON.stringify(observedUi)}`;
        throw error;
      });
      await nativeWorkbench(async send => {
        await send("Input.insertText", { text: "cold-b.dyn" });
        await send("Input.dispatchKeyEvent", { type: "keyDown", key: "Enter", code: "Enter", windowsVirtualKeyCode: 13, nativeVirtualKeyCode: 13 });
        await send("Input.dispatchKeyEvent", { type: "keyUp", key: "Enter", code: "Enter", windowsVirtualKeyCode: 13, nativeVirtualKeyCode: 13 });
      });
      await waitFor(() => service.chosenRootForDocument(document)?.fsPath.toLowerCase() === parentPaths[1].toLowerCase(), `${marker} accepts the unopened .dyn parent`);
      assertParentsClosed();
      evidence.checks.push(`workspace discovery offers both unopened parents for ${path.extname(filename)}; the native picker selects the closed .dyn parent`);
    }
    assert.equal(service.chosenRootForDocument(incDocument)?.fsPath.toLowerCase(), parentPaths[1].toLowerCase(), "the include retains its explicit owner before parent removal");
    const beforeRemoval = await service.execute("dynare/workspaceStatus");
    await fs.writeFile(parentPaths[1], "var y; model; y=1; end;\n");
    await waitFor(async () => {
      const status = await service.execute("dynare/workspaceStatus");
      const owners = service.knownOwners(vscode.Uri.file(sharedPath));
      return status?.complete && status.pass_revision > beforeRemoval.pass_revision &&
        owners.some(owner => owner.fsPath.toLowerCase() === parentPaths[0].toLowerCase()) &&
        !owners.some(owner => owner.fsPath.toLowerCase() === parentPaths[1].toLowerCase());
    }, "workspace recheck removes the former closed parent from owner choices");
    assertParentsClosed();
    evidence.checks.push("editing an unopened parent to remove an include removes that parent from current workspace owners");
    const single = await vscode.workspace.openTextDocument(singlePath);
    await vscode.window.showTextDocument(single);
    await waitFor(() => service.knownOwners(single.uri).some(owner => owner.fsPath.toLowerCase() === parentPaths[2].toLowerCase()), "single model include receives its unopened workspace owner");
    await waitFor(async () => (await service.rootForDocument(single))?.fsPath.toLowerCase() === parentPaths[2].toLowerCase(), "single workspace owner is selected");
    await vscode.commands.executeCommand("workbench.action.focusActiveEditorGroup");
    await waitFor(async () => {
      const rows = await openMenu("cold-single-site", waitFor); menuOpen = true;
      const found = rows.some(row => row.text.includes("Treat this file as a model root"));
      await closeMenu(); menuOpen = false;
      return found;
    }, "single cold model include offers Treat this file as a model root");
    assertParentsClosed();
    evidence.checks.push("a model include with one unopened workspace owner offers Treat this file as a model root");
  } finally {
    try { if (menuOpen) await closeMenu(); }
    finally { await settings.update("workspaceDiagnostics", previous, vscode.ConfigurationTarget.Workspace); }
  }
}

// Guard against adding analysis or owner discovery to menu/caret events.
async function traceLargeMenu(service, workspaceRoot, evidence, { waitFor, currentSnapshot, writeObservedInput }) {
  const filename = path.join(workspaceRoot, "menu-large-trace.mod");
  const equations = Array.from({ length: 1000 }, (_, i) => `[name='trace_${String(i).padStart(4, "0")}'] y=1;`);
  await writeObservedInput(service, filename, `var y;\nmodel;\n${equations.join("\n")}\nend;\n`);
  const document = await vscode.workspace.openTextDocument(filename);
  const editor = await vscode.window.showTextDocument(document);
  await currentSnapshot(service, document, () => service.modelInfo(document.uri), "large menu model snapshot");
  await waitFor(() => service.sourceMenuFacts(document, service.knownOwners(document.uri)).hasNamedEquation, "large model menu facts are warm");
  if (vscode.workspace.getConfiguration("dynare").get("workspaceDiagnostics")) {
    await waitFor(async () => {
      const status = await service.execute("dynare/workspaceStatus");
      return status?.complete && status.roots.some(root => vscode.Uri.parse(root.root_uri).fsPath.toLowerCase() === filename.toLowerCase() && root.state === "checked");
    }, "large model workspace pass completes before the menu trace");
  }
  editor.revealRange(new vscode.Range(502, 0, 502, 0), vscode.TextEditorRevealType.InCenter);
  await vscode.commands.executeCommand("workbench.action.focusActiveEditorGroup");
  await vscode.commands.executeCommand("notifications.clearAll");
  // The wrappers count even cached modelInfo calls. Wait for already queued
  // owner and status work to settle before starting the measured interval.
  const calls = [], originals = new Map();
  let lastCall = Date.now();
  for (const name of ["execute", "modelInfo", "ownerChoices"]) {
    const original = service[name];
    assert.equal(typeof original, "function", `${name} is available for the menu trace`);
    originals.set(name, original);
    service[name] = function (...args) {
      calls.push(name === "execute" ? `${name}:${args[0]}` : name);
      lastCall = Date.now();
      return original.apply(this, args);
    };
  }
  let menuOpen = false;
  try {
    await waitFor(() => Date.now() - lastCall >= 1000, "large model owner and status activity settles before tracing");
    calls.length = 0;
    for (const index of [500, 501, 502]) {
      editor.selection = new vscode.Selection(index + 2, 0, index + 2, 0);
      const label = `trace_${String(index).padStart(4, "0")}`;
      const rows = await openMenu(label, waitFor); menuOpen = true;
      assert.ok(rows.some(row => row.text.includes("Filter analysis by equation")), "the large model menu is the active text menu");
      await closeMenu(); menuOpen = false;
    }
    await new Promise(resolve => setTimeout(resolve, 500));
    assert.deepEqual(calls, [], `menu opens and caret moves must not request analysis or owner discovery: ${JSON.stringify(calls)}`);
    evidence.menu_request_trace = { equations: equations.length, menu_opens: 3, caret_moves: 3, observed_calls: calls };
    evidence.checks.push("warm 1,000-equation model: three text menus and caret moves issue no analysis, model-info or owner-discovery calls");
  } finally {
    for (const [name, original] of originals) service[name] = original;
    if (menuOpen) await closeMenu();
  }
}

async function checkMenu(service, workspaceRoot, evidence, { waitFor, currentSnapshot, writeObservedInput }) {
  assert.ok(process.env.DYGNOSIS_HOST_CDP_PORT, "menu acceptance requires the isolated native host CDP port");
  const filename = path.join(workspaceRoot, "menu-acceptance.mod");
  const source = "@#define FLAG = 1\n@#if FLAG\nvar y;\nmodel;\n[name='policy'] y=1;\nend;\n@#endif\n";
  await writeObservedInput(service, filename, source);
  const document = await vscode.workspace.openTextDocument(filename);
  const editor = await vscode.window.showTextDocument(document);
  await currentSnapshot(service, document, () => service.modelInfo(document.uri), "menu fixture model snapshot");
  await vscode.commands.executeCommand("workbench.action.focusActiveEditorGroup");
  const settings = vscode.workspace.getConfiguration("dynare"), previous = settings.inspect("editorActions")?.workspaceValue;
  let menuOpen = false;
  try {
    await settings.update("editorActions", ["toolbar", "contextMenu"], vscode.ConfigurationTarget.Workspace);
    editor.selection = new vscode.Selection(0, 0, 0, 0);
    let rows = await openMenu("policy", waitFor); menuOpen = true;
    await waitFor(() => editor.selection.active.line === 4, "first right-click moves the caret to the clicked equation");
    const labels = rows.map(row => row.text);
    const own = rows.map((row, index) => ({ ...row, index })).filter(row => row.text.startsWith("Dygnosis:"));
    assert.ok(own.length >= 1, `expected direct Dygnosis rows: ${JSON.stringify(rows)}`);
    assert.ok(own.every((row, index) => !index || row.index === own[index - 1].index + 1), "Dygnosis rows form one contiguous block");
    assert.ok(own.every(row => (row.text.match(/dygnosis:/gi) ?? []).length === 1), "each row has one prefix");
    assert.ok(!labels.some(label => label === "Dygnosis"), "the old submenu is absent");
    const call = labels.findIndex(label => label.includes("Show Call Hierarchy"));
    assert.ok(call >= 0 && call < own[0].index, "the block follows Show Call Hierarchy");
    const chat = labels.findIndex(label => label.includes("Add File to Chat"));
    if (chat >= 0) assert.ok(own.at(-1).index < chat, "the block precedes Add File to Chat");
    const rename = rows.find(row => row.text.startsWith("Rename symbol"));
    assert.ok(rename && /F2/.test(`${rename.text} ${rename.aria ?? ""}`), `Rename retains its native label and F2: ${JSON.stringify(rename)}`);
    evidence.menu_rows = rows;
    evidence.checks.push("first text right-click renders a contiguous direct Dygnosis block after navigation and before Chat when present; Rename keeps its native label and F2");
    await closeMenu(); menuOpen = false;
    await waitFor(async () => {
      rows = await openMenu("policy", waitFor); menuOpen = true;
      const ready = ["Override macro definition for analysis", "Skip macro definition for analysis", "Preview macro branch", "Filter analysis by equation"].every(label => rows.some(row => row.text.includes(label)));
      await closeMenu(); menuOpen = false;
      return ready;
    }, "file facts expose macro and equation actions away from their exact target");
    evidence.checks.push("macro definition, branch and named-equation rows use current file facts on the first text menu");
    await vscode.commands.executeCommand("notifications.clearAll");
    editor.selection = new vscode.Selection(4, 0, 4, 0);
    rows = await openMenu("@#define", waitFor); menuOpen = true;
    await closeMenu(); menuOpen = false;
    void vscode.commands.executeCommand("dygnosis.overrideMacroDefinition", document.uri).then(undefined, () => {});
    await waitFor(async () => evaluate(`(() => { const roots = [document, ...[...document.querySelectorAll('.shadow-root-host')].map(e => e.shadowRoot).filter(Boolean)]; const widget = roots.flatMap(root => [...root.querySelectorAll('.quick-input-widget')]).find(e => e.getClientRects().length && getComputedStyle(e).display !== 'none'); return !!widget && widget.textContent.includes('Override FLAG') && widget.textContent.includes('menu-acceptance.mod'); })()`), "clicked macro action opens input for the clicked document and definition").catch(async error => {
      error.message += `; selection=${JSON.stringify(editor.selection)}; visible UI=${JSON.stringify(await evaluate(`(() => { const roots = [document, ...[...document.querySelectorAll('.shadow-root-host')].map(e => e.shadowRoot).filter(Boolean)]; return roots.flatMap(root => [...root.querySelectorAll('.quick-input-widget, .notification-toast')]).filter(e=>e.getClientRects().length).map(e=>e.innerText); })()`))}`;
      throw error;
    });
    await closeMenu();
    evidence.checks.push("right-clicking a different line then invoking Override with the document URI opens the FLAG input");
    await vscode.commands.executeCommand("dygnosis.showEffectiveModel");
    await waitFor(() => vscode.window.activeTextEditor?.document.uri.scheme === "dygnosis-effective", "macro expansion preview opens");
    await vscode.commands.executeCommand("workbench.action.focusActiveEditorGroup");
    await waitFor(async () => {
      rows = await openMenu("y=1", waitFor); menuOpen = true;
      const ownRows = rows.filter(row => row.text.startsWith("Dygnosis:"));
      const ready = ownRows.length === 1 && ownRows[0].text.includes("Go to written source");
      await closeMenu(); menuOpen = false;
      return ready;
    }, "current preview offers only Go to written source");
    evidence.checks.push("current macro expansion preview offers only Go to written source in the Dygnosis block");
    await vscode.window.showTextDocument(document);
    await vscode.commands.executeCommand("workbench.action.focusActiveEditorGroup");
    const sharedPath = path.join(workspaceRoot, "menu-shared.mod");
    await writeObservedInput(service, sharedPath, "y=1; // shared-menu-site\n");
    let shared;
    for (const name of ["menu-parent-a.mod", "menu-parent-b.mod"]) {
      const parentPath = path.join(workspaceRoot, name);
      await writeObservedInput(service, parentPath, 'var y;\nmodel;\n@#include "menu-shared.mod"\nend;\n');
      const parent = await vscode.workspace.openTextDocument(parentPath);
      await vscode.window.showTextDocument(parent);
      await currentSnapshot(service, parent, () => service.modelInfo(parent.uri), `${name} owner snapshot`);
      if (name === "menu-parent-a.mod") {
        shared = await vscode.workspace.openTextDocument(sharedPath);
        await vscode.window.showTextDocument(shared);
        await waitFor(async () => (await service.rootForDocument(shared))?.toString() === parent.uri.toString(), "shared model selects its sole owner");
        await vscode.commands.executeCommand("workbench.action.focusActiveEditorGroup");
        await waitFor(async () => {
          rows = await openMenu("shared-menu-site", waitFor); menuOpen = true;
          const ready = rows.some(row => row.text.includes("Treat this file as a model root"));
          await closeMenu(); menuOpen = false;
          return ready;
        }, "single-owner model offers Treat this file as a model root");
        evidence.checks.push("a model include with one current owner offers Treat this file as a model root");
      }
    }
    await vscode.window.showTextDocument(shared);
    await waitFor(() => service.knownOwners(shared.uri).length >= 2, "shared model retains two known owners");
    await vscode.commands.executeCommand("workbench.action.focusActiveEditorGroup");
    await waitFor(async () => {
      rows = await openMenu("shared-menu-site", waitFor); menuOpen = true;
      const ready = rows.some(row => row.text.includes("Choose model owner"));
      await closeMenu(); menuOpen = false;
      return ready;
    }, "shared source offers Choose model owner");
    evidence.checks.push("a shared model with two known owners offers Choose model owner in its text menu");
    await traceLargeMenu(service, workspaceRoot, evidence, { waitFor, currentSnapshot, writeObservedInput });
    await checkColdOwners(service, workspaceRoot, evidence, waitFor);
    await vscode.window.showTextDocument(document);
    await vscode.commands.executeCommand("workbench.action.focusActiveEditorGroup");
    await settings.update("editorActions", ["toolbar"], vscode.ConfigurationTarget.Workspace);
    await waitFor(async () => {
      rows = await openMenu("policy", waitFor); menuOpen = true;
      const hidden = !rows.some(row => row.text.startsWith("Dygnosis:"));
      assert.ok(rows.some(row => row.text.startsWith("Rename symbol") && /F2/.test(row.text)), "Rename remains available when context actions are hidden");
      await closeMenu(); menuOpen = false;
      return hidden;
    }, "editorActions hides the direct context rows");
    evidence.checks.push("editorActions removes direct Dygnosis rows from the native text menu");
  } finally {
    if (menuOpen) await closeMenu();
    await settings.update("editorActions", previous, vscode.ConfigurationTarget.Workspace);
  }
}

module.exports = { checkMenu };
