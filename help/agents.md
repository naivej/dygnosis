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
| [dynare_model_info](tool:dynare_model_info) | Names, counts, timing, dimensions, model locals and selected declaration, assignment and source sections. |
| [dynare_equations](tool:dynare_equations) | Equations, tags, identifiers, timing and source locations. |
| [dynare_related_files](tool:dynare_related_files) | Includes and companion files. |
| [dynare_expand](tool:dynare_expand) | Expanded model text, selected ranges and verified written source locations. |
| [dynare_symbol_info](tool:dynare_symbol_info) | Declaration, definition, labels, timing and assignments for a selected name in the model. |
| [dynare_find_references](tool:dynare_find_references) | Uses of that name across the selected model. |

## Write and revise

| Tool | Available result |
|---|---|
| [dynare_list_options](tool:dynare_list_options) | Recognized commands and their options. |
| [dynare_rename](tool:dynare_rename) | Proposed edits to rename a symbol across the selected model, with checks that its source has not changed. |
| [dynare_code_actions](tool:dynare_code_actions) | Available source edits for a selected diagnostic or source range. |
| [dynare_auto_fix](tool:dynare_auto_fix) | Selected or bulk diagnostic fixes, including supplied files. |
| [dynare_format](tool:dynare_format) | Formatted model text or selected whole lines. |
| [dynare_extract](tool:dynare_extract) | Selected equations with required declarations and model locals, as a fragment. |

## Check and compare

| Tool | Available result |
|---|---|
| [dynare_diagnose](tool:dynare_diagnose) | Problems found in model text, files supplied by the agent or a saved main model file. |
| [dynare_explain](tool:dynare_explain) | Explanation of a diagnostic code. |
| [dynare_list_diagnostic_codes](tool:dynare_list_diagnostic_codes) | Check codes with the Shared, Added and Skipped labels explained in the [Reference](reference.md). |
| [dynare_compare_models](tool:dynare_compare_models) | Model and source changes between supplied models or Git and saved Working inputs. |
| [dynare_workspace_diagnose](tool:dynare_workspace_diagnose) | Checks for several model roots, from supplied files or saved paths. |

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
