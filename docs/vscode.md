# Dygnosis in VS Code

Install the native Dygnosis package for the host running your workspace. The
extension bundles the Rust engine; opening a model or a project folder starts
the client. VS Code 1.102.0 is the minimum supported editor. Host and runtime
requirements are in [distribution.md](distribution.md).

Open Settings and search `@ext:dygnosis.dygnosis`. The grouped controls cover
the engine path, model/project views, names, block tint, value hints, actions,
comparison and background checking. Most presentation choices apply to the
displayed resource. The project switch applies to the window; exclusions and
include paths apply to workspace folders. A remote workspace uses its host's
native engine and filesystem paths.

| Task | Guide |
|---|---|
| Model counts, Outline and written equation navigation | [Model view](vscode-model-view.md), [status](vscode-status.md) |
| Name colors, block tint and value hints | [Colors and tint](vscode-colors.md), [LSP presentation settings](editor-settings.md) |
| Browse equations and optional declaration references | [CodeLens](vscode-lenses.md) |
| Ignore, Explain and restore diagnostics | [Diagnostic actions](vscode-diagnostics.md) |
| Compare parameters, equations and shock setup | [Structural Diff](vscode-diff.md) |
| Trace a read-only effective model to written source or macros | [Preview navigation](preview-navigation.md) |
| Check unopened saved models, cancel, recheck and exclude folders | [Project coverage](vscode-project-status.md) |
| Connect agent tools automatically or set up a project | [VS Code MCP](vscode-mcp.md), [project MCP setup](project-mcp.md) |

Dygnosis reports file problems before MATLAB or Octave. For simulations,
estimation and other numerical results, run official Dynare. The
[product README](../README.md) includes the full scope, credits and licenses.
