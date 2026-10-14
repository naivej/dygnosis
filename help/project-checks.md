# Check project models

Open a folder in VS Code to check its unopened saved `.mod` and `.dyn` models.
Background checking is on by default and can be
[turned off](settings:dynare.projectDiagnostics) while retaining checks for
open models.

Run **Dygnosis: Show project checks** from the Command Palette, or click
**Dynare project: ...** on the status bar. This opens **Dynare project checks**
in Explorer. Expand folders and subfolders to see model files by their file
names. Model rows show their check state and Error or Warning counts. Click a
model to open it. The messages themselves appear in **Problems**.

Saved `.inc` files appear once at their actual folder path, including files
that no model currently includes. These rows say **Include file** and open the
file. They have no separate check state or counts. Their diagnostics appear in
**Problems** when the file is open or checked as part of a model that includes
it. A folder can contain include files even when no root models were found.
The include entries remain visible and the folder reports that no root models
were found. A discovery failure explains when the model or include file list is
incomplete.

**Dygnosis: Recheck project** finds and checks the models again.
**Dygnosis: Cancel project checks** stops the current pass until a file
change or Recheck. Completed results remain.

For example, **3/3 checked** on the status bar means that analysis finished for
three models. Those models can still have Errors or Warnings in Problems.

## Scope and exclusions

[Project exclusions](settings:dynare.projectExcludePaths) and
**Dygnosis: Configure project exclusions** select excluded paths per workspace
folder. The toolbar gear opens **Configure project exclusions**, where you can
choose **Add exclusion…**, **Remove exclusion…**, **Reset folder exclusions**
or **Open project settings**. To ignore one folder directly, choose **Ignore
this folder** on its workspace or subfolder row. This adds `**` for a workspace
folder, or `<folder path>/**` for a subfolder, to that workspace folder's
exclusions. For example, ignoring `models/old` adds `models/old/**`. The model
roots beneath that path stop receiving background checks. Saved `.inc` entries
remain visible. Patterns are relative to the workspace folder: `*` and `?`
match within a path component, `**` crosses folders, and a trailing `/`
includes descendants.

Background checks discover `.mod` and `.dyn` root models beneath each
workspace folder. They skip generated `+` directories. Loose files and
excluded models still receive editor checks when open. Exclusions control
which roots are checked in the background. They do not hide `.inc` entries,
and excluding a file from root discovery does not remove it from another
model's includes.
