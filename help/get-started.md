# Dynare workflow

Dygnosis adds Dynare language support to VS Code for `.mod`, `.dyn` and `.inc`
files. Editing and source analysis work without a separate Dynare, MATLAB or
Octave installation.

Open a model file and check that the language mode in the bottom-right status
bar is **Dynare**. If it shows another language, click it and select
**Dynare**.

Press `F1` to open the **Command Palette** and type `Dygnosis` to run extension
commands such as **Dygnosis: Open Help**, **Dygnosis: Expand macros** or
**Dygnosis: Open Settings**. Common actions also appear in the editor title
bar, the editor right-click menu, **Outline**, **Problems**, and **Dynare
workspace checks** in Explorer.

In **Dygnosis Help**, **Search** finds features, commands, settings and check
codes; setting links open the matching VS Code Settings entry.

## Understand the model

[Understand the model and files](model-and-includes.md) covers model roots,
`@#include` files, status bar counts and equation navigation. [Follow equations
and names](navigate-code.md) connects declarations to their uses across the
model with **Go to Definition**, **Find All References**, **Rename symbol**,
**Outline** and **Show Call Hierarchy**.

## Write equations and experiment setup

[Write equations and commands](edit-models.md) covers declarations, parameters,
equations, steady-state blocks, shock templates, formatting and quick fixes.
[Read labels, hints and colors](appearance.md) configures `long_name` and TeX
labels, parameter value hints, block shading and semantic colors.

## Inspect and check

[Inspect macro expansion](effective-model.md) previews the text produced by
includes, loops and substitutions, with jumps back to the written source.
[Check the model](diagnostics.md) explains **Problems**, check severities,
related locations and quick fixes.

## Review changes and model variants

[Review model changes](structural-diff.md) compares a model against Git history
or another `.mod` file in **Dygnosis Changes**. [Check workspace
models](workspace-checks.md) checks unopened saved models across your workspace in
**Dynare workspace checks**.

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

Untitled and loose files receive editor analysis. Resolving `@#include` and
related files requires a saved file path, and an included helper file uses its
[model owner](model-and-includes.md#model-roots-and-includes). Virtual
filesystems provide syntax highlighting only.
