# Review model changes

Compare your model against Git history, another `.mod`/`.dyn` file, or another
analysis configuration with **Dygnosis: Show mod changes** from the Command
Palette, the editor title bar (`$(diff)`), or **Show mod changes** on a model
file in **Source Control**. The baseline chooser searches recent commits,
branches and tags. Type a revision such as `HEAD~2` and choose
**Use revision: HEAD~2** to compare it. Choose **.mod file…** to select
another model file. For a Working model, choose **config…** to compare its
current configuration with **Written source** or a saved configuration.

Your active model appears under **After** (`Working` includes saved files plus
any open unsaved edits) and the baseline you select appears under **Before**.
Each side shows its filename and configuration choices. Hover over a summary
for its revision and model counts.

| Baseline choice | Comparison input for Before |
|---|---|
| Recent commits, branches and tags | The selected local commit or ref. Use **Load more commits…** for older history. |
| **Use revision: …** | The local revision named in the chooser, for example `HEAD~2`. |
| **.mod file…** | Another `.mod` or `.dyn` file on disk. |
| **config…** | **Written source** or a saved configuration for the same model. Written source is always available. The current configuration is **After**. |

## Available views and actions

The **Dygnosis Changes** tab lists categories on the left (for example
**Equations** or **Parameters**) and shows **Before** and **After** cards side
by side.

| Control | Result |
|---|---|
| Category buttons on the left (such as **Equations** or **Parameters**) | Displays changes in that category along with its match count. |
| **Find changes** | Filters rows by symbol name, equation tag, expression or field value. |
| **Type of changes** | Filters by **All**, **Added** (`+`), **Removed** (`−`) and **Replaced** (`~`). Configure initial defaults in [Dynare › Diff: Default Change Kinds](settings:dynare.diff.defaultChangeKinds). |
| **`filename:line`** or **`Open source…`** on a **Before** or **After** card | Opens that side's file at the source line, or lets you choose from contributing files. Historical Git files open read-only. |
| **Direct equation references** | Jumps to equations that use a changed symbol or parameter. |
| **Text diff** / **Text diff…** | Opens VS Code's side-by-side text diff. A single changed file opens directly when its captured text is available; otherwise choose a changed model or include file from the picker. The picker also lists files whose text is unavailable. |
| **Change comparison…** | Selects a different **Before** baseline. |
| **Choose model path** | Appears when a model path was renamed or could not be found on one side. |

## Comparison scope

The **config…** comparison can show changed model facts when source text
is the same. It compares structural model information; it does not run Dynare
or compare numerical results.

Comparisons report structural source changes rather than algebraic or numerical
equivalence. Equations that cannot be paired appear as separate **Removed** and
**Added** rows; give an equation the same unique `[name='...']` tag in both
models to pair it as **Replaced**.

Git comparisons read local repository history without fetching or checking out
files and resolve relative includes using the launching model's current
[Dynare: Search Paths](settings:dynare.searchPaths).
