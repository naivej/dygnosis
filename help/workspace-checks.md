# Check workspace models

Workspace checks analyze saved `.mod` and `.dyn` root models across your VS Code
workspace folders, including unopened models (skipping Dynare's generated
`+model` output directories). Enable background checking in [Dynare: Workspace
Diagnostics](settings:dynare.workspaceDiagnostics).

Open **Dynare workspace checks** in Explorer, or run **Dygnosis: Show workspace
checks** from the Command Palette. Click **Show not fully checked models**
(`$(filter)`) in the view title bar to filter for incomplete or failed checks.
You can also switch this filter from the Command Palette with **Dygnosis: Show
not fully checked models** and **Dygnosis: Show all workspace files**.

## Scope and exclusions

Exclude a folder with **Ignore this folder** on its row, or configure [Dynare:
Workspace Exclude Paths](settings:dynare.workspaceExcludePaths). Exclusions
skip unopened models in those folders while still checking open files and
`@#include` files used by other models.

Each model root is checked using its selected [analysis
configuration](effective-model.md#choose-the-analysis-configuration); models
using a custom configuration display its name or selected choices beside the file.
