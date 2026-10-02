# CodeLens in VS Code

**Browse N equations** appears above each safely mapped `model` opener. Choose an equation to open its written source, including equations in an included file. The list shows its name, aggregate or heterogeneity dimension, source file, and Dygnosis equation number before transformation.

The count covers surviving counted equations in that block. Model locals, static-only rows, and removed equations have no active counted number. Replacement equations use their surviving numbers. These counts and numbers describe the written model before Dynare transformation; later transformations can change MATLAB runtime numbering.

Repeated macro executions at one written opener share one lens. **Browse equations (N occurrences)** first asks which block occurrence to use, showing the dimension, macro values, and each occurrence's count. It then lists that occurrence's equations. An incomplete expansion or unsafe source mapping withholds a counted browser. Open an include's model root first; when several known roots own the include, use **Dygnosis: Choose model owner** to choose its context.

Two optional actions are available:

- **Find references** appears at declaration lines. A line declaring several names opens a symbol picker, then native Find All References. Results use the existing language-server name-occurrence search and may include declarations. The lens requests references when clicked and shows no usage count.
- **Show effective model** appears at the first safe model opener. It opens the chosen root's read-only effective preview beside the editor.

Control these actions in Settings under **Dygnosis: Actions**:

| Setting | Default | Action |
|---|---|---|
| `dynare.codeLens.modelEquations` | `true` | Browse a model block's equations |
| `dynare.codeLens.declarationReferences` | `false` | Find references from declarations |
| `dynare.codeLens.effectiveModel` | `false` | Open the effective preview |

All three settings apply to the displayed file and support workspace/folder overrides. Changes apply immediately. Use **Reset Setting** to restore a default. Native `editor.codeLens` controls overall visibility; it also stops this surface's model requests when off. For Dynare files only, a language override is convenient:

```json
"[dynare]": {
  "editor.codeLens": false
}
```

Native commands remain available through the Command Palette and editor menus when lenses are hidden: **Dygnosis: Go to equation**, **Dygnosis: Jump to named equation**, **Dygnosis: Show effective model**, and **Find All References**. Configure their shortcuts in VS Code's Keyboard Shortcuts editor.

Lenses refresh with edits, include changes, owner choices, and server restarts. A click made from older model data stops and asks you to use the refreshed action. Only verified written locations open; equation text and numbering are never used to guess a source location.
