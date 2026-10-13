# Write equations and commands

Dygnosis supports the declarations, equations and computing instructions in a
Dynare model. [Checks](diagnostics.md) update while you edit.

Type part of a name or command to see suggestions. To open the list yourself,
run **Trigger Suggest** from the Command Palette, or press Ctrl+Space. Select
a suggestion and press Enter. Hover over a name, command or option to read
its description.

## Names and parameters

Completion offers declared names. Hover shows a name's kind, written long name
and TeX label, known parameter value and timing.
[Labels and value hints](appearance.md) have their own visibility controls.

When you edit a declaration's `long_name`, the suggestion list offers the
symbol's name as a label. To fill several labels at once, open Quick Fix on an
`I209` message and choose **Add long names**. This fills the missing or empty
labels counted by that message, including in included files. Existing values
stay unchanged.

## Equations

Type `model` and choose its block snippet from the suggestions to insert an
empty `model` block. Equation-tag suggestions offer an editable name such as
`[name='eq1']`. To name several equations at once, open Quick Fix on an `I208`
message and choose **Add equation tags**. This fills the missing or empty
names counted by that message and keeps existing values.

[Definition, reference and rename actions](navigate-code.md) connect an
equation's names to the rest of the model. Naming actions and fixes can
change included files.

### Model-local variables

Model-local variables (`#name = expression;`) have completion and expression
hover within their model, plus [navigation and rename](navigate-code.md#model-local-variables).
Hover over a model-local name to see its expression. If macros changed that
expression, the hover identifies it as expanded. It shows the expression
without calculating its value.

## Steady state, shocks and experiment commands

Block snippets also provide empty `steady_state_model`, `initval`, `endval`
and `shocks` blocks. Command and option completion, hover and signature help
cover recognized commands and their arguments. While the cursor is inside a
command's parentheses, **Trigger Parameter Hints** in the Command Palette
opens the argument help. The [option reference](reference.md)
lists the supported options.

When the model has no ordinary `shocks` block, place the cursor on an eligible
exogenous declaration and open **Refactor...** from the editor context menu.
Choose **Insert stochastic shocks template** or **Insert deterministic shocks
template** from the available actions.
These actions insert commented examples for you to fill with shock sizes or periods.

Type `@#` at the start of a line to see macro directive suggestions such as
`define`, `if` and `for`.
[Macro expansion](effective-model.md) shows the resulting model text.

## Source edits

Right-click in the editor and choose **Format Document** to format the file,
or select text and choose **Format Selection** to format that selection. Both use
[tabs or spaces](settings:dynare.formatIndent). Native MATLAB lines and
`verbatim` text keep their spacing. Run **Expand Selection** from the Command
Palette to select a larger source region; run it again to grow the selection.
Enable **Editor: Linked Editing** in Settings to edit known occurrences of a
declared name together in the current document.

[Quick Fix](diagnostics.md) offers available repairs for a selected check,
such as a missing declaration, identifier or semicolon.
