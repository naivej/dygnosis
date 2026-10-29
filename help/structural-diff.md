# Review model changes

Compare your active model against a local Git revision (commit, branch or tag),
another `.mod`/`.dyn` file, or another [analysis
configuration](effective-model.md#choose-the-analysis-configuration)
(`config…`). Run **Dygnosis: Show model changes** from the Command Palette, the
editor title bar (`$(diff)`), or a model file's context menu in **Source
Control**.

In **Dygnosis Changes**, your active model appears under **After** and the
selected baseline under **Before**. Browse structural categories such as
**Equations** and **Parameters**, follow source links, or click **Text diff**
for VS Code's side-by-side file comparison. Configure default filters in
[Dynare › Diff: Default Change Kinds](settings:dynare.diff.defaultChangeKinds).

## Comparison scope

Comparisons report structural changes—declarations, parameter values, shock
setups and equations—rather than algebraic equivalence or numerical Dynare
results. Comparing with **config…** shows how macro overrides or
`include_eqs`/`exclude_eqs` selections change the effective model even when the
written `.mod` file is unchanged.

Modified equations that cannot be paired appear as separate **Removed** and
**Added** rows; give an equation the same unique `[name='...']` tag in both
models to pair it as **Replaced**.

Git comparisons read local repository history directly and resolve relative
`@#include` paths using your current [Dynare: Search
Paths](settings:dynare.searchPaths).
