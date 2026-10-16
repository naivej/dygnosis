# Understand the model and files

## Model roots and includes

A **model root** is a main `.mod` or `.dyn` file. An included file (such as an
`.inc` file or a helper `.mod`/`.dyn` file) can use macro values and
declarations from the parent model that includes it—its **model owner**.

| Context | Editor behavior |
|---|---|
| Single known owner | Uses that parent model automatically. |
| Several known owners | Shows **Dynare: choose model owner** on the status bar and waits for your choice before running model-wide features. |
| Chosen model owner | Uses your chosen parent model while it still includes this file in the current session. |
| Standalone `.mod` or `.dyn` | Checks the file as its own model root when no parent includes it, or after you run **Dygnosis: Treat this file as a model root**. |
| Unowned `.inc` helper | Open the parent `.mod` or `.dyn` file first so Dygnosis can connect them. |

To select or switch an owner for the active file, press `F1` and run
**Dygnosis: Choose model owner**, then pick the parent model path from the
dropdown. Following an `@#include` link or jumping to source from a model view
also sets that model as the owner. To check an included `.mod` or `.dyn` file on
its own instead, run **Dygnosis: Treat this file as a model root**.

The selected owner controls editor diagnostics, definitions, references,
**Outline**, status bar counts, equation navigation and macro expansion
previews, and updates live as you edit.

Hover over an `@#include` path or a recognized parameter, data or
external-function filename in the editor to open it through VS Code's document
link. Add lookup folders in [Dynare: Search Paths](settings:dynare.searchPaths)
(resolved relative to the workspace folder, or the model root's folder for a
loose file).

## Model structure and counts

Status bar model counts combine the aggregate model and all heterogeneous
dimensions.

Toggle the status bar item with [Dynare › Status Bar:
Enabled](settings:dynare.statusBar.enabled) and choose which counts appear (and
their order) in [Dynare › Status Bar: Counts](settings:dynare.statusBar.counts).

## Equations

| Action | Result |
|---|---|
| **Dygnosis: Jump to equation...** in the Command Palette | Opens the **Jump to an equation before transformation** picker for every counted equation in the active model. |
| Click **Jump to equation...** above a `model` block | Opens the equation picker filtered to that block ([Dynare › Code Lens: Model Equations](settings:dynare.codeLens.modelEquations)). |
| Click **Find references** above a declaration | Runs **Find All References** for that symbol, or lets you pick a symbol when the declaration lists several ([Dynare › Code Lens: Declaration References](settings:dynare.codeLens.declarationReferences)). |
| Click **Expand macros** above the first `model` block | Opens the macro expansion preview ([Dynare › Code Lens: Effective Model](settings:dynare.codeLens.effectiveModel)). |

Each picker row shows `<number> · <name or Unnamed equation>`, **Aggregate** or
**Dimension `<name>`**, `filename:line`, and the equation text (plus macro loop
values such as `j=1`). Type a number, tag, symbol or expression fragment to
filter the list, then press `Enter` to jump to that line.

The clickable labels above blocks and declarations are VS Code **CodeLens**
actions. They require **Editor: Code Lens** (`editor.codeLens`) and the
corresponding setting under **Dygnosis: Actions**.

Equation numbers reflect the model before Dynare's equation transformations and
can differ from numbers printed by MATLAB or Octave. Model-local `#`
definitions, `[static]` replacement rows and removed equations are not
numbered.

## Timing classes

| Class | Equation use |
|---|---|
| Static | Current period only (no lead or lag). |
| Predetermined | Lags only, no leads. |
| Forward-looking | Leads only, no lags. |
| Mixed | Both leads and lags. |

Variable hover and **Outline** show each endogenous variable's timing class.
When a variable is listed in `predetermined_variables k;`, its hover adds:

Timing: **predetermined**, end-of-period convention

With `predetermined_variables`, written `k(+1)` is current-period `k` for
timing classification, and written `k` is lagged `k(-1)`.
