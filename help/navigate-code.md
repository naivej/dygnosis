# Follow equations and names

Navigation works across the active model and its `@#include` files. To trace a
parameter, hover over its name for its details, then right-click and choose
**Go to Definition** or **Peek Definition** to inspect its declaration. Choose
**Find All References** to see its uses across the model and includes.

To rename a symbol across the model, its includes and other known models that
share those files, place the cursor on the name and press `F2`, or choose
**Rename symbol** from the editor context menu.

To inspect dependencies across the selected model and its includes, right-click
a symbol or equation and choose **Show Call Hierarchy**. If prompted, choose
the symbol or equation. In the results view, use **Show Incoming Calls** to see
equations that use a symbol, or **Show Outgoing Calls** to see the declared
symbols an equation uses. Outgoing calls for an equation fragment cover the
displayed fragment. The toolbar offers the opposite direction from the one
currently shown.

Use [Jump to equation...](model-and-includes.md#equations) to filter and jump to
equations across the model, including repeated macro copies. Editor gutter
arrows fold Dynare blocks, `@#for` loops, `@#if` branches, and multiline
declarations or commands.

**Rename symbol** also updates symbol names inside equation `endogenous` and
`mcp` tags, while leaving equation `name` tags, symbol `long_name` labels and
MATLAB `.m` files unchanged.

## Outline

Open **Outline** in Explorer or use editor **breadcrumbs** to browse the file's
structure. Run **Go to Symbol in Editor...** from the Command Palette to find a
declaration, block, command or equation in the current file, or **Go to Symbol
in Workspace...** to search analyzed models. Choose which groups appear in
[Dynare › Outline: Sections](settings:dynare.outline.sections) and toggle
equation numbers in
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
cursor on the name and run **Dygnosis: Find possible declarations...** from the
Command Palette or the editor context menu under **Dygnosis**. To analyze a
different branch, see [Choose the analysis
configuration](effective-model.md#choose-the-analysis-configuration).

## Model-local variables

For a model-local variable (`#name = expression;`), **Go to Definition** opens
the `#name` definition and **Go to Declaration** opens an explicit
`model_local_variable` declaration when present. **Find All References** and
**Rename symbol** follow that local within its model or heterogeneity
dimension.
