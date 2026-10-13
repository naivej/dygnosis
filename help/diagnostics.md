# Check the model

Dygnosis underlines problems in open models and their included files. It also
lists them in VS Code's **Problems** panel. [Project checks](project-checks.md)
add problems from unopened saved models.

## Find a problem

Open **View > Problems**. Click a problem's message to open its file at the
reported line. Hover over an underlined name or expression in the editor to
read the message there.

With the editor focused, **Alt+F8** opens the next problem in the current file
and shows its details. **F8** moves through problems across files. These are
the default shortcuts; VS Code's **Keyboard Shortcuts** editor shows your
current bindings.

| Severity | Meaning |
|---|---|
| Error | An identified source problem that would make the official Dynare preprocessor refuse the model. |
| Warning | A potential problem that does not itself cause that refusal. |
| Information | A point to review, such as missing equation names or incomplete analysis. |

Each message has a check code, such as `W031`. Search for that code in Help
or open the [diagnostic code list](reference.md) for its explanation.
A clean Problems panel does not establish that Dynare can accept or solve the model; see [About Dygnosis](about.md).

## Follow a related location

Some problems have an extra entry that points to another line involved in the
problem. In the Problems tree view, click the small arrow beside the problem's
message, then click the entry below it to open that line. In the details opened
by Alt+F8, click the extra entry's filename.

For example, these two declarations produce [W031](check:W031) on the second `y`:

```dynare
var y;
var y;
```

Expand that warning in Problems. The entry **Earlier declaration or creating
occurrence** points to the first `y`, so you can see both declarations.

## Actions

Place the cursor on an underlined problem and open **Quick Fix...** from the
editor context menu, or click the lightbulb beside the line. Choose a repair,
**Explain this check**, or **Ignore this check** from the action list.

| Action | Result |
|---|---|
| Quick Fix | An available repair for the selected check. |
| Explain this check | Opens this problem's explanation in Help. Its [Quick Fix entry](settings:dynare.diagnosticActions.explain) is optional. |
| Dygnosis: Explain a check in the Command Palette | Asks for a code, such as `W031`, then opens its explanation in Help. |
| Ignore this check | Hides every diagnostic with that code in this window, including its attached fixes. Its [Quick Fix entry](settings:dynare.diagnosticActions.ignore) is optional. |
| Dygnosis: Show a hidden check / Dygnosis: Show all hidden checks in the Command Palette | Restores one hidden code or all hidden codes. |

Hiding affects only this window's display. It does not change model files or
agent results. The status bar shows **1 hidden check**, for example. Click
that item to choose a code to show again.
The list clears when the window reloads or workspace folders change; restarting
the language server keeps it.
