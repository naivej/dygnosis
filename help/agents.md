# Work with an agent

Dygnosis provides these MCP tools to agents in VS Code and other MCP clients.
They analyze model source; they do not run Dynare or numerical experiments.

## Understand the model

| Tool | Available result |
|---|---|
| [dynare_model_info](tool:dynare_model_info) | Names, counts, timing, dimensions, model locals and selected declaration, assignment and source sections. |
| [dynare_equations](tool:dynare_equations) | Equations, tags, identifiers, timing and source locations. |
| [dynare_related_files](tool:dynare_related_files) | Includes and companion files. |
| [dynare_expand](tool:dynare_expand) | Expanded model text, selected ranges and verified written source locations. |
| [dynare_symbol_info](tool:dynare_symbol_info) | Declaration, definition, labels, timing and assignment evidence for one binding. |
| [dynare_find_references](tool:dynare_find_references) | Accepted uses of a selected symbol binding. |

## Write and revise

| Tool | Available result |
|---|---|
| [dynare_list_options](tool:dynare_list_options) | Recognized commands and their options. |
| [dynare_rename](tool:dynare_rename) | Complete, guarded edits for a selected symbol binding. |
| [dynare_code_actions](tool:dynare_code_actions) | Available source edits for a selected diagnostic or source range. |
| [dynare_auto_fix](tool:dynare_auto_fix) | Selected or bulk diagnostic fixes, including supplied files. |
| [dynare_format](tool:dynare_format) | Formatted model text or selected whole lines. |
| [dynare_extract](tool:dynare_extract) | Selected equations with required declarations and model locals, as a fragment. |

References and Rename distinguish model-local definitions and scopes, as [editor navigation](navigate-code.md#model-local-variables) does. Select a symbol by name, written target or returned binding identity.

Editing tools return guarded proposals for the agent to apply with its file tools.

## Check and compare

| Tool | Available result |
|---|---|
| [dynare_diagnose](tool:dynare_diagnose) | Diagnostics for supplied text, supplied files or saved model roots. |
| [dynare_explain](tool:dynare_explain) | Explanation of a diagnostic code. |
| [dynare_list_diagnostic_codes](tool:dynare_list_diagnostic_codes) | Shared, added and skipped check codes. |
| [dynare_compare_models](tool:dynare_compare_models) | Model and source changes between supplied models or Git and saved Working inputs. |
| [dynare_workspace_diagnose](tool:dynare_workspace_diagnose) | Checks for several model roots, from supplied files or saved paths. |

Single-model tools accept unnamed text, an isolated supplied-file map, or an explicit saved root on the server host. Supplied files keep their keys and cannot read omitted disk files. Repository comparison reads saved Working files.

Results use `schema_version: 2`. Structured clients read facts from `structuredContent.data` and read the outcome, coverage, selection and proposal beside it. Text clients can parse the final JSON content item. Existing default leading text items remain; `detail: "compact"` omits those mirrors. MCP metadata contains the input and guarded-edit rules.

## Connections

The extension registers a **Dygnosis** server with VS Code's MCP controls.
No open model is required. The agent client controls tool access and approval.

**Set up MCP for this project** creates or updates a project `.mcp.json` for
clients that read that format. It requires a trusted workspace, preserves other
server entries, and offers a review for a changed existing setup. Other client
formats need their own configuration.

Setup uses a managed copy of the bundled executable at a stable path. The
extension updates that copy; a running agent needs to reconnect to load the
new version. On Windows, a running server can delay replacement until it stops.
The path belongs to the host where setup ran and is not portable across machines.

[Server path](settings:dynare.serverPath) selects a user-managed executable for
the editor and VS Code MCP. An existing project config changes only when
project setup runs again.

## MCP without VS Code

Standalone binaries are available from
[GitHub Releases](https://github.com/naivej/dygnosis/releases) when published for
the host. The MCP connection uses the executable's absolute path as its command
and `mcp` as a separate argument, over standard input and output. No VS Code or
Node.js installation is required. Client configuration formats differ.
