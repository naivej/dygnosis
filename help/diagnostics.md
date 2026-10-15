# Check the model

Dygnosis underlines issues in open `.mod`, `.dyn` and `.inc` files and lists
them in VS Code's **Problems** panel (**View > Problems**). [Project
checks](project-checks.md) add issues from unopened saved models in your
workspace.

## Find a problem

Click an entry in **Problems** to jump to its file and line, or hover over an
underlined token in the editor to read the message. Press `Alt+F8` to step
through problems in the active file with details expanded, or `F8` to move
across files.

| Severity | Meaning |
|---|---|
| Error | A source problem that would cause the official Dynare preprocessor to refuse the model. |
| Warning | A likely modeling mistake or inconsistency that Dynare accepts with a warning or without stopping. |
| Information | A style or completeness note, such as missing equation `name` tags (`I208`), missing `long_name` labels (`I209`) or incomplete macro expansion (`I211`). |

Each diagnostic has a code such as `W031`. Search for the code in Help or
browse the [diagnostic code list](reference.md). A clean
**Problems** panel means no static source issues were detected; run official
Dynare in MATLAB or Octave to verify steady states and solver conditions.

## Follow a related location

When a diagnostic involves a second location, expand the arrow beside its
message in **Problems** (or inspect the `Alt+F8` peek view) and click the
linked location below it.

For example, declaring `y` twice produces [W031](check:W031) on the second
declaration:

```dynare
var y;
var y;
```

Expanding the warning in **Problems** reveals **Earlier declaration or creating
occurrence**, which jumps directly to the first `var y;`.

## Actions

Place the cursor on an underlined diagnostic and click the lightbulb (`Ctrl+.`)
or choose **Quick Fix...** from the editor context menu.

| Action | Result |
|---|---|
| Repair action | Applies an available source fix for the selected diagnostic. |
| **Explain this check (`<code>`)** | Opens the check's explanation in **Dygnosis Help** ([Dynare › Diagnostic Actions: Explain](settings:dynare.diagnosticActions.explain)). |
| **Dygnosis: Explain a check** (Command Palette) | Prompts for a check code (such as `W031`) and opens its explanation from the active engine. |
| **Ignore this check (`<code>`)** | Hides all diagnostics with that code in the current window ([Dynare › Diagnostic Actions: Ignore](settings:dynare.diagnosticActions.ignore)). |
| **Dygnosis: Show a hidden check** / **Dygnosis: Show all hidden checks** (Command Palette) | Restores one hidden check code or all hidden check codes. |

When any check code is hidden, the status bar displays **`1 hidden check`** (or
the hidden count). Click that status bar item to restore a hidden check. Hiding
applies only to the current window and does not modify your files.
