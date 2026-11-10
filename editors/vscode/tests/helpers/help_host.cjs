// Verify native Help access through the Command Palette and Dygnosis's Extensions menu.
const assert = require("node:assert/strict");
const vscode = require("vscode");
const { nativeWorkbench, nativeHelp } = require("./native_webview.cjs");

async function click(point, button = "left") {
  await nativeWorkbench(async send => {
    await send("Input.dispatchMouseEvent", { type: "mouseMoved", x: point.x, y: point.y });
    for (const type of ["mousePressed", "mouseReleased"])
      await send("Input.dispatchMouseEvent", { type, button, buttons: type === "mousePressed" ? button === "right" ? 2 : 1 : 0, clickCount: 1, x: point.x, y: point.y });
  });
}

async function checkHelpAccess(evidence, waitFor) {
  evidence.extension_id = vscode.extensions.getExtension("CoconutWater.dygnosis").id;
  const isHelp = tab => tab.input?.viewType?.endsWith("dygnosis.help");
  const hasHelp = () => vscode.window.tabGroups.all.some(group => group.tabs.some(isHelp));
  await vscode.commands.executeCommand("workbench.action.closeAllEditors");
  await vscode.commands.executeCommand("workbench.action.quickOpen", ">Dygnosis: Open Help");
  await waitFor(async () => nativeWorkbench(async send => {
    const result = await send("Runtime.evaluate", { returnByValue: true, expression:
      "[...document.querySelectorAll('.quick-input-list .monaco-list-row')].some(row=>row.textContent.includes('Dygnosis: Open Help') && row.getBoundingClientRect().height>0)" });
    return result.result.value;
  }), "Open Help is available in the Command Palette with no model open");
  await nativeWorkbench(async send => {
    await send("Input.dispatchKeyEvent", { type: "keyDown", key: "Enter", code: "Enter", windowsVirtualKeyCode: 13 });
    await send("Input.dispatchKeyEvent", { type: "keyUp", key: "Enter", code: "Enter", windowsVirtualKeyCode: 13 });
  });
  await waitFor(hasHelp, "Command Palette opens Help");
  await vscode.commands.executeCommand("workbench.action.closeActiveEditor");
  await waitFor(() => !hasHelp(), "Help closes before testing its other entry point");
  await vscode.commands.executeCommand("workbench.extensions.search", "@id:CoconutWater.dygnosis");
  let extension;
  await waitFor(async () => {
    extension = await nativeWorkbench(async send => {
      const result = await send("Runtime.evaluate", { returnByValue: true, expression:
        "(() => { const row=[...document.querySelectorAll('.extensions-viewlet .monaco-list-row')].find(row=>row.querySelector('.name')?.textContent==='Dygnosis'); const box=row?.getBoundingClientRect(); return box?.width && box.height ? {x:box.x+box.width/2,y:box.y+box.height/2} : null; })()" });
      return result.result.value;
    });
    return extension;
  }, "Dygnosis is available in Extensions");
  await click(extension, "right");
  let help;
  await waitFor(async () => {
    help = await nativeWorkbench(async send => {
      const result = await send("Runtime.evaluate", { returnByValue: true, expression:
        "(() => { const action=[...document.querySelectorAll('.monaco-menu .action-label')].find(item=>item.textContent==='Open Help'); const box=action?.getBoundingClientRect(); return box?.width && box.height ? {x:box.x+box.width/2,y:box.y+box.height/2} : null; })()" });
      return result.result.value;
    });
    return help;
  }, "Dygnosis's Extensions menu offers Open Help").catch(async error => {
    const state = await nativeWorkbench(async send => {
      const result = await send("Runtime.evaluate", { returnByValue: true, expression:
        "({menus:[...document.querySelectorAll('.monaco-menu .action-label')].map(item=>item.textContent),extensions:[...document.querySelectorAll('.extensions-viewlet .monaco-list-row')].map(row=>({text:row.textContent,id:row.getAttribute('data-extension-id')}))})" });
      return result.result.value;
    });
    error.message += `; extension=${evidence.extension_id}; native state=${JSON.stringify(state)}`;
    throw error;
  });
  await nativeWorkbench(async send => {
    await send("Input.dispatchMouseEvent", { type: "mouseMoved", x: help.x, y: help.y });
    await send("Input.dispatchKeyEvent", { type: "keyDown", key: "Enter", code: "Enter", windowsVirtualKeyCode: 13 });
    await send("Input.dispatchKeyEvent", { type: "keyUp", key: "Enter", code: "Enter", windowsVirtualKeyCode: 13 });
  });
  await waitFor(hasHelp, "Extensions menu opens Help").catch(async error => {
    const state = await nativeWorkbench(async send => {
      const result = await send("Runtime.evaluate", { returnByValue: true, expression:
        "({menus:[...document.querySelectorAll('.monaco-menu .action-label')].map(item=>item.textContent),notifications:[...document.querySelectorAll('.notification-list-item-message')].map(item=>item.textContent)})" });
      return result.result.value;
    });
    error.message += `; tabs=${JSON.stringify(vscode.window.tabGroups.all.flatMap(group=>group.tabs).map(tab=>({label:tab.label,type:tab.input?.viewType})))}; native state=${JSON.stringify(state)}`;
    throw error;
  });
  await waitFor(async () => {
    try {
      return await nativeHelp(async ({ evaluate }) => await evaluate("document.querySelector('#article h1')?.textContent") === "Dynare workflow");
    } catch (error) { if (error.code === "NATIVE_FRAME_PENDING") return false; throw error; }
  }, "Extensions menu opens the general Help topic");
  assert.equal(vscode.window.tabGroups.all.flatMap(group => group.tabs).filter(isHelp).length, 1);
  evidence.checks.push("native Command Palette and Dygnosis Extensions context menu each open one Help panel with no model open");

  const readHelp = expression => nativeHelp(({ evaluate }) => evaluate(expression)).catch(error => {
    if (error.code === "NATIVE_FRAME_PENDING") return undefined;
    throw error;
  });
  const initialWidth = await readHelp("innerWidth");
  for (let side = 0; side < 2; side++) {
    const scratch = await vscode.workspace.openTextDocument({ language: "plaintext", content: "" });
    await vscode.window.showTextDocument(scratch, { viewColumn: vscode.ViewColumn.Beside });
  }
  await vscode.commands.executeCommand("workbench.action.focusFirstEditorGroup");
  await waitFor(() => readHelp("innerWidth <= 700 && !document.getElementById('topics').open"), "split Help shows one closed Topics control");
  const disclosure = await readHelp("(() => { const control=document.querySelector('#topics summary'); control.focus(); return control===document.activeElement; })()");
  assert.equal(disclosure, true, "the native Topics disclosure takes keyboard focus");
  await readHelp("document.querySelector('#topics summary').click()");
  await waitFor(() => readHelp("document.getElementById('topics').open"), "Topics opens in the split Help panel");
  await readHelp("document.querySelector('[data-topic=workspace-checks]').click()");
  await waitFor(() => readHelp("!document.getElementById('topics').open && document.querySelector('#article h1')?.textContent === 'Check workspace models' && document.activeElement.id === 'main'"),
    "selecting a topic closes the list and focuses the article");
  if (initialWidth > 700) {
    await vscode.commands.executeCommand("workbench.action.joinAllGroups");
    await waitFor(() => readHelp("innerWidth > 700 && document.getElementById('topics').open"), "wide Help restores topic navigation");
  }
  evidence.checks.push("split Help uses a focusable native Topics control and focuses the selected article");

  await vscode.commands.executeCommand("workbench.action.openSettings", "@ext:CoconutWater.dygnosis block tint");
  let settings;
  await waitFor(async () => {
    settings = await nativeWorkbench(async send => {
      const result = await send("Runtime.evaluate", { returnByValue: true, expression:
        "Array.from(document.querySelectorAll('.setting-item-contents[data-key^=\"dynare.blockTint.\"]')).map(row=>({key:row.getAttribute('data-key'),text:row.textContent}))" });
      return result.result.value;
    });
    return settings?.length > 1;
  }, "native Settings shows block tint controls");
  assert.equal(settings[0].key, "dynare.blockTint.enabled", "the master switch is first");
  assert.match(settings[0].text, /When off, all block tints are disabled/);
  evidence.checks.push("native Settings shows the master tint switch before block options");
}

module.exports = { checkHelpAccess };
