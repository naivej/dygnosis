// Verify Help and Getting Started in the installed VS Code host.
const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const { Buffer } = require("node:buffer");
const vscode = require("vscode");
const { nativeWorkbench, nativeHelp } = require("./native_webview.cjs");
const { compiledModule } = require("./extension_modules.cjs");
const { changesViewType, resourceName, resourceQuery } = require(compiledModule("changes_resource"));

const workbench = expression => nativeWorkbench(async send => {
  const evaluation = await send("Runtime.evaluate", { expression, returnByValue: true });
  assert.equal(evaluation.exceptionDetails, undefined, expression);
  return evaluation.result.value;
});
const readHelp = expression => nativeHelp(({ evaluate }) => evaluate(expression)).catch(error => {
  if (error.code === "NATIVE_FRAME_PENDING") return undefined;
  throw error;
});

async function captureHelp(name) {
  const filename = path.join(path.dirname(process.env.DYGNOSIS_HOST_RESULT), `help-${name}.png`);
  await nativeWorkbench(async send => {
    const screenshot = await send("Page.captureScreenshot", { format: "png", captureBeyondViewport: false });
    await fs.writeFile(filename, Buffer.from(screenshot.data, "base64"));
  });
  return filename;
}

async function checkWalkthrough(evidence, waitFor, model) {
  const category = `${evidence.extension_id}#dygnosis.getStarted`;
  const show = () => vscode.commands.executeCommand("workbench.action.openWalkthrough", category);
  const checked = step => workbench(`document.querySelector('[data-step-id$="#dygnosis.getStarted#${step}"]')?.getAttribute('aria-checked')`);
  await vscode.commands.executeCommand("workbench.action.openWalkthrough");
  const categorySelector = `[x-dispatch="selectCategory:${category}"]`;
  await waitFor(() => workbench(`Boolean(document.querySelector(${JSON.stringify(categorySelector)}) || document.querySelector('[x-dispatch="seeAllWalkthroughs"]'))`), "Welcome lists the walkthroughs");
  const direct = await workbench(`(() => {
    const entry = document.querySelector(${JSON.stringify(categorySelector)});
    (entry ?? document.querySelector('[x-dispatch="seeAllWalkthroughs"]')).click();
    return Boolean(entry);
  })()`);
  if (!direct) {
    const pickerRow = `[...document.querySelectorAll('.quick-input-list .monaco-list-row')].find(row=>row.textContent.includes('Get started with Dygnosis'))`;
    await waitFor(() => workbench(`Boolean(${pickerRow})`), "the walkthrough picker lists Dygnosis");
    await workbench(`(${pickerRow}).click()`);
  }
  await waitFor(() => workbench("document.querySelectorAll('.getting-started-step[data-step-id*=\"#dygnosis.getStarted#\"]').length === 4"), "Getting Started opens the four Dygnosis steps");
  assert.equal(await workbench("[...document.querySelectorAll('.notification-list-item-message')].some(item=>item.textContent.includes('Help explains editing and model checks'))"), false);
  assert.equal(await checked("openModel"), "false");
  const fileStep = '[data-step-id$="#dygnosis.getStarted#openModel"]';
  await workbench(`document.querySelector('${fileStep}').click()`);
  const settings = vscode.workspace.getConfiguration("files");
  const simpleDialog = settings.inspect("simpleDialog.enable")?.globalValue;
  await settings.update("simpleDialog.enable", true, vscode.ConfigurationTarget.Global);
  try {
    await workbench(`[...document.querySelectorAll('${fileStep} a')].find(link=>link.textContent.includes('Open File')).click()`);
    await waitFor(() => workbench("[...document.querySelectorAll('.quick-input-widget input')].some(input=>input.getBoundingClientRect().height>0)"), "walkthrough opens the native file picker");
    await vscode.commands.executeCommand("workbench.action.closeQuickOpen");
    assert.equal(await checked("openModel"), "false", "opening and cancelling the picker does not complete the step");
  } finally {
    await settings.update("simpleDialog.enable", simpleDialog, vscode.ConfigurationTarget.Global);
  }
  await vscode.window.showTextDocument(await vscode.workspace.openTextDocument(model));
  await show();
  await waitFor(async () => await checked("openModel") === "true", "active Dynare model completes its walkthrough step");
  const actions = [
    ["checkModel", "Open Problems", true],
    ["checkWorkspace", "Show workspace checks", !!vscode.workspace.workspaceFolders?.length],
    ["findWorkflow", "Open Help", true],
  ];
  for (const [step, label, complete] of actions) {
    if (step !== "checkModel") assert.equal(await checked(step), "false", step);
    const selector = `[data-step-id$="#dygnosis.getStarted#${step}"]`;
    await workbench(`document.querySelector('${selector}').click()`);
    await workbench(`[...document.querySelectorAll('${selector} a')].find(link=>link.textContent.includes(${JSON.stringify(label)})).click()`);
    if (step === "checkModel") await waitFor(() => workbench("Boolean(document.querySelector('.markers-panel')?.getBoundingClientRect().height)"), "the walkthrough opens Problems");
    if (step === "findWorkflow") await waitFor(() => readHelp("document.querySelector('#article h1')?.textContent === 'Dynare workflow'"), "the walkthrough opens Help");
    await show();
    await waitFor(async () => await checked(step) === String(complete), `${step} records the expected walkthrough progress`);
  }
  evidence.checks.push("native Getting Started opens the file picker and tracks the model, Problems, workspace prerequisite and Help");
}

async function click(point, button = "left") {
  await nativeWorkbench(async send => {
    await send("Input.dispatchMouseEvent", { type: "mouseMoved", x: point.x, y: point.y });
    for (const type of ["mousePressed", "mouseReleased"])
      await send("Input.dispatchMouseEvent", { type, button, buttons: type === "mousePressed" ? button === "right" ? 2 : 1 : 0, clickCount: 1, x: point.x, y: point.y });
  });
}

async function checkHelpAccess(evidence, waitFor) {
  evidence.extension_id = vscode.extensions.getExtension("CoconutWater.dygnosis").id;
  const model = vscode.Uri.file(path.join(path.dirname(process.env.DYGNOSIS_HOST_RESULT), "workspace", "help-model.mod"));
  const isHelp = tab => tab?.input?.viewType?.endsWith("dygnosis.help");
  const hasHelp = () => vscode.window.tabGroups.all.some(group => group.tabs.some(isHelp));
  await fs.writeFile(model.fsPath, "var y;\nmodel; y = 1; end;\n");
  await checkWalkthrough(evidence, waitFor, model);
  await vscode.commands.executeCommand("workbench.action.closeAllEditors");
  await vscode.commands.executeCommand("workbench.action.closePanel");
  await vscode.commands.executeCommand("workbench.action.joinAllGroups");
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
  assert.match(settings[0].text, /When off, all Dynare block tints are disabled/);
  evidence.checks.push("native Settings shows the master tint switch before block options");

  await vscode.commands.executeCommand("dygnosis.openHelp", "appearance");
  await waitFor(() => readHelp("Boolean(document.querySelector('[data-link=\"settings:dynare.MATLAB/OctaveTint\"]'))"), "Appearance Help has the MATLAB/Octave setting link");
  await readHelp("document.querySelector('[data-link=\"settings:dynare.MATLAB/OctaveTint\"]').click()");
  await waitFor(() => nativeWorkbench(async send => {
    const result = await send("Runtime.evaluate", { returnByValue: true, expression:
      "document.querySelector('.setting-item-contents[data-key=\"dynare.MATLAB/OctaveTint\"]')?.querySelector('.setting-item-title')?.textContent" });
    return result.result.value === "Dynare: MATLAB/Octave Tint";
  }), "Appearance Help opens the exact native MATLAB/Octave setting");
  await vscode.commands.executeCommand("dygnosis.openHelp");
  await waitFor(() => readHelp("Boolean(document.querySelector('[data-link=\"settings:dynare.MATLAB/OctaveTint\"]'))"), "plain Help returns to Appearance after Settings");
  evidence.checks.push("Appearance Help opens the exact MATLAB/Octave tint setting");

  await vscode.commands.executeCommand("dygnosis.openHelp", "about#support");
  await waitFor(() => readHelp("Boolean(document.querySelector('#support') && document.querySelector('[data-link=\"action:settings\"]'))"), "About and support provides Open Settings");
  await readHelp("document.querySelector('[data-link=\"action:settings\"]').click()");
  await waitFor(() => workbench(`[...document.querySelectorAll('.settings-editor input, .settings-editor textarea, .settings-editor .settings-header .view-line')]
    .some(node => (node.value ?? node.textContent).trim().toLowerCase() === '@ext:coconutwater.dygnosis'
      && node.getBoundingClientRect().height > 0)`), "the Help link opens Settings filtered to Dygnosis");
  evidence.checks.push("About and support opens native Settings filtered to Dygnosis");

  await vscode.commands.executeCommand("workbench.action.closeAllEditors");
  await vscode.commands.executeCommand("workbench.action.closePanel");
  await vscode.commands.executeCommand("workbench.action.joinAllGroups");
  const editor = await vscode.window.showTextDocument(await vscode.workspace.openTextDocument(model));
  const modelColumn = editor.viewColumn;
  await vscode.commands.executeCommand("dygnosis.openHelp", "reference");
  await waitFor(() => readHelp("document.querySelector('#commands')?.textContent === 'Commands'"), "Reference is shown beside the model");
  const helpGroup = vscode.window.tabGroups.all.find(group => group.activeTab && isHelp(group.activeTab));
  assert.ok(helpGroup, "Help is the visible tab in its group");
  assert.ok(vscode.window.visibleTextEditors.some(visible => visible.document === editor.document), "opening Help keeps the model visible");
  assert.notEqual(helpGroup.viewColumn, modelColumn, "Help is in the neighboring group");
  const search = value => readHelp(`(() => {
    const field = document.getElementById('search');
    field.value = ${JSON.stringify(value)};
    field.dispatchEvent(new Event('input'));
    return { count: document.getElementById('result-count').textContent,
      titles: [...document.querySelectorAll('#result-list button')].map(button => button.firstChild.textContent),
      target: [...document.querySelectorAll('#result-list small')].some(text=>/\\((settings:|help:|https?:)/.test(text.textContent)) };
  })()`);
  const parse = await search("E001");
  assert.match(parse.titles[0], /^E001\b/);
  const shocks = await search("shocks");
  assert.match(shocks.count, /^Showing 50 of (?:[5-9]\d|\d{3,}) matching topics$/);
  assert.equal(shocks.titles.length, 50);
  assert.match(shocks.titles[0], /shocks/i);
  assert.equal(shocks.target, false);
  assert.match((await search("no-matching-dynare-help-topic")).count, /^No Help results/);
  await nativeHelp(async ({ evaluate, send }) => {
    await send("Emulation.setFocusEmulationEnabled", { enabled: true });
    await evaluate("document.getElementById('search').focus()");
    await send("Input.dispatchKeyEvent", { type: "keyDown", key: "Tab", code: "Tab", windowsVirtualKeyCode: 9 });
    await send("Input.dispatchKeyEvent", { type: "keyUp", key: "Tab", code: "Tab", windowsVirtualKeyCode: 9 });
    evidence.help_keyboard = await evaluate("({ active: document.activeElement.id, focused: document.hasFocus() })");
    await waitFor(() => evaluate("document.activeElement.id === 'clear'"), "Tab moves from Search to Clear search");
    await send("Input.dispatchKeyEvent", { type: "keyDown", key: "Enter", code: "Enter", text: "\r", windowsVirtualKeyCode: 13 });
    await send("Input.dispatchKeyEvent", { type: "keyUp", key: "Enter", code: "Enter", windowsVirtualKeyCode: 13 });
    await waitFor(() => evaluate("document.activeElement.id === 'search' && document.getElementById('clear').hidden && !document.getElementById('article').hidden"), "Enter clears search and returns focus to Search");
  });
  for (const section of ["commands", "dygnosis-shortcuts", "diagnostic-codes", "command-options"]) {
    await readHelp(`document.querySelector('[data-link="#${section}"]').click()`);
    await waitFor(() => readHelp(`Math.abs(document.getElementById('${section}').getBoundingClientRect().top - document.getElementById('main').getBoundingClientRect().top) < 50`), `Reference scrolls to ${section}`);
  }
  await readHelp("document.querySelector('[data-link=\"check:E001\"]').click()");
  await waitFor(() => readHelp("/E001/.test(document.querySelector('article h3').textContent)"), "Reference opens the bundled E001 check");
  await vscode.commands.executeCommand("dygnosis.openHelp", { code: "E001", markdown: "# Override explanation\n\nThe active engine supplied this text.", engineVersion: "override" });
  await waitFor(() => readHelp("document.querySelector('#article h1')?.textContent === 'Override explanation'"), "explicit Help request opens the active engine explanation");
  await readHelp("document.getElementById('back').click()");
  await waitFor(() => readHelp("/E001/.test(document.querySelector('article h3')?.textContent)"), "Back returns to the bundled explanation");
  await readHelp("document.getElementById('forward').click()");
  await waitFor(() => readHelp("document.getElementById('edition').textContent === 'Explanation from engine override'"), "Forward restores the active engine explanation");
  await vscode.window.showTextDocument(editor.document, editor.viewColumn);
  await vscode.commands.executeCommand("dygnosis.openHelp");
  assert.equal(await readHelp("document.querySelector('#article h1')?.textContent"), "Override explanation");
  const openAbout = async () => {
    await vscode.commands.executeCommand("dygnosis.openHelp", "about");
    await waitFor(() => readHelp(`document.querySelector('#article h1')?.textContent === 'About and support'
      && document.querySelector('#article > :last-child').textContent === "Help edition ${evidence.extension_version}"`), "About shows the bundled Help edition");
  };
  await openAbout();
  evidence.checks.push("Help stays beside a model, ranks and caps search, clears to the article, follows every Reference section, resumes on reopen and keeps both check explanations in history");
  evidence.help_shocks = shocks.count;

  await vscode.commands.executeCommand("workbench.action.joinAllGroups");
  await vscode.commands.executeCommand("workbench.action.closeSidebar");
  await vscode.commands.executeCommand("workbench.action.closeAuxiliaryBar");
  await openAbout();
  evidence.help_about_wide = await captureHelp("about-wide");
  await vscode.commands.executeCommand("dygnosis.openHelp", "get-started");
  await waitFor(() => readHelp("innerWidth >= 1000 && document.querySelector('#article h1')?.textContent === 'Dynare workflow'"), "wide Help opens the workflow article");
  const measureProse = () => readHelp(`(() => {
    const main = document.getElementById('main'), padding = getComputedStyle(main);
    const paragraph = document.querySelector('#article p');
    const range = document.createRange();
    range.selectNodeContents(paragraph);
    const top = paragraph.getBoundingClientRect().top;
    const lineHeight = parseFloat(getComputedStyle(paragraph).lineHeight);
    const lines = new Set(Array.from(range.getClientRects(), rect => Math.round((rect.top - top) / lineHeight)));
    return { width: innerWidth, proseWidth: paragraph.getBoundingClientRect().width, lines: lines.size,
      availableWidth: main.clientWidth - parseFloat(padding.paddingLeft) - parseFloat(padding.paddingRight) };
  })()`);
  const wideProse = await measureProse();
  assert.ok(Math.abs(wideProse.proseWidth - wideProse.availableWidth) <= 1, JSON.stringify(wideProse));
  await vscode.commands.executeCommand("dygnosis.openHelp", "reference");
  await waitFor(() => readHelp("document.querySelector('#article h1')?.textContent === 'Reference'"), "wide Help opens Reference");
  for (const section of ["commands", "dygnosis-shortcuts", "diagnostic-codes", "command-options"]) {
    await readHelp(`document.querySelector('[data-link="#${section}"]').click()`);
    await waitFor(() => readHelp(`Math.abs(document.getElementById('${section}').getBoundingClientRect().top - document.getElementById('main').getBoundingClientRect().top) < 50`), `wide Reference scrolls to ${section}`);
  }
  assert.equal(await readHelp(`(() => {
    const main = document.getElementById('main');
    main.scrollTop = main.scrollHeight;
    const selected = document.querySelector('#contents [aria-current="page"]').getBoundingClientRect();
    return selected.top >= 0 && selected.bottom <= innerHeight && main.scrollTop > 0;
  })()`), true, "topics stay visible at the end of Reference");
  evidence.help_wide = await captureHelp("wide");

  // Keep the existing Help tab in the first group and size it to about 390 CSS pixels.
  await vscode.commands.executeCommand("dygnosis.openHelp", "get-started");
  await waitFor(() => readHelp("document.querySelector('#article h1')?.textContent === 'Dynare workflow'"), "wide Help returns to the workflow article");
  const ratio = 390 / wideProse.width;
  await vscode.commands.executeCommand("vscode.setEditorLayout", { orientation: 0, groups: [{ size: ratio }, { size: 1 - ratio }] });
  await vscode.commands.executeCommand("workbench.action.focusFirstEditorGroup");
  await waitFor(() => readHelp("innerWidth >= 350 && innerWidth <= 430 && !document.getElementById('topics').open"), "Help fits a split near 390 CSS pixels");
  const splitProse = await measureProse();
  assert.ok(Math.abs(splitProse.proseWidth - splitProse.availableWidth) <= 1, JSON.stringify(splitProse));
  assert.ok(splitProse.lines > wideProse.lines, "resizing the open article reflows the same paragraph");
  evidence.help_reading = { wide: wideProse, split: splitProse };
  await openAbout();
  evidence.help_about_split = await captureHelp("about-split");
  await vscode.commands.executeCommand("dygnosis.openHelp", "get-started");
  await waitFor(() => readHelp("document.querySelector('#article h1')?.textContent === 'Dynare workflow'"), "split Help returns to the workflow article");
  await search("shocks");
  evidence.help_split = await readHelp(`(() => {
    const header = document.querySelector('header'), rows = [];
    for (const control of header.querySelectorAll('input, button')) {
      const { top, height } = control.getBoundingClientRect();
      if (!height) continue;
      const center = top + height / 2;
      if (!rows.some(row => Math.abs(row - center) < 2)) rows.push(center);
    }
    return { width: innerWidth, rows: rows.length,
      height: header.getBoundingClientRect().height, overflow: header.scrollWidth > innerWidth };
  })()`);
  assert.equal(evidence.help_split.overflow, false);
  assert.ok(evidence.help_split.rows <= 2, JSON.stringify(evidence.help_split));
  const theme = vscode.workspace.getConfiguration("workbench");
  const oldTheme = theme.inspect("colorTheme")?.globalValue;
  evidence.help_themes = {};
  try {
    for (const [name, setting] of [["Dark Modern", "Dark Modern"], ["Light Modern", "Light Modern"], ["Dark High Contrast", "Default High Contrast"]]) {
      await theme.update("colorTheme", setting, vscode.ConfigurationTarget.Global);
      await waitFor(() => readHelp(`document.body.dataset.vscodeThemeName === ${JSON.stringify(name)}`), `Help receives the ${name} theme`);
      await readHelp("document.getElementById('clear').click(); document.querySelector('#topics summary').click(); document.querySelector('#contents [aria-current]').focus()");
      const states = await readHelp(`(() => {
        const selected = document.querySelector('#contents [aria-current]');
        const before = getComputedStyle(selected).outlineStyle;
        document.getElementById('search').focus();
        const normal = getComputedStyle(document.querySelector('#contents button:not([aria-current])'));
        const style = getComputedStyle(selected);
        return { before, focus: getComputedStyle(document.activeElement).outlineStyle,
          selectionVisible: style.backgroundColor !== normal.backgroundColor || style.borderTopColor !== normal.borderTopColor };
      })()`);
      assert.equal(states.before, "solid");
      assert.equal(states.focus, "solid");
      assert.equal(states.selectionVisible, true, name);
      await readHelp("document.querySelector('#topics summary').click()");
      assert.equal(await readHelp("document.querySelector('#article h1').getBoundingClientRect().bottom < innerHeight"), true);
      evidence.help_themes[name] = await captureHelp(name.toLowerCase().replaceAll(" ", "-"));
      await search("shocks");
    }
  } finally {
    await theme.update("colorTheme", oldTheme, vscode.ConfigurationTarget.Global);
  }
  const windowSettings = vscode.workspace.getConfiguration("window");
  const zoom = windowSettings.inspect("zoomLevel")?.globalValue;
  try {
    await windowSettings.update("zoomLevel", 2, vscode.ConfigurationTarget.Global);
    await waitFor(() => readHelp("innerWidth < 345"), "increased zoom narrows the Help viewport");
    assert.equal(await readHelp(`(() => {
      const header = document.querySelector('header');
      return header.scrollWidth <= header.clientWidth && header.scrollHeight <= header.clientHeight;
    })()`), true, "zoomed Help retains every toolbar control");
  } finally {
    await windowSettings.update("zoomLevel", zoom, vscode.ConfigurationTarget.Global);
  }
  evidence.checks.push("native Help checks prose reflow, persistent Topics, split toolbar rows and focus versus selection in three themes");

  await vscode.commands.executeCommand("workbench.action.closeAllEditors");
  await vscode.commands.executeCommand("workbench.action.joinAllGroups");
  const before = vscode.Uri.file(path.join(path.dirname(model.fsPath), "help-before.mod"));
  await fs.writeFile(before.fsPath, "var y;\nmodel; y = 0; end;\n");
  const after = { kind: "working", root_uri: model.toString() };
  const resource = { schema_version: 1, before: { kind: "working", root_uri: before.toString() }, after, anchor: after, context_uri: after.root_uri };
  const comparisonUri = vscode.Uri.from({ scheme: "dygnosis-changes", path: `/${resourceName(resource)}`, query: resourceQuery(resource) });
  await vscode.commands.executeCommand("vscode.openWith", comparisonUri, changesViewType);
  const isChanges = tab => tab?.input?.viewType === changesViewType;
  await waitFor(() => isChanges(vscode.window.tabGroups.activeTabGroup.activeTab), "Changes is active before Help opens");
  const comparisonColumn = vscode.window.tabGroups.activeTabGroup.viewColumn;
  await vscode.commands.executeCommand("dygnosis.openHelp", "structural-diff");
  await waitFor(() => readHelp("document.querySelector('#article h1')?.textContent === 'Review model changes'"), "Help opens the comparison topic");
  const visibleTabs = vscode.window.tabGroups.all.map(group => [group.viewColumn, group.activeTab]);
  assert.ok(visibleTabs.some(([column, tab]) => column === comparisonColumn && isChanges(tab)));
  assert.ok(visibleTabs.some(([column, tab]) => column !== comparisonColumn && isHelp(tab)));
  evidence.checks.push("opening Help from Changes keeps both editors visible in different groups");
}

module.exports = { checkHelpAccess };
