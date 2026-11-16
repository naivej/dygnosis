# Review model changes

Compare your active model against a local Git revision (commit, branch or tag),
another `.mod`/`.dyn` file, or another [analysis
configuration](effective-model.md#choose-the-analysis-configuration)
(`Configuration…`). Run **Dygnosis: Show model changes** from the Command Palette, the
editor title bar, or a model file's context menu in **Source
Control**.

In **Dygnosis Changes**, your active model appears under **After** and the
selected baseline under **Before**. Open either input for its full path,
configuration, and `@#include` files.

Browse **Equations**, **Parameters**, and other groups. Choose **Added**,
**Removed**, or **Changed**, or press Ctrl+F (Cmd+F on macOS) for **Find changes**.
Open one or more rows to review their **Before** and **After** details and
source links together. A changed parameter can group
its direct dependents, and a history card can group several written periods.
**Macro context** counts edits to each macro name separately and groups them
with added, removed, and edited `@#include` files. Open a source link to review
an edit.
Configure default filters in
[Dynare › Diff: Default Change Kinds](settings:dynare.diff.defaultChangeKinds).

Use **Swap sides**, **Refresh comparison**, and **Text diff** from the editor
title bar or the Command Palette while Changes is active. **Text diff** opens
the captured files side by side in VS Code. Run **Dygnosis: Show model changes**
to choose a different baseline.

## Comparison scope

Changes covers model declarations, equations, calibration, shock setups, and
other model instructions. Comparing with **Configuration…** shows how macro overrides or
`include_eqs`/`exclude_eqs` selections change the effective model even when the
written `.mod` file is unchanged. The captured text covers the roots and the
includes each side executed.

Modified equations that cannot be paired appear as separate **Removed** and
**Added** rows; give an equation the same unique `[name='...']` tag in both
models to pair it as **Changed**.

Git comparisons read local repository history directly and resolve relative
`@#include` paths using your current [Dynare: Search
Paths](settings:dynare.searchPaths).
