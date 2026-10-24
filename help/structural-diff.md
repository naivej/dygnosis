# Review model changes

Compare your model against Git history, another `.mod`/`.dyn` file, or another
analysis configuration with **Dygnosis: Show mod changes** from the Command
Palette, the editor title bar (`$(diff)`), or **Show mod changes** on a model
file in **Source Control**. Choose a local commit, branch or tag, enter a
revision such as `HEAD~2`, or select **.mod file…** for another model.
Choose **config…** to compare a Working model's current configuration with
**Written source** or a saved configuration.

Your active model appears under **After** (`Working` includes saved files plus
any open unsaved edits) and the baseline you select appears under **Before**.
Each side shows its filename and configuration choices. Hover over a summary
for its revision and model counts.

In **Dygnosis Changes**, browse categories such as **Equations** and
**Parameters**, follow source links, or open **Text diff** for VS Code's
side-by-side file comparison. Historical source opens read-only. Configure
initial filters in [Dynare › Diff: Default Change
Kinds](settings:dynare.diff.defaultChangeKinds).

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
