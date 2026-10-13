# Check project models

Open a folder in VS Code to check its unopened saved `.mod` models. Background
checking is on by default and can be
[turned off](settings:dynare.projectDiagnostics) while retaining checks for
open models.

Run **Dygnosis: Show project checks** from the Command Palette, or click
**Dynare project: ...** on the status bar. This opens **Dynare project checks**
in Explorer. Expand a folder to see its model files, check state and Error or
Warning counts. Click a model to open it. The messages themselves appear in
**Problems**.

**Dygnosis: Recheck project** finds and checks the models again.
**Dygnosis: Cancel project checks** stops the current pass until a file
change or Recheck. Completed results remain.

For example, **3/3 checked** on the status bar means that analysis finished for
three models. Those models can still have Errors or Warnings in Problems.

## Scope and exclusions

[Project exclusions](settings:dynare.projectExcludePaths) and
**Dygnosis: Configure project exclusions** select excluded paths per workspace
folder. Choose **Add exclusion…**, **Remove exclusion…**, **Reset folder
exclusions** or **Open project settings**. For example, enter `generated/**`
after choosing **Add exclusion…** to skip models under the folder's `generated`
directory. Patterns are relative to that folder: `*` and `?` match within a
path component, `**` crosses folders, and a trailing `/` includes descendants.

Discovery covers `.mod` roots and skips generated `+` directories. `.dyn`
models, loose files and excluded models still receive editor checks when open.
Excluding a file from discovery does not remove it from another model's
includes.
