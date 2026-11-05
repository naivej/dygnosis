# Write equations and commands

Dygnosis provides completion, hover, option signature hints, snippets and
formatting for Dynare files while updating [model checks](diagnostics.md) as
you type.

## Names, equations and shocks

Completion and hover show each declared symbol's kind, timing class, evaluated
parameter value, `long_name` and TeX label (configured in [Read labels, hints
and colors](appearance.md)). Inside command or block parentheses, parameter
hints describe options, and completion and hover explain values such as
`pac_model(kind=dl)` and other fixed syntax. To request suggestions or option
hints while editing, run **Trigger Suggest** or **Trigger Parameter Hints**
from the Command Palette. See the
[Dynare manual](https://www.dynare.org/manual/).

Use **Quick Fix...** (`Ctrl+.`) or **Refactor...** on declarations and blocks
to automate repetitive model setup:

| Task | Where to trigger | Action |
|---|---|---|
| Fill missing `long_name` labels (`I209`) | On `var`, `varexo`, `varexo_det` or `parameters` | **Quick Fix...** → **Add long names** (fills missing or empty `(long_name='...')` labels across the declaration, including in `@#include` files) |
| Tag unnamed equations (`I208`) | On `model` or `model_replace` | **Quick Fix...** → **Add equation tags** (adds `[name='eq1']`, `[name='eq2']`, … to untagged equations) |
| Generate a `shocks` block | On a `varexo` declaration when no standard `shocks` block exists | **Refactor...** → **Insert stochastic shocks template** or **Insert deterministic shocks template** |
| Insert block scaffolding | At top level in the editor | Type `model`, `steady_state_model`, `initval`, `endval` or `shocks` and select the block snippet |
| Complete macro directives | At the start of a line | Type `@#` to complete `define`, `if`, `for`, `include`, and other directives ([Inspect macro expansion](effective-model.md)) |

### Model-local variables

Model-local variables (`#name = expression;`) provide completion and hover
within their model, plus [scoped navigation and
rename](navigate-code.md#model-local-variables). Hovering over a model-local
name displays its written or macro-expanded expression.

## Formatting and source edits

- **Format Document** / **Format Selection** from the Command Palette or editor
  context menu: Indents Dynare blocks and statements using [Dynare: Format
  Indent](settings:dynare.formatIndent) (`tab`
  or `1`–`8` spaces) while leaving native MATLAB lines and `verbatim` blocks
  untouched.
- **Expand Selection** from the Command Palette: Select an expression or
  statement, then expand the selection to its enclosing statement or block.
- **Linked Editing**: Enable **Editor: Linked Editing** (`editor.linkedEditing`)
  to edit all occurrences of a symbol together when it only appears in the
  current file; use **Rename symbol** (`F2`) when the symbol is shared across
  `@#include` files.
- **Diagnostic repairs**: Use [Quick Fix](diagnostics.md#actions) (`Ctrl+.`) on
  underlined diagnostics to fix missing declarations, typos or missing
  semicolons.
