// Check inactive-source Outline, hover, and rendered dimming in the native editor.
const assert = require("node:assert/strict");
const vscode = require("vscode");
const { nativeWorkbench } = require("./native_webview.cjs");

async function checkInactiveSource(open, evidence, waitFor) {
  const source = "var y;\nmodel; [name='law'] y=y(-1); end;\n@#if 0\nvar_model(model_name=forecast,eqtags=['law'],structural);\n@#else\nstoch_simul(order=1,nograph);\n@#endif\n";
  const input = await open("inactive-source.mod", source);
  assert.ok(input.info.inactive_ranges?.length > 0, "cold model information supplies inactive proof");
  const rows = await vscode.commands.executeCommand("vscode.executeDocumentSymbolProvider", input.document.uri);
  const flatten = rows => rows.flatMap(row => [row, ...flatten(row.children ?? [])]);
  assert.ok(flatten(rows).some(row => row.name === "var_model(forecast)"));
  const hovers = await vscode.commands.executeCommand("vscode.executeHoverProvider", input.document.uri, new vscode.Position(3, 46));
  const description = hovers.flatMap(hover => hover.contents.map(content => content.value ?? content)).join("\n");
  assert.match(description, /contemporaneous/); assert.doesNotMatch(description, /inactive/);
  const appearance = vscode.workspace.getConfiguration("workbench"), settings = vscode.workspace.getConfiguration("dynare");
  const previousTheme = appearance.inspect("colorTheme").workspaceValue;
  const previousEnabled = settings.inspect("inactiveCode.enabled").workspaceValue;
  const styles = () => nativeWorkbench(async send => {
    const response = await send("Runtime.evaluate", { returnByValue: true, expression:
      `(() => { const state=needle=>{const line=[...document.querySelectorAll('.view-lines .view-line')].find(line=>line.textContent.includes(needle));return line?{dim:[...line.querySelectorAll('span')].some(span=>Number(getComputedStyle(span).opacity)<1),colors:[...new Set([...line.querySelectorAll('span')].map(span=>getComputedStyle(span).color))]}:null};return {inactive:state('var_model('),active:state('stoch_simul(')}; })()` });
    return response.result.value;
  });
  try {
    await settings.update("inactiveCode.enabled", true, vscode.ConfigurationTarget.Workspace);
    for (const [theme, kind] of [["Default Dark Modern", vscode.ColorThemeKind.Dark], ["Default Light Modern", vscode.ColorThemeKind.Light],
      ["Default High Contrast", vscode.ColorThemeKind.HighContrast], ["Default High Contrast Light", vscode.ColorThemeKind.HighContrastLight]]) {
      await appearance.update("colorTheme", theme, vscode.ConfigurationTarget.Workspace);
      await waitFor(() => vscode.window.activeColorTheme.kind === kind, `${theme} is applied`);
      let observed;
      await waitFor(async () => {
        observed = await styles();
        return observed.inactive?.dim === true && observed.active?.dim === false;
      }, `${theme} dims only inactive source`);
      assert.ok(observed.inactive.colors.length > 0);
    }
    await settings.update("inactiveCode.enabled", false, vscode.ConfigurationTarget.Workspace);
    await waitFor(async () => (await styles()).inactive?.dim === false, "inactive dimming can be disabled live");
    evidence.checks.push("inactive source: cold ranges, named Outline, local option hover without activity labels, and exact dimming in dark/light/high-contrast themes with live disabling");
  } finally {
    await appearance.update("colorTheme", previousTheme, vscode.ConfigurationTarget.Workspace);
    await settings.update("inactiveCode.enabled", previousEnabled, vscode.ConfigurationTarget.Workspace);
  }
}

module.exports = { checkInactiveSource };
