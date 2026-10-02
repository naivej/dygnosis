# Dygnosis for VS Code

Unpublished v0.11.0 development scaffold. The extension has no runtime features yet.
Ext-launch will build our own client in this folder and connect it to the
Rust language server using `vscode-languageclient`. LLMacro's `vscode-dynare/`
is a reference only. Language analysis stays in the binary.

The planned client uses VS Code's native navigation, Problems and quick fixes,
Outline, Explorer Tree View, status bar, command menus, Settings, and theme
support. Its actions should be easy to find and use with the keyboard.
Planned model comparison uses an interactive webview of the binary's structured
changes, with Before/After rows, filters, and source jumps.
The planned CodeLens opens a model block's equation browser; declaration
references and the effective-model preview are optional actions. It adds no
solver actions or claimed usage counts.
From 0.11.2, the read-only effective preview will offer written-source jumps,
macro-origin picks, and Refresh using the engine's source mapping.
Most UI features will be configurable through grouped Dygnosis settings or
native VS Code controls. Block tinting has separate aggregate/heterogeneous
model controls and detailed choices for other blocks, with sensible defaults.
Name colors will use meaningful Dynare-specific labels for endogenous,
exogenous, model-parameter, and model-local names, with standard fallbacks
and independent styling through VS Code's semantic-token customization.
Project diagnostics will be on by default from 0.11.3, checking unopened models
in the background with active-file priority, progress, folder exclusions, and an off
switch. Editor responsiveness and retained memory must pass the release checks.

From this folder, with Node.js 22.13+ (22.x) or 24+:

```sh
npm ci
npm run check
npm run lint
npm run compile
```

In the `dygnosis_dev` development repo, open its root folder or
`dygnosis.code-workspace` in VS Code. The `.vscode/` folder lives at the
development repo root. Press Ctrl+Shift+B to compile the extension,
or F5 to compile and launch the Extension Development Host. The tasks and debug
configuration point to this folder explicitly.

Build output is written to `out/` and stays out of Git. The public
product's Rust crate remains at the repository root; builds and rustfmt only
process Rust sources, and Cargo source packages exclude `editors/**`.

The planned series starts with Rust preparation in 0.11.0. Version 0.11.1
ships the first packaged client and matching bundled binary; 0.11.2 adds
structural Diff, effective-preview origin jumps, and repo-local MCP setup;
0.11.3 adds background project checks.
The development manifest currently says `0.11.0`; the first published extension
will be `0.11.1`, matching its bundled binary. Grammar, lifecycle, commands, and
extension tests remain planned work.
