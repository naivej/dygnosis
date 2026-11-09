// Check the rendered editor context menu in an isolated native VS Code host.
const assert = require("node:assert/strict");
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

// Guard against adding analysis or owner discovery to menu/caret events.
async function traceLargeMenu(service, workspaceRoot, evidence, { waitFor, currentSnapshot, writeObservedInput }) {
  const filename = path.join(workspaceRoot, "menu-large-trace.mod");
  const equations = Array.from({ length: 1000 }, (_, i) => `[name='trace_${String(i).padStart(4, "0")}'] y=1;`);
  await writeObservedInput(service, filename, `var y;\nmodel;\n${equations.join("\n")}\nend;\n`);
  const document = await vscode.workspace.openTextDocument(filename);
  const editor = await vscode.window.showTextDocument(document);
  await currentSnapshot(service, document, () => service.modelInfo(document.uri), "large menu model snapshot");
  await waitFor(() => service.sourceMenuFacts(document, service.knownOwners(document.uri)).hasNamedEquation, "large model menu facts are warm");
  editor.revealRange(new vscode.Range(502, 0, 502, 0), vscode.TextEditorRevealType.InCenter);
  await vscode.commands.executeCommand("workbench.action.focusActiveEditorGroup");
  await vscode.commands.executeCommand("notifications.clearAll");
  // Let work already queued by opening and revealing the document finish before
  // starting the measured interval. The wrappers count even cached modelInfo calls.
  await new Promise(resolve => setTimeout(resolve, 500));
  const calls = [], originals = new Map();
  for (const name of ["execute", "modelInfo", "ownerChoices"]) {
    const original = service[name];
    assert.equal(typeof original, "function", `${name} is available for the menu trace`);
    originals.set(name, original);
    service[name] = function (...args) {
      calls.push(name === "execute" ? `${name}:${args[0]}` : name);
      return original.apply(this, args);
    };
  }
  let menuOpen = false;
  try {
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
