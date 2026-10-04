# Project checks in VS Code

Open a file-backed workspace folder to check unopened saved `.mod` models. Project diagnostics are on by default and start without opening a model. Dygnosis skips generated `+` directories, using recursive saved-model discovery. Open `.dyn`, excluded, loose, and untitled models retain ordinary editor diagnostics.

The **Dynare project coverage** status item is separate from active-model counts. Click it to open **Dynare project checks** in Explorer. The view groups models and discovery failures by workspace folder and opens a model when you select its row.

| State | Meaning |
|---|---|
| Discovering | Dygnosis is finding saved root models. |
| Pending / Checking | A model still needs a current check. |
| Checked | The check finished. The model may still have diagnostic Errors or Warnings. |
| Incomplete | Include or macro expansion is incomplete; coverage is incomplete. |
| Failed | A file could not be read or another infrastructure operation failed. |
| Excluded | This model has no background-check contribution. Opening it still checks it. |
| Cancelled | This pass stopped; completed reports remain and pending models remain pending. |
| Off | Background checking is persistently disabled. |

A finished pass can have incomplete coverage. Folder discovery failures are shown even when models in other folders finish. **No root models** means discovery finished without selecting a root; **No folders** means there is no file-backed workspace folder. Neither project coverage nor model counts is a solver result. Dygnosis stops before MATLAB or Octave and does not run the official preprocessor.

**Dygnosis: Recheck project** reruns discovery and all selected models. **Dygnosis: Cancel project checks** stops the current pass until a later file edit/change or Recheck. Cancellation does not change Settings. Turning `dynare.projectDiagnostics` off cancels background work and clears project contributions while keeping diagnostics belonging to open models.

[Project diagnostics](settings:dynare.projectDiagnostics) is a window switch. [Exclusions](settings:dynare.projectExcludePaths) apply to the resource; patterns are relative to the containing workspace folder. In multi-folder workspaces, the most specific containing folder supplies the exclusions. `*` and `?` match within a path component, `**` crosses directories, `**/` also matches no directory, and a trailing slash includes descendants. Both slash styles work.

Put these exclusions in the selected folder's `.vscode/settings.json`:

```json
{
  "dynare.projectExcludePaths": ["generated/**", "archive/", "**/scratch_*.mod"]
}
```

Use the folder row's **Configure project exclusions** action, or **Dygnosis: Configure project exclusions**, to choose a folder and add/remove patterns. **Reset folder exclusions** removes that folder's override and restores inherited/default settings. The list is also editable in VS Code Settings. Exclusions select background roots; an excluded include still affects its checked owners. Creating a missing include or companion file, changing a dependency of any extension, and creating/deleting a root refresh the affected checks.

Active-model priority follows the current root or an explicitly chosen include owner. Focusing an include never silently chooses its owner for project priority. After a server restart, the extension queries current coverage and replaces the previous instance's status. An older or incompatible `dynare.serverPath` override shows project checks as unavailable; use the bundled binary or update the override while retaining its supported ordinary LSP features.

The [project protocol](integrations.md) describes the server contract and report lifetimes.
