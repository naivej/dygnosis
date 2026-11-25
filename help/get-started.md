# Dynare workflow

Use Dygnosis to edit and check `.mod`, `.dyn` and `.inc` files before
running Dynare. If syntax highlighting is missing, select **Dynare** in
the language mode picker. For a guided start, choose **Get started with
Dygnosis** from **Getting Started** in VS Code.

| Dynare workflow | VS Code and Dygnosis tools | Topic |
|---|---|---|
| Inspect model counts, `@#include` chains and variable timing | Status bar counts, **Choose model owner**, **Jump to equation...** | [Understand the model and files](model-and-includes.md) |
| Trace declarations, parameters and equations across files | **Go to Definition**, **Find All References**, **Rename symbol**, **Outline**, **Show Call Hierarchy** | [Follow equations and names](navigate-code.md) |
| Write declarations, equations, steady states and shocks | Completion, hover, option hints, `shocks` templates, bulk `long_name` and equation tagging | [Write equations and commands](edit-models.md) |
| Read `long_name` and TeX labels, parameter values and block roles | Inlay hints, block shading, semantic token colors | [Read labels, hints and colors](appearance.md) |
| Preview `@#for`, `@#if` and `@#include`, or test `-D` and `exclude_eqs` | **Expand macros**, **Select analysis configuration** | [Inspect macro expansion](effective-model.md) |
| Catch preprocessor errors and modeling mistakes before MATLAB | **Problems** panel, related locations, **Quick Fix** | [Check the model](diagnostics.md) |
| Compare model revisions, variants or analysis configurations | **Dygnosis Changes** (**Show model changes**) | [Review model changes](structural-diff.md) |
| Check all saved `.mod` and `.dyn` models in a project | **Dynare Workspace** in Explorer | [Check workspace models](workspace-checks.md) |

## Numerical work and agents

Run Dynare in MATLAB or Octave for steady states, Blanchard-Kahn conditions,
simulations and estimation. For MATLAB integration, see the
[MATLAB extension for VS Code](https://github.com/mathworks/MATLAB-extension-for-vscode)
and [MATLAB Agentic Toolkit](https://github.com/matlab/matlab-agentic-toolkit).
[Work with an agent](agents.md) covers Dygnosis MCP setup and the `use-dynare`
skill.

## File support

Editing and checks work without MATLAB, Octave or Dynare installed.
Untitled files support single-file checks. Save the file to resolve
`@#include` and companion file paths through its
[model owner](model-and-includes.md#model-roots-and-includes). Virtual
filesystems provide syntax highlighting only.
