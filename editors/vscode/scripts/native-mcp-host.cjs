const assert = require("node:assert/strict");
const { canonicalMcpResult, mcpProbeCases } = require("./common.cjs");

const expectedTools = Object.keys(mcpProbeCases).map(name => name.replace(/^dynare_/, ""));
const textSource = { kind: "text", text: "var y; model; y=0; end;\n" };

/** Exercise the real workbench provider and language-model tool routing. */
async function probeNativeMcp(vscode, extensionId) {
  const serverId = `${extensionId.toLowerCase()}/Dygnosis`;
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
    const results = {};
    for (const [name, input] of Object.entries(mcpProbeCases)) {
      const tool = tools.find(item => item.name.endsWith(`_${name}`));
      const result = await vscode.lm.invokeTool(tool.name, { input }, cancellation.token);
      const content = result.content
        .filter(item => item instanceof vscode.LanguageModelTextPart)
        .map(item => ({ type: "text", text: item.value }));
      results[name] = canonicalMcpResult({ content });
    }
    const model = results.dynare_model_info.data.structure;
    assert.equal(model.n_equations, 1); assert.equal(model.n_endogenous, 1);
    const invokeSemantic = async (name, input) => {
      const tool = tools.find(item => item.name.endsWith(`_dynare_${name}`));
      const result = await vscode.lm.invokeTool(tool.name, { input }, cancellation.token);
      return canonicalMcpResult({ content: result.content
        .filter(item => item instanceof vscode.LanguageModelTextPart)
        .map(item => ({ type: "text", text: item.value })) });
    };
    const symbol = await invokeSemantic("symbol_info", { source: textSource, symbol: { kind: "name", name: "y" } });
    assert.equal(symbol.outcome.status, "ok");
    assert.equal(symbol.data.binding.name, "y");
    assert.ok(symbol.data.binding.binding_id, "symbol info must return a reusable binding identity");
    const revision = symbol.inputs?.[0]?.revision;
    assert.ok(revision, "symbol info must return its source revision");
    const reference = await invokeSemantic("symbol_info", {
      source: textSource, symbol: { kind: "binding", binding_id: symbol.data.binding.binding_id },
      sections: ["references"], expected_revision: revision,
    });
    assert.equal(reference.outcome.status, "ok");
    assert.ok(reference.data.references.some(item => item.role === "use"),
      "semantic References must include the selected binding's use");
    const rename = await invokeSemantic("rename", {
      source: textSource, symbol: { kind: "binding", binding_id: symbol.data.binding.binding_id },
      expected_revision: revision, new_name: "z",
    });
    assert.equal(rename.outcome.status, "changed");
    assert.ok(rename.proposal?.edits?.length, "Rename must return its complete guarded proposal");
    assert.equal(rename.proposal.revision, revision, "Rename proposal must retain the selected input revision");
    assert.ok(rename.proposal.edits.every(edit => edit.new_text === "z"), "Rename edits must replace the selected binding");
    return { server_id: serverId, tools: tools.map(item => item.name), tool: tools.find(item => item.name.endsWith("_dynare_model_info")).name, invoked_tools: Object.keys(results),
      n_equations: model.n_equations, semantic_symbol: symbol.data.binding.name, semantic_reference_count: reference.data.references.length,
      guarded_rename_edits: rename.proposal.edits.length, workspace_trusted: vscode.workspace.isTrusted, passed: true };
  } finally {
    cancellation.cancel(); cancellation.dispose();
    await vscode.commands.executeCommand("workbench.mcp.stopServer", serverId);
  }
}
module.exports = { probeNativeMcp };
