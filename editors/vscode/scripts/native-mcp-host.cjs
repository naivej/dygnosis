const assert = require("node:assert/strict");

const expectedTools = ["auto_fix", "compare_models", "diagnose", "equations", "expand", "explain", "extract", "find_references", "format", "list_diagnostic_codes", "list_options", "model_info", "related_files", "rename", "workspace_diagnose"];

/** Exercise the real workbench provider and language-model tool routing. */
async function probeNativeMcp(vscode, extensionId) {
  const serverId = `${extensionId}/Dygnosis`;
  const cancellation = new vscode.CancellationTokenSource();
  try {
    assert.ok((await vscode.commands.getCommands(true)).includes("workbench.mcp.startServer"));
    let tools = [];
    for (let attempt = 0; attempt < 100; ++attempt) {
      await vscode.commands.executeCommand("workbench.mcp.startServer", serverId);
      tools = vscode.lm.tools.filter(tool => expectedTools.some(name => tool.name.endsWith(`_dynare_${name}`)));
      if (expectedTools.every(name => tools.some(tool => tool.name.endsWith(`_dynare_${name}`)))) break;
      await new Promise(resolve => setTimeout(resolve, 100));
    }
    for (const name of expectedTools) assert.ok(tools.some(tool => tool.name.endsWith(`_dynare_${name}`)), `Native MCP omitted dynare_${name}`);
    const tool = tools.find(item => item.name.endsWith("_dynare_model_info"));
    const result = await vscode.lm.invokeTool(tool.name, { input: { file_content: "var y; model; y=0; end;" } }, cancellation.token);
    const text = result.content.filter(item => item instanceof vscode.LanguageModelTextPart).map(item => item.value).join("\n");
    const model = JSON.parse(text);
    assert.equal(model.n_equations, 1); assert.equal(model.n_endogenous, 1);
    return { server_id: serverId, tools: tools.map(item => item.name), tool: tool.name,
      n_equations: model.n_equations, workspace_trusted: vscode.workspace.isTrusted, passed: true };
  } finally {
    cancellation.cancel(); cancellation.dispose();
    await vscode.commands.executeCommand("workbench.mcp.stopServer", serverId);
  }
}
module.exports = { probeNativeMcp };
