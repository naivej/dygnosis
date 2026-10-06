# Edit a model

Open a `.mod` or `.dyn` file and set its language mode to **Dynare**. Dygnosis
checks text as you edit. Open includes through their model root to retain that
model's context. See [model roots and includes](model-and-includes.md).

![Name hover, completion and signature help in the editor](assets/edit-assistance.png)

1. **Hover** shows a declared name's kind, written metadata and proven values where available.
2. **Trigger Suggest** completes names, commands and recognized options with role icons and documentation.

## Names and commands

Hover over a declared name to see its kind and available written metadata.
`long_name` and TeX names remain literal text. Proven parameter values and
timing appear where the engine supplies them. These facts do not establish a
steady state or a numerical solution.

Use **Trigger Suggest** to complete names, commands and recognized command
options. Accepting a name inserts its identifier. The long name is additional
documentation. Command options have their own descriptions; the
[option reference](reference.md) lists the recognized options.

Empty `model`, `steady_state_model`, `initval`, `endval` and `shocks` skeletons
use native snippet placeholders. Press Tab to move between placeholders.
**Trigger Parameter Hints** shows the recognized function or command signature
and the active argument when the engine can identify it.

Use **Trigger Suggest** before an equation or at its incomplete or empty
`name` tag to insert an editable name such as `[name='eq1']`. Dygnosis chooses
the lowest unused positive `eqN`; the number does not depend on equation
position, and existing names do not change when equations move. Typed prefixes
such as `[name=` and empty values such as `[name='']` are replaced without
adding another key or quote. Editors that support snippet placeholders select
the whole name for immediate replacement. Other clients receive plain text.

After a declared symbol, or in its incomplete or empty `long_name` option,
**Trigger Suggest** can insert that symbol's own name. For example, `var y;`
becomes `var y (long_name='y');`. A typed prefix such as
`y (long_name=...)` or an empty value such as `y (long_name='')` can be
completed in place. If a name cannot be chosen safely for the active model,
the suggestion is withheld. At the end of a final declaration, you can accept
the completion before typing `;`; the edit adds only the metadata, so type the
semicolon yourself.

## Fixes and refactors

Place the cursor on a diagnostic and open **Quick Fix**. Apply only an action
whose proposed edit fits your model. Actions can edit an unopened included
file. VS Code's edit preview shows the affected files. Independent refactors
can remain available when you hide a diagnostic.

The offered diagnostic fixes depend on the source: declare a missing name,
correct a suggested identifier, remove a redundant declaration or repair
statement syntax. **Add equation tags** on I208 proposes names only for the
equations counted by that note's model block. **Add long names** on I209 edits
only the symbols counted by that declaration note. A model opener can be in the
root while its equations are in an include. Review the proposed files and edits.

Both metadata actions fill empty literal values and keep nonempty values. They
preserve other equation tags, declaration options, TeX, partition options,
comments, and macro text. Each action changes only the equations or symbols
counted by its I208 or I209 note, including those in an included file. When an
include belongs to several model files, the action title identifies the model
file. If some rows cannot be edited safely, the title ends with `(N skipped)`
and gives the number. Repeated macro text is edited only when one literal
change works for all affected copies; otherwise it stays as written. After the
text or model inputs change, request the action again. Applying it again leaves
existing metadata unchanged.

When a check is selected, Quick Fix shows only that check's edits. Another
check at the same place is not a substitute. With no check selected, each
offered fix names its check. Quick Fix and refactor stay separate. A captured
action runs only while that open file is still the one that offered it. After
the text changes, request the action again.

**Insert stochastic shocks template** and **Insert deterministic shocks
template** are independent refactors. They insert commented templates for the
available exogenous names when an ordinary shocks block is absent. The model's
simulation commands determine which form is offered; mixed or unspecified
context can offer both. Fill in the template and uncomment it only when it fits
the intended experiment. The editor inserts it only while that open file is
still unchanged. A later edit, a dependency change, or a new engine does not
insert the captured template. Request it again from the current text.

## Format and select

Use **Format Document** or **Format Selection**. Formatting changes layout,
not model meaning. [Format indentation](settings:dynare.formatIndent) selects
a tab or 1–8 spaces. For four spaces, put this in User or Workspace settings:

```json
{"dynare.formatIndent": 4}
```

**Expand Selection** uses the language server's nested source ranges. Enable native `editor.linkedEditing` to edit all known occurrences of a declared name in the current document together. At least two source ranges are needed.
An incomplete or unsafe region can have no edit or range; finish the construct
and request the action again. Undo uses VS Code's normal edit history.

[Diagnostic controls](diagnostics.md) explain checks and temporary hiding.
[Appearance](appearance.md) explains metadata and hint visibility.
