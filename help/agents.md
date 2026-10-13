# Work with an agent

Dygnosis provides these MCP tools to agents in VS Code and other MCP clients.
They analyze model source; they do not run Dynare or numerical experiments.

## Understand the model

| Tool | Available result |
|---|---|
| [dynare_model_info](tool:dynare_model_info) | Names, counts, timing, dimensions and model-local definitions. |
| [dynare_equations](tool:dynare_equations) | Equations, tags, identifiers, timing and source locations. |
| [dynare_related_files](tool:dynare_related_files) | Includes and companion files. |
| [dynare_expand](tool:dynare_expand) | Expanded model text and written source locations. |
| [dynare_find_references](tool:dynare_find_references) | Occurrences of a name by identifier spelling. |

## Write and revise

| Tool | Available result |
|---|---|
| [dynare_list_options](tool:dynare_list_options) | Recognized commands and their options. |
| [dynare_rename](tool:dynare_rename) | Rewritten text with matching identifier spellings renamed. |
| [dynare_auto_fix](tool:dynare_auto_fix) | Text with available stored diagnostic fixes applied. |
| [dynare_format](tool:dynare_format) | Formatted model text. |
| [dynare_extract](tool:dynare_extract) | Selected equations with required declarations and model locals, as a fragment. |

MCP reference search and rename match spelling; they do not distinguish
model-local definitions as [editor navigation](navigate-code.md#model-local-variables)
does. Extracted fragments need completion before they can run. Editing tools
return text for the agent to apply. Auto Fix returns the resulting text first;
its second item has a status, message and remaining diagnostics.

## Check and compare

| Tool | Available result |
|---|---|
| [dynare_diagnose](tool:dynare_diagnose) | Diagnostics for supplied model text and includes. |
| [dynare_explain](tool:dynare_explain) | Explanation of a diagnostic code. |
| [dynare_list_diagnostic_codes](tool:dynare_list_diagnostic_codes) | Shared, added and skipped check codes. |
| [dynare_compare_models](tool:dynare_compare_models) | Model and source changes between supplied models or Git and saved Working inputs. |
| [dynare_workspace_diagnose](tool:dynare_workspace_diagnose) | Checks for several model roots, from supplied files or saved paths. |

Supplied-text tools use the text the agent provides; repository comparison
reads saved Working files, not unsaved editor buffers.

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
