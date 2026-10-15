# Work with an agent

The `use-dynare` skill gives an agent instructions for working with Dynare
models. To install it, run `npx skills add naivej/dygnosis` in a terminal.

Dygnosis also provides model analysis and editing tools through MCP, the
protocol that lets agents call external tools. After connecting the server,
ask your agent to inspect a model, find a name's uses or propose a change.
The tables below use the names shown in the agent's tool list; click a name
to read its reference. Editing tools return proposed changes for the agent to
apply.

## Understand the model

| Tool | Available result |
|---|---|
| [dynare_model_info](tool:dynare_model_info) | Model overview, with selected timing, declaration, assignment, local definition and source relationships. |
| [dynare_equations](tool:dynare_equations) | Equations, tags, identifiers, timing and source locations. |
| [dynare_expand](tool:dynare_expand) | Expanded model text, selected ranges and verified written source locations. |
| [dynare_symbol_info](tool:dynare_symbol_info) | One name's declaration, definition, labels, timing, selected assignments and accepted uses. |

## Write and revise

| Tool | Available result |
|---|---|
| [dynare_syntax](tool:dynare_syntax) | Dynare commands, blocks, instructions, options, functions, operators and macros, with contexts and signatures. |
| [dynare_rename](tool:dynare_rename) | Proposed edits to rename a symbol across the selected model, with checks that its source has not changed. |
| [dynare_actions](tool:dynare_actions) | Available source action summaries, selected complete proposals or compatible proven diagnostic fixes. |
| [dynare_format](tool:dynare_format) | Formatted model text or selected whole lines. |
| [dynare_extract](tool:dynare_extract) | Selected equations with required declarations and model locals, as a fragment. |

## Check and compare

| Tool | Available result |
|---|---|
| [dynare_diagnose](tool:dynare_diagnose) | Problems in one model or a batch of explicit roots or saved paths, with each root's status. |
| [dynare_diagnostic_info](tool:dynare_diagnostic_info) | Diagnostic search and correction guidance, with Shared, Added and Skipped classifications. |
| [dynare_compare_models](tool:dynare_compare_models) | Model and source changes between supplied models or Git and saved Working inputs. |

## Connect MCP

For supported agents such as GitHub Copilot, run **MCP: List Servers** from the
Command Palette and select **Dygnosis**. That list provides the server's start,
stop and Output actions.

For other agents, open your project folder and run **Dygnosis: Set up MCP for
this project** from the Command Palette. It creates or updates `.mcp.json` in
the project. Point your agent to that configuration if it does not load the
file automatically. The command fills in the executable path; the example
below shows the file's structure.

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

[Server path](settings:dynare.serverPath) selects a user-managed executable (available from [GitHub Releases](https://github.com/naivej/dygnosis/releases)) for the editor and VS Code MCP. Run project setup again to update an existing project config.
