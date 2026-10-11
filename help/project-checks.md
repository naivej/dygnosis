# Check project models

**Dynare project checks** checks unopened saved `.mod` models in workspace
folders. Background checking is on by default and can be
[turned off](settings:dynare.projectDiagnostics) while retaining checks for
open models. The Explorer view groups models by folder and
shows their check state and diagnostic counts. The **Dynare project** status
item shows progress; Problems shows the diagnostics.

| State | Meaning |
|---|---|
| Discovering | Finding saved model roots. |
| Pending / Checking | Waiting for a current result. |
| Checked | Analysis finished; Errors or Warnings may still be present. |
| Incomplete | Include or macro expansion is incomplete. |
| Failed | A file or folder could not be read, or checking failed. |
| Excluded | The model is outside background checks. |
| Cancelled | The current pass stopped; completed results remain. |
| Off | Background checking is disabled in Settings. |

**Show project checks** opens the view. **Recheck project** runs discovery and
checks again. **Cancel project checks** stops the current pass until a file
change or Recheck.

## Scope and exclusions

[Project exclusions](settings:dynare.projectExcludePaths) and
**Configure project exclusions** select excluded paths per workspace
folder. Patterns are relative to that folder: `*` and `?` match within a path
component, `**` crosses folders, and a trailing `/` includes descendants.

Discovery covers `.mod` roots and skips generated `+` directories. `.dyn`
models, loose files and excluded models still receive editor checks when open.
Excluding a file from discovery does not remove it from another model's
includes.

**Checked** counts measure completed analysis, not models that pass all checks.
Folder failures and incomplete models remain visible even after the pass ends.
