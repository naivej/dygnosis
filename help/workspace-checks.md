# Check workspace models

Workspace checks analyze saved `.mod` and `.dyn` root models across your VS Code
workspace folders, including unopened models. Generated `+` directories are
skipped. Enable background checking in [Dynare: Workspace
Diagnostics](settings:dynare.workspaceDiagnostics).

Find **Dynare workspace checks** in Explorer, or run **Dygnosis: Show workspace
checks** from the Command Palette. Use **Show not fully checked models**
(`$(filter)`) in the view title bar to find incomplete or failed checks.

## Scope and exclusions

Exclude folders with **Ignore this folder** on a folder row, or configure
[Dynare: Workspace Exclude Paths](settings:dynare.workspaceExcludePaths).

Exclusions apply only to background model roots. Open files and dependencies of
other models remain checked. Saved `.inc` files are not separate workspace
checks and do not contribute to the model counts.

Workspace checks use the selected analysis configuration for each model root.
When a root uses a non-Source configuration, its row shows that
configuration's name (and **preview** when applicable). The workspace status
also reports how many roots use a non-Source configuration. Choose a root's
configuration from its open source editor or macro expansion preview; see
[Inspect macro expansion](effective-model.md#choose-the-analysis-configuration).
