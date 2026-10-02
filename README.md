![dygnosis](media/logo_s.png)

Dygnosis is a second Dynare preprocessor that helps you check and edit Dynare `.mod` files in an editor or with an AI agent. It catches most problems Dynare would report before MATLAB runs, points to them in your file, and adds guidance for possible problems Dynare does not report.

*This project is under active development.*

## Install

With Git, Rust and Cargo installed:

```sh
git clone https://github.com/naivej/dygnosis.git
cd dygnosis
cargo install --path . --locked
```

## Use

### Command line

| Command | Purpose |
|---------|---------|
| `dygnosis check <file.mod>` | Check a model file |
| `dygnosis check <directory>` | Check `.mod` files recursively |
| `dygnosis explain <CODE>` | Get help for a diagnostic |
| `dygnosis explain --list` | List diagnostic codes |
| `dygnosis mcp` | Start the MCP server over stdio |
| `dygnosis` | Start the language server over stdio |
| `dygnosis --tcp` | Start the language server over TCP for debugging (default `127.0.0.1:2087`) |

`check` accepts several file or directory paths. Errors or unreadable paths give exit status 1; warnings alone give status 0. Generated folders whose names start with `+` are skipped.

### Editor (LSP)

Configure an LSP client to launch `dygnosis` over stdio. Available features include:

- Diagnostics while typing and on save
- Hover, completions and command-option help
- Go to definition, find references and rename, including across known include files
- Outline, workspace symbols, syntax colors, folding and links to related files
- Document/range formatting, available diagnostic fixes and commented shock templates
- Linked duplicate locations and proven expression-value inlay hints
- Expanded model text with jumps back to source equations

Workspace folders have separate settings and include paths. The `dynare/modelInfo` command supplies shared counts, written source locations, and input revisions for client model views. Equation labels are Dygnosis numbers before transformation. See the [editor settings guide](docs/editor-settings.md) for configuration, root ownership, and semantic token migration. The VS Code extension remains an unpublished scaffold in this release.

### AI agents (MCP)

Configure an MCP client to launch `dygnosis` with the argument `mcp` over stdio.

| Tool | Purpose |
|------|---------|
| `dynare_diagnose` | Check a model file |
| `dynare_workspace_diagnose` | Check several model files or directories |
| `dynare_model_info` | Summarize names, counts and variable timing |
| `dynare_compare_models` | Compare declarations, parameter values, equations and shock settings |
| `dynare_find_references` | Find uses of a name |
| `dynare_rename` | Rename a name |
| `dynare_auto_fix` | Apply available diagnostic fixes |
| `dynare_explain` | Get help for a diagnostic |
| `dynare_list_diagnostic_codes` | List diagnostic codes |
| `dynare_list_options` | List command options |
| `dynare_equations` | List equations and their source locations |
| `dynare_related_files` | List include files and companion files |
| `dynare_expand` | Show expanded model text and source locations |
| `dynare_format` | Format a model file |
| `dynare_extract` | Extract selected equations and their supporting declarations |

Extracted text is a model fragment that must be completed before running it in Dynare. For release details, see the [changelog](CHANGELOG.md).

## Limits

- Language support follows **Dynare 7.2**. Some pre-MATLAB checks are outside coverage; use Dynare for final validation.
- Unsupported macro expressions are flagged as incomplete. Affected checks are withheld and model views are marked incomplete.
- Dygnosis does not launch Dynare, MATLAB or Octave. Numerical results, including steady states, stability and model solutions, require running Dynare with MATLAB or Octave.

For MATLAB integration, see the [MATLAB extension for VS Code](https://github.com/mathworks/MATLAB-extension-for-vscode) and [MATLAB agentic toolkit](https://github.com/matlab/matlab-agentic-toolkit).

## Credits

1. Dygnosis v0.1.0 is a fork and rewrite of [LLMacro-Dynare-LSP](https://github.com/pdwhoward/LLMacro-Dynare-LSP) by Anthony Diercks, Philip Howard and Mehrdad Samadi. That repository accompanies the working paper *LLMacro: A Language Server for Dynare — Structured Context for AI-Assisted Macroeconomic Modeling*.
2. Equation, tag and extraction ideas are informed by the Dynare Team's [modBuilder](https://git.dynare.org/Dynare/modBuilder).
3. The bundled agent skill is adapted from [EconSolider/dynare-copilot](https://github.com/EconSolider/dynare-copilot).

## License

[GPL-3.0-or-later](LICENSE). The bundled dynare-copilot skill retains its upstream [MIT license](https://github.com/EconSolider/dynare-copilot/blob/main/LICENSE).
