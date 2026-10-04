# Dygnosis

![Dygnosis](media/logo_s.png)

Dygnosis helps people and agents check and edit Dynare models. It shows problems in the written source and shares its Rust analysis engine between the editor and MCP.

- Edit with live diagnostics, completion, fixes, references and rename.
- Inspect model counts, equations, includes and expanded source.
- Compare models and check unopened saved models in a project.

Install from the [VS Code Marketplace](https://marketplace.visualstudio.com/items?itemName=dygnosis.dygnosis). The extension includes the engine. Run **Dygnosis: Open Help**, or select the question mark in either Dygnosis Explorer view, for the complete offline reference. Its [source topics](../../help/get-started.md) are also readable here.

For MCP without VS Code, download the native Rust binary from [GitHub Releases](https://github.com/naivej/dygnosis/releases) and follow [MCP setup](../../help/get-started.md#mcp-without-vs-code). Each tool describes its inputs.

Language support follows Dynare 7.2. Dygnosis stops before MATLAB or Octave and does not run the official preprocessor. Use official Dynare for final validation and numerical results. See [scope and credits](../../help/about.md) and the [changelog](../../CHANGELOG.md).

[GPL-3.0-or-later](../../LICENSE).
