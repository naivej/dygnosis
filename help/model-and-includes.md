# Understand the model and files

## Model roots and includes

A **model root** is a top-level `.mod` or `.dyn` file. Included files (such as
`.inc` files or shared `.mod` fragments loaded via `@#include`) inherit macro
values and declarations from the parent model that includes them—their **model
owner**.

- **Single parent model**: Dygnosis links the included file to its owner
  automatically. If an `.inc` helper is unlinked, open its parent `.mod` or
  `.dyn` file once to connect them.
- **Shared across several models**: Click **Dynare: choose model owner** in the
  status bar (or run **Dygnosis: Choose model owner**) to pick which parent
  model provides context for diagnostics, **Outline**, navigation and macro
  expansion.
- **Standalone `.mod` or `.dyn` file**: When a `.mod` or `.dyn` file is also
  included by another model, run **Dygnosis: Treat this file as a model root**
  to check it on its own.

Hover over an `@#include` path or a recognized parameter, data or
external-function filename in the editor to open it through VS Code's document
link. Add lookup folders in [Dynare: Search Paths](settings:dynare.searchPaths)
(resolved relative to the workspace folder, or the model root's folder for an
outside file).

## Model structure and counts

The status bar shows variable and equation counts across the aggregate model
and any heterogeneous dimensions. Click the status bar item to open **Outline**,
or hover to inspect per-dimension counts and the active
[analysis configuration](effective-model.md#choose-the-analysis-configuration)
(**Source** vs. **Current config**).

Toggle the status bar item with [Dynare › Status Bar:
Enabled](settings:dynare.statusBar.enabled) and choose which counts appear (and
their order) in [Dynare › Status Bar: Counts](settings:dynare.statusBar.counts).

## Equations

| Action | Result |
|---|---|
| **Dygnosis: Jump to equation...** in the Command Palette | Filters and jumps to any counted equation by number, `[name]` tag, symbol or expression. |
| Click **Find references** above a declaration | Runs **Find All References** for that symbol, or lets you pick one when several are declared ([Dynare › Code Lens: Declaration References](settings:dynare.codeLens.declarationReferences)). |
| Click **Expand macros** above the first `model` block | Opens the macro expansion preview ([Dynare › Code Lens: Effective Model](settings:dynare.codeLens.effectiveModel)). |

CodeLens actions require VS Code's **Editor: Code Lens** (`editor.codeLens`).

Equations are numbered before Dynare's internal lead/lag transformations
(excluding `#` model-local definitions, `[static]` replacement rows and removed
equations), so numbers can differ from MATLAB or Octave runtime output.

## Timing classes

| Class | Equation use |
|---|---|
| Static | Current period only (no lead or lag). |
| Predetermined | Lags only, no leads. |
| Forward-looking | Leads only, no lags. |
| Mixed | Both leads and lags. |

Variable hover shows each endogenous variable's timing class. When a stock
variable is declared with
[`predetermined_variables`](https://www.dynare.org/manual/the-model-file.html#predetermined-variables)
(for example `predetermined_variables k;`), Dynare shifts its timing to the
end-of-period convention: written `k(+1)` is current-period `k`, and written `k`
is lagged `k(-1)`. Hover indicates this with:

Timing: **predetermined**, end-of-period convention
