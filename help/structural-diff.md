# Review model changes

Compare your model against Git history or another `.mod`/`.dyn` file from any
of these places:

- Click **Show mod changes** (`$(diff)`) in the editor title bar or editor right-click menu.
- Right-click a `.mod`, `.dyn` or included `.inc` file in the **Source Control** view and choose **Show mod changes**.
- Run **Dygnosis: Show mod changes** from the Command Palette (or **Dygnosis: Show mod changes with .mod file…** to pick a file directly).

Your active model appears under **After** (`Working` includes saved files plus
any open unsaved edits) and the baseline you select appears under **Before**.

| Baseline choice | Comparison input for Before |
|---|---|
| **With previous revision** | `HEAD` for a `Working` model, or the parent commit for an open historical revision. |
| **With revision…** | Any local commit selected from the history list or entered by ref (such as `HEAD~2`). |
| **With branch or tag…** | The commit pointed to by a local branch, remote-tracking branch or tag. |
| **With .mod file…** | Another `.mod` or `.dyn` file on disk. |

## Available views and actions

The **Dygnosis Changes** tab organizes model differences by category on the
left—such as **Symbols**, **Parameters**, **Equations**, **Model-local
variables**, **Shock setup**, **State**, **Priors**, **Commands**, **Data**,
**OccBin**, **Policy**, **Method of moments**, **Heterogeneity** and **Macro
context**—and displays **Before** and **After** cards side by side.

| Control | Result |
|---|---|
| Category buttons on the left (such as **Equations** or **Parameters**) | Displays changes in that category along with its match count. |
| **Find changes** | Filters rows by symbol name, equation tag, expression or field value. |
| **Type of changes** | Filters by **All**, **Added** (`+`), **Removed** (`−`) and **Replaced** (`~`). Configure initial defaults in [Dynare › Diff: Default Change Kinds](settings:dynare.diff.defaultChangeKinds). |
| **`filename:line`** or **`Open source…`** on a **Before** or **After** card | Opens that side's file at the source line, or lets you choose from contributing files. Historical Git files open read-only. |
| **Direct equation references** | Jumps to equations that use a changed symbol or parameter. |
| **Text diff** / **Text diff…** | Opens VS Code's side-by-side text diff for the main model file, or lets you pick any changed model or included file (including comment and formatting edits). |
| **Change comparison…** | Selects a different **Before** baseline. |
| **Choose model path** | Appears when a model path was renamed or could not be found on one side. |

## Comparison scope

To compare two macro configurations of the same model, run **Dynare: Compare
Analysis Configurations** from the source editor or its macro expansion
preview. Choose configuration **A** and configuration **B**. Model Diff can show
changed model facts when the source text is the same. This compares structural
model information; it does not run Dynare or compare numerical results.

Comparisons report structural source changes rather than algebraic or numerical
equivalence. Equations that cannot be paired appear as separate **Removed** and
**Added** rows; give an equation the same unique `[name='...']` tag in both
models to pair it as **Replaced**.

Git comparisons read local repository history without fetching or checking out
files and resolve relative includes using the launching model's current
[Dynare: Search Paths](settings:dynare.searchPaths).
