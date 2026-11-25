# Check workspace models

Workspace checks analyze saved `.mod` and `.dyn` root models across your VS Code
workspace folders, including unopened models (skipping Dynare's generated
`+model` output directories). Enable background checking in [Dynare: Workspace
Diagnostics](settings:dynare.workspaceDiagnostics).

Use **Dynare Workspace** in Explorer or run **Dygnosis: Show workspace
checks**. Use **Show not fully checked models** to focus on incomplete or
failed checks. Select the reason under an incomplete model to find its
cause, such as the `@#include` directive for a missing file.

## Scope and exclusions

Exclude a folder with **Ignore this folder** on its row, or configure [Dynare:
Workspace Exclude Paths](settings:dynare.workspaceExcludePaths). Exclusions
skip unopened models in those folders while still checking open files and
`@#include` files used by other models.

Each model root is checked with its selected [analysis
configuration](effective-model.md#choose-the-analysis-configuration).
