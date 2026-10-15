# Write equations and commands

Dygnosis provides completion, hover, signature help, snippets, templates and
formatting for Dynare files while updating [model checks](diagnostics.md) as
you type.

Start typing a symbol, command, block or option to see suggestions, or press
`Ctrl+Space` (**Trigger Suggest**). Hover over any declared name, command,
operator or option to read its description.

## Names and parameters

Completion offers declared endogenous variables, exogenous variables and
parameters with their `long_name` and TeX metadata. Hover displays a symbol's
kind, timing class, known parameter value, `long_name` and TeX label (configured
in [Read labels, hints and colors](appearance.md)).

Inside `long_name='...'`, completion offers the symbol's name. To fill missing
or empty `long_name` labels across a declaration statement (including in
included files), place the cursor on the declaration keyword (`var`, `varexo`,
`varexo_det` or `parameters`) with an `I209` note, open **Quick Fix...**
(`Ctrl+.` or the lightbulb), and choose **Add long names**. Existing nonempty
labels stay unchanged.

## Equations

Type `model` at the top level and select the `model block` snippet to insert an
empty `model; ... end;` block. Inside `[name='...']`, completion suggests an
editable equation tag such as `[name='eq1']`.

To tag all unnamed equations in a `model` or `model_replace` block at once,
open **Quick Fix...** on the block keyword's `I208` note and choose **Add
equation tags**.

[Navigation and rename actions](navigate-code.md) connect every symbol in an
equation to its declaration and other uses across the model.

### Model-local variables

Model-local variables (`#name = expression;`) have completion and hover within
their model, plus [scoped navigation and
rename](navigate-code.md#model-local-variables). Hovering over a model-local
name displays its written or macro-expanded expression.

## Steady state, shocks and experiment commands

Top-level block snippets also insert `steady_state_model`, `initval`, `endval`
and `shocks` blocks. Inside a command's or block's parentheses, typing `(`, `,`
or `=` (or running **Trigger Parameter Hints**) opens option help. See the
[option reference](reference.md) for all supported options.

When the model has no standard `shocks` block, place the cursor on an exogenous
declaration in `varexo`, open **Refactor...** from the editor context menu, and
choose **Insert stochastic shocks template** or **Insert deterministic shocks
template** to insert a commented `shocks; ... end;` block.

Type `@#` at the start of a line to complete macro directives such as `define`,
`if`, `for` and `include`. Use [Expand macros](effective-model.md) to inspect
the expanded model text.

## Source edits

Right-click in the editor and choose **Format Document** or **Format
Selection**. Indentation uses [Dynare: Format
Indent](settings:dynare.formatIndent) (`tab` or `1`–`8` spaces), while native
MATLAB lines and `verbatim` blocks keep their spacing.

Run **Expand Selection** from the Command Palette to expand the selection from
an expression to its statement and enclosing block.

Enable **Editor: Linked Editing** (`editor.linkedEditing`) in Settings to edit
all uses of a declared name together when it only appears in the current file;
use **Rename symbol** (`F2`) when the name is shared with included files.
[Quick Fix](diagnostics.md#actions) offers repairs for common diagnostics such
as missing declarations, typos or missing semicolons.
