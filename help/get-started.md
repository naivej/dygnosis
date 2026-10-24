# Dynare workflow

Dygnosis adds Dynare language support to VS Code for `.mod`, `.dyn` and `.inc`
files. Editing and source analysis work without a separate Dynare, MATLAB or
Octave installation.

Open a model file and check that the language mode in the bottom-right status
bar is **Dynare**. If it shows another language, click it and select
**Dynare**.

Press `F1` to open the **Command Palette** and type `Dygnosis` or `Dynare` to
run extension commands such as **Dygnosis: Open Help**, **Dygnosis: Expand
macros** or **Dynare: Select Analysis Configuration**. Common actions also
appear in the editor title bar, the editor context menu, **Outline**,
**Problems**, and **Dynare workspace checks** in Explorer.

In **Dygnosis Help**, **Search** finds features, commands, settings and check
codes; setting links open the matching VS Code Settings entry.

## Workflow guide

| Dynare workflow | VS Code and Dygnosis tools | Topic |
|---|---|---|
| Inspect model counts, `@#include` chains and variable timing | Status bar counts, **Choose model owner**, **Jump to equation...** | [Understand the model and files](model-and-includes.md) |
| Trace declarations, parameters and equations across files | **Go to Definition**, **Find All References**, **Rename symbol**, **Outline**, **Show Call Hierarchy** | [Follow equations and names](navigate-code.md) |
| Write declarations, equations, steady states and shocks | Completion, hover, option hints, `shocks` templates, bulk `long_name` and equation tagging | [Write equations and commands](edit-models.md) |
| Read `long_name` and TeX labels, parameter values and block roles | Inlay hints, block shading, semantic token colors | [Read labels, hints and colors](appearance.md) |
| Preview `@#for`, `@#if` and `@#include`, or test `-D` and `exclude_eqs` | **Expand macros**, **Select Analysis Configuration** | [Inspect macro expansion](effective-model.md) |
| Catch preprocessor errors and modeling mistakes before MATLAB | **Problems** panel, related locations, **Quick Fix** | [Check the model](diagnostics.md) |
| Compare model revisions, variants or analysis configurations | **Dygnosis Changes** (**Show mod changes**) | [Review model changes](structural-diff.md) |
| Check all saved `.mod` and `.dyn` models in a project | **Dynare workspace checks** in Explorer | [Check workspace models](workspace-checks.md) |

## Numerical work and agents

Official Dynare in MATLAB or Octave computes steady states, checks the
Blanchard-Kahn conditions, and runs simulations and estimation. Dygnosis checks
and navigates the source files before you run them; a clean **Problems** panel
does not guarantee that Dynare will solve the model numerically.

[Work with an agent](agents.md) provides the same source analysis and editing
tools through MCP and the `use-dynare` skill.

Run **Dygnosis: Open Settings** to browse all settings. The
[reference](reference.md) lists commands, shortcuts, diagnostic codes and
command options; [support](troubleshoot.md) covers output logs, server restarts
and problem reports; [About Dygnosis](about.md) lists compatibility and license
details.

## Availability

Requires **VS Code 1.102.0 or later**. Install from the [VS Code
Marketplace](https://marketplace.visualstudio.com/items?itemName=CoconutWater.dygnosis)
or download packages and standalone binaries from [GitHub
Releases](https://github.com/naivej/dygnosis/releases).

Unsaved (`Untitled`) files support single-file editing and checks; save the
file to resolve `@#include` and companion file paths through its
[model owner](model-and-includes.md#model-roots-and-includes). Virtual
filesystems provide syntax highlighting only.
