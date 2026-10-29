# Follow equations and names

Navigation works across the active model and its `@#include` files. Place the
cursor on a symbol and right-click in the editor, or press `F1` to run a
command from the Command Palette.

| Action | What it does |
|---|---|
| **Go to Definition** / **Peek Definition** | Opens or previews the symbol's declaration or `#name` model-local definition. |
| **Go to Declaration** / **Go to Type Definition** | Opens the symbol's declaration. |
| **Find All References** | Lists every use of the symbol across the model and its includes. |
| **Rename symbol** (`F2` or **Dygnosis: Rename symbol**) | Renames the symbol across the model, its includes, and other known models that share those files. |
| **Show Call Hierarchy** | On a symbol, incoming calls list the equations that use it; on an equation, outgoing calls list the declared symbols it uses in the current file. |
| **Go to Symbol in Editor...** | Lists declarations, blocks, commands and equations in the current file. |
| **Go to Symbol in Workspace...** | Searches symbols across models analyzed by Dygnosis. |

Use [Jump to equation...](model-and-includes.md#equations) to filter and jump to
equations across the model, including repeated macro copies. Editor gutter
arrows fold Dynare blocks, `@#for` loops, `@#if` branches, and multiline
declarations or commands.

**Rename symbol** also updates symbol names inside equation `endogenous` and
`mcp` tags, while leaving equation `name` tags, symbol `long_name` labels and
MATLAB `.m` files unchanged.

## Outline

Open **Outline** in Explorer or use editor **breadcrumbs** to browse the file's
structure. Choose which groups appear in [Dynare › Outline:
Sections](settings:dynare.outline.sections) and toggle equation numbers in
[Dynare › Outline: Equation Numbers](settings:dynare.outline.equationNumbers).

Macro branches (`@#if`) and `@#for` loops follow the [selected analysis
configuration](effective-model.md#choose-the-analysis-configuration); filter
**Active**, **Inactive** or **Both** from the Outline toolbar, **Dygnosis:
Filter Outline by macro activity...** in the Command Palette, or [Dynare ›
Outline: Activity](settings:dynare.outline.activity).

## Inactive source

When an `@#if` branch or `@#for` loop is inactive under the current analysis
configuration, **Outline** can still list its declarations and equations
without active equation numbers.

To jump to a symbol declared only inside an inactive macro branch, place the
cursor on the name and run **Dygnosis: Go to source candidate**. To analyze a
different branch, see [Choose the analysis
configuration](effective-model.md#choose-the-analysis-configuration).

## Model-local variables

For a model-local variable (`#name = expression;`), **Go to Definition** opens
the `#name` definition and **Go to Declaration** opens an explicit
`model_local_variable` declaration when present. **Find All References** and
**Rename symbol** follow that local within its model or heterogeneity
dimension.
