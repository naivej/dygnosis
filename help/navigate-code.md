# Follow equations and names

Navigation works across the active model and its known `@#include` files.

Place the cursor on a symbol and right-click in the editor, or press `F1` to
run a command from the Command Palette. Dygnosis editor actions appear together
in one context-menu group.

| Action | What you see |
|---|---|
| **Go to Definition** / **Peek Definition** | Opens or previews the symbol's declaration or `#name` model-local definition. |
| **Go to Declaration** / **Go to Type Definition** | Opens the symbol's declaration. |
| **Find All References** | Lists every use of the symbol across the model and its includes. |
| Click a declared name | Highlights its occurrences in the current file. |
| **Rename symbol** (`F2` or **Dygnosis: Rename symbol**) | Opens **Dygnosis: Rename symbol** and renames the symbol across the model, its includes, and other known models that share those files. |
| **Show Call Hierarchy** | On a symbol, incoming calls list the equations that use it; on an equation, outgoing calls list the declared symbols it uses in the current file. |
| **Go to Symbol in Editor...** | Lists declarations, blocks, commands and equations in the current file. |
| **Go to Symbol in Workspace...** | Searches symbols across models analyzed by Dygnosis. |
| Click a folding arrow beside a line number | Collapses a Dynare block, `@#for` loop, `@#if` branch, or multiline declaration, command or assignment. |

Use [Jump to equation...](model-and-includes.md#equations) to filter and jump to
equations across the model, including repeated macro copies.

**Rename symbol** also updates literal symbol names inside equation `endogenous`
and `mcp` tags while keeping existing quotes. Equation `name` tags, symbol
`long_name` labels and MATLAB `.m` files stay unchanged.

## Outline

Open **Outline** in Explorer or click the **breadcrumbs** trail above the
editor to browse the file's structure. Choose which groups appear in [Dynare ›
Outline: Sections](settings:dynare.outline.sections) and toggle equation
numbers in [Dynare › Outline: Equation
Numbers](settings:dynare.outline.equationNumbers).

Equations appear in the file where they start (including when an expression
continues through includes). Macro arms and `@#for` loops follow the [selected
analysis configuration](effective-model.md#choose-the-analysis-configuration);
filter **Active**, **Inactive** or **Both** with the Outline toolbar or
[Dynare › Outline: Activity](settings:dynare.outline.activity).

## Inactive source

The Outline can show recognizable entries written in inactive macro branches
and loop bodies, without active equation numbers.

For a name that appears only in inactive source, place the cursor on the name
and run **Dynare: Go to Source Candidate**. To analyze a different branch,
see [Choose the analysis configuration](effective-model.md#choose-the-analysis-configuration).

## Model-local variables

For a model-local variable (`#name = expression;`), **Go to Definition** opens
the `#name` definition and **Go to Declaration** opens an explicit
`model_local_variable` declaration when present. **Find All References** and
**Rename symbol** follow that local within its model or heterogeneity
dimension.
