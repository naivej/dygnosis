# Write equations and commands

Dygnosis supports the declarations, equations and computing instructions in a
Dynare model. [Checks](diagnostics.md) update while you edit.

## Names and parameters

Completion offers declared names. Hover shows a name's kind, written long name
and TeX label, known parameter value and timing.
[Labels and value hints](appearance.md) have their own visibility controls.

Metadata completion offers a declaration's `long_name`. **Add long names**
fills missing or empty values covered by an I209 note, including in included
files. Existing values stay unchanged.

## Equations

Block snippets provide an empty `model` block. Equation-tag completion offers
an editable name such as `[name='eq1']`. **Add equation tags** fills missing or
empty names covered by an I208 note and keeps existing values.

[Definition, reference and rename actions](navigate-code.md) connect an
equation's names to the rest of the model. Metadata actions and fixes can
change included files; ambiguous or macro-generated text can prevent an edit.

### Model-local variables

Model-local variables (`#name = expression;`) have completion and expression
hover within their model, plus [navigation and rename](navigate-code.md#model-local-variables).
An expression changed by macros is marked as expanded. Hover does not evaluate
model-local expressions.

## Steady state, shocks and experiment commands

Block snippets also provide empty `steady_state_model`, `initval`, `endval`
and `shocks` blocks. Command and option completion, hover and signature help
cover recognized commands and their arguments. The [option reference](reference.md)
lists the supported options.

Stochastic and deterministic shock templates are available for eligible
exogenous names when the model has no ordinary shocks block. Templates are
commented and contain no assumed shock sizes or periods.

Macro directive completion is available after `@#`.
[Macro expansion](effective-model.md) shows the resulting model text.

## Source edits

**Format Document** and **Format Selection** apply consistent layout with
[tabs or spaces](settings:dynare.formatIndent). Native MATLAB lines and
`verbatim` text keep their spacing. **Expand Selection** selects progressively
larger source regions. Native `editor.linkedEditing` enables simultaneous edits
to known occurrences of a declared name in the current document.

[Quick Fix](diagnostics.md) offers available repairs for a selected check,
such as a missing declaration, identifier or semicolon.
