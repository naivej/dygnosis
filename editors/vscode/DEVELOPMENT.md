# Developing the VS Code extension

Language analysis stays in the Rust engine. The client uses native VS Code
navigation, Problems, Outline, tree views, status items and Settings; structural
Diff uses a webview of engine results.

Use Node.js 22.13+ or 24+:

```sh
npm ci
npm run check
npm run lint
npm run compile
npm test
```

Build output is written to `out/` and stays out of Git. In the `dygnosis_dev`
development checkout, open its root folder or `dygnosis.code-workspace`.
Ctrl+Shift+B compiles the extension; F5 launches the Extension Development Host.
The root `.vscode/` tasks and debug configuration point to this folder.

For actual host tests, set `DYGNOSIS_TEST_BINARY` to a matching built engine,
`DYGNOSIS_VSCODE_EXECUTABLE` to the chosen minimum/current Code executable and
`DYGNOSIS_EXPECTED_VSCODE` to its exact version, then run `npm run test:host`.
Each run uses an isolated profile and fresh evidence. Native package creation,
verification and target/runtime support are documented in
[distribution.md](../../docs/distribution.md).

## README illustrations

The three README images are illustrations, not screenshots. Their editable SVG
sources are under `media/readme/source/`; the packaged README uses the PNGs.
The renderer preserves exact text and draws UI symbols as paths.

Install `@resvg/resvg-js` in separate development storage, then run:

```sh
node scripts/render-readme.cjs /absolute/path/to/node_modules/@resvg/resvg-js
```

Check the output at normal README width and verify every depicted action and
label against the current implementation. Generated PNGs are committed. The
renderer is excluded from the VSIX along with other development scripts; it
is not an extension runtime dependency. The package script sets an immutable
image base for this nested extension folder. Its content base stays at the
product root for copied CHANGELOG links; README documentation links are
absolute. Check the actual VSIX README after packaging.
