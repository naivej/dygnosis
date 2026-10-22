# Follow equations and names

Navigation works across the active model and its known `@#include` files.

Place the cursor on a symbol and right-click in the editor, or press `F1` to
run a command from the Command Palette.

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

Declared heterogeneity dimensions, trends (`trend_var`, `log_trend_var`), named
subsamples, and named `var_model`, `trend_component_model`,
`var_expectation_model` and `pac_model` definitions also support navigation and
rename. Database and external-function names support **Go to Definition** and
**Find All References**. Built-in operators such as `EXPECTATION` and
`STEADY_STATE` support **Find All References** and hover help.

## Outline

Open **Outline** in Explorer or click the **breadcrumbs** trail above the
editor to browse the file's structure. Choose which groups appear in [Dynare ›
Outline: Sections](settings:dynare.outline.sections) and toggle equation
numbers in [Dynare › Outline: Equation
Numbers](settings:dynare.outline.equationNumbers).

Under declarations, each variable row shows its timing class, `long_name` and
`$TeX$` label beside the name. Heterogeneous blocks and declarations include
their dimension (such as `model(heterogeneity=households)`). Named `@#define`
macros also appear as entries.

## Inactive source

The Outline can show recognizable entries written in inactive macro branches
and loop bodies, without active equation numbers. Use [Dynare: Outline: Inactive
Source](settings:dynare.outline.inactiveSource) to hide or show them.

For a name that appears only in inactive source, place the cursor on the name
and choose **Go to Source Candidate**. The picker labels each result
**source candidate · unverified**. Choosing one opens its written location.
You can also follow literal include links to browse unused files. Browsing
these locations does not add them to the active model.
To analyze a different branch, see [Choose the analysis configuration](effective-model.md#choose-the-analysis-configuration).

## Model-local variables

For a model-local variable (`#name = expression;`), **Go to Definition** opens
the `#name` definition and **Go to Declaration** opens an explicit
`model_local_variable` declaration when present. **Find All References** and
**Rename symbol** follow that local within its model or heterogeneity
dimension.
