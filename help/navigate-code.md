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

## Outline

Open **Outline** in Explorer or use **breadcrumbs** above the editor. Configure
groups in [Dynare › Outline: Sections](settings:dynare.outline.sections) and
equation numbers in [Dynare › Outline: Equation
Numbers](settings:dynare.outline.equationNumbers).

An equation has one entry in the file where it starts, including when its
expression continues through includes. Equations written entirely inside an
include appear in that include's Outline.

Each `@#if`, `@#elseif` and `@#else` arm has a separate Outline entry and folding
range. Collapsing one arm keeps the next arm visible. Activity labels appear on
these arm entries and `@#for` entries; entries inside do not repeat the label.

Use **Filter Outline by macro activity...** or [Dynare › Outline:
Activity](settings:dynare.outline.activity) to show **Active**, **Inactive** or
**Both**. Repeated code that is active in some macro visits and inactive in
others matches both **Active** and **Inactive** filters.

## Inactive source

The Outline can show recognizable entries written in inactive macro branches
and loop bodies, without active equation numbers.

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
