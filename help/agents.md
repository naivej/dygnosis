# Work with an agent

The `use-dynare` skill gives AI coding agents domain instructions for building,
checking and replicating Dynare models. To install it in your workspace, run
`npx skills add naivej/dygnosis` in a terminal.

Dygnosis also provides an MCP server so agents can inspect, check, compare and
edit Dynare models directly. Editing tools return proposed changes for your
agent to apply. For example, ask “Find the equations that use `k`” or “Compare
this model with `HEAD~1`.”

Repeated model requests reuse completed analysis automatically. Each request
still checks its inputs.

## Understand the model

| Tool | What it provides |
|---|---|
| [dynare_model_info](tool:dynare_model_info) | Model summary, counts, variable timing classes, declarations, parameter assignments, model locals and included files. |
| [dynare_equations](tool:dynare_equations) | Equations, tags, used symbols, timing and source locations. |
| [dynare_expand](tool:dynare_expand) | Macro- and include-expanded model text with links back to the written source lines. |
| [dynare_symbol_info](tool:dynare_symbol_info) | A symbol's declaration, metadata (`long_name`, TeX), timing, parameter assignments and references. |

## Write and revise

| Tool | What it provides |
|---|---|
| [dynare_syntax](tool:dynare_syntax) | Reference for Dynare commands, blocks, options, functions, operators and macro directives. |
| [dynare_rename](tool:dynare_rename) | Proposed edits to rename a symbol safely across the model and its includes. |
| [dynare_actions](tool:dynare_actions) | Available source refactorings (such as shock templates or bulk naming) and diagnostic quick fixes. |
| [dynare_format](tool:dynare_format) | Indented, formatted model text for a file or selected line range. |
| [dynare_extract](tool:dynare_extract) | Standalone model fragment containing selected equations with their required declarations and model locals. |

## Check and compare

| Tool | What it provides |
|---|---|
| [dynare_diagnose](tool:dynare_diagnose) | Diagnostics and check status for a model or a batch of model roots. |
| [dynare_diagnostic_info](tool:dynare_diagnostic_info) | Explanations and fix guidance for diagnostic codes (`Shared`, `Added` and `Skipped`). |
| [dynare_compare_models](tool:dynare_compare_models) | Structural and text differences between two models or between Git revisions and working files. |

## Connect MCP

For VS Code agents such as GitHub Copilot, run **MCP: List Servers** from the
Command Palette and select **Dygnosis** to start, stop or inspect the server's
output.

For external agents, open a trusted workspace folder and run **Dygnosis: Set up
MCP for this workspace** from the Command Palette. This creates or updates
`.mcp.json` in the selected folder's Git repository root (or workspace folder).
Use [Dynare: Server Path](settings:dynare.serverPath) to select a custom
executable instead of the bundled one. A manual stdio configuration uses:

```json
{
  "mcpServers": {
    "dygnosis": {
      "type": "stdio",
      "command": "<absolute path to dygnosis executable>",
      "args": [
        "mcp"
      ]
    }
  }
}
```
