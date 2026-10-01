# Dygnosis for VS Code

Development scaffold for v0.11.0. The extension has no runtime features yet.
Ext-launch will port LLMacro's client into this folder and connect it to the
Rust language server. Language analysis stays in the binary.

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

The scaffold uses extension version `0.11.0`; the binary stays at its current
release until v0.11 is ready. The grammar, language-client lifecycle, commands,
and extension tests belong to the planned client port.
